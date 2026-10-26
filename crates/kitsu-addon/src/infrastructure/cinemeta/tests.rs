use serde_json::json;

use super::*;
use crate::infrastructure::{
    http::{AXIOS_ACCEPT, AXIOS_USER_AGENT, HttpResponse, TransportError, Verb},
    testing::{FakeHttp, Observed},
};

const SERIES: &str = "https://v3-cinemeta.strem.io/meta/series/tt0388629.json";
const MOVIE: &str = "https://v3-cinemeta.strem.io/meta/movie/tt0388629.json";

fn cinemeta() -> (Cinemeta, Arc<FakeHttp>, Arc<Observed>) {
    let (http, observed) = (FakeHttp::new(), Observed::new());
    (Cinemeta::new(http.clone(), observed.clone()), http, observed)
}

fn far() -> Instant {
    Instant::now() + Duration::from_secs(3600)
}

fn meta_body(kind: &str) -> String {
    json!({ "meta": { "id": "tt0388629", "imdb_id": "tt0388629", "type": kind, "name": "One Piece" } }).to_string()
}

#[tokio::test]
async fn series_found_on_first_attempt() {
    let (cinemeta, http, observed) = cinemeta();
    http.reply(Verb::Get, SERIES, 200, &meta_body("series"));
    let meta = cinemeta.get("tt0388629", far()).await.meta;
    assert_eq!(meta["type"], "series");
    assert_eq!(http.urls(), vec![SERIES]);
    assert_eq!(observed.outcomes(), vec![("cinemeta", Outcome::Ok)]);
}

#[tokio::test]
async fn sends_axios_headers_and_sixty_second_timeout() {
    let (cinemeta, http, _) = cinemeta();
    http.reply(Verb::Get, SERIES, 200, &meta_body("series"));
    cinemeta.get("tt0388629", far()).await;
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
    let meta = cinemeta.get("tt0388629", far()).await.meta;
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
    let meta = cinemeta.get("tt0388629", far()).await.meta;
    assert_eq!(meta["type"], "series");
    assert_eq!(http.urls(), vec![SERIES, SERIES]);
    assert_eq!(observed.outcomes(), vec![("cinemeta", Outcome::Error), ("cinemeta", Outcome::Ok)]);
}

#[tokio::test]
async fn not_found_on_both_types_resolves_the_bare_imdb_meta_without_retrying() {
    let (cinemeta, http, observed) = cinemeta();
    let meta = cinemeta.get("tt0388629", far()).await.meta;
    assert_eq!(meta, json!({ "id": "tt0388629", "imdb_id": "tt0388629" }));
    assert_eq!(http.urls(), vec![SERIES, MOVIE]);
    assert_eq!(observed.outcomes(), vec![("cinemeta", Outcome::Error); 2]);
}

