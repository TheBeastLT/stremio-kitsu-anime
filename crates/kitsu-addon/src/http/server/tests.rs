use std::{
    collections::HashMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, LazyLock, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use axum::http::{HeaderMap, Request};
use kitsu_search::{
    SharedIndex,
    index::Index,
    records::{Aliases, read_imdb_mapping, read_records},
};
use serde_json::json;
use tower::ServiceExt;

use super::*;
use crate::{
    application::{Mapping, TitleSearch, Upstreams},
    infrastructure::{
        Config, KitsuTransport, Store, Verb,
        http::TransportError,
        kitsu::{KITSU_URL, KitsuResponse, full_url},
        store::meta_key,
        testing::{FakeHttp, MemoryDocuments},
    },
};

const AUTHORIZATION: &str = "Basic cHJvbTpzZWNyZXQ=";
const KITSU_NOT_FOUND: &str = r#"{"errors":[{"title":"Record not found","status":"404","code":"404"}]}"#;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

fn kitsu_fixture(name: &str) -> String {
    fs::read_to_string(fixture(&format!("kitsu/{name}.json"))).unwrap()
}

fn golden_url(name: &str) -> String {
    let golden: Value = serde_json::from_str(&kitsu_fixture(&format!("{name}.golden"))).unwrap();
    golden["url"].as_str().unwrap().to_owned()
}

static DATA: LazyLock<(Arc<SharedIndex>, Arc<Mapping>)> = LazyLock::new(|| {
    let records = read_records(fs::File::open(fixture("meta/records.jsonl")).unwrap()).unwrap();
    let imdb = read_imdb_mapping(fs::File::open(fixture("meta/mapping_rows.json")).unwrap()).unwrap();
    let mapping = Mapping::load(&fixture("meta/mapping_rows.json"), &records).unwrap();
    (Arc::new(SharedIndex::new(Index::build(&records, &imdb, &Aliases::new()))), Arc::new(mapping))
});

#[derive(Default)]
struct FakeKitsu {
    resolves_on_blocking_pool: AtomicBool,
    by_url: Mutex<HashMap<String, (u16, String)>>,
    by_path: Mutex<HashMap<String, (u16, String)>>,
    urls: Mutex<Vec<String>>,
}

impl FakeKitsu {
    fn reply_url(&self, url: &str, status: u16, body: &str) {
        self.by_url.lock().unwrap().insert(url.to_owned(), (status, body.to_owned()));
    }

    fn reply_path(&self, path: &str, status: u16, body: &str) {
        self.by_path.lock().unwrap().insert(path.to_owned(), (status, body.to_owned()));
    }

    fn urls(&self) -> Vec<String> {
        self.urls.lock().unwrap().clone()
    }
}

#[async_trait]
impl KitsuTransport for FakeKitsu {
    async fn get(&self, path: &str, query: &str) -> Result<KitsuResponse, TransportError> {
        if self.resolves_on_blocking_pool.load(Ordering::SeqCst) {
            tokio::task::spawn_blocking(|| "kitsu.io resolved").await.unwrap();
        }
        let url = full_url(KITSU_URL, path, query);
        self.urls.lock().unwrap().push(url.clone());
        let by_url = self.by_url.lock().unwrap().get(&url).cloned();
        let reply = by_url.or_else(|| self.by_path.lock().unwrap().get(path).cloned());
        let (status, body) = reply.unwrap_or((404, KITSU_NOT_FOUND.to_owned()));
        Ok(KitsuResponse { status, body: body.into_bytes() })
    }
}

struct Harness {
    app: Router,
    state: Arc<AppState>,
    kitsu: Arc<FakeKitsu>,
    http: Arc<FakeHttp>,
    metrics: Metrics,
}

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: String,
}

impl Reply {
    fn header(&self, name: header::HeaderName) -> Option<&str> {
        self.headers.get(name).map(|value| value.to_str().unwrap())
    }

    fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap()
    }
}

impl Harness {
    fn new() -> Self {
        Self::with(None, Some(("prom".to_owned(), "secret".to_owned())))
    }

    fn with(store: Option<Store>, metrics_credentials: Option<(String, String)>) -> Self {
        Self::build(store, metrics_credentials, &[], None)
    }

    fn searching(search: Arc<dyn TitleSearch>, concurrency: &str, queue: &str) -> Self {
        Self::build(None, None, &[("SEARCH_CONCURRENCY", concurrency), ("SEARCH_QUEUE", queue)], Some(search))
    }

