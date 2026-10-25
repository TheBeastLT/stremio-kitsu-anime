use std::convert::Infallible;

use serde_json::json;

use super::*;
use crate::{
    application::{metadata::object, testing::REAL_MAPPING},
    infrastructure::config::DEFAULT_ADDON_URL,
};

fn cinemeta_videos(imdb_id: &str, seasons: &[(u32, u32)]) -> Value {
    let videos: Vec<Value> = seasons
        .iter()
        .flat_map(|&(season, count)| {
            (1..=count).map(move |episode| {
                json!({
                    "id": format!("{imdb_id}:{season}:{episode}"), "season": season, "episode": episode,
                    "released": "2020-01-01T00:00:00.000Z",
                })
            })
        })
        .collect();
    Value::Array(videos)
}

fn kitsu_meta(kitsu_id: JsValue, episode_count: u32, kind: &str) -> JsObject {
    let id = kitsu_id.text();
    let videos = (1..=episode_count)
        .map(|episode| {
            object([
                ("id", JsValue::String(format!("kitsu:{id}:{episode}"))),
                ("title", JsValue::String(format!("Episode {episode}"))),
                ("season", JsValue::Number(1.0)),
                ("episode", JsValue::Number(f64::from(episode))),
                ("released", JsValue::Date(1_577_836_800_000.0)),
            ])
        })
        .collect();
    let JsValue::Object(meta) = object([
        ("kitsu_id", kitsu_id),
        ("type", kind.into()),
        ("genres", JsValue::Array(vec!["Action".into()])),
        ("links", JsValue::Array(Vec::new())),
        ("videos", JsValue::Array(videos)),
    ]) else {
        unreachable!()
    };
    meta
}

fn numeric(kitsu_id: u32) -> JsValue {
    JsValue::Number(f64::from(kitsu_id))
}

async fn kitsu(meta: JsObject, imdb: Option<Value>) -> Value {
    enrich_kitsu(meta, &REAL_MAPPING, &GenreUrl::new(DEFAULT_ADDON_URL), |_| async move { imdb }).await
}

async fn imdb(meta: Value) -> Value {
    enrich_imdb(meta, &REAL_MAPPING, |_| async { panic!("no kitsu lookup") as Result<JsObject, Infallible> })
        .await
        .unwrap()
}

fn aot_videos() -> Value {
    cinemeta_videos("tt2560140", &[(0, 2), (1, 25), (2, 12), (3, 22)])
}

fn one_piece_specials() -> Value {
    cinemeta_videos("tt0388629", &[(0, 60)])
}

fn fields(meta: &Value, names: &[&str]) -> Vec<Vec<Value>> {
    meta["videos"]
        .as_array()
        .unwrap()
        .iter()
        .map(|video| names.iter().map(|name| video.get(*name).cloned().unwrap_or(Value::Null)).collect())
        .collect()
}

fn find(meta: &Value, season: u32, episode: u32) -> &Value {
    meta["videos"]
        .as_array()
        .unwrap()
        .iter()
        .find(|video| video["season"] == season && video["episode"] == episode)
        .unwrap()
}

#[tokio::test]
async fn a_kitsu_entry_starting_mid_series_counts_on_from_the_imdb_episodes_before_its_start() {
    let meta = kitsu(kitsu_meta(numeric(41982), 10, "series"), Some(json!({ "videos": aot_videos() }))).await;
    let first = fields(&meta, &["imdbSeason", "imdbEpisode", "absoluteEpisode"]);
    assert_eq!(first[..2], [vec![json!(3), json!(13), json!(50)], vec![json!(3), json!(14), json!(51)]]);
}

#[tokio::test]
async fn kitsu_episodes_missing_on_imdb_keep_kitsus_own_numbering() {
    let videos = cinemeta_videos("tt0388629", &[(1, 591)]);
    let meta = kitsu(kitsu_meta(numeric(12), 592, "series"), Some(json!({ "videos": videos }))).await;
    let video = meta["videos"].as_array().unwrap().iter().find(|video| video["episode"] == 591).unwrap();
    assert_eq!((video["imdbEpisode"].clone(), video["absoluteEpisode"].clone()), (json!(590), json!(591)));
}

#[tokio::test]
async fn without_imdb_episodes_only_an_entry_starting_the_first_season_is_counted() {
    let first = kitsu(kitsu_meta(numeric(7442), 2, "series"), None).await;
    let later = kitsu(kitsu_meta(numeric(41982), 2, "series"), None).await;
    assert_eq!(fields(&first, &["absoluteEpisode"]), [[json!(1)], [json!(2)]]);
    assert_eq!(fields(&later, &["absoluteEpisode"]), [[Value::Null], [Value::Null]]);
}

