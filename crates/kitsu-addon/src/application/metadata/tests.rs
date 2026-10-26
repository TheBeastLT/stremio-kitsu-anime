use serde_json::json;

use super::*;
use crate::{application::testing::REAL_MAPPING, infrastructure::config::DEFAULT_ADDON_URL};

fn build(anime: &Value) -> Result<Value, TypeError> {
    let images = Images::default();
    stremio_meta(&JsValue::from(anime), &images, false, &GenreUrl::new(DEFAULT_ADDON_URL))
        .map(|meta| JsValue::Object(meta).to_json())
}

fn anime(episode_count: u32, listed: &[u32]) -> Value {
    let episodes: Vec<Value> = listed
        .iter()
        .map(|number| {
            json!({
                "number": number, "titles": { "en_us": format!("Title {number}") },
                "airdate": format!("2018-01-{:02}", number + 7),
            })
        })
        .collect();
    json!({
        "id": "14095", "subtype": "TV", "canonicalTitle": "Gintama.: Shirogane no Tamashii-hen", "titles": {},
        "startDate": "2018-01-08", "episodeCount": episode_count, "genres": { "data": [] },
        "mediaRelationships": { "data": [] }, "episodes": { "data": episodes },
    })
}

fn videos(meta: &Value) -> &Vec<Value> {
    meta["videos"].as_array().unwrap()
}

#[test]
fn an_episode_list_stopping_short_of_the_episode_count_gets_the_missing_episodes_too() {
    let meta = build(&anime(12, &[1, 2, 3, 4, 5, 6, 7])).unwrap();
    let episodes: Vec<i64> = videos(&meta).iter().map(|video| video["episode"].as_i64().unwrap()).collect();
    assert_eq!(episodes, (1..=12).collect::<Vec<_>>());
    assert_eq!(videos(&meta)[7]["title"], "Episode 8");
    assert_eq!(videos(&meta)[7]["id"], "kitsu:14095:8");
    assert!(videos(&meta)[7]["released"].as_str() >= videos(&meta)[6]["released"].as_str());
}

#[test]
fn a_complete_episode_list_stays_as_kitsu_lists_it() {
    let meta = build(&anime(3, &[1, 2, 3])).unwrap();
    let titles: Vec<&str> = videos(&meta).iter().map(|video| video["title"].as_str().unwrap()).collect();
    assert_eq!(titles, ["Title 1", "Title 2", "Title 3"]);
}

#[test]
fn episode_released_dates_never_go_back_and_skip_dates_past_the_next_episode() {
    let mut anime = anime(3, &[1, 2, 3]);
    anime["episodes"]["data"][1]["airdate"] = json!("2019-05-01");
    let meta = build(&anime).unwrap();
    let released: Vec<&str> = videos(&meta).iter().map(|video| video["released"].as_str().unwrap()).collect();
    assert_eq!(released, ["2018-01-08T00:00:00.000Z", "2018-01-08T00:00:00.000Z", "2018-01-10T00:00:00.000Z"]);
}

#[test]
fn a_null_start_date_releases_episodes_at_the_epoch() {
    let mut anime = anime(1, &[]);
    anime["startDate"] = Value::Null;
    let meta = build(&anime).unwrap();
    assert_eq!(videos(&meta)[0]["released"], "1970-01-01T00:00:00.000Z");
    assert!(meta.get("releaseInfo").is_none());
}

#[test]
fn a_missing_start_date_releases_episodes_as_invalid_dates() {
    let mut anime = anime(1, &[]);
    anime.as_object_mut().unwrap().remove("startDate");
    let meta = build(&anime).unwrap();
    assert_eq!(videos(&meta)[0]["released"], Value::Null);
}

#[test]
fn a_non_iso_date_is_invalid_where_v8_would_use_its_legacy_parser() {
    let mut anime = anime(1, &[]);
    anime["startDate"] = json!("2018-1-8");
    let meta = build(&anime).unwrap();
    assert_eq!(videos(&meta)[0]["released"], Value::Null);
    assert_eq!(meta["releaseInfo"], "2018");
}

#[test]
fn a_start_date_without_leading_digits_is_a_type_error() {
    let mut anime = anime(1, &[]);
    anime["startDate"] = json!("soon");
    assert!(build(&anime).is_err());
}

