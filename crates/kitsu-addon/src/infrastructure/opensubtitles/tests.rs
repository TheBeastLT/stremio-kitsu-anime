use serde_json::json;

use super::*;

fn series() -> Value {
    json!({
        "imdb_id": "tt0388629",
        "videos": [
            { "episode": 1, "imdb_id": "tt0388629", "imdbSeason": 1, "imdbEpisode": 1 },
            { "episode": 2 },
            { "episode": 3, "imdb_id": "tt0388629", "imdbSeason": 2, "imdbEpisode": 5 },
        ],
    })
}

fn no_extra() -> Map<String, Value> {
    Map::new()
}

#[test]
fn movie_uses_the_meta_imdb_id() {
    let url = redirect_url(&json!({ "imdb_id": "tt0275277" }), "kitsu:2", &no_extra()).unwrap();
    assert_eq!(url, "https://opensubtitles-v3.strem.io/subtitles/movie/tt0275277.json");
}

#[test]
fn series_without_episode_uses_the_meta_imdb_id() {
    let url = redirect_url(&series(), "kitsu:12", &no_extra()).unwrap();
    assert_eq!(url, "https://opensubtitles-v3.strem.io/subtitles/series/tt0388629.json");
}

#[test]
fn episode_maps_to_the_imdb_season_and_episode() {
    let url = redirect_url(&series(), "kitsu:12:3", &no_extra()).unwrap();
    assert_eq!(url, "https://opensubtitles-v3.strem.io/subtitles/series/tt0388629:2:5.json");
}

#[test]
fn episode_number_is_parsed_like_parse_int() {
    let url = redirect_url(&series(), "kitsu:12:03", &no_extra()).unwrap();
    assert_eq!(url, "https://opensubtitles-v3.strem.io/subtitles/series/tt0388629:2:5.json");
}

#[test]
fn unknown_episode_has_no_imdb_mapping() {
    let error = redirect_url(&series(), "kitsu:12:9", &no_extra()).unwrap_err();
    assert_eq!(error.to_string(), "No imdb mapping found");
}

#[test]
fn episode_without_imdb_id_has_no_imdb_mapping() {
    let error = redirect_url(&series(), "kitsu:12:2", &no_extra()).unwrap_err();
    assert_eq!(error.to_string(), "No imdb mapping found");
}

#[test]
fn meta_without_imdb_id_has_no_imdb_mapping() {
    let error = redirect_url(&json!({ "id": "kitsu:2" }), "kitsu:2", &no_extra()).unwrap_err();
    assert_eq!(error.to_string(), "No imdb mapping found");
}

#[test]
fn episode_on_a_movie_meta_fails() {
    let error = redirect_url(&json!({ "imdb_id": "tt0275277" }), "kitsu:2:1", &no_extra()).unwrap_err();
    assert!(matches!(error, Error::Failed(_)));
}

#[test]
fn a_non_numeric_episode_falls_back_to_the_meta_imdb_id() {
    let url = redirect_url(&series(), "kitsu:12:x", &no_extra()).unwrap();
    assert_eq!(url, "https://opensubtitles-v3.strem.io/subtitles/series/tt0388629.json");
}

#[test]
fn extras_become_a_path_segment_in_order() {
    let extra = json!({ "videoHash": "abc", "videoSize": "123", "filename": "One Piece 03.mkv" });
    let url = redirect_url(&series(), "kitsu:12:3", extra.as_object().unwrap()).unwrap();
    assert_eq!(
        url,
        "https://opensubtitles-v3.strem.io/subtitles/series/tt0388629:2:5/videoHash=abc&videoSize=123&filename=One Piece 03.mkv.json"
    );
}