#[tokio::test]
async fn specials_and_unmapped_kitsu_entries() {
    let special = kitsu(kitsu_meta(numeric(7838), 1, "series"), Some(json!({ "videos": aot_videos() }))).await;
    let unmapped = enrich_kitsu(
        kitsu_meta(numeric(99_999_999), 2, "series"),
        &REAL_MAPPING,
        &GenreUrl::new(DEFAULT_ADDON_URL),
        |_| async { panic!("no imdb lookup") },
    )
    .await;
    assert!(special["videos"][0].get("absoluteEpisode").is_none());
    assert_eq!(fields(&unmapped, &["absoluteEpisode"]), [[json!(1)], [json!(2)]]);
}

#[tokio::test]
async fn an_imdb_meta_numbers_its_episodes_like_the_kitsu_metas_do() {
    let meta = imdb(json!({ "id": "tt2560140", "type": "series", "videos": aot_videos() })).await;
    let absolute = |season, episode| find(&meta, season, episode).get("absoluteEpisode").cloned();
    assert_eq!(
        [absolute(0, 1), absolute(1, 1), absolute(2, 12), absolute(3, 13)],
        [None, Some(json!(1)), Some(json!(37)), Some(json!(50))]
    );
}

#[tokio::test]
async fn an_imdb_meta_without_cinemeta_episodes_counts_the_first_seasons_kitsu_episodes() {
    let meta = enrich_imdb(json!({ "id": "tt0388629" }), &REAL_MAPPING, |kitsu_id| async move {
        Ok::<_, Infallible>(kitsu_meta(JsValue::String(kitsu_id), 2, "series"))
    })
    .await
    .unwrap();
    let counted: Vec<Vec<Value>> =
        fields(&meta, &["kitsu_id", "absoluteEpisode"]).into_iter().filter(|fields| !fields[1].is_null()).collect();
    assert_eq!(counted, [vec![json!("12"), json!(1)], vec![json!("12"), json!(2)]]);
}

#[tokio::test]
async fn an_imdb_meta_skips_kitsu_episodes_missing_on_imdb() {
    let videos = cinemeta_videos("tt0388629", &[(1, 591)]);
    let meta = imdb(json!({ "id": "tt0388629", "type": "series", "videos": videos })).await;
    let video = find(&meta, 1, 590);
    assert_eq!((video["kitsuEpisode"].clone(), video["absoluteEpisode"].clone()), (json!(591), json!(591)));
}

#[tokio::test]
async fn an_imdb_meta_gives_no_kitsu_episode_past_a_finished_entrys_last_one() {
    let videos = cinemeta_videos("tt0374407", &[(1, 50), (2, 50)]);
    let meta = imdb(json!({ "id": "tt0374407", "type": "series", "videos": videos })).await;
    let summary = |video: &Value| {
        ["kitsu_id", "kitsuEpisode", "absoluteEpisode"].map(|name| video.get(name).cloned().unwrap_or(Value::Null))
    };
    assert_eq!(summary(find(&meta, 1, 50)), [json!("72"), json!(50), json!(50)]);
    assert_eq!(summary(find(&meta, 2, 1)), [Value::Null, Value::Null, json!(51)]);
}

#[tokio::test]
async fn movies_get_no_franchise_wide_number() {
    let movie = enrich_kitsu(
        kitsu_meta(numeric(99_999_999), 1, "movie"),
        &REAL_MAPPING,
        &GenreUrl::new(DEFAULT_ADDON_URL),
        |_| async { panic!("no imdb lookup") },
    )
    .await;
    let to_imdb_movie = kitsu(kitsu_meta(numeric(7442), 1, "series"), Some(json!({ "type": "movie" }))).await;
    assert!(movie["videos"][0].get("absoluteEpisode").is_none());
    assert!(to_imdb_movie["videos"][0].get("absoluteEpisode").is_none());
}

#[tokio::test]
async fn a_kitsu_entry_listing_its_imdb_episodes_takes_them_out_of_sequence() {
    let meta = kitsu(kitsu_meta(numeric(45690), 4, "series"), Some(json!({ "videos": one_piece_specials() }))).await;
    assert_eq!(fields(&meta, &["imdbEpisode"]), [[json!(40)], [json!(41)], [json!(49)], [json!(50)]]);
    let stargazer = kitsu(
        kitsu_meta(numeric(1092), 3, "series"),
        Some(json!({ "videos": cinemeta_videos("tt0434694", &[(0, 10)]) })),
    )
    .await;
    assert_eq!(fields(&stargazer, &["imdbEpisode"]), [[json!(7)], [Value::Null], [Value::Null]]);
}

