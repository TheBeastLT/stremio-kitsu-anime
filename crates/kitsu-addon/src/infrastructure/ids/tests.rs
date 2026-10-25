use super::*;
use crate::infrastructure::{
    http::Verb,
    testing::{FakeHttp, memory_cache},
};

const MAL_1: &str = "https://relations.yuna.moe/api/v2/ids?source=myanimelist&id=1&include=kitsu";

fn ids() -> (Ids<Arc<Error>>, Arc<FakeHttp>) {
    let http = FakeHttp::new();
    (Ids::new(http.clone(), memory_cache()), http)
}

fn message(error: &Error) -> String {
    error.to_string()
}

#[tokio::test]
async fn kitsu_ids_are_returned_without_a_request() {
    let (ids, http) = ids();
    assert_eq!(ids.to_kitsu("kitsu:7442").await.unwrap(), "7442");
    assert_eq!(http.urls().len(), 0);
}

#[tokio::test]
async fn mal_ids_query_yuna_as_myanimelist() {
    let (ids, http) = ids();
    http.reply(Verb::Get, MAL_1, 200, r#"{"anilist":1,"myanimelist":1,"kitsu":1}"#);
    assert_eq!(ids.to_kitsu("mal:1").await.unwrap(), "1");
    assert_eq!(http.urls(), vec![MAL_1]);
    assert_eq!(http.requests()[0].timeout, Some(Duration::from_secs(30)));
}

#[tokio::test]
async fn other_id_types_pass_their_source_through() {
    let (ids, http) = ids();
    let url = "https://relations.yuna.moe/api/v2/ids?source=anilist&id=21&include=kitsu";
    http.reply(Verb::Get, url, 200, r#"{"kitsu":12}"#);
    assert_eq!(ids.to_kitsu("anilist:21").await.unwrap(), "12");
    assert_eq!(http.urls(), vec![url]);
}

#[tokio::test]
async fn mappings_are_cached() {
    let (ids, http) = ids();
    http.reply(Verb::Get, MAL_1, 200, r#"{"kitsu":1}"#);
    ids.to_kitsu("mal:1").await.unwrap();
    assert_eq!(ids.to_kitsu("mal:1").await.unwrap(), "1");
    assert_eq!(http.urls().len(), 1);
}

#[tokio::test]
async fn a_missing_kitsu_id_is_not_found_and_not_cached() {
    let (ids, http) = ids();
    http.reply(Verb::Get, MAL_1, 200, r#"{"myanimelist":1,"kitsu":null}"#);
    let error = ids.to_kitsu("mal:1").await.unwrap_err();
    assert!(matches!(*error, Error::NotFound(_)));
    assert_eq!(message(&error), "No kitsu id found for: mal:1");
    ids.to_kitsu("mal:1").await.unwrap_err();
    assert_eq!(http.urls().len(), 2);
}

#[tokio::test]
async fn zero_ids_are_rejected_without_a_request() {
    let (ids, http) = ids();
    let error = ids.to_kitsu("mal:0").await.unwrap_err();
    assert_eq!(message(&error), "No kitsu id found for: mal:0");
    assert_eq!(http.urls().len(), 0);
}

#[tokio::test]
async fn negative_ids_are_rejected_without_a_request() {
    let (ids, http) = ids();
    let error = ids.to_kitsu("anidb:-3").await.unwrap_err();
    assert!(matches!(*error, Error::NotFound(_)));
    assert_eq!(http.urls().len(), 0);
}

#[tokio::test]
async fn a_yuna_error_status_is_a_failure() {
    let (ids, http) = ids();
    http.reply(Verb::Get, MAL_1, 500, "");
    let error = ids.to_kitsu("mal:1").await.unwrap_err();
    assert!(matches!(*error, Error::Failed(_)));
    assert_eq!(message(&error), "Request failed with status code 500");
}

#[tokio::test]
async fn a_transport_error_is_a_failure() {
    let (ids, http) = ids();
    http.fail(Verb::Get, MAL_1, "timeout of 30000ms exceeded");
    let error = ids.to_kitsu("mal:1").await.unwrap_err();
    assert!(matches!(*error, Error::Failed(_)));
    assert_eq!(message(&error), "timeout of 30000ms exceeded");
}
