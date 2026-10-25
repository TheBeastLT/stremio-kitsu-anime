use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use base64::{
    Engine, alphabet,
    engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig},
};
use prometheus::{
    CounterVec, Encoder, Gauge, Histogram, HistogramOpts, HistogramVec, IntGauge, Opts, Registry, TextEncoder,
};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use super::{
    cache::{CacheStats, Kind, StoreOp},
    js,
    upstream::{Outcome, UpstreamObserver},
};
use crate::domain::manifest::manifest;

pub const BUCKETS: [f64; 9] = [0.01, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0];
const LAG_BUCKETS: [f64; 11] = [0.0005, 0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0];
const LAG_SAMPLE_INTERVAL: Duration = Duration::from_millis(100);
const BASE64: GeneralPurpose = GeneralPurpose::new(
    &alphabet::STANDARD,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    Manifest,
    Catalog,
    Meta,
    Subtitles,
    Search,
    Metrics,
    Landing,
    Other,
}

impl Route {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manifest => "manifest",
            Self::Catalog => "catalog",
            Self::Meta => "meta",
            Self::Subtitles => "subtitles",
            Self::Search => "search",
            Self::Metrics => "metrics",
            Self::Landing => "landing",
            Self::Other => "other",
        }
    }
}

pub fn route_of(url: &str) -> Route {
    let raw = url.split('?').next().unwrap_or_default();
    let Some(path) = js::decode_uri_component(raw) else {
        return Route::Other;
    };
    if path == "/" || path == "/configure" {
        return Route::Landing;
    }
    if !path.starts_with('/') {
        return Route::Other;
    }
    let segment = path.split('/').nth(1).unwrap_or_default();
    match segment.strip_suffix(".json").unwrap_or(segment) {
        "manifest" => Route::Manifest,
        "catalog" => Route::Catalog,
        "meta" => Route::Meta,
        "subtitles" => Route::Subtitles,
        "search" => Route::Search,
        "metrics" => Route::Metrics,
        _ => Route::Other,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricsAccess {
    NotFound,
    Unauthorized,
    Allowed,
}

fn digest(value: &str) -> [u8; 32] {
    Sha256::digest(value.as_bytes()).into()
}

fn safe_equal(a: &str, b: &str) -> bool {
    digest(a).ct_eq(&digest(b)).into()
}

pub fn authorize_metrics(header: Option<&str>, user: Option<&str>, password: Option<&str>) -> MetricsAccess {
    let (Some(user), Some(password)) = (user.filter(|v| !v.is_empty()), password.filter(|v| !v.is_empty())) else {
        return MetricsAccess::NotFound;
    };
    let mut parts = header.unwrap_or_default().split(' ');
    let scheme = parts.next().unwrap_or_default();
    let encoded = parts.next().unwrap_or_default();
    let decoded = BASE64.decode(encoded).map(|bytes| String::from_utf8_lossy(&bytes).into_owned()).unwrap_or_default();
    let (given_user, given_password) = decoded.split_once(':').unwrap_or((decoded.as_str(), ""));
    let user_matches = safe_equal(given_user, user);
    let password_matches = safe_equal(given_password, password);
    if scheme == "Basic" && user_matches && password_matches {
        MetricsAccess::Allowed
    } else {
        MetricsAccess::Unauthorized
    }
}

pub fn catalog_label(id: &str) -> Option<&'static str> {
    manifest().catalogs.iter().find(|catalog| catalog.id == id).map(|catalog| catalog.id)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogKind {
    Search,
    LastVideos,
    Browse,
}

impl CatalogKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Search => "search",
            Self::LastVideos => "last_videos",
            Self::Browse => "browse",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchResult {
    Found,
    Empty,
}

impl SearchResult {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Found => "found",
            Self::Empty => "empty",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdType {
    Kitsu,
    Imdb,
}

impl IdType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Kitsu => "kitsu",
            Self::Imdb => "imdb",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetaOutcome {
    Ok,
    NotFound,
    Error,
}

impl MetaOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::NotFound => "not_found",
            Self::Error => "error",
        }
    }
}

#[derive(Clone)]
struct RuntimeMetrics {
    scheduling_lag: Histogram,
    utilization: Gauge,
    alive_tasks: IntGauge,
    global_queue_depth: IntGauge,
    sample: Arc<Mutex<(Instant, Duration)>>,
}

impl RuntimeMetrics {
    fn new(registry: &Registry) -> Self {
        let scheduling_lag = Histogram::with_opts(
            HistogramOpts::new("runtime_scheduling_lag_seconds", "Delay of a 100ms timer on the async runtime")
                .buckets(LAG_BUCKETS.to_vec()),
        )
        .expect("valid histogram");
        registry.register(Box::new(scheduling_lag.clone())).expect("unique metric");
        let utilization =
            Gauge::new("runtime_utilization", "Share of worker time spent busy since the previous scrape (0-1)")
                .expect("valid gauge");
        registry.register(Box::new(utilization.clone())).expect("unique metric");
        let alive_tasks =
            IntGauge::new("runtime_alive_tasks", "Tasks currently alive on the async runtime").expect("valid gauge");
        registry.register(Box::new(alive_tasks.clone())).expect("unique metric");
        let global_queue_depth =
            IntGauge::new("runtime_global_queue_depth", "Tasks waiting in the runtime global queue")
                .expect("valid gauge");
        registry.register(Box::new(global_queue_depth.clone())).expect("unique metric");
        Self {
            scheduling_lag,
            utilization,
            alive_tasks,
            global_queue_depth,
            sample: Arc::new(Mutex::new((Instant::now(), Duration::ZERO))),
        }
    }