#[tokio::test]
async fn a_kitsu_entrys_run_steps_over_the_episodes_another_row_lists() {
    let meta = kitsu(kitsu_meta(numeric(48291), 3, "series"), Some(json!({ "videos": one_piece_specials() }))).await;
    assert_eq!(fields(&meta, &["imdbEpisode"]), [[json!(47)], [json!(48)], [json!(51)]]);
}

#[tokio::test]
async fn an_imdb_meta_gives_listed_episodes_to_the_row_listing_them_not_to_the_run_they_fall_in() {
    let meta = imdb(json!({ "id": "tt0388629", "type": "series", "videos": one_piece_specials() })).await;
    let special = |episode| {
        let video = find(&meta, 0, episode);
        (video["kitsu_id"].clone(), video["kitsuEpisode"].clone())
    };
    assert_eq!(
        [special(41), special(42), special(49), special(50)],
        [
            (json!("45690"), json!(2)),
            (json!("46215"), json!(1)),
            (json!("45690"), json!(3)),
            (json!("45690"), json!(4))
        ]
    );
}

#[tokio::test]
async fn a_row_at_s1e7_catches_s2e7_onwards() {
    let uncapped = Mapping::new(REAL_MAPPING.entries("tt0109633").to_vec(), &[]);
    let videos = cinemeta_videos("tt0109633", &[(1, 16), (2, 16)]);
    let meta = enrich_imdb(json!({ "id": "tt0109633", "type": "series", "videos": videos }), &uncapped, |_| async {
        panic!("no kitsu lookup") as Result<JsObject, Infallible>
    })
    .await
    .unwrap();
    let kitsu_id = |season, episode| find(&meta, season, episode)["kitsu_id"].clone();
    assert_eq!([kitsu_id(2, 6), kitsu_id(2, 7), kitsu_id(2, 12)], [json!("1621"), json!("6708"), json!("6709")]);
}

#[tokio::test]
async fn finished_entries_stop_a_row_catching_a_season_without_rows() {
    let videos = cinemeta_videos("tt0109633", &[(1, 16), (2, 16)]);
    let meta = imdb(json!({ "id": "tt0109633", "type": "series", "videos": videos })).await;
    assert!(find(&meta, 2, 7).get("kitsu_id").is_none());
    assert_eq!(find(&meta, 1, 16)["kitsu_id"], "6709");
}

#[tokio::test]
async fn a_numeric_kitsu_id_does_not_exclude_its_own_row_from_the_next_row_lookup() {
    let imdb_meta = json!({ "videos": cinemeta_videos("tt0143353", &[(1, 12), (2, 12)]) });
    let string_id = kitsu(kitsu_meta(JsValue::from("8124"), 8, "series"), Some(imdb_meta.clone())).await;
    let number_id = kitsu(kitsu_meta(numeric(8124), 8, "series"), Some(imdb_meta)).await;
    assert_eq!(fields(&string_id, &["imdbSeason", "imdbEpisode"])[6], [json!(2), json!(7)]);
    assert_eq!(fields(&number_id, &["imdbSeason", "imdbEpisode"])[6], [json!(1), json!(13)]);
}

#[tokio::test]
async fn an_airing_season_0_row_claims_every_later_special() {
    let videos = cinemeta_videos("tt22248376", &[(0, 10), (1, 28)]);
    let meta = imdb(json!({ "id": "tt22248376", "type": "series", "videos": videos })).await;
    assert_eq!(find(&meta, 0, 10)["kitsu_id"], "48105");
    assert_eq!(find(&meta, 0, 10)["kitsuEpisode"], 10);
}

#[tokio::test]
async fn an_imdb_meta_with_one_season_row_names_its_kitsu_id_as_a_string() {
    let meta = imdb(json!({ "id": "tt0374407", "type": "movie" })).await;
    assert_eq!(meta["kitsu_id"], "72");
}

#[tokio::test]
async fn an_imdb_meta_with_several_season_rows_names_their_kitsu_ids_as_a_list() {
    let meta = imdb(json!({ "id": "tt0143353", "type": "movie" })).await;
    assert_eq!(meta["kitsu_id"], json!(["1191", "8124"]));
    assert_eq!(meta["imdb_id"], "tt0143353");
}

