use std::{
    any::Any,
    sync::{Arc, LazyLock},
    time::Instant,
};

use axum::{
    Router,
    body::{Body, HttpBody},
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, Method, Response as HttpResponse, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::any,
};
use fastjs_regex::{Match, Regex};
use serde_json::Value;
use tower_http::{
    catch_panic::CatchPanicLayer,
    compression::{CompressionLayer, Predicate},
};

use super::{
    express::{self, JSON_CONTENT_TYPE},
    extras::resource_extras,
    landing::LANDING_HTML,
};
use crate::{
    application::{Addon, Answer, Error, Extras, jsvalue::number_text},
    domain::manifest,
    infrastructure::{
        Metrics, js,
        metrics::{MetricsAccess, authorize_metrics, route_of},
    },
};

const NOT_FOUND: &str = "{\"err\":\"not found\"}";
const HANDLER_ERROR: &str = "{\"err\":\"handler error\"}";
const METRICS_CONTENT_TYPE: &str = "text/plain; version=0.0.4; charset=utf-8";
const ALLOWED_METHODS: &str = "GET,HEAD,PUT,PATCH,POST,DELETE";
const COMPRESSION_THRESHOLD: u64 = 1024;
const CACHE_HEADERS: [(&str, &str); 3] =
    [("cacheMaxAge", "max-age"), ("staleRevalidate", "stale-while-revalidate"), ("staleError", "stale-if-error")];

static MANIFEST_ROUTE: LazyLock<Regex> = LazyLock::new(|| route(r"^\/manifest\.json\/?$", "i"));
static RESOURCE_ROUTE: LazyLock<Regex> = LazyLock::new(|| {
    route(r"^\/(?:(catalog|meta|subtitles))\/(?:([^\/]+?))\/(?:([^\/]+?))(?:\/([^\/]+?))?\.json\/?$", "i")
});
static LANDING_ROUTE: LazyLock<Regex> = LazyLock::new(|| route(r"^\/?$", "i"));
static SEARCH_ROUTE: LazyLock<Regex> = LazyLock::new(|| route(r"^\/search\/(.+)\.json$", ""));
static MANIFEST_JSON: LazyLock<String> =
    LazyLock::new(|| serde_json::to_string(manifest()).expect("serialisable manifest"));

fn route(source: &str, flags: &str) -> Regex {
    Regex::new(source, flags).expect("valid route")
}

pub struct AppState {
    pub addon: Addon,
    pub metrics: Metrics,
    pub metrics_credentials: Option<(String, String)>,
}

pub fn app(state: Arc<AppState>) -> Router {
    layered(Router::new().route("/metrics", any(metrics_handler)).fallback(dispatch), state)
}

fn layered(routes: Router<Arc<AppState>>, state: Arc<AppState>) -> Router {
    routes
        .layer(middleware::from_fn(vary_accept_encoding))
        .layer(CompressionLayer::new().compress_when(Compressible))
        .layer(CatchPanicLayer::custom(panicked))
        .layer(middleware::from_fn_with_state(Arc::clone(&state), record_metrics))
        .with_state(state)
}

fn panicked(panic: Box<dyn Any + Send + 'static>) -> Response {
    let message = match panic.downcast::<String>() {
        Ok(message) => *message,
        Err(panic) => panic.downcast::<&str>().map(|message| (*message).to_owned()).unwrap_or_default(),
    };
    tracing::error!("Handler panicked: {message}");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}

#[derive(Clone, Copy)]
struct Compressible;

impl Predicate for Compressible {
    fn should_compress<B: HttpBody>(&self, response: &HttpResponse<B>) -> bool {
        response.headers().contains_key(header::CONTENT_TYPE)
            && response.body().size_hint().exact().is_some_and(|size| size >= COMPRESSION_THRESHOLD)
    }
}

async fn record_metrics(State(state): State<Arc<AppState>>, request: Request, next: Next) -> Response {
    let route = route_of(request.uri().path_and_query().map_or("/", |target| target.as_str()));
    let started = Instant::now();
    let response = next.run(request).await;
    state.metrics.http_request(route, response.status().as_u16(), started.elapsed());
    response
}

async fn vary_accept_encoding(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    if response.headers().contains_key(header::CONTENT_TYPE) {
        let vary = match response.headers().get(header::VARY).and_then(|vary| vary.to_str().ok()) {
            Some(vary) => format!("{vary}, Accept-Encoding"),
            None => "Accept-Encoding".to_owned(),
        };
        if let Ok(vary) = HeaderValue::from_str(&vary) {
            response.headers_mut().insert(header::VARY, vary);
        }
    }
    response
}

async fn metrics_handler(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let credentials = state.metrics_credentials.as_ref();
    let access = authorize_metrics(
        headers.get(header::AUTHORIZATION).and_then(|value| value.to_str().ok()),
        credentials.map(|(user, _)| user.as_str()),
        credentials.map(|(_, password)| password.as_str()),
    );
    match access {
        MetricsAccess::NotFound => StatusCode::NOT_FOUND.into_response(),
        MetricsAccess::Unauthorized => {
            (StatusCode::UNAUTHORIZED, [(header::WWW_AUTHENTICATE, "Basic realm=\"metrics\"")]).into_response()
        }
        MetricsAccess::Allowed => {
            state.metrics.observe_runtime();
            ([(header::CONTENT_TYPE, METRICS_CONTENT_TYPE)], state.metrics.render()).into_response()
        }
    }
}

