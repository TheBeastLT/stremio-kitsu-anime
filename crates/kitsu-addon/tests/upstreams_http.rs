use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use kitsu_addon::{
    application::Error,
    infrastructure::{
        Cache, CacheSettings, Cinemeta, Fanart, HttpRequest, HttpResponse, HttpTransport, Ids, MappingInfo, Outcome,
        ReqwestHttp, UpstreamObserver, Verb, cache::NoStats, http::TransportError,
    },
};
use serde_json::json;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{header, method, path, query_param},
};

fn far() -> tokio::time::Instant {
    tokio::time::Instant::now() + Duration::from_secs(3600)
}

const HOSTS: [&str; 5] = [
    "https://v3-cinemeta.strem.io",
    "https://relations.yuna.moe",
    "https://webservice.fanart.tv",
    "https://thetvdb.com",
    "https://images.metahub.space",
];

struct Rewrite {
    inner: ReqwestHttp,
    target: String,
}

#[async_trait]
impl HttpTransport for Rewrite {
    async fn send(&self, request: &HttpRequest) -> Result<HttpResponse, TransportError> {
        let mut request = request.clone();
        for host in HOSTS {
            request.url = request.url.replace(host, &self.target);
        }
        self.inner.send(&request).await
    }
}

#[derive(Default)]
struct Observed(Mutex<Vec<(&'static str, Outcome)>>);

impl UpstreamObserver for Observed {
    fn observe(&self, upstream: &'static str, outcome: Outcome, _duration: Duration) {
        self.0.lock().unwrap().push((upstream, outcome));
    }
}

fn http(server: &MockServer) -> Arc<Rewrite> {
    Arc::new(Rewrite { inner: ReqwestHttp::new().unwrap(), target: server.uri() })
}

fn cache<E: Clone>() -> Arc<Cache<E>> {
    let settings = CacheSettings {
        enabled: true,
        budget_bytes: 1024 * 1024,
        meta_ttl: Duration::from_secs(60),
        catalog_ttl: Duration::from_secs(60),
        cache_max_age: 43_200,
    };
    Arc::new(Cache::new(settings, None, Arc::new(NoStats)))
}

#[tokio::test]
async fn cinemeta_retries_series_four_times_then_takes_the_movie() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/meta/series/tt0275277.json"))
        .and(header("user-agent", "axios/1.8.2"))
        .respond_with(ResponseTemplate::new(500))
        .expect(4)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/meta/movie/tt0275277.json"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "meta": { "imdb_id": "tt0275277", "type": "movie" } })),
        )
        .expect(1)
        .mount(&server)
        .await;
    let observed = Arc::new(Observed::default());
    let meta = Cinemeta::new(http(&server), observed.clone()).get("tt0275277", far()).await.meta;
    assert_eq!(meta, json!({ "imdb_id": "tt0275277", "type": "movie" }));
    assert_eq!(observed.0.lock().unwrap().len(), 5);
}

#[tokio::test]
async fn cinemeta_not_found_resolves_the_bare_imdb_meta_after_one_attempt_per_type() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).respond_with(ResponseTemplate::new(404)).expect(2).mount(&server).await;
    let observed = Arc::new(Observed::default());
    let meta = Cinemeta::new(http(&server), observed.clone()).get("tt0000001", far()).await.meta;
    assert_eq!(meta, json!({ "id": "tt0000001", "imdb_id": "tt0000001" }));
    assert_eq!(*observed.0.lock().unwrap(), vec![("cinemeta", Outcome::Error); 2]);
}

#[tokio::test]
async fn yuna_is_queried_with_source_id_and_include() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v2/ids"))
        .and(query_param("source", "myanimelist"))
        .and(query_param("id", "1"))
        .and(query_param("include", "kitsu"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "myanimelist": 1, "kitsu": 1 })))
        .expect(1)
        .mount(&server)
        .await;
    let ids: Ids<Arc<Error>> = Ids::new(http(&server), cache());
    assert_eq!(ids.to_kitsu("mal:1").await.unwrap(), "1");
}

#[tokio::test]
async fn metahub_probes_are_head_requests_that_follow_redirects() {
    let server = MockServer::start().await;
    Mock::given(method("HEAD"))
        .and(path("/logo/medium/tt0275277/img"))
        .respond_with(ResponseTemplate::new(302).insert_header("location", "/final/logo.png"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("HEAD"))
        .and(path("/final/logo.png"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("HEAD"))
        .and(path("/poster/small/tt0275277/img"))
        .respond_with(ResponseTemplate::new(302).insert_header("location", "/missing.png"))
        .expect(1)
        .mount(&server)
        .await;
    let observed = Arc::new(Observed::default());
    let fanart: Fanart<()> = Fanart::new(http(&server), observed.clone(), cache(), "KEY".to_owned());
    let mapping = MappingInfo { imdb_id: Some("tt0275277".to_owned()), ..MappingInfo::default() };
    let images = fanart.images("2", Some(&mapping)).await;
    assert_eq!(images.logo.as_deref(), Some("https://images.metahub.space/logo/medium/tt0275277/img"));
    assert_eq!(images.background, None);
    assert_eq!(images.poster, None);
    assert_eq!(*observed.0.lock().unwrap(), vec![("fanart", Outcome::Error); 2]);
}

#[tokio::test]
async fn fanart_sends_the_api_key_as_query() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v3/tv/74796"))
        .and(query_param("api_key", "KEY"))
        .and(header("accept", "application/json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "thetvdb_id": "74796",
            "hdtvlogo": [{ "id": "1", "url": "http://logo.png", "lang": "en" }],
            "tvposter": [{ "id": "2", "url": "http://poster.png", "lang": "en" }],
            "showbackground": [{ "id": "3", "url": "http://background.png", "lang": "en" }],
        })))
        .expect(1)
        .mount(&server)
        .await;
    let fanart: Fanart<()> = Fanart::new(http(&server), Arc::new(Observed::default()), cache(), "KEY".to_owned());
    let mapping = MappingInfo { tvdb_id: Some("74796".to_owned()), ..MappingInfo::default() };
    let images = fanart.images("43078", Some(&mapping)).await;
    assert_eq!(images.logo.as_deref(), Some("https://logo.png"));
}

#[tokio::test]
async fn a_request_timeout_reads_like_axios() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_millis(500)))
        .mount(&server)
        .await;
    let request = HttpRequest {
        verb: Verb::Get,
        url: format!("{}/slow", server.uri()),
        accept: None,
        user_agent: None,
        timeout: Some(Duration::from_millis(50)),
    };
    let error = ReqwestHttp::new().unwrap().send(&request).await.unwrap_err();
    assert_eq!(error.0, "timeout of 50ms exceeded");
}
