use std::{fs, sync::Mutex};

use serde_json::Value;

use super::*;

struct Fake {
    response: KitsuResponse,
    urls: Mutex<Vec<String>>,
}

#[async_trait]
impl KitsuTransport for Fake {
    async fn get(&self, path: &str, query: &str) -> Result<KitsuResponse, TransportError> {
        self.urls.lock().unwrap().push(full_url(KITSU_URL, path, query));
        Ok(self.response.clone())
    }
}

#[derive(Default)]
struct Observed(Mutex<Vec<(&'static str, Outcome)>>);

impl UpstreamObserver for Observed {
    fn observe(&self, upstream: &'static str, outcome: Outcome, _duration: Duration) {
        self.0.lock().unwrap().push((upstream, outcome));
    }
}

struct Harness {
    api: KitsuApi,
    fake: Arc<Fake>,
    observed: Arc<Observed>,
}

impl Harness {
    fn new(status: u16, body: Vec<u8>) -> Self {
        let fake = Arc::new(Fake { response: KitsuResponse { status, body }, urls: Mutex::default() });
        let observed = Arc::new(Observed::default());
        Self { api: KitsuApi::new(fake.clone(), observed.clone()), fake, observed }
    }

    fn recorded(name: &str) -> Self {
        Self::new(200, fs::read(fixture(&format!("{name}.json"))).unwrap())
    }

    fn urls(&self) -> Vec<String> {
        self.fake.urls.lock().unwrap().clone()
    }

    fn outcomes(&self) -> Vec<(&'static str, Outcome)> {
        self.observed.0.lock().unwrap().clone()
    }
}

fn fixture(name: &str) -> String {
    format!("{}/tests/fixtures/kitsu/{name}", env!("CARGO_MANIFEST_DIR"))
}

fn golden(name: &str) -> Value {
    serde_json::from_str(&fs::read_to_string(fixture(&format!("{name}.golden.json"))).unwrap()).unwrap()
}

fn golden_data(name: &str) -> Vec<Value> {
    golden(name)["document"]["data"].as_array().unwrap().clone()
}

fn assert_deserialises_like_node(name: &str) {
    let body = fs::read(fixture(&format!("{name}.json"))).unwrap();
    let deserialised = jsonapi::deserialise(jsonapi::parse(&body).unwrap()).unwrap();
    let expected = &golden(name)["document"];
    assert_eq!(&deserialised, expected);
    assert_eq!(deserialised.to_string(), expected.to_string());
}

#[test]
fn anime_data_document_deserialises_like_node() {
    assert_deserialises_like_node("anime_data");
}

#[test]
fn catalog_metas_document_deserialises_like_node() {
    assert_deserialises_like_node("catalog_metas");
}

#[test]
fn browse_document_deserialises_like_node() {
    assert_deserialises_like_node("entries_browse");
}

#[test]
fn trending_document_deserialises_like_node() {
    assert_deserialises_like_node("entries_trending");
}

#[test]
fn title_page_document_deserialises_like_node() {
    assert_deserialises_like_node("title_page");
}

#[tokio::test]
async fn anime_data_requests_like_node_and_returns_the_resource() {
    let harness = Harness::recorded("anime_data");
    let data = harness.api.anime_data("7442").await.unwrap();
    assert_eq!(harness.urls(), vec![golden("anime_data")["url"].as_str().unwrap()]);
    assert_eq!(data.to_string(), golden("anime_data")["document"]["data"].to_string());
    assert_eq!(harness.outcomes(), vec![("kitsu", Outcome::Ok)]);
}

#[tokio::test]
async fn catalog_metas_request_like_node_and_follow_the_id_order() {
    let ids: Vec<String> = [
        "1", "7442", "12", "21", "11", "13", "1376", "1555", "3936", "41370", "12268", "6448", "8271", "10740",
        "11469", "42323", "99999999",
    ]
    .map(str::to_owned)
    .to_vec();
    let harness = Harness::recorded("catalog_metas");
    let metas = harness.api.anime_catalog_metas(&ids).await.unwrap();
    assert_eq!(harness.urls(), vec![golden("catalog_metas")["url"].as_str().unwrap()]);
    let resources = golden_data("catalog_metas");
    let expected: Vec<Option<Value>> =
        ids.iter().map(|id| resources.iter().find(|resource| resource["id"] == id.as_str()).cloned()).collect();
    assert_eq!(metas, expected);
    assert_eq!(metas.last(), Some(&None));
    assert_eq!(harness.outcomes(), vec![("kitsu", Outcome::Ok)]);
}

#[tokio::test]
async fn catalog_metas_without_ids_skip_the_request() {
    let harness = Harness::new(200, Vec::new());
    assert_eq!(harness.api.anime_catalog_metas(&[]).await.unwrap(), Vec::<Option<Value>>::new());
    assert_eq!(harness.urls(), Vec::<String>::new());
}

#[tokio::test]
async fn browse_entries_request_like_node() {
    let harness = Harness::recorded("entries_browse");
    let options = EntriesOptions {
        offset: Some("20".to_owned()),
        genre: Some("Action".to_owned()),
        status: Some("current".to_owned()),
        sort: Some("-average_rating".to_owned()),
        ..EntriesOptions::default()
    };
    let entries = harness.api.anime_entries(&options).await.unwrap();
    assert_eq!(harness.urls(), vec![golden("entries_browse")["url"].as_str().unwrap()]);
    assert_eq!(entries, golden_data("entries_browse"));
    assert_eq!(harness.outcomes(), vec![("kitsu", Outcome::Ok)]);
}

#[tokio::test]
async fn trending_entries_request_like_node() {
    let harness = Harness::recorded("entries_trending");
    let options = EntriesOptions { trending: true, ..EntriesOptions::default() };
    let entries = harness.api.anime_entries(&options).await.unwrap();
    assert_eq!(harness.urls(), vec![golden("entries_trending")["url"].as_str().unwrap()]);
    assert_eq!(entries, golden_data("entries_trending"));
}

#[tokio::test]
async fn default_browse_fills_status_and_limit() {
    let harness = Harness::new(200, br#"{"data":[]}"#.to_vec());
    harness
        .api
        .anime_entries(&EntriesOptions { offset: Some(String::new()), ..EntriesOptions::default() })
        .await
        .unwrap();
    assert_eq!(
        harness.urls(),
        vec![
            "https://kitsu.io/api/edge/anime?filter%5Bsubtype%5D=TV%2COVA%2CONA%2Cmovie%2Cspecial\
             &filter%5Bstatus%5D=finished%2Ccurrent%2Cupcoming&page%5Blimit%5D=20&include=genres"
        ]
    );
}

#[tokio::test]
async fn title_page_requests_like_node_and_is_not_timed() {
    let harness = Harness::recorded("title_page");
    let page = harness.api.anime_title_page(&TitlePage::default()).await.unwrap();
    let golden = golden("title_page");
    assert_eq!(harness.urls(), vec![golden["url"].as_str().unwrap()]);
    assert_eq!(page.entries, golden_data("title_page"));
    assert_eq!(page.count, golden["document"]["meta"]["count"].as_u64());
    assert!(page.count.is_some());
    assert_eq!(harness.outcomes(), vec![]);
}

#[tokio::test]
async fn kitsu_not_found_body_maps_to_not_found() {
    let golden = golden("not_found");
    let harness = Harness::new(404, golden["rejection"].to_string().into_bytes());
    let error = harness.api.anime_data("99999999").await.unwrap_err();
    assert_eq!(harness.urls(), vec![golden["url"].as_str().unwrap()]);
    assert!(
        matches!(&error, Error::NotFound(message) if message == "The record identified by 99999999 could not be found.")
    );
    assert_eq!(harness.outcomes(), vec![("kitsu", Outcome::Error)]);
}

#[tokio::test]
async fn numeric_404_status_in_the_body_is_not_found() {
    let harness = Harness::new(404, br#"{"errors":[{"status":404,"title":"Record not found"}]}"#.to_vec());
    assert!(
        matches!(harness.api.anime_data("1").await.unwrap_err(), Error::NotFound(message) if message == "Record not found")
    );
}

#[tokio::test]
async fn http_404_without_kitsu_errors_is_a_failure() {
    let harness = Harness::new(404, b"<html>gone</html>".to_vec());
    assert!(
        matches!(harness.api.anime_data("1").await.unwrap_err(), Error::Failed(error) if error.to_string() == "<html>gone</html>")
    );
}

#[tokio::test]
async fn empty_error_body_reports_the_status() {
    let harness = Harness::new(502, Vec::new());
    let error = harness.api.anime_entries(&EntriesOptions::default()).await.unwrap_err();
    assert_eq!(error.to_string(), "Request failed with status code 502");
    assert_eq!(harness.outcomes(), vec![("kitsu", Outcome::Error)]);
}

#[tokio::test]
async fn null_data_fails_anime_data() {
    let harness = Harness::new(200, br#"{"data":null}"#.to_vec());
    assert!(matches!(harness.api.anime_data("1").await.unwrap_err(), Error::Failed(_)));
    assert_eq!(harness.outcomes(), vec![("kitsu", Outcome::Ok)]);
}
