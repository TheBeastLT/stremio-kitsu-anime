use std::time::Duration;

use base64::{Engine, engine::general_purpose::STANDARD};

use super::*;

fn basic(user: &str, password: &str) -> String {
    format!("Basic {}", STANDARD.encode(format!("{user}:{password}")))
}

fn access(header: Option<&str>) -> MetricsAccess {
    authorize_metrics(header, Some("user"), Some("secret"))
}

#[test]
fn route_of_classifies_catalog_urls() {
    assert_eq!(route_of("/catalog/anime/kitsu-anime-trending.json"), Route::Catalog);
}

#[test]
fn route_of_classifies_search_urls() {
    assert_eq!(route_of("/search/overlord%20S3.json"), Route::Search);
}

#[test]
fn route_of_classifies_unknown_urls_as_other() {
    assert_eq!(route_of("/anything/else?x=1"), Route::Other);
}

#[test]
fn route_of_classifies_malformed_encoding_as_other() {
    assert_eq!(route_of("/%E0%A4%A"), Route::Other);
}

#[test]
fn route_of_classifies_invalid_utf8_escapes_as_other() {
    assert_eq!(route_of("/%FF"), Route::Other);
}

#[test]
fn route_of_classifies_manifest() {
    assert_eq!(route_of("/manifest.json"), Route::Manifest);
}

#[test]
fn route_of_classifies_meta() {
    assert_eq!(route_of("/meta/anime/kitsu:1.json"), Route::Meta);
}

#[test]
fn route_of_classifies_subtitles() {
    assert_eq!(route_of("/subtitles/series/kitsu:1:1.json"), Route::Subtitles);
}

#[test]
fn route_of_classifies_metrics() {
    assert_eq!(route_of("/metrics"), Route::Metrics);
}

#[test]
fn route_of_classifies_landing() {
    assert_eq!(route_of("/"), Route::Landing);
    assert_eq!(route_of("/configure"), Route::Landing);
    assert_eq!(route_of("/?x=1"), Route::Landing);
}

#[test]
fn route_of_classifies_urls_without_a_leading_slash_as_other() {
    assert_eq!(route_of("*"), Route::Other);
    assert_eq!(route_of(""), Route::Other);
}

#[test]
fn route_of_strips_the_query_before_decoding() {
    assert_eq!(route_of("/manifest.json?%E0%A4%A"), Route::Manifest);
}

#[test]
fn route_of_matches_only_the_first_segment() {
    assert_eq!(route_of("/other/catalog"), Route::Other);
    assert_eq!(route_of("//catalog"), Route::Other);
}

#[test]
fn route_labels_are_the_node_values() {
    let labels = [
        Route::Manifest,
        Route::Catalog,
        Route::Meta,
        Route::Subtitles,
        Route::Search,
        Route::Metrics,
        Route::Landing,
        Route::Other,
    ]
    .map(Route::as_str);
    assert_eq!(labels, ["manifest", "catalog", "meta", "subtitles", "search", "metrics", "landing", "other"]);
}

#[test]
fn metrics_endpoint_is_not_found_when_credentials_are_unset() {
    let header = basic("u", "p");
    assert_eq!(authorize_metrics(Some(&header), None, None), MetricsAccess::NotFound);
    assert_eq!(authorize_metrics(Some(&header), Some("user"), None), MetricsAccess::NotFound);
    assert_eq!(authorize_metrics(Some(&header), Some(""), Some("")), MetricsAccess::NotFound);
}

#[test]
fn metrics_endpoint_is_unauthorized_without_a_header() {
    assert_eq!(access(None), MetricsAccess::Unauthorized);
}

#[test]
fn metrics_endpoint_is_unauthorized_with_a_wrong_password() {
    assert_eq!(access(Some(&basic("user", "wrong"))), MetricsAccess::Unauthorized);
}

#[test]
fn metrics_endpoint_is_unauthorized_with_a_wrong_user() {
    assert_eq!(access(Some(&basic("other", "secret"))), MetricsAccess::Unauthorized);
}

#[test]
fn metrics_endpoint_is_unauthorized_with_another_scheme() {
    let token = STANDARD.encode("user:secret");
    assert_eq!(access(Some(&format!("Bearer {token}"))), MetricsAccess::Unauthorized);
}

#[test]
fn metrics_endpoint_is_unauthorized_with_undecodable_credentials() {
    assert_eq!(access(Some("Basic !!!")), MetricsAccess::Unauthorized);
    assert_eq!(access(Some("Basic")), MetricsAccess::Unauthorized);
}

