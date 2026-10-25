use serde_json::json;

use super::*;
use crate::infrastructure::{
    http::{AXIOS_ACCEPT, AXIOS_USER_AGENT, Verb},
    testing::{FakeHttp, Observed},
};

const SERIES: &str = "https://v3-cinemeta.strem.io/meta/series/tt0388629.json";
const MOVIE: &str = "https://v3-cinemeta.strem.io/meta/movie/tt0388629.json";

fn cinemeta() -> (Cinemeta, Arc<FakeHttp>, Arc<Observed>) {
    let (http, observed) = (FakeHttp::new(), Observed::new());
    (Cinemeta::new(http.clone(), observed.clone()), http, observed)
}

fn meta_body(kind: &str) -> String {
    json!({ "meta": { "id": "tt0388629", "imdb_id": "tt0388629", "type": kind, "name": "One Piece" } }).to_string()
}

#[tokio::test]
async fn series_found_on_first_attempt() {
    let (cinemeta, http, observed) = cinemeta();
    http.reply(Verb::Get, SERIES, 200, &meta_body("series"));
    let meta = cinemeta.get("tt0388629").await;
    assert_eq!(meta["type"], "series");
    assert_eq!(http.urls(), vec![SERIES]);
    assert_eq!(observed.outcomes(), vec![("cinemeta", Outcome::Ok)]);
}

#[tokio::test]
async fn sends_axios_headers_and_sixty_second_timeout() {
    let (cinemeta, http, _) = cinemeta();
    http.reply(Verb::Get, SERIES, 200, &meta_body("series"));
    cinemeta.get("tt0388629").await;
    let request = &http.requests()[0];
    assert_eq!(request.accept, Some(AXIOS_ACCEPT));
    assert_eq!(request.user_agent, Some(AXIOS_USER_AGENT));
    assert_eq!(request.timeout, Some(Duration::from_secs(60)));
}

#[tokio::test]
async fn series_is_retried_three_times_before_falling_back_to_movie() {
    let (cinemeta, http, observed) = cinemeta();
    http.reply(Verb::Get, SERIES, 500, "");
    http.reply(Verb::Get, MOVIE, 200, &meta_body("movie"));
    let meta = cinemeta.get("tt0388629").await;
    assert_eq!(meta["type"], "movie");
    assert_eq!(http.urls(), vec![SERIES, SERIES, SERIES, SERIES, MOVIE]);
    let mut expected = vec![("cinemeta", Outcome::Error); 4];
    expected.push(("cinemeta", Outcome::Ok));
    assert_eq!(observed.outcomes(), expected);
}

#[tokio::test]
async fn a_retry_that_succeeds_stops_retrying() {
    let (cinemeta, http, observed) = cinemeta();
    http.fail(Verb::Get, SERIES, "socket hang up");
    http.reply(Verb::Get, SERIES, 200, &meta_body("series"));
    let meta = cinemeta.get("tt0388629").await;
    assert_eq!(meta["type"], "series");
    assert_eq!(http.urls(), vec![SERIES, SERIES]);
    assert_eq!(observed.outcomes(), vec![("cinemeta", Outcome::Error), ("cinemeta", Outcome::Ok)]);
}

#[tokio::test]
async fn not_found_on_both_types_resolves_the_bare_imdb_meta() {
    let (cinemeta, http, observed) = cinemeta();
    let meta = cinemeta.get("tt0388629").await;
    assert_eq!(meta, json!({ "id": "tt0388629", "imdb_id": "tt0388629" }));
    assert_eq!(http.urls(), [vec![SERIES; 4], vec![MOVIE; 4]].concat());
    assert_eq!(observed.outcomes(), vec![("cinemeta", Outcome::Error); 8]);
}

#[tokio::test]
async fn a_meta_without_imdb_id_counts_ok_but_is_retried() {
    let (cinemeta, http, observed) = cinemeta();
    http.reply(Verb::Get, SERIES, 200, r#"{"meta":{"id":"tt0388629","imdb_id":""}}"#);
    http.reply(Verb::Get, MOVIE, 200, &meta_body("movie"));
    let meta = cinemeta.get("tt0388629").await;
    assert_eq!(meta["type"], "movie");
    assert_eq!(observed.outcomes(), vec![("cinemeta", Outcome::Ok); 5]);
}

#[tokio::test]
async fn a_non_json_body_is_no_search_results() {
    let (cinemeta, http, _) = cinemeta();
    http.reply(Verb::Get, SERIES, 200, "<html>");
    http.reply(Verb::Get, MOVIE, 200, "null");
    let meta = cinemeta.get("tt0388629").await;
    assert_eq!(meta, json!({ "id": "tt0388629", "imdb_id": "tt0388629" }));
    assert_eq!(http.urls().len(), 8);
}

#[tokio::test]
async fn returns_the_meta_object_only() {
    let (cinemeta, http, _) = cinemeta();
    http.reply(Verb::Get, SERIES, 200, &meta_body("series"));
    let meta = cinemeta.get("tt0388629").await;
    assert_eq!(meta, json!({ "id": "tt0388629", "imdb_id": "tt0388629", "type": "series", "name": "One Piece" }));
}