#[test]
fn an_anime_without_titles_is_a_type_error() {
    let mut anime = anime(1, &[]);
    anime.as_object_mut().unwrap().remove("titles");
    assert!(build(&anime).is_err());
}

#[test]
fn a_string_episode_count_without_episodes_makes_one_video() {
    let mut anime = anime(1, &[]);
    anime["episodeCount"] = json!("12");
    let meta = build(&anime).unwrap();
    assert_eq!(videos(&meta).len(), 1);
}

#[test]
fn a_single_video_special_is_the_default_video() {
    let mut anime = anime(1, &[]);
    anime["subtype"] = json!("OVA");
    let meta = build(&anime).unwrap();
    assert_eq!(videos(&meta)[0]["id"], "kitsu:14095");
    assert_eq!(meta["behaviorHints"], json!({ "defaultVideoId": "kitsu:14095" }));
}

#[test]
fn a_movie_without_episodes_has_no_videos() {
    let mut anime = anime(1, &[]);
    anime["subtype"] = json!("movie");
    let meta = build(&anime).unwrap();
    assert_eq!(meta["type"], "movie");
    assert!(meta.get("videos").is_none());
}

#[test]
fn release_info_spans_the_airing_years() {
    let mut anime = anime(1, &[]);
    anime["endDate"] = json!("2019-03-01");
    assert_eq!(build(&anime).unwrap()["releaseInfo"], "2018-2019");
    anime["endDate"] = json!("2018-03-01");
    assert_eq!(build(&anime).unwrap()["releaseInfo"], "2018");
    anime["status"] = json!("current");
    assert_eq!(build(&anime).unwrap()["year"], "2018-");
}

#[test]
fn ratings_round_to_one_decimal_on_a_ten_point_scale() {
    assert_eq!(rounded_rating(&JsValue::from("82.47")), JsValue::from("8.2"));
    assert_eq!(rounded_rating(&JsValue::from("84.95")), JsValue::from("8.5"));
    assert_eq!(rounded_rating(&JsValue::Number(70.0)), JsValue::from("7.0"));
}

#[test]
fn a_falsy_rating_is_returned_as_is() {
    assert_eq!(rounded_rating(&JsValue::Null), JsValue::Null);
    assert_eq!(rounded_rating(&JsValue::from("")), JsValue::from(""));
    let mut anime = anime(1, &[]);
    anime["averageRating"] = json!("");
    assert_eq!(build(&anime).unwrap()["imdbRating"], "");
}

#[test]
fn aliases_drop_case_insensitive_duplicates() {
    let mut anime = anime(1, &[]);
    anime["titles"] = json!({ "en": "Naruto", "en_jp": "NARUTO" });
    anime["abbreviatedTitles"] = json!(["naruto", "NRT"]);
    assert_eq!(build(&anime).unwrap()["aliases"], json!(["Naruto", "NRT"]));
}

#[test]
fn descriptions_lose_their_source_and_note_tails() {
    let cleaned = clean_description(&JsValue::from("Story.\n\n(Source: Crunchyroll)")).unwrap();
    assert_eq!(cleaned, JsValue::from("Story."));
    let cleaned = clean_description(&JsValue::from("Story.\n\nNote: aired first\nin 2001")).unwrap();
    assert_eq!(cleaned, JsValue::from("Story."));
}

#[test]
fn franchise_links_point_at_prequels_and_sequels_with_this_entrys_type() {
    let mut anime = anime(1, &[]);
    anime["mediaRelationships"]["data"] = json!([
        { "role": "sequel", "destination": { "data": { "id": "2", "type": "anime", "canonicalTitle": "Two" } } },
        { "role": "spinoff", "destination": { "data": { "id": "3", "type": "anime", "canonicalTitle": "Three" } } },
    ]);
    let links = build(&anime).unwrap()["links"].clone();
    assert_eq!(
        links,
        json!([{ "name": "Sequel: Two", "category": "Franchise", "url": "stremio:///detail/series/kitsu:2" }])
    );
}

#[test]
fn genre_links_point_at_the_popular_catalog() {
    assert_eq!(
        GenreUrl::new(DEFAULT_ADDON_URL).url("Action"),
        "stremio:///discover/https%3A%2F%2Fanime-kitsu.strem.fun%2Fmanifest.json/anime/kitsu-anime-popular?genre=Action"
    );
}