#[tokio::test]
async fn an_unmapped_imdb_meta_is_returned_unchanged() {
    let cinemeta = json!({ "id": "tt0111161", "type": "movie", "name": "x", "videos": null });
    assert_eq!(imdb(cinemeta.clone()).await, cinemeta);
}

#[tokio::test]
async fn a_failed_imdb_lookup_keeps_the_kitsu_episodes_counted_from_the_row_start() {
    let meta = kitsu(kitsu_meta(JsValue::from("41982"), 2, "series"), None).await;
    assert_eq!(fields(&meta, &["imdbSeason", "imdbEpisode"]), [[json!(3), json!(13)], [json!(3), json!(14)]]);
    assert_eq!(meta["imdb_id"], "tt2560140");
}

#[tokio::test]
async fn kitsu_genres_and_rating_win_over_cinemetas() {
    let cinemeta = json!({
        "videos": aot_videos(), "imdbRating": "9.0", "runtime": "24 min", "genres": ["Animation", "Drama"],
        "links": [{ "name": "9.0", "category": "imdb", "url": "imdb" }, { "name": "Drama", "category": "Genres", "url": "x" }],
    });
    let mut meta = kitsu_meta(JsValue::from("7442"), 1, "series");
    let with_genres = kitsu(meta.clone(), Some(cinemeta.clone())).await;
    assert_eq!(with_genres["genres"], json!(["Action"]));
    assert_eq!(with_genres["imdbRating"], "9.0");
    assert_eq!(with_genres["runtime"], "24 min");
    meta.insert("genres".to_owned(), JsValue::Array(Vec::new()));
    meta.insert("imdbRating".to_owned(), "8.5".into());
    let without_genres = kitsu(meta, Some(cinemeta)).await;
    assert_eq!(without_genres["genres"], json!(["Drama"]));
    assert_eq!(without_genres["imdbRating"], "8.5");
    assert_eq!(
        without_genres["links"],
        json!([{ "name": "Drama", "category": "Genres", "url": GenreUrl::new(DEFAULT_ADDON_URL).url("Drama") }])
    );
}

#[tokio::test]
async fn imdb_released_dates_never_go_back_across_a_run() {
    let mut videos = cinemeta_videos("tt2560140", &[(1, 3)]);
    videos[1]["released"] = json!("2019-01-01T00:00:00.000Z");
    videos[2]["released"] = json!("not a date");
    let meta = kitsu(kitsu_meta(JsValue::from("7442"), 3, "series"), Some(json!({ "videos": videos }))).await;
    assert_eq!(
        fields(&meta, &["released"]),
        [[json!("2020-01-01T00:00:00.000Z")], [json!("2020-01-01T00:00:00.000Z")], [Value::Null]]
    );
}

fn single_row_mapping() -> (Mapping, MappingRow) {
    let row = MappingRow {
        kitsu_id: 1,
        imdb_id: Some("tt1".to_owned()),
        title: None,
        from_season: Some(1),
        from_episode: Some(1),
        non_imdb_episodes: None,
        imdb_episodes: None,
        tmdb_id: None,
        tvdb_id: None,
        fanart_logo_id: None,
    };
    (Mapping::new(vec![row.clone()], &[]), row)
}

fn season_videos(seasons: &[f64]) -> Vec<JsValue> {
    seasons.iter().map(|season| JsValue::from(&json!({ "season": season, "episode": 1 }))).collect()
}

#[test]
fn a_season_beyond_the_js_array_index_range_is_ignored_like_a_js_property() {
    let (mapping, row) = single_row_mapping();
    let counts = season_counts(&JsValue::from("1"), &row, &mapping, &season_videos(&[1.0, 2.0, 1e10]));
    assert_eq!(counts, vec![Some(1), Some(1)]);
}

#[test]
fn a_season_beyond_the_season_cap_is_ignored_instead_of_allocating() {
    let (mapping, row) = single_row_mapping();
    let counts = season_counts(&JsValue::from("1"), &row, &mapping, &season_videos(&[1.0, 4e9]));
    assert_eq!(counts, vec![Some(1)]);
}

#[test]
fn seasons_up_to_the_cap_keep_their_sparse_positions() {
    let (mapping, row) = single_row_mapping();
    let counts = season_counts(&JsValue::from("1"), &row, &mapping, &season_videos(&[1.0, 4.0]));
    assert_eq!(counts, vec![Some(1), None, None, Some(1)]);
}