#[tokio::test]
async fn a_meta_without_imdb_id_counts_ok_and_is_not_retried() {
    let (cinemeta, http, observed) = cinemeta();
    http.reply(Verb::Get, SERIES, 200, r#"{"meta":{"id":"tt0388629","imdb_id":""}}"#);
    http.reply(Verb::Get, MOVIE, 200, &meta_body("movie"));
    let meta = cinemeta.get("tt0388629", far()).await.meta;
    assert_eq!(meta["type"], "movie");
    assert_eq!(http.urls(), vec![SERIES, MOVIE]);
    assert_eq!(observed.outcomes(), vec![("cinemeta", Outcome::Ok); 2]);
}

#[tokio::test]
async fn a_non_json_body_is_no_search_results() {
    let (cinemeta, http, _) = cinemeta();
    http.reply(Verb::Get, SERIES, 200, "<html>");
    http.reply(Verb::Get, MOVIE, 200, "null");
    let meta = cinemeta.get("tt0388629", far()).await.meta;
    assert_eq!(meta, json!({ "id": "tt0388629", "imdb_id": "tt0388629" }));
    assert_eq!(http.urls(), vec![SERIES, MOVIE]);
}

#[tokio::test]
async fn returns_the_meta_object_only() {
    let (cinemeta, http, _) = cinemeta();
    http.reply(Verb::Get, SERIES, 200, &meta_body("series"));
    let meta = cinemeta.get("tt0388629", far()).await.meta;
    assert_eq!(meta, json!({ "id": "tt0388629", "imdb_id": "tt0388629", "type": "series", "name": "One Piece" }));
}

#[tokio::test]
async fn too_many_requests_is_retried() {
    let (cinemeta, http, _) = cinemeta();
    http.reply(Verb::Get, SERIES, 429, "");
    http.reply(Verb::Get, SERIES, 200, &meta_body("series"));
    let meta = cinemeta.get("tt0388629", far()).await.meta;
    assert_eq!(meta["type"], "series");
    assert_eq!(http.urls(), vec![SERIES, SERIES]);
}

#[tokio::test]
async fn a_client_error_is_not_retried() {
    let (cinemeta, http, _) = cinemeta();
    http.reply(Verb::Get, SERIES, 403, "");
    http.reply(Verb::Get, MOVIE, 200, &meta_body("movie"));
    cinemeta.get("tt0388629", far()).await;
    assert_eq!(http.urls(), vec![SERIES, MOVIE]);
}

struct SlowHttp {
    delay: Duration,
    inner: Arc<FakeHttp>,
}

#[async_trait::async_trait]
impl HttpTransport for SlowHttp {
    async fn send(&self, request: &HttpRequest) -> Result<HttpResponse, TransportError> {
        let reply = self.inner.send(request).await;
        tokio::time::sleep(self.delay).await;
        reply
    }
}

fn slow_cinemeta(delay: Duration) -> (Cinemeta, Arc<FakeHttp>) {
    let inner = FakeHttp::new();
    let http = Arc::new(SlowHttp { delay, inner: inner.clone() });
    (Cinemeta::new(http, Observed::new()), inner)
}

#[tokio::test(start_paused = true)]
async fn a_hanging_cinemeta_gives_the_bare_imdb_meta_at_the_deadline() {
    let (cinemeta, _) = slow_cinemeta(Duration::from_secs(600));
    let started = Instant::now();
    let fetched = cinemeta.get("tt0388629", started + Duration::from_secs(20)).await;
    assert_eq!(fetched, Fetched { meta: json!({ "id": "tt0388629", "imdb_id": "tt0388629" }), complete: false });
    assert_eq!(started.elapsed(), Duration::from_secs(20));
}

#[tokio::test(start_paused = true)]
async fn retries_and_the_movie_fallback_share_one_deadline() {
    let (cinemeta, http) = slow_cinemeta(Duration::from_secs(4));
    http.reply(Verb::Get, SERIES, 500, "");
    http.reply(Verb::Get, MOVIE, 200, &meta_body("movie"));
    let started = Instant::now();
    let fetched = cinemeta.get("tt0388629", started + Duration::from_secs(18)).await;
    assert_eq!(fetched, Fetched { meta: json!({ "id": "tt0388629", "imdb_id": "tt0388629" }), complete: false });
    assert_eq!(http.urls()[..4], [SERIES; 4]);
    assert_eq!(started.elapsed(), Duration::from_secs(18));
}

struct HangFirst {
    calls: std::sync::atomic::AtomicUsize,
    inner: Arc<FakeHttp>,
}

#[async_trait::async_trait]
impl HttpTransport for HangFirst {
    async fn send(&self, request: &HttpRequest) -> Result<HttpResponse, TransportError> {
        if self.calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed) == 0 {
            std::future::pending::<()>().await;
        }
        self.inner.send(request).await
    }
}

#[tokio::test(start_paused = true)]
async fn a_hung_attempt_is_cut_at_half_the_remaining_time_so_a_retry_fits() {
    let (inner, observed) = (FakeHttp::new(), Observed::new());
    inner.reply(Verb::Get, SERIES, 200, &meta_body("series"));
    let http = Arc::new(HangFirst { calls: std::sync::atomic::AtomicUsize::new(0), inner: inner.clone() });
    let cinemeta = Cinemeta::new(http, observed.clone());
    let started = Instant::now();
    let fetched = cinemeta.get("tt0388629", started + Duration::from_secs(20)).await;
    assert!(fetched.complete);
    assert_eq!(fetched.meta["type"], "series");
    assert_eq!(started.elapsed(), Duration::from_millis(10_500));
    assert_eq!(observed.outcomes(), vec![("cinemeta", Outcome::Error), ("cinemeta", Outcome::Ok)]);
}

#[tokio::test(start_paused = true)]
async fn a_transient_failure_is_retried_after_a_growing_delay() {
    let (cinemeta, http, _) = cinemeta();
    http.reply(Verb::Get, SERIES, 503, "");
    http.reply(Verb::Get, SERIES, 503, "");
    http.reply(Verb::Get, SERIES, 200, &meta_body("series"));
    let started = Instant::now();
    assert!(cinemeta.get("tt0388629", far()).await.complete);
    assert_eq!(started.elapsed(), Duration::from_millis(1_500));
}

#[tokio::test(start_paused = true)]
async fn a_final_miss_is_complete_and_a_transient_one_is_not() {
    let (cinemeta, http, _) = cinemeta();
    assert!(cinemeta.get("tt0388629", far()).await.complete);
    http.reply(Verb::Get, MOVIE, 502, "");
    assert!(!cinemeta.get("tt0388629", far()).await.complete);
}