#[test]
fn metrics_endpoint_is_unauthorized_when_the_value_has_no_separator() {
    assert_eq!(access(Some(&format!("Basic {}", STANDARD.encode("user")))), MetricsAccess::Unauthorized);
}

#[test]
fn metrics_endpoint_is_allowed_with_correct_credentials() {
    assert_eq!(access(Some(&basic("user", "secret"))), MetricsAccess::Allowed);
}

#[test]
fn metrics_endpoint_splits_on_the_first_colon_only() {
    let header = basic("user", "se:cret");
    assert_eq!(authorize_metrics(Some(&header), Some("user"), Some("se:cret")), MetricsAccess::Allowed);
}

#[test]
fn metrics_endpoint_accepts_unpadded_base64() {
    let header = format!("Basic {}", STANDARD.encode("user:secrets").trim_end_matches('='));
    assert_eq!(authorize_metrics(Some(&header), Some("user"), Some("secrets")), MetricsAccess::Allowed);
}

#[test]
fn catalog_label_is_bounded_to_manifest_ids() {
    assert_eq!(catalog_label("kitsu-anime-list"), Some("kitsu-anime-list"));
    for catalog in &manifest().catalogs {
        assert_eq!(catalog_label(catalog.id), Some(catalog.id));
    }
    assert_eq!(catalog_label("not-a-real-catalog"), None);
    assert_eq!(catalog_label(""), None);
}

fn lines<'a>(text: &'a str, name: &str) -> Vec<&'a str> {
    text.lines().filter(|line| line.starts_with(name)).collect()
}

fn has(text: &str, series: &str) -> bool {
    text.lines().any(|line| line == series)
}

fn exercised() -> Metrics {
    let metrics = Metrics::new();
    metrics.http_request(Route::Catalog, 200, Duration::from_millis(30));
    metrics.http_request(Route::Meta, 404, Duration::from_millis(3));
    metrics.catalog_request("kitsu-anime-list", CatalogKind::Search);
    metrics.catalog_request("kitsu-anime-popular", CatalogKind::Browse);
    metrics.catalog_request("kitsu-anime-last-videos", CatalogKind::LastVideos);
    metrics.search_request(SearchResult::Found);
    metrics.search_request(SearchResult::Empty);
    metrics.meta_request(IdType::Imdb, MetaOutcome::NotFound);
    metrics.meta_request(IdType::Kitsu, MetaOutcome::Ok);
    metrics.meta_request(IdType::Kitsu, MetaOutcome::Error);
    CacheStats::hit(&metrics, Kind::Meta);
    CacheStats::miss(&metrics, Kind::Catalog);
    CacheStats::observe(&metrics, StoreOp::Get, Duration::from_millis(2));
    UpstreamObserver::observe(&metrics, "kitsu", Outcome::Ok, Duration::from_millis(20));
    UpstreamObserver::observe(&metrics, "cinemeta", Outcome::Error, Duration::from_millis(20));
    metrics
}