#[test]
fn the_catalog_meta_takes_the_imdb_id_from_the_mapping() {
    let meta = json!({ "id": "kitsu:7442", "kitsu_id": "7442", "type": "series", "videos": [], "links": [] });
    let catalog = catalog_fields(&meta, &REAL_MAPPING);
    assert_eq!(
        catalog,
        json!({ "id": "kitsu:7442", "kitsu_id": "7442", "imdb_id": "tt2560140", "type": "series", "links": [] })
    );
}

#[test]
fn the_catalog_meta_keeps_nulls_of_a_cached_meta() {
    let meta = json!({ "id": "kitsu:1", "kitsu_id": "99999999", "poster": null });
    assert_eq!(
        catalog_fields(&meta, &REAL_MAPPING),
        json!({ "id": "kitsu:1", "kitsu_id": "99999999", "poster": null })
    );
}

fn quadratic_fill(id: &str, videos: &mut Vec<JsValue>, limit: f64, released: f64) {
    let mut episode = 1.0;
    while episode <= limit {
        let number = JsValue::Number(episode);
        if !videos.iter().any(|video| video.get("episode").same_value_zero(&number)) {
            videos.push(object([
                ("id", JsValue::String(format!("kitsu:{id}:{}", number.text()))),
                ("title", JsValue::String(format!("Episode {}", number.text()))),
                ("released", JsValue::Date(released)),
                ("season", JsValue::Number(1.0)),
                ("episode", number),
            ]));
        }
        episode += 1.0;
    }
}

fn listed(episodes: &[JsValue]) -> Vec<JsValue> {
    episodes.iter().map(|episode| object([("episode", episode.clone())])).collect()
}

#[test]
fn filling_missing_episodes_matches_the_scan_over_the_list() {
    let n = JsValue::Number;
    let text = |value: &str| JsValue::String(value.to_owned());
    let cases: [(Vec<JsValue>, f64); 8] = [
        (vec![], 0.0),
        (vec![], 5.0),
        (vec![n(1.0), n(2.0), n(3.0)], 3.0),
        (vec![n(2.0), n(2.0), n(5.0)], 6.0),
        (vec![n(1.5), n(3.0), n(-0.0), n(f64::NAN)], 4.0),
        (vec![text("2"), n(3.0), JsValue::Null, JsValue::Undefined], 4.0),
        (vec![n(9.0), n(1.0)], 3.5),
        (vec![n(1.0)], f64::NAN),
    ];
    for (episodes, limit) in cases {
        let mut expected = listed(&episodes);
        quadratic_fill("1", &mut expected, limit, 86_400_000.0);
        let mut actual = listed(&episodes);
        fill_missing_episodes("1", &mut actual, limit, 86_400_000.0);
        assert_eq!(JsValue::Array(actual).to_json(), JsValue::Array(expected).to_json(), "{episodes:?} up to {limit}");
    }
}

#[test]
fn a_long_episode_count_fills_in_linear_time() {
    let fastest = |count: u32| {
        let anime = anime(count, &[1]);
        (0..3)
            .map(|_| {
                let started = std::time::Instant::now();
                assert_eq!(videos(&build(&anime).unwrap()).len(), count as usize);
                started.elapsed()
            })
            .min()
            .unwrap()
    };
    let (short, long) = (fastest(600), fastest(6_000));
    assert!(long < short * 40, "600 episodes took {short:?}, 6,000 took {long:?}");
}

struct HangingImages;

#[async_trait]
impl ImageLookup for HangingImages {
    async fn images(&self, _kitsu_id: &str) -> Images {
        std::future::pending().await
    }
}

#[tokio::test(start_paused = true)]
async fn images_still_missing_at_the_meta_deadline_are_left_out() {
    let metadata =
        Metadata::new(Arc::new(Mapping::default()), Arc::new(HangingImages), GenreUrl::new(DEFAULT_ADDON_URL));
    let started = Instant::now();
    let options = EntryOptions::within(started + std::time::Duration::from_secs(20));
    let meta = metadata.to_entry_meta(&anime(2, &[1, 2]), options).await.unwrap();
    assert_eq!(started.elapsed(), std::time::Duration::from_secs(20));
    let meta = JsValue::Object(meta).to_json();
    assert_eq!(meta["name"], "Gintama.: Shirogane no Tamashii-hen");
    assert_eq!(["logo", "poster", "background"].map(|field| meta.get(field)), [None, None, None]);
}
