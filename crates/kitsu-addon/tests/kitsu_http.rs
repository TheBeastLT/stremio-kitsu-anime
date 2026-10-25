use std::{
    fs,
    sync::{Arc, Mutex},
    time::Duration,
};

use kitsu_addon::{
    application::Error,
    infrastructure::{
        EntriesOptions, KitsuApi, Outcome, ReqwestTransport, TitlePage, UpstreamObserver, kitsu::KITSU_TIMEOUT,
    },
};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{header, method, path, query_param},
};

#[derive(Default)]
struct Observed(Mutex<Vec<(&'static str, Outcome)>>);

impl UpstreamObserver for Observed {
    fn observe(&self, upstream: &'static str, outcome: Outcome, _duration: Duration) {
        self.0.lock().unwrap().push((upstream, outcome));
    }
}

impl Observed {
    fn outcomes(&self) -> Vec<(&'static str, Outcome)> {
        self.0.lock().unwrap().clone()
    }
}

fn api(server: &MockServer, timeout: Duration) -> (KitsuApi, Arc<Observed>) {
    let observed = Arc::new(Observed::default());
    let transport = ReqwestTransport::new(&server.uri(), timeout).unwrap();
    (KitsuApi::new(Arc::new(transport), observed.clone()), observed)
}

fn recorded(name: &str) -> Vec<u8> {
    fs::read(format!("{}/tests/fixtures/kitsu/{name}.json", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

#[tokio::test]
async fn anime_data_sends_json_api_headers_and_params() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/anime/7442"))
        .and(query_param("include", "genres,episodes,mediaRelationships.destination"))
        .and(header("accept", "application/vnd.api+json"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(recorded("anime_data")))
        .expect(1)
        .mount(&server)
        .await;
    let (api, observed) = api(&server, KITSU_TIMEOUT);
    let data = api.anime_data("7442").await.unwrap();
    assert_eq!(data["canonicalTitle"], "Attack on Titan");
    assert_eq!(observed.outcomes(), vec![("kitsu", Outcome::Ok)]);
}

#[tokio::test]
async fn not_found_body_maps_to_not_found() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(404).set_body_string(
            r#"{"errors":[{"title":"Record not found","detail":"The record identified by 99999999 could not be found.","code":"404","status":"404"}]}"#,
        ))
        .mount(&server)
        .await;
    let (api, observed) = api(&server, KITSU_TIMEOUT);
    let error = api.anime_data("99999999").await.unwrap_err();
    assert!(matches!(error, Error::NotFound(_)));
    assert_eq!(observed.outcomes(), vec![("kitsu", Outcome::Error)]);
}

#[tokio::test]
async fn server_errors_carry_the_body() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(500).set_body_string("upstream down"))
        .mount(&server)
        .await;
    let (api, observed) = api(&server, KITSU_TIMEOUT);
    let error = api.anime_entries(&EntriesOptions::default()).await.unwrap_err();
    assert!(matches!(&error, Error::Failed(_)));
    assert_eq!(error.to_string(), "upstream down");
    assert_eq!(observed.outcomes(), vec![("kitsu", Outcome::Error)]);
}

#[tokio::test]
async fn slow_responses_time_out() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("{}").set_delay(Duration::from_millis(500)))
        .mount(&server)
        .await;
    let (api, observed) = api(&server, Duration::from_millis(50));
    let error = api.anime_catalog_metas(&["1".to_owned()]).await.unwrap_err();
    assert_eq!(error.to_string(), "timeout of 50ms exceeded");
    assert_eq!(observed.outcomes(), vec![("kitsu", Outcome::Error)]);
}

#[tokio::test]
async fn non_json_success_fails_after_an_ok_request() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html></html>"))
        .mount(&server)
        .await;
    let (api, observed) = api(&server, KITSU_TIMEOUT);
    let error = api.anime_entries(&EntriesOptions { trending: true, ..EntriesOptions::default() }).await.unwrap_err();
    assert!(matches!(error, Error::Failed(_)));
    assert_eq!(observed.outcomes(), vec![("kitsu", Outcome::Ok)]);
}

#[tokio::test]
async fn title_page_is_not_counted() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/anime"))
        .and(query_param("page[offset]", "40"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(recorded("title_page")))
        .mount(&server)
        .await;
    let (api, observed) = api(&server, KITSU_TIMEOUT);
    let page = api.anime_title_page(&TitlePage { offset: 40, sort: "id".to_owned() }).await.unwrap();
    assert_eq!(page.entries.len(), 20);
    assert_eq!(observed.outcomes(), vec![]);
}