#[test]
fn catalog_search_counts_the_catalog_and_search_kind() {
    let text = exercised().render();
    assert!(has(&text, r#"kitsu_catalog_requests_total{catalog="kitsu-anime-list",kind="search"} 1"#));
}

#[test]
fn catalog_browse_counts_the_browse_kind() {
    let text = exercised().render();
    assert!(has(&text, r#"kitsu_catalog_requests_total{catalog="kitsu-anime-popular",kind="browse"} 1"#));
}

#[test]
fn catalog_last_videos_counts_the_last_videos_kind() {
    let text = exercised().render();
    assert!(has(&text, r#"kitsu_catalog_requests_total{catalog="kitsu-anime-last-videos",kind="last_videos"} 1"#));
}

#[test]
fn search_counts_found_and_empty_results() {
    let text = exercised().render();
    assert!(has(&text, r#"kitsu_search_requests_total{result="found"} 1"#));
    assert!(has(&text, r#"kitsu_search_requests_total{result="empty"} 1"#));
}

#[test]
fn meta_for_an_unmapped_imdb_id_counts_not_found() {
    let text = exercised().render();
    assert!(has(&text, r#"kitsu_meta_requests_total{id_type="imdb",outcome="not_found"} 1"#));
    assert!(has(&text, r#"kitsu_meta_requests_total{id_type="kitsu",outcome="ok"} 1"#));
    assert!(has(&text, r#"kitsu_meta_requests_total{id_type="kitsu",outcome="error"} 1"#));
}

#[test]
fn upstream_observer_records_ok_and_error_outcomes() {
    let text = exercised().render();
    assert!(has(&text, r#"kitsu_upstream_request_duration_seconds_count{outcome="ok",upstream="kitsu"} 1"#));
    assert!(has(&text, r#"kitsu_upstream_request_duration_seconds_count{outcome="error",upstream="cinemeta"} 1"#));
}

#[test]
fn cache_stats_record_lookups_and_store_operations() {
    let text = exercised().render();
    assert!(has(&text, r#"kitsu_cache_requests_total{kind="meta",result="hit"} 1"#));
    assert!(has(&text, r#"kitsu_cache_requests_total{kind="catalog",result="miss"} 1"#));
    assert!(has(&text, r#"kitsu_cache_store_operation_duration_seconds_count{op="get"} 1"#));
}

#[test]
fn http_requests_count_route_and_status() {
    let text = exercised().render();
    assert!(has(&text, r#"kitsu_http_requests_total{route="catalog",status="200"} 1"#));
    assert!(has(&text, r#"kitsu_http_requests_total{route="meta",status="404"} 1"#));
    assert!(has(&text, r#"kitsu_http_request_duration_seconds_count{route="catalog"} 1"#));
}

#[test]
fn histograms_use_the_node_bucket_boundaries() {
    let text = exercised().render();
    let expected = ["0.01", "0.05", "0.1", "0.25", "0.5", "1", "2.5", "5", "10", "+Inf"];
    for name in [
        "kitsu_http_request_duration_seconds_bucket",
        "kitsu_upstream_request_duration_seconds_bucket",
        "kitsu_cache_store_operation_duration_seconds_bucket",
    ] {
        let first_series: Vec<&str> = lines(&text, name).into_iter().take(expected.len()).collect();
        let bounds: Vec<&str> = first_series
            .iter()
            .map(|line| line.split("le=\"").nth(1).and_then(|rest| rest.split('"').next()).unwrap())
            .collect();
        assert_eq!(bounds, expected, "{name}");
    }
}

#[test]
fn http_histogram_label_sets_are_exact() {
    let text = exercised().render();
    assert!(text.contains(r#"kitsu_http_request_duration_seconds_bucket{route="catalog",le="0.05"} 1"#));
    assert!(
        text.contains(r#"kitsu_upstream_request_duration_seconds_bucket{outcome="ok",upstream="kitsu",le="0.05"} 1"#)
    );
    assert!(text.contains(r#"kitsu_cache_store_operation_duration_seconds_bucket{op="get",le="0.01"} 1"#));
}

#[tokio::test]
async fn renders_runtime_series_and_no_nodejs_series() {
    let metrics = exercised();
    metrics.spawn_lag_sampler();
    tokio::time::sleep(Duration::from_millis(250)).await;
    metrics.observe_runtime();
    let text = metrics.render();
    for name in [
        "kitsu_runtime_utilization",
        "kitsu_runtime_alive_tasks",
        "kitsu_runtime_global_queue_depth",
        "kitsu_runtime_scheduling_lag_seconds_count",
    ] {
        assert!(!lines(&text, name).is_empty(), "{name}");
    }
    assert!(!text.contains("kitsu_nodejs_"));
}

const KEEP_LIST: [&str; 13] = [
    "kitsu_http_requests_total",
    "kitsu_http_request_duration_seconds_bucket",
    "kitsu_upstream_request_duration_seconds_bucket",
    "kitsu_cache_requests_total",
    "kitsu_cache_store_operation_duration_seconds_bucket",
    "kitsu_catalog_requests_total",
    "kitsu_search_requests_total",
    "kitsu_meta_requests_total",
    "kitsu_http_request_duration_seconds_sum",
    "kitsu_http_request_duration_seconds_count",
    "kitsu_upstream_request_duration_seconds_sum",
    "kitsu_upstream_request_duration_seconds_count",
    "kitsu_cache_store_operation_duration_seconds_count",
];

#[test]
fn produces_every_keep_listed_series_except_the_nodejs_ones() {
    let text = exercised().render();
    for name in KEEP_LIST {
        assert!(!lines(&text, name).is_empty(), "{name}");
    }
}

#[cfg(target_os = "linux")]
#[test]
fn produces_the_prefixed_process_series() {
    let text = exercised().render();
    for name in ["kitsu_process_cpu_seconds_total", "kitsu_process_resident_memory_bytes"] {
        assert!(!lines(&text, name).is_empty(), "{name}");
    }
}