    fn build(
        store: Option<Store>,
        metrics_credentials: Option<(String, String)>,
        env: &[(&str, &str)],
        search: Option<Arc<dyn TitleSearch>>,
    ) -> Self {
        let kitsu = Arc::new(FakeKitsu::default());
        let http = FakeHttp::new();
        let metrics = Metrics::new();
        let upstreams = Upstreams { kitsu: kitsu.clone(), http: http.clone() };
        let (index, mapping) = DATA.clone();
        let config =
            Config::from_lookup(|name| env.iter().find(|(key, _)| *key == name).map(|(_, value)| (*value).to_owned()));
        let mut addon = Addon::build(&config, upstreams, store, mapping, index, metrics.clone());
        if let Some(search) = search {
            addon = addon.with_search(search);
        }
        let state = Arc::new(AppState { addon, metrics: metrics.clone(), metrics_credentials });
        Self { app: app(Arc::clone(&state)), state, kitsu, http, metrics }
    }

    async fn get(&self, path: &str) -> Reply {
        self.send(Request::get(path).body(Body::empty()).unwrap()).await
    }

    async fn send(&self, request: Request<Body>) -> Reply {
        let response = self.app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let mut decoded = String::new();
        match headers.get(header::CONTENT_ENCODING).and_then(|value| value.to_str().ok()) {
            Some("gzip") => flate2::read::GzDecoder::new(bytes.as_ref()).read_to_string(&mut decoded).unwrap(),
            Some("deflate") => flate2::read::ZlibDecoder::new(bytes.as_ref()).read_to_string(&mut decoded).unwrap(),
            Some("br") => {
                brotli_decompressor::Decompressor::new(bytes.as_ref(), 4096).read_to_string(&mut decoded).unwrap()
            }
            _ => bytes.as_ref().read_to_string(&mut decoded).unwrap(),
        };
        let body = decoded;
        Reply { status, headers, body }
    }

    fn counter(&self, series: &str) -> f64 {
        self.metrics
            .render()
            .lines()
            .find_map(|line| line.strip_prefix(series).and_then(|rest| rest.strip_prefix(' ')))
            .map_or(0.0, |value| value.parse().unwrap())
    }

    fn anime(&self, id: &str, body: &str) {
        self.kitsu.reply_path(&format!("anime/{id}"), 200, body);
    }
}

fn with_header(path: &str, name: header::HeaderName, value: &str) -> Request<Body> {
    Request::get(path).header(name, value).body(Body::empty()).unwrap()
}

#[tokio::test]
async fn manifest_equals_the_node_fixture() {
    let reply = Harness::new().get("/manifest.json").await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.body, fs::read_to_string(fixture("manifest.json")).unwrap().trim_end());
    assert_eq!(reply.header(header::CONTENT_TYPE), Some(JSON_CONTENT_TYPE));
    assert_eq!(reply.header(header::ACCESS_CONTROL_ALLOW_ORIGIN), Some("*"));
    assert_eq!(reply.header(header::CACHE_CONTROL), None);
}

#[tokio::test]
async fn manifest_is_gzipped_when_accepted() {
    let reply = Harness::new().send(with_header("/manifest.json", header::ACCEPT_ENCODING, "gzip")).await;
    assert_eq!(reply.header(header::CONTENT_ENCODING), Some("gzip"));
    assert_eq!(reply.header(header::VARY), Some("Accept-Encoding"));
    assert_eq!(reply.json()["id"], "community.anime.kitsu");
}

#[tokio::test]
async fn manifest_is_brotli_compressed_when_accepted() {
    let reply = Harness::new().send(with_header("/manifest.json", header::ACCEPT_ENCODING, "br")).await;
    assert_eq!(reply.header(header::CONTENT_ENCODING), Some("br"));
    assert_eq!(reply.header(header::VARY), Some("Accept-Encoding"));
    assert_eq!(reply.json()["id"], "community.anime.kitsu");
}

#[tokio::test]
async fn manifest_is_deflated_when_accepted() {
    let reply = Harness::new().send(with_header("/manifest.json", header::ACCEPT_ENCODING, "deflate")).await;
    assert_eq!(reply.header(header::CONTENT_ENCODING), Some("deflate"));
    assert_eq!(reply.json()["id"], "community.anime.kitsu");
}

#[tokio::test]
async fn manifest_is_plain_without_accept_encoding() {
    let reply = Harness::new().get("/manifest.json").await;
    assert_eq!(reply.header(header::CONTENT_ENCODING), None);
    assert_eq!(reply.header(header::VARY), Some("Accept-Encoding"));
}

