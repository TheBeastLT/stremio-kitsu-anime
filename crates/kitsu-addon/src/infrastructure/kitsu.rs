use std::{
    collections::HashMap,
    future::Future,
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use reqwest::{
    Client,
    header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderValue},
};
use serde_json::{Map, Value, json};

use super::upstream::{Outcome, UpstreamObserver};
use crate::application::Error;

pub mod jsonapi;

pub const KITSU_URL: &str = "https://kitsu.io/api/edge";
pub const KITSU_TIMEOUT: Duration = Duration::from_secs(30);
pub const PAGE_SIZE: u64 = 20;
const UPSTREAM: &str = "kitsu";
const JSON_API: &str = "application/vnd.api+json";
const USER_AGENT: &str = "axios/0.27.2";
const ANIME_SUBTYPES: &str = "TV,OVA,ONA,movie,special";
const DEFAULT_STATUS: &str = "finished,current,upcoming";
const TITLE_FIELDS: &str = "canonicalTitle,titles,abbreviatedTitles,subtype,status,startDate,episodeCount";
const TRENDING_LIMIT: u64 = 50;
const ANIME_DATA_INCLUDE: &str = "genres,episodes,mediaRelationships.destination";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KitsuResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct TransportError(pub String);

#[async_trait]
pub trait KitsuTransport: Send + Sync {
    async fn get(&self, path: &str, query: &str) -> Result<KitsuResponse, TransportError>;
}

pub struct ReqwestTransport {
    http: Client,
    base_url: String,
    timeout: Duration,
}

impl ReqwestTransport {
    pub fn new(base_url: &str, timeout: Duration) -> Result<Self, reqwest::Error> {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static(JSON_API));
        headers.insert(CONTENT_TYPE, HeaderValue::from_static(JSON_API));
        let http = Client::builder().timeout(timeout).user_agent(USER_AGENT).default_headers(headers).build()?;
        Ok(Self { http, base_url: base_url.trim_end_matches('/').to_owned(), timeout })
    }

    pub fn url(&self, path: &str, query: &str) -> String {
        full_url(&self.base_url, path, query)
    }

    fn describe(&self, error: &reqwest::Error) -> TransportError {
        if error.is_timeout() {
            TransportError(format!("timeout of {}ms exceeded", self.timeout.as_millis()))
        } else {
            TransportError(error.to_string())
        }
    }
}

pub fn full_url(base_url: &str, path: &str, query: &str) -> String {
    let path = path.trim_start_matches('/');
    if query.is_empty() { format!("{base_url}/{path}") } else { format!("{base_url}/{path}?{query}") }
}

