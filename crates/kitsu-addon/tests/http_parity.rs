use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use axum::{
    Router,
    body::Body,
    http::{Request, header},
};
use kitsu_addon::{
    application::{Addon, GenreUrl, ImageLookup, IndexSearch, Mapping, Metadata, Parts, SearchGate},
    http::{AppState, app},
    infrastructure::{
        Cache, CacheSettings, Cinemeta, Config, HttpRequest, HttpResponse, HttpTransport, Ids, Images, KitsuApi,
        KitsuTransport, Metrics,
        http::TransportError,
        kitsu::{KITSU_URL, KitsuResponse, full_url},
    },
};
use kitsu_search::{
    SharedIndex,
    index::Index,
    records::{Aliases, read_imdb_mapping, read_records},
};
use serde_json::Value;
use tower::ServiceExt;

const IGNORED_NODE_HEADERS: [&str; 6] =
    ["date", "connection", "keep-alive", "x-powered-by", "transfer-encoding", "content-length"];

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

static RECORDED: LazyLock<Value> = LazyLock::new(|| {
    let bytes = zstd::decode_all(File::open(fixture("http/exchanges.json.zst")).unwrap()).unwrap();
    serde_json::from_slice(&bytes).unwrap()
});

#[derive(Default)]
struct Recorded {
    unknown: Mutex<Vec<String>>,
}

impl Recorded {
    fn answer(&self, kind: &str, url: &str) -> Option<(u16, Vec<u8>)> {
        let Some(answer) = RECORDED["upstream"][kind].get(url) else {
            self.unknown.lock().unwrap().push(url.to_owned());
            return None;
        };
        let status = u16::try_from(answer["status"].as_u64().unwrap()).unwrap();
        Some((status, answer["body"].as_str().unwrap().as_bytes().to_vec()))
    }
}

#[async_trait]
impl KitsuTransport for Recorded {
    async fn get(&self, path: &str, query: &str) -> Result<KitsuResponse, TransportError> {
        let url = full_url(KITSU_URL, path, query);
        let (status, body) = self.answer("kitsu", &url).ok_or_else(|| TransportError(format!("unrecorded {url}")))?;
        Ok(KitsuResponse { status, body })
    }
}

#[async_trait]
impl HttpTransport for Recorded {
    async fn send(&self, request: &HttpRequest) -> Result<HttpResponse, TransportError> {
        let (status, body) =
            self.answer("http", &request.url).ok_or_else(|| TransportError(format!("unrecorded {}", request.url)))?;
        Ok(HttpResponse { status, body })
    }
}

struct SyntheticImages;

#[async_trait]
impl ImageLookup for SyntheticImages {
    async fn images(&self, kitsu_id: &str) -> Images {
        let base = format!("https://images.test/{kitsu_id}");
        match kitsu_id.parse::<u64>().unwrap() % 4 {
            0 => Images {
                logo: Some(format!("{base}/logo.png")),
                logo_lang: Some("en".to_owned()),
                poster: Some(format!("{base}/poster.jpg")),
                background: Some(format!("{base}/background.jpg")),
                thumbnail: None,
            },
            2 => Images { background: Some(format!("{base}/background.jpg")), ..Images::default() },
            _ => Images::default(),
        }
    }
}

fn node_app(upstream: &Arc<Recorded>) -> Router {
    let config = Config::from_lookup(|_| None);
    let records = read_records(File::open(fixture("meta/records.jsonl")).unwrap()).unwrap();
    let mapping = Arc::new(Mapping::load(&fixture("meta/mapping_rows.json"), &records).unwrap());
    let imdb = read_imdb_mapping(File::open(fixture("meta/mapping_rows.json")).unwrap()).unwrap();
    let search = Arc::new(SharedIndex::new(Index::build(&records, &imdb, &Aliases::new())));
    let metrics = Metrics::new();
    let settings = CacheSettings {
        enabled: true,
        budget_bytes: 64 * 1024 * 1024,
        meta_ttl: Duration::from_secs(3600),
        catalog_ttl: Duration::from_secs(3600),
    };
    let cache = Arc::new(Cache::new(settings, None, Arc::new(metrics.clone())));
    let http: Arc<dyn HttpTransport> = upstream.clone();
    let addon = Addon::new(Parts {
        kitsu: KitsuApi::new(upstream.clone(), Arc::new(metrics.clone())),
        cinemeta: Cinemeta::new(Arc::clone(&http), Arc::new(metrics.clone())),
        ids: Ids::new(http, Arc::clone(&cache)),
        cache,
        metadata: Metadata::new(Arc::clone(&mapping), Arc::new(SyntheticImages), GenreUrl::new(&config.addon_url)),
        search: Arc::new(IndexSearch { index: search, mapping: Arc::clone(&mapping) }),
        mapping,
        gate: SearchGate::new(config.search_concurrency, config.search_queue),
        metrics: metrics.clone(),
        cache_max_age: config.cache_max_age,
    });
    app(Arc::new(AppState { addon, metrics, metrics_credentials: Some(("prom".to_owned(), "secret".to_owned())) }))
}