    fn observe(&self) {
        let runtime = tokio::runtime::Handle::current().metrics();
        let workers = runtime.num_workers();
        let busy: Duration = (0..workers).map(|worker| runtime.worker_total_busy_duration(worker)).sum();
        let mut sample = self.sample.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (previous_at, previous_busy) = *sample;
        let capacity = previous_at.elapsed().as_secs_f64() * workers as f64;
        if capacity > 0.0 {
            self.utilization.set((busy.saturating_sub(previous_busy).as_secs_f64() / capacity).min(1.0));
        }
        *sample = (Instant::now(), busy);
        self.alive_tasks.set(runtime.num_alive_tasks() as i64);
        self.global_queue_depth.set(runtime.global_queue_depth() as i64);
    }

    fn spawn_lag_sampler(&self) {
        let histogram = self.scheduling_lag.clone();
        tokio::spawn(async move {
            loop {
                let started = Instant::now();
                tokio::time::sleep(LAG_SAMPLE_INTERVAL).await;
                histogram.observe(started.elapsed().saturating_sub(LAG_SAMPLE_INTERVAL).as_secs_f64());
            }
        });
    }
}

fn histogram(registry: &Registry, name: &str, help: &str, labels: &[&str]) -> HistogramVec {
    let metric =
        HistogramVec::new(HistogramOpts::new(name, help).buckets(BUCKETS.to_vec()), labels).expect("valid histogram");
    registry.register(Box::new(metric.clone())).expect("unique metric");
    metric
}

fn counter(registry: &Registry, name: &str, help: &str, labels: &[&str]) -> CounterVec {
    let metric = CounterVec::new(Opts::new(name, help), labels).expect("valid counter");
    registry.register(Box::new(metric.clone())).expect("unique metric");
    metric
}

#[derive(Clone)]
pub struct Metrics {
    registry: Registry,
    http_requests: CounterVec,
    http_duration: HistogramVec,
    upstream_duration: HistogramVec,
    store_duration: HistogramVec,
    cache_requests: CounterVec,
    catalog_requests: CounterVec,
    search_requests: CounterVec,
    meta_requests: CounterVec,
    runtime: RuntimeMetrics,
}

impl Metrics {
    pub fn new() -> Self {
        let registry = Registry::new_custom(Some("kitsu".to_owned()), None).expect("valid registry");
        #[cfg(target_os = "linux")]
        registry
            .register(Box::new(prometheus::process_collector::ProcessCollector::for_self()))
            .expect("process collector");
        Self {
            runtime: RuntimeMetrics::new(&registry),
            http_requests: counter(&registry, "http_requests_total", "HTTP requests", &["route", "status"]),
            http_duration: histogram(&registry, "http_request_duration_seconds", "HTTP request duration", &["route"]),
            upstream_duration: histogram(
                &registry,
                "upstream_request_duration_seconds",
                "Upstream request duration",
                &["upstream", "outcome"],
            ),
            store_duration: histogram(
                &registry,
                "cache_store_operation_duration_seconds",
                "Cache store operation duration",
                &["op"],
            ),
            cache_requests: counter(&registry, "cache_requests_total", "Cache lookups", &["kind", "result"]),
            catalog_requests: counter(&registry, "catalog_requests_total", "Catalog requests", &["catalog", "kind"]),
            search_requests: counter(&registry, "search_requests_total", "Search requests", &["result"]),
            meta_requests: counter(&registry, "meta_requests_total", "Meta requests", &["id_type", "outcome"]),
            registry,
        }
    }

    pub fn http_request(&self, route: Route, status: u16, elapsed: Duration) {
        let route = route.as_str();
        self.http_requests.with_label_values(&[route, &status.to_string()]).inc();
        self.http_duration.with_label_values(&[route]).observe(elapsed.as_secs_f64());
    }

    pub fn catalog_request(&self, catalog: &'static str, kind: CatalogKind) {
        self.catalog_requests.with_label_values(&[catalog, kind.as_str()]).inc();
    }

    pub fn search_request(&self, result: SearchResult) {
        self.search_requests.with_label_values(&[result.as_str()]).inc();
    }

    pub fn meta_request(&self, id_type: IdType, outcome: MetaOutcome) {
        self.meta_requests.with_label_values(&[id_type.as_str(), outcome.as_str()]).inc();
    }

    pub fn observe_runtime(&self) {
        self.runtime.observe();
    }

    pub fn spawn_lag_sampler(&self) {
        self.runtime.spawn_lag_sampler();
    }

    pub fn render(&self) -> String {
        let mut buffer = Vec::new();
        TextEncoder::new().encode(&self.registry.gather(), &mut buffer).expect("metrics encode");
        String::from_utf8(buffer).unwrap_or_default()
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

impl CacheStats for Metrics {
    fn hit(&self, kind: Kind) {
        self.cache_requests.with_label_values(&[kind.as_str(), "hit"]).inc();
    }

    fn miss(&self, kind: Kind) {
        self.cache_requests.with_label_values(&[kind.as_str(), "miss"]).inc();
    }

    fn observe(&self, op: StoreOp, duration: Duration) {
        self.store_duration.with_label_values(&[op.as_str()]).observe(duration.as_secs_f64());
    }
}

impl UpstreamObserver for Metrics {
    fn observe(&self, upstream: &'static str, outcome: Outcome, duration: Duration) {
        self.upstream_duration.with_label_values(&[upstream, outcome.as_str()]).observe(duration.as_secs_f64());
    }
}

#[cfg(test)]
mod tests;