#[tokio::test]
async fn bodies_under_a_kilobyte_are_not_compressed() {
    let reply = Harness::new().send(with_header("/subtitles/anime/foo.json", header::ACCEPT_ENCODING, "gzip")).await;
    assert_eq!(reply.header(header::CONTENT_ENCODING), None);
    assert_eq!(reply.header(header::VARY), Some("Accept-Encoding"));
}

#[tokio::test]
async fn route_matching_ignores_case_and_a_trailing_slash() {
    let harness = Harness::new();
    assert_eq!(harness.get("/MANIFEST.JSON").await.status, StatusCode::OK);
    assert_eq!(harness.get("/manifest.json/").await.status, StatusCode::OK);
}

#[tokio::test]
async fn head_answers_headers_without_a_body() {
    let reply = Harness::new().send(Request::head("/manifest.json").body(Body::empty()).unwrap()).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.header(header::CONTENT_TYPE), Some(JSON_CONTENT_TYPE));
    assert_eq!(reply.body, "");
}

#[tokio::test]
async fn options_is_a_cors_preflight() {
    let request = Request::options("/catalog/anime/kitsu-anime-trending.json")
        .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "x-custom")
        .body(Body::empty())
        .unwrap();
    let reply = Harness::new().send(request).await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT);
    assert_eq!(reply.header(header::ACCESS_CONTROL_ALLOW_ORIGIN), Some("*"));
    assert_eq!(reply.header(header::ACCESS_CONTROL_ALLOW_METHODS), Some("GET,HEAD,PUT,PATCH,POST,DELETE"));
    assert_eq!(reply.header(header::ACCESS_CONTROL_ALLOW_HEADERS), Some("x-custom"));
    assert_eq!(reply.header(header::VARY), Some("Access-Control-Request-Headers"));
}

#[tokio::test]
async fn other_methods_are_not_found() {
    let reply = Harness::new().send(Request::post("/manifest.json").body(Body::empty()).unwrap()).await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    assert_eq!(reply.body, NOT_FOUND);
}

#[tokio::test]
async fn landing_is_the_install_page() {
    let reply = Harness::new().get("/").await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.header(header::CONTENT_TYPE), Some("text/html"));
    assert!(reply.body.contains("Anime Kitsu"));
}