fn request(method: &str, path: &str, headers: &serde_json::Map<String, Value>) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(path);
    for (name, value) in headers {
        builder = builder.header(name.as_str(), value.as_str().unwrap());
    }
    builder.body(Body::empty()).unwrap()
}

fn node_headers(exchange: &Value) -> BTreeMap<String, String> {
    exchange["headers"]
        .as_object()
        .unwrap()
        .iter()
        .filter(|(name, _)| !IGNORED_NODE_HEADERS.contains(&name.as_str()))
        .map(|(name, value)| (name.clone(), value.as_str().unwrap().to_owned()))
        .collect()
}

fn rust_headers(headers: &axum::http::HeaderMap) -> BTreeMap<String, String> {
    headers
        .iter()
        .filter(|(name, _)| !IGNORED_NODE_HEADERS.contains(&name.as_str()))
        .map(|(name, value)| (name.as_str().to_owned(), value.to_str().unwrap().to_owned()))
        .collect()
}

fn gunzip(bytes: &[u8]) -> Vec<u8> {
    let mut decoded = Vec::new();
    flate2::read::GzDecoder::new(bytes).read_to_end(&mut decoded).unwrap();
    decoded
}

fn counters(lines: impl Iterator<Item = String>) -> BTreeSet<(String, BTreeSet<String>, String)> {
    lines
        .map(|line| {
            let (series, value) = line.rsplit_once(' ').unwrap();
            let (name, labels) = series.split_once('{').unwrap();
            let labels = labels.trim_end_matches('}').split(',').map(str::to_owned).collect();
            (name.to_owned(), labels, value.to_owned())
        })
        .collect()
}

const COUNTER_FAMILIES: [&str; 6] = [
    "kitsu_http_requests_total{",
    "kitsu_cache_requests_total{",
    "kitsu_catalog_requests_total{",
    "kitsu_search_requests_total{",
    "kitsu_meta_requests_total{",
    "kitsu_upstream_request_duration_seconds_count{",
];

#[tokio::test]
async fn responses_equal_the_node_app() {
    let upstream = Arc::new(Recorded::default());
    let app = node_app(&upstream);
    let mut differences = Vec::new();
    for exchange in RECORDED["exchanges"].as_array().unwrap() {
        let method = exchange["method"].as_str().unwrap();
        let path = exchange["path"].as_str().unwrap();
        let headers = exchange["requestHeaders"].as_object().unwrap();
        let response = app.clone().oneshot(request(method, path, headers)).await.unwrap();
        let status = response.status().as_u16();
        let actual_headers = rust_headers(response.headers());
        let gzip = response.headers().get(header::CONTENT_ENCODING).is_some_and(|value| value == "gzip");
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body = String::from_utf8(if gzip { gunzip(&bytes) } else { bytes.to_vec() }).unwrap();
        let expected_status = u16::try_from(exchange["status"].as_u64().unwrap()).unwrap();
        if status != expected_status {
            differences.push(format!("{method} {path}: status {status}, node {expected_status}"));
        }
        if actual_headers != node_headers(exchange) {
            differences.push(format!("{method} {path}: headers {actual_headers:?}, node {:?}", node_headers(exchange)));
        }
        if body != exchange["body"].as_str().unwrap() {
            let expected = exchange["body"].as_str().unwrap();
            let position = body.bytes().zip(expected.bytes()).position(|(a, b)| a != b).unwrap_or(0);
            differences.push(format!(
                "{method} {path}: body differs at {position}: {:?} / node {:?}",
                body.get(position.saturating_sub(80)..(position + 120).min(body.len())),
                expected.get(position.saturating_sub(80)..(position + 120).min(expected.len()))
            ));
        }
    }
    assert!(upstream.unknown.lock().unwrap().is_empty(), "unrecorded upstream requests {:?}", upstream.unknown);
    assert!(differences.is_empty(), "{} differences:\n{}", differences.len(), differences.join("\n"));

    let metrics = app
        .oneshot(request(
            "GET",
            "/metrics",
            &serde_json::json!({ "authorization": "Basic cHJvbTpzZWNyZXQ=" }).as_object().unwrap().clone(),
        ))
        .await
        .unwrap();
    let text =
        String::from_utf8(axum::body::to_bytes(metrics.into_body(), usize::MAX).await.unwrap().to_vec()).unwrap();
    let actual = counters(
        text.lines().filter(|line| COUNTER_FAMILIES.iter().any(|family| line.starts_with(family))).map(str::to_owned),
    );
    let expected =
        counters(RECORDED["counters"].as_array().unwrap().iter().map(|line| line.as_str().unwrap().to_owned()));
    assert_eq!(
        actual.difference(&expected).collect::<Vec<_>>(),
        Vec::<&(String, BTreeSet<String>, String)>::new(),
        "only in rust"
    );
    assert_eq!(
        expected.difference(&actual).collect::<Vec<_>>(),
        Vec::<&(String, BTreeSet<String>, String)>::new(),
        "only in node"
    );
}