#[async_trait]
impl KitsuTransport for ReqwestTransport {
    async fn get(&self, path: &str, query: &str) -> Result<KitsuResponse, TransportError> {
        let response = self.http.get(self.url(path, query)).send().await.map_err(|error| self.describe(&error))?;
        let status = response.status().as_u16();
        let body = response.bytes().await.map_err(|error| self.describe(&error))?;
        Ok(KitsuResponse { status, body: body.to_vec() })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EntriesOptions {
    pub offset: Option<String>,
    pub genre: Option<String>,
    pub status: Option<String>,
    pub sort: Option<String>,
    pub trending: bool,
    pub limit: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitlePage {
    pub offset: u64,
    pub sort: String,
}

impl Default for TitlePage {
    fn default() -> Self {
        Self { offset: 0, sort: "id".to_owned() }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TitlePageResult {
    pub entries: Vec<Value>,
    pub count: Option<u64>,
}

pub struct KitsuApi {
    transport: Arc<dyn KitsuTransport>,
    observer: Arc<dyn UpstreamObserver>,
}

impl KitsuApi {
    pub fn new(transport: Arc<dyn KitsuTransport>, observer: Arc<dyn UpstreamObserver>) -> Self {
        Self { transport, observer }
    }

    pub async fn anime_entries(&self, options: &EntriesOptions) -> Result<Vec<Value>, Error> {
        if options.trending {
            let params = object(json!({ "limit": options.limit.unwrap_or(TRENDING_LIMIT) }));
            return self.catalog_entries("trending/anime", params).await;
        }
        let mut filter = Map::new();
        let mut page = Map::new();
        if let Some(offset) = non_empty(options.offset.as_ref()) {
            page.insert("offset".to_owned(), offset.into());
        }
        if let Some(genre) = non_empty(options.genre.as_ref()) {
            filter.insert("genres".to_owned(), genre.into());
        }
        if let Some(status) = non_empty(options.status.as_ref()) {
            filter.insert("status".to_owned(), status.into());
        }
        let mut params = Map::new();
        params.insert("filter".to_owned(), Value::Object(filter));
        params.insert("page".to_owned(), Value::Object(page));
        if let Some(sort) = non_empty(options.sort.as_ref()) {
            params.insert("sort".to_owned(), sort.into());
        }
        self.catalog_entries("anime", params).await
    }

    pub async fn anime_catalog_metas(&self, ids: &[String]) -> Result<Vec<Option<Value>>, Error> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let params =
            object(json!({ "filter": { "id": ids.join(",") }, "include": "genres", "page": { "limit": PAGE_SIZE } }));
        let document = self.timed(self.get("anime", &params)).await?;
        let resources = list(document, "anime")?;
        let mut by_id: HashMap<String, Value> = HashMap::with_capacity(resources.len());
        for resource in resources {
            by_id.insert(jsonapi::js_text(resource.get("id")), resource);
        }
        Ok(ids.iter().map(|id| by_id.get(id).cloned()).collect())
    }

    pub async fn anime_title_page(&self, page: &TitlePage) -> Result<TitlePageResult, Error> {
        let params = object(json!({
            "fields": { "anime": TITLE_FIELDS },
            "sort": page.sort,
            "page": { "limit": PAGE_SIZE, "offset": page.offset },
        }));
        let mut document = self.get("anime", &params).await?;
        let count = document.pointer("/meta/count").and_then(Value::as_u64);
        let entries = list(document.take(), "anime")?;
        Ok(TitlePageResult { entries, count })
    }

    pub async fn anime_data(&self, id: &str) -> Result<Value, Error> {
        let params = object(json!({ "include": ANIME_DATA_INCLUDE }));
        let document = self.timed(self.get(&format!("anime/{id}"), &params)).await?;
        match document {
            Value::Object(mut document) => match document.shift_remove("data") {
                Some(data @ Value::Object(_)) => Ok(data),
                _ => Err(Error::failed(anyhow::anyhow!("Kitsu returned no anime data for {id}"))),
            },
            _ => Err(Error::failed(anyhow::anyhow!("Kitsu returned no anime data for {id}"))),
        }
    }

    async fn catalog_entries(&self, endpoint: &str, mut params: Map<String, Value>) -> Result<Vec<Value>, Error> {
        params.insert("include".to_owned(), "genres".into());
        let filter = object_entry(&mut params, "filter");
        filter.insert("subtype".to_owned(), ANIME_SUBTYPES.into());
        if !jsonapi::truthy(filter.get("status")) {
            filter.insert("status".to_owned(), DEFAULT_STATUS.into());
        }
        object_entry(&mut params, "page").insert("limit".to_owned(), PAGE_SIZE.into());
        let document = self.timed(self.get(endpoint, &params)).await?;
        list(document, endpoint)
    }

    async fn get(&self, path: &str, params: &Map<String, Value>) -> Result<Value, Error> {
        let response = self
            .transport
            .get(path, &jsonapi::query(params))
            .await
            .map_err(|error| Error::failed(anyhow::anyhow!(error.0)))?;
        if !(200..300).contains(&response.status) {
            return Err(rejection(&response));
        }
        match jsonapi::parse(&response.body) {
            Ok(document) => jsonapi::deserialise(document).map_err(Error::failed),
            Err(_) => Ok(Value::String(String::from_utf8_lossy(&response.body).into_owned())),
        }
    }

    async fn timed<T>(&self, request: impl Future<Output = Result<T, Error>>) -> Result<T, Error> {
        let started = Instant::now();
        let result = request.await;
        let outcome = if result.is_ok() { Outcome::Ok } else { Outcome::Error };
        self.observer.observe(UPSTREAM, outcome, started.elapsed());
        result
    }
}

fn rejection(response: &KitsuResponse) -> Error {
    let text = String::from_utf8_lossy(&response.body).into_owned();
    let body = jsonapi::parse(&response.body).ok();
    let not_found = body
        .as_ref()
        .and_then(|body| body.get("errors"))
        .and_then(Value::as_array)
        .and_then(|errors| errors.iter().find(|error| jsonapi::js_text(error.get("status")) == "404"));
    if let Some(error) = not_found {
        let detail = ["detail", "title"].iter().find_map(|field| error.get(*field).and_then(Value::as_str));
        return Error::NotFound(detail.map_or_else(|| text.clone(), str::to_owned));
    }
    let message = match body {
        Some(body) if jsonapi::truthy(Some(&body)) => body.to_string(),
        _ if !text.is_empty() => text,
        _ => format!("Request failed with status code {}", response.status),
    };
    Error::failed(anyhow::anyhow!(message))
}

fn list(document: Value, endpoint: &str) -> Result<Vec<Value>, Error> {
    match document {
        Value::Object(mut document) => match document.shift_remove("data") {
            Some(Value::Array(resources)) => Ok(resources),
            _ => Err(Error::failed(anyhow::anyhow!("Kitsu returned no {endpoint} list"))),
        },
        _ => Err(Error::failed(anyhow::anyhow!("Kitsu returned no {endpoint} list"))),
    }
}

fn object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

fn object_entry<'a>(params: &'a mut Map<String, Value>, key: &str) -> &'a mut Map<String, Value> {
    let entry = params.entry(key).or_insert_with(|| Value::Object(Map::new()));
    if !entry.is_object() {
        *entry = Value::Object(Map::new());
    }
    entry.as_object_mut().expect("object entry")
}

fn non_empty(value: Option<&String>) -> Option<&str> {
    value.map(String::as_str).filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests;