#[tokio::test]
async fn configure_is_not_found_like_node() {
    assert_eq!(Harness::new().get("/configure").await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn unmatched_paths_are_not_found_without_a_content_type() {
    let reply = Harness::new().get("/nothing/here.json").await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    assert_eq!(reply.body, NOT_FOUND);
    assert_eq!(reply.header(header::CONTENT_TYPE), None);
    assert_eq!(reply.header(header::ACCESS_CONTROL_ALLOW_ORIGIN), Some("*"));
}

#[tokio::test]
async fn browse_catalogs_carry_the_cache_max_age() {
    let harness = Harness::new();
    harness.kitsu.reply_url(&golden_url("entries_trending"), 200, &kitsu_fixture("entries_trending"));
    let reply = harness.get("/catalog/anime/kitsu-anime-trending.json").await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.header(header::CACHE_CONTROL), Some("max-age=43200, public"));
    assert_eq!(reply.json()["metas"].as_array().unwrap().len(), 50);
    assert_eq!(reply.json()["cacheMaxAge"], 43200);
}

#[tokio::test]
async fn browse_catalogs_are_cached_and_ignore_a_timestamp_query() {
    let harness = Harness::new();
    harness.kitsu.reply_url(&golden_url("entries_trending"), 200, &kitsu_fixture("entries_trending"));
    let first = harness.get("/catalog/anime/kitsu-anime-trending.json").await;
    let second = harness.get("/catalog/anime/kitsu-anime-trending.json?1700000000").await;
    assert_eq!(first.body, second.body);
    assert_eq!(harness.kitsu.urls().len(), 1);
    assert_eq!(harness.counter("kitsu_cache_requests_total{kind=\"catalog\",result=\"miss\"}"), 1.0);
    assert_eq!(harness.counter("kitsu_cache_requests_total{kind=\"catalog\",result=\"hit\"}"), 1.0);
}

#[tokio::test]
async fn genre_and_skip_extras_reach_kitsu_like_node() {
    let harness = Harness::new();
    harness.kitsu.reply_url(&golden_url("entries_browse"), 200, &kitsu_fixture("entries_browse"));
    let reply = harness.get("/catalog/anime/kitsu-anime-airing/genre=Action&skip=20.json").await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(harness.kitsu.urls(), [golden_url("entries_browse")]);
}

#[tokio::test]
async fn an_encoded_space_in_a_genre_is_decoded() {
    let harness = Harness::new();
    harness.get("/catalog/anime/kitsu-anime-popular/genre=Slice%20of%20Life.json").await;
    assert!(harness.kitsu.urls()[0].contains("filter%5Bgenres%5D=Slice%20of%20Life"));
}

#[tokio::test]
async fn a_non_numeric_skip_is_passed_to_kitsu_as_is() {
    let harness = Harness::new();
    harness.kitsu.reply_path("anime", 400, r#"{"errors":[{"title":"Invalid field value","status":"400"}]}"#);
    let reply = harness.get("/catalog/anime/kitsu-anime-airing/skip=abc.json").await;
    assert!(harness.kitsu.urls()[0].contains("page%5Boffset%5D=abc"));
    assert_eq!(reply.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(reply.body, HANDLER_ERROR);
}

#[tokio::test]
async fn unknown_extra_keys_are_ignored() {
    let harness = Harness::new();
    harness.kitsu.reply_path("anime", 200, &kitsu_fixture("entries_browse"));
    let reply = harness.get("/catalog/anime/kitsu-anime-rating/unknown=1&other=2.json").await;
    assert_eq!(reply.status, StatusCode::OK);
    assert!(!harness.kitsu.urls()[0].contains("unknown"));
}

#[tokio::test]
async fn an_unknown_catalog_is_not_found_and_not_counted() {
    let harness = Harness::new();
    let reply = harness.get("/catalog/anime/kitsu-anime-unknown/search=one.json").await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    assert_eq!(reply.body, NOT_FOUND);
    assert_eq!(reply.header(header::CONTENT_TYPE), Some(JSON_CONTENT_TYPE));
    assert!(!harness.metrics.render().contains("kitsu_catalog_requests_total{"));
}

#[tokio::test]
async fn a_url_as_search_term_is_a_handler_error() {
    let harness = Harness::new();
    let reply = harness.get("/catalog/anime/kitsu-anime-list/search=https%3A%2F%2Fexample.com.json").await;
    assert_eq!(reply.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(reply.body, HANDLER_ERROR);
    assert_eq!(harness.counter("kitsu_catalog_requests_total{catalog=\"kitsu-anime-list\",kind=\"search\"}"), 1.0);
    assert!(!harness.metrics.render().contains("kitsu_search_requests_total{"));
}

#[tokio::test]
async fn a_repeated_search_key_is_not_found_like_the_sdk_router() {
    let reply = Harness::new().get("/catalog/anime/kitsu-anime-list/search=a&search=b.json").await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_malformed_escape_in_a_path_param_is_a_bad_request() {
    let harness = Harness::new();
    let reply = harness.get("/catalog/anime/kitsu-anime-list/search=%E0%A4%A.json").await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST);
    assert_eq!(reply.body, "");
    assert_eq!(reply.header(header::CONTENT_TYPE), None);
    assert!(!harness.metrics.render().contains("kitsu_catalog_requests_total{"));
}

#[tokio::test]
async fn an_empty_search_browses_the_catalog() {
    let harness = Harness::new();
    harness.kitsu.reply_path("anime", 200, &kitsu_fixture("entries_browse"));
    harness.get("/catalog/anime/kitsu-anime-list/search=.json").await;
    assert!(harness.kitsu.urls()[0].contains("sort=createdAt"));
    assert_eq!(harness.counter("kitsu_catalog_requests_total{catalog=\"kitsu-anime-list\",kind=\"browse\"}"), 1.0);
}

#[tokio::test]
async fn a_search_fetches_the_found_metas_in_one_kitsu_request() {
    let harness = Harness::new();
    harness.kitsu.reply_path("anime", 200, &kitsu_fixture("catalog_metas"));
    let reply = harness.get("/catalog/anime/kitsu-anime-list/search=cowboy%20bebop.json").await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(
        harness.kitsu.urls(),
        [format!("{KITSU_URL}/anime?filter%5Bid%5D=1%2C2&include=genres&page%5Blimit%5D=20")]
    );
    let body = reply.json();
    let ids: Vec<&str> = body["metas"].as_array().unwrap().iter().map(|meta| meta["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["kitsu:1"]);
    assert_eq!(harness.counter("kitsu_search_requests_total{result=\"found\"}"), 1.0);
}

#[tokio::test]
async fn a_search_serves_cached_metas_without_asking_kitsu_again() {
    let harness = Harness::new();
    harness.anime("7442", &kitsu_fixture("anime_data"));
    harness.get("/meta/anime/kitsu:7442.json").await;
    harness.kitsu.reply_path("anime", 200, r#"{"data":[]}"#);
    let reply = harness.get("/catalog/anime/kitsu-anime-list/search=attack%20on%20titan.json").await;
    let metas = reply.json()["metas"].as_array().unwrap().clone();
    assert!(metas.iter().any(|meta| meta["id"] == "kitsu:7442"));
    let search_request = harness.kitsu.urls().into_iter().find(|url| url.contains("filter%5Bid%5D")).unwrap();
    assert!(!search_request.contains("7442"));
}

#[tokio::test]
async fn an_empty_search_result_asks_kitsu_nothing() {
    let harness = Harness::new();
    let reply = harness.get("/catalog/anime/kitsu-anime-list/search=zzzzqqqq.json").await;
    assert_eq!(reply.body, r#"{"metas":[],"cacheMaxAge":43200}"#);
    assert_eq!(harness.kitsu.urls(), Vec::<String>::new());
    assert_eq!(harness.counter("kitsu_search_requests_total{result=\"empty\"}"), 1.0);
}

#[tokio::test]
async fn last_videos_reads_at_most_twenty_ids_and_twenty_videos() {
    let harness = Harness::new();
    harness.anime("7442", &kitsu_fixture("anime_data"));
    let ids: Vec<String> =
        (100..119).map(|id| format!("kitsu:{id}")).chain(["kitsu:7442".to_owned(), "kitsu:1".to_owned()]).collect();
    let reply =
        harness.get(&format!("/catalog/series/kitsu-anime-last-videos/lastVideosIds={}.json", ids.join(","))).await;
    assert_eq!(reply.status, StatusCode::OK);
    let detailed = reply.json()["metasDetailed"].as_array().unwrap().clone();
    assert_eq!(detailed.len(), 1);
    assert_eq!(detailed[0]["id"], "kitsu:7442");
    let videos = detailed[0]["videos"].as_array().unwrap();
    assert_eq!(videos.len(), 20);
    assert_eq!(videos[19]["episode"], 25);
    assert!(!harness.kitsu.urls().iter().any(|url| url.contains("anime/1?")));
    assert_eq!(harness.kitsu.urls().len(), 20);
}

#[tokio::test]
async fn a_repeated_last_videos_key_is_not_found() {
    let reply = Harness::new()
        .get("/catalog/series/kitsu-anime-last-videos/lastVideosIds=kitsu:1&lastVideosIds=kitsu:2.json")
        .await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_kitsu_meta_is_served_with_the_cache_max_age() {
    let harness = Harness::new();
    harness.anime("7442", &kitsu_fixture("anime_data"));
    let reply = harness.get("/meta/anime/kitsu:7442.json").await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.header(header::CACHE_CONTROL), Some("max-age=43200, public"));
    assert_eq!(reply.json()["meta"]["id"], "kitsu:7442");
    assert_eq!(harness.counter("kitsu_meta_requests_total{id_type=\"kitsu\",outcome=\"ok\"}"), 1.0);
}

#[tokio::test]
async fn an_encoded_meta_id_is_decoded() {
    let harness = Harness::new();
    harness.anime("7442", &kitsu_fixture("anime_data"));
    assert_eq!(harness.get("/meta/anime/kitsu%3A7442.json").await.status, StatusCode::OK);
}

#[tokio::test]
async fn a_cached_meta_is_served_verbatim_with_its_own_cache_max_age() {
    let documents = Arc::new(MemoryDocuments::default());
    let store = Store::new(documents, Duration::from_secs(1));
    let cached = json!({ "meta": { "id": "kitsu:5", "name": null, "videos": [] }, "cacheMaxAge": 60 });
    store.set(&meta_key("5"), &cached, Duration::from_secs(3600)).await.unwrap();
    let harness = Harness::with(Some(store), None);
    let reply = harness.get("/meta/anime/kitsu:5.json").await;
    assert_eq!(reply.body, r#"{"meta":{"id":"kitsu:5","name":null,"videos":[]},"cacheMaxAge":60}"#);
    assert_eq!(reply.header(header::CACHE_CONTROL), Some("max-age=60, public"));
    assert_eq!(harness.kitsu.urls(), Vec::<String>::new());
}

#[tokio::test]
async fn an_invalid_meta_id_is_not_found_and_not_counted() {
    let harness = Harness::new();
    let reply = harness.get("/meta/anime/foo.json").await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    assert_eq!(reply.body, NOT_FOUND);
    assert!(!harness.metrics.render().contains("kitsu_meta_requests_total{"));
}

#[tokio::test]
async fn an_imdb_id_without_mapping_is_not_found() {
    let harness = Harness::new();
    assert_eq!(harness.get("/meta/series/tt0944947.json").await.status, StatusCode::NOT_FOUND);
    assert_eq!(harness.counter("kitsu_meta_requests_total{id_type=\"imdb\",outcome=\"not_found\"}"), 1.0);
    assert_eq!(harness.http.urls(), Vec::<String>::new());
}

#[tokio::test]
async fn an_id_unknown_to_kitsu_is_not_found() {
    let harness = Harness::new();
    assert_eq!(harness.get("/meta/anime/kitsu:99999999.json").await.status, StatusCode::NOT_FOUND);
    assert_eq!(harness.counter("kitsu_meta_requests_total{id_type=\"kitsu\",outcome=\"not_found\"}"), 1.0);
}

#[tokio::test]
async fn an_id_without_kitsu_mapping_is_not_found() {
    let harness = Harness::new();
    let yuna = "https://relations.yuna.moe/api/v2/ids?source=myanimelist&id=7&include=kitsu";
    harness.http.reply(Verb::Get, yuna, 200, r#"{"kitsu":null}"#);
    assert_eq!(harness.get("/meta/anime/mal:7.json").await.status, StatusCode::NOT_FOUND);
    assert_eq!(harness.counter("kitsu_meta_requests_total{id_type=\"kitsu\",outcome=\"not_found\"}"), 1.0);
}

#[tokio::test]
async fn a_zero_id_is_not_found_without_asking_yuna() {
    let harness = Harness::new();
    assert_eq!(harness.get("/meta/anime/mal:0.json").await.status, StatusCode::NOT_FOUND);
    assert_eq!(harness.http.urls(), Vec::<String>::new());
}

#[tokio::test]
async fn an_upstream_failure_is_a_handler_error() {
    let harness = Harness::new();
    harness.kitsu.reply_path("anime/7442", 503, "<html>down</html>");
    let reply = harness.get("/meta/anime/kitsu:7442.json").await;
    assert_eq!(reply.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(reply.body, HANDLER_ERROR);
    assert_eq!(reply.header(header::CONTENT_TYPE), Some(JSON_CONTENT_TYPE));
    assert_eq!(harness.counter("kitsu_meta_requests_total{id_type=\"kitsu\",outcome=\"error\"}"), 1.0);
}

#[tokio::test]
async fn a_yuna_failure_is_a_handler_error() {
    let harness = Harness::new();
    let yuna = "https://relations.yuna.moe/api/v2/ids?source=anilist&id=5&include=kitsu";
    harness.http.reply(Verb::Get, yuna, 500, "down");
    assert_eq!(harness.get("/meta/anime/anilist:5.json").await.status, StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn an_imdb_meta_whose_kitsu_entry_is_gone_is_a_handler_error_counted_as_not_found() {
    let harness = Harness::new();
    let reply = harness.get("/meta/series/tt2560140.json").await;
    assert_eq!(reply.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(harness.counter("kitsu_meta_requests_total{id_type=\"imdb\",outcome=\"not_found\"}"), 1.0);
}

#[tokio::test]
async fn subtitles_redirect_to_opensubtitles_with_the_extras() {
    let harness = Harness::new();
    harness.anime("7442", &kitsu_fixture("anime_data"));
    let reply =
        harness.get("/subtitles/anime/kitsu:7442:3/filename=Attack%20on%20Titan%2003.mkv&videoSize=123.json").await;
    assert_eq!(reply.status, StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(
        reply.header(header::LOCATION),
        Some(
            "https://opensubtitles-v3.strem.io/subtitles/series/tt2560140:1:3/filename=Attack%20on%20Titan%2003.mkv&videoSize=123.json"
        )
    );
    assert_eq!(reply.header(header::VARY), Some("Accept, Accept-Encoding"));
    assert_eq!(reply.header(header::CACHE_CONTROL), None);
}

#[tokio::test]
async fn subtitles_fall_back_to_an_empty_list() {
    let reply = Harness::new().get("/subtitles/anime/kitsu:99999999.json").await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.body, r#"{"subtitles":[]}"#);
}

#[tokio::test]
async fn subtitles_of_an_invalid_id_are_not_found() {
    assert_eq!(Harness::new().get("/subtitles/anime/tt1.json").await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn search_returns_ids_with_the_cache_max_age() {
    let harness = Harness::new();
    let reply = harness.get("/search/cowboy%20bebop.json").await;
    assert_eq!(reply.body, r#"{"ids":["1","2"]}"#);
    assert_eq!(reply.header(header::CACHE_CONTROL), Some("max-age=43200, public"));
    assert_eq!(reply.header(header::ACCESS_CONTROL_ALLOW_ORIGIN), Some("*"));
    assert_eq!(harness.counter("kitsu_search_requests_total{result=\"found\"}"), 1.0);
}

#[tokio::test]
async fn search_of_an_imdb_id_returns_twenty_mapped_ids() {
    let reply = Harness::new().get("/search/tt0388629.json").await;
    assert_eq!(reply.json()["ids"].as_array().unwrap().len(), 20);
}

#[tokio::test]
async fn search_spans_slashes_in_the_query() {
    assert_eq!(Harness::new().get("/search/a/b.json").await.status, StatusCode::OK);
}

#[tokio::test]
async fn a_malformed_search_is_a_bad_request_and_not_counted() {
    let harness = Harness::new();
    let reply = harness.get("/search/%E0%A4%A.json").await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST);
    assert_eq!(reply.body, "");
    assert!(!harness.metrics.render().contains("kitsu_search_requests_total{"));
}

#[tokio::test]
async fn metrics_are_not_found_without_credentials_configured() {
    let harness = Harness::with(None, None);
    let reply = harness.send(with_header("/metrics", header::AUTHORIZATION, AUTHORIZATION)).await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn metrics_need_the_right_credentials() {
    let reply = Harness::new().send(with_header("/metrics", header::AUTHORIZATION, "Basic cHJvbTp3cm9uZw==")).await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
    assert_eq!(reply.header(header::WWW_AUTHENTICATE), Some("Basic realm=\"metrics\""));
    assert_eq!(reply.header(header::ACCESS_CONTROL_ALLOW_ORIGIN), None);
}

#[tokio::test]
async fn metrics_count_requests_by_route_and_status() {
    let harness = Harness::new();
    harness.get("/manifest.json").await;
    harness.get("/meta/anime/foo.json").await;
    harness.get("/nothing").await;
    let reply = harness.send(with_header("/metrics", header::AUTHORIZATION, AUTHORIZATION)).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.header(header::CONTENT_TYPE), Some(METRICS_CONTENT_TYPE));
    assert!(reply.body.contains("kitsu_http_requests_total{route=\"manifest\",status=\"200\"} 1"));
    assert!(reply.body.contains("kitsu_http_requests_total{route=\"meta\",status=\"404\"} 1"));
    assert!(reply.body.contains("kitsu_http_requests_total{route=\"other\",status=\"404\"} 1"));
}

#[tokio::test]
async fn a_metrics_path_with_a_trailing_slash_is_not_the_endpoint() {
    let reply = Harness::new().send(with_header("/metrics/", header::AUTHORIZATION, AUTHORIZATION)).await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    assert_eq!(reply.body, NOT_FOUND);
}

#[test]
fn cache_control_lists_every_integer_cache_field() {
    let value = json!({ "cacheMaxAge": 3600, "staleRevalidate": 60.0, "staleError": 1.5 });
    assert_eq!(cache_control(&value).as_deref(), Some("max-age=3600, stale-while-revalidate=60, public"));
}

#[test]
fn cache_control_is_absent_without_cache_fields() {
    assert_eq!(cache_control(&json!({ "subtitles": [] })), None);
}

#[derive(Default)]
struct BlockedSearch {
    open: Mutex<bool>,
    changed: Condvar,
    started: AtomicUsize,
}

impl BlockedSearch {
    fn release(&self) {
        *self.open.lock().unwrap() = true;
        self.changed.notify_all();
    }
}

impl TitleSearch for BlockedSearch {
    fn ids(&self, _query: &str) -> Vec<String> {
        self.started.fetch_add(1, Ordering::SeqCst);
        let mut open = self.open.lock().unwrap();
        while !*open {
            open = self.changed.wait(open).unwrap();
        }
        vec!["1".to_owned()]
    }
}

struct PanickingSearch;

impl TitleSearch for PanickingSearch {
    fn ids(&self, _query: &str) -> Vec<String> {
        panic!("index broken")
    }
}

struct Released(Arc<BlockedSearch>);

impl Drop for Released {
    fn drop(&mut self) {
        self.0.release();
    }
}

async fn until(condition: impl Fn() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(tokio::time::Instant::now() < deadline, "condition not reached");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

#[tokio::test]
async fn a_failing_search_is_a_server_error_with_an_empty_body() {
    let harness = Harness::searching(Arc::new(PanickingSearch), "2", "32");
    let reply = harness.get("/search/naruto.json").await;
    assert_eq!(reply.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(reply.body, "");
    assert!(!harness.metrics.render().contains("kitsu_search_requests_total{"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_search_beyond_the_queue_is_unavailable_and_counted_only_as_http() {
    let search = Arc::new(BlockedSearch::default());
    let _released = Released(Arc::clone(&search));
    let harness = Arc::new(Harness::searching(search.clone(), "1", "1"));
    let running = tokio::spawn({
        let harness = Arc::clone(&harness);
        async move { harness.get("/search/a.json").await.status }
    });
    until(|| search.started.load(Ordering::SeqCst) == 1).await;
    let queued = tokio::spawn({
        let harness = Arc::clone(&harness);
        async move { harness.get("/search/b.json").await.status }
    });
    until(|| harness.state.addon.search_gate().waiting() == 1).await;
    let overflow = harness.get("/search/c.json").await;
    assert_eq!(overflow.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(overflow.body, "");
    search.release();
    assert_eq!(running.await.unwrap(), StatusCode::OK);
    assert_eq!(queued.await.unwrap(), StatusCode::OK);
    let metrics = harness.metrics.render();
    assert!(metrics.contains("kitsu_http_requests_total{route=\"search\",status=\"503\"} 1"), "{metrics}");
    assert!(metrics.contains("kitsu_search_requests_total{result=\"found\"} 2"), "{metrics}");
}

#[test]
fn queued_searches_leave_cheap_routes_and_kitsu_lookups_on_the_blocking_pool_free() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(3)
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let search = Arc::new(BlockedSearch::default());
        let _released = Released(Arc::clone(&search));
        let harness = Arc::new(Harness::searching(search.clone(), "2", "32"));
        harness.kitsu.resolves_on_blocking_pool.store(true, Ordering::SeqCst);
        let searches: Vec<_> = (0..6)
            .map(|index| {
                let harness = Arc::clone(&harness);
                tokio::spawn(async move { harness.get(&format!("/search/q{index}.json")).await.status })
            })
            .collect();
        until(|| harness.state.addon.search_gate().waiting() == 4).await;
        assert_eq!(search.started.load(Ordering::SeqCst), 2);
        let manifest = tokio::time::timeout(Duration::from_secs(2), harness.get("/manifest.json")).await.unwrap();
        assert_eq!(manifest.status, StatusCode::OK);
        let meta =
            tokio::time::timeout(Duration::from_secs(2), harness.get("/meta/anime/kitsu:99999999.json")).await.unwrap();
        assert_eq!(meta.status, StatusCode::NOT_FOUND);
        search.release();
        for task in searches {
            assert_eq!(task.await.unwrap(), StatusCode::OK);
        }
    });
}

#[tokio::test]
async fn a_panicking_handler_is_a_counted_server_error_and_the_service_survives() {
    let harness = Harness::new();
    let state = Arc::new(AppState {
        addon: {
            let (index, mapping) = DATA.clone();
            let upstreams = Upstreams { kitsu: harness.kitsu.clone(), http: harness.http.clone() };
            Addon::build(&Config::from_lookup(|_| None), upstreams, None, mapping, index, harness.metrics.clone())
        },
        metrics: harness.metrics.clone(),
        metrics_credentials: None,
    });
    let routes = Router::new().route("/boom", axum::routing::get(boom)).fallback(dispatch);
    let service = layered(routes, state);
    let response = service.clone().oneshot(Request::get("/boom").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(bytes.len(), 0);
    let after = service.oneshot(Request::get("/manifest.json").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(after.status(), StatusCode::OK);
    assert!(harness.metrics.render().contains("kitsu_http_requests_total{route=\"other\",status=\"500\"} 1"));
}

async fn boom() -> StatusCode {
    panic!("handler broken")
}