async fn dispatch(State(state): State<Arc<AppState>>, request: Request) -> Response {
    let method = request.method().clone();
    if method == Method::OPTIONS {
        return preflight(request.headers());
    }
    let mut response = if method == Method::GET || method == Method::HEAD {
        routed(&state, request.uri().path(), request.headers()).await
    } else {
        not_found()
    };
    response.headers_mut().insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    if method == Method::HEAD {
        *response.body_mut() = Body::empty();
    }
    response
}

async fn routed(state: &AppState, path: &str, headers: &HeaderMap) -> Response {
    if MANIFEST_ROUTE.is_match(path) {
        return json_response(StatusCode::OK, MANIFEST_JSON.clone(), None);
    }
    if let Some(found) = RESOURCE_ROUTE.find(path) {
        let Some(params) = decoded_params(path, &found) else {
            return bad_request();
        };
        let extras = if params[3].is_some() { resource_extras(path) } else { Extras::default() };
        let resource = params[0].as_deref().unwrap_or_default();
        let id = params[2].as_deref().unwrap_or_default();
        let accept = headers.get(header::ACCEPT).and_then(|value| value.to_str().ok());
        return resource_response(state, resource, id, &extras, accept).await;
    }
    if LANDING_ROUTE.is_match(path) {
        return ([(header::CONTENT_TYPE, "text/html")], LANDING_HTML).into_response();
    }
    if let Some(found) = SEARCH_ROUTE.find(path) {
        let Some(query) = found.group(1).and_then(|range| js::decode_uri_component(&path[range])) else {
            return bad_request();
        };
        return search_response(state, &query).await;
    }
    not_found()
}

fn decoded_params(path: &str, found: &Match) -> Option<Vec<Option<String>>> {
    (1..=found.group_count())
        .map(|group| match found.group(group) {
            Some(range) => js::decode_uri_component(&path[range]).map(Some),
            None => Some(None),
        })
        .collect()
}

async fn resource_response(
    state: &AppState,
    resource: &str,
    id: &str,
    extras: &Extras,
    accept: Option<&str>,
) -> Response {
    let answer = match resource {
        "catalog" => state.addon.catalog(id, extras).await.map(Answer::Json),
        "meta" => state.addon.meta(id).await.map(Answer::Json),
        "subtitles" => state.addon.subtitles(id, extras).await,
        _ => Err(Error::NotFound(format!("No handler for {resource}"))),
    };
    match answer {
        Ok(Answer::Json(value)) => match serde_json::to_string(&*value) {
            Ok(body) => json_response(StatusCode::OK, body, cache_control(&value)),
            Err(error) => handler_error(&error),
        },
        Ok(Answer::Redirect(url)) => express::redirect(&url, accept),
        Err(Error::NotFound(_)) => json_response(StatusCode::NOT_FOUND, NOT_FOUND.to_owned(), None),
        Err(Error::Failed(error)) => handler_error(&error),
        Err(Error::Busy(reason)) => busy(&reason),
    }
}

async fn search_response(state: &AppState, query: &str) -> Response {
    match state.addon.search(query).await {
        Ok(ids) => {
            let body = serde_json::json!({ "ids": ids }).to_string();
            json_response(StatusCode::OK, body, Some(format!("max-age={}, public", state.addon.cache_max_age())))
        }
        Err(Error::Busy(reason)) => busy(&reason),
        Err(error) => {
            tracing::error!("Search failed for \"{query}\": {error}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

fn cache_control(value: &Value) -> Option<String> {
    let directives: Vec<String> = CACHE_HEADERS
        .iter()
        .filter_map(|(field, directive)| integer(value.get(*field)?).map(|seconds| format!("{directive}={seconds}")))
        .collect();
    (!directives.is_empty()).then(|| format!("{}, public", directives.join(", ")))
}

fn integer(value: &Value) -> Option<String> {
    let number = value.as_f64()?;
    (number.is_finite() && number.fract() == 0.0).then(|| match (value.as_i64(), value.as_u64()) {
        (Some(number), _) => number.to_string(),
        (_, Some(number)) => number.to_string(),
        _ => number_text(number),
    })
}

fn json_response(status: StatusCode, body: String, cache_control: Option<String>) -> Response {
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;
    response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static(JSON_CONTENT_TYPE));
    if let Some(value) = cache_control.and_then(|value| HeaderValue::from_str(&value).ok()) {
        response.headers_mut().insert(header::CACHE_CONTROL, value);
    }
    response
}

fn handler_error(error: &impl std::fmt::Display) -> Response {
    tracing::error!("{error}");
    json_response(StatusCode::INTERNAL_SERVER_ERROR, HANDLER_ERROR.to_owned(), None)
}

fn busy(reason: &str) -> Response {
    tracing::warn!("{reason}");
    StatusCode::SERVICE_UNAVAILABLE.into_response()
}

fn not_found() -> Response {
    let mut response = Response::new(Body::from(NOT_FOUND));
    *response.status_mut() = StatusCode::NOT_FOUND;
    response
}

fn bad_request() -> Response {
    StatusCode::BAD_REQUEST.into_response()
}

fn preflight(headers: &HeaderMap) -> Response {
    let mut response = StatusCode::NO_CONTENT.into_response();
    let response_headers = response.headers_mut();
    response_headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    response_headers.insert(header::ACCESS_CONTROL_ALLOW_METHODS, HeaderValue::from_static(ALLOWED_METHODS));
    response_headers.insert(header::VARY, HeaderValue::from_static("Access-Control-Request-Headers"));
    if let Some(requested) = headers.get(header::ACCESS_CONTROL_REQUEST_HEADERS) {
        response_headers.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, requested.clone());
    }
    response_headers.insert(header::CONTENT_LENGTH, HeaderValue::from_static("0"));
    response
}

#[cfg(test)]
mod tests;
