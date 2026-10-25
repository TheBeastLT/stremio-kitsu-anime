use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::{
    cache::Cache,
    http::{AXIOS_USER_AGENT, HttpRequest, HttpResponse, HttpTransport, Verb},
    js,
    kitsu::jsonapi,
    upstream::{Outcome, UpstreamObserver},
};

pub const FANART_URL: &str = "https://webservice.fanart.tv/v3/";
pub const TVDB_URL: &str = "https://thetvdb.com/api/GetSeriesByRemoteID.php";
pub const METAHUB_URL: &str = "https://images.metahub.space";
pub const SAFETY_TIMEOUT: Duration = Duration::from_secs(30);
const UPSTREAM: &str = "fanart";
const GOT_USER_AGENT: &str = "got/7.1.0 (https://github.com/sindresorhus/got)";
const GOT_RETRIES: u32 = 2;
const PREFERRED_LANGS: [&str; 2] = ["en", "00"];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MappingInfo {
    pub imdb_id: Option<String>,
    pub tmdb_id: Option<String>,
    pub tvdb_id: Option<String>,
    pub fanart_logo_id: Option<String>,
    pub from_season: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Images {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,
    #[serde(rename = "logoLang", skip_serializing_if = "Option::is_none")]
    pub logo_lang: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub poster: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<String>,
}

type Fields = Vec<(&'static str, Option<Value>)>;

pub struct Fanart<E> {
    http: Arc<dyn HttpTransport>,
    observer: Arc<dyn UpstreamObserver>,
    cache: Arc<Cache<E>>,
    api_key: String,
}

impl<E: Clone> Fanart<E> {
    pub fn new(
        http: Arc<dyn HttpTransport>,
        observer: Arc<dyn UpstreamObserver>,
        cache: Arc<Cache<E>>,
        api_key: String,
    ) -> Self {
        Self { http, observer, cache, api_key }
    }

    pub async fn images(&self, kitsu_id: &str, mapping: Option<&MappingInfo>) -> Images {
        let Some(mapping) = mapping.filter(|mapping| {
            has(mapping.imdb_id.as_ref()) || has(mapping.tmdb_id.as_ref()) || has(mapping.tvdb_id.as_ref())
        }) else {
            return Images::default();
        };
        let key =
            if has(mapping.tmdb_id.as_ref()) || has(mapping.tvdb_id.as_ref()) || has(mapping.fanart_logo_id.as_ref()) {
                kitsu_id
            } else {
                mapping.imdb_id.as_deref().unwrap_or_default()
            };
        let Ok(images) = self.cache.wrap_images(key, || async { Ok(self.retrieve(mapping).await) }).await else {
            return Images::default();
        };
        serde_json::from_value(Value::clone(&images)).unwrap_or_else(|error| {
            tracing::warn!(key, %error, "Failed reading cached images");
            Images::default()
        })
    }

    pub async fn tvdb_id(&self, mapping: &MappingInfo) -> Result<String, String> {
        if let Some(tvdb_id) = mapping.tvdb_id.as_ref().filter(|id| !id.is_empty()) {
            return Ok(tvdb_id.clone());
        }
        let request = HttpRequest {
            verb: Verb::Get,
            url: format!("{TVDB_URL}?imdbid={}", text(mapping.imdb_id.as_ref())),
            accept: Some("application/xml"),
            user_agent: Some(AXIOS_USER_AGENT),
            timeout: Some(SAFETY_TIMEOUT),
        };
        let started = Instant::now();
        let response = match self.http.send(&request).await {
            Ok(response) if response.is_success() => Ok(response),
            Ok(response) => Err(format!("Request failed with status code {}", response.status)),
            Err(error) => Err(error.0),
        };
        let outcome = if response.is_ok() { Outcome::Ok } else { Outcome::Error };
        self.observer.observe(UPSTREAM, outcome, started.elapsed());
        tvdb_series_id(&String::from_utf8_lossy(&response?.body))
    }

    async fn retrieve(&self, mapping: &MappingInfo) -> Value {
        let mut fields = match self.series_images(mapping).await {
            Ok(fields) => fields,
            Err(_) => self.movie_images(mapping).await.unwrap_or_default(),
        };
        let imdb_id = text(mapping.imdb_id.as_ref());
        for (field, size) in [("logo", "medium"), ("background", "medium"), ("poster", "small")] {
            let position = fields.iter().position(|(name, _)| *name == field);
            if position.is_some_and(|index| jsonapi::truthy(fields[index].1.as_ref())) {
                continue;
            }
            let found = self.exists(format!("{METAHUB_URL}/{field}/{size}/{imdb_id}/img")).await.map(Value::String);
            match position {
                Some(index) => fields[index].1 = found,
                None => fields.push((field, found)),
            }
        }
        Value::Object(
            fields.into_iter().filter_map(|(name, value)| value.map(|value| (name.to_owned(), value))).collect(),
        )
    }

    async fn series_images(&self, mapping: &MappingInfo) -> Result<Fields, String> {
        let tvdb_id = self.tvdb_id(mapping).await?;
        let response = self.fanart(&format!("tv/{tvdb_id}")).await?;
        let thetvdb_id = prop(&response, "thetvdb_id")?;
        let valid = jsonapi::truthy(thetvdb_id) && thetvdb_id.and_then(Value::as_str) != Some("0");
        let empty = Map::new();
        let response = if valid { response.as_object().unwrap_or(&empty) } else { &empty };
        let (hd, clear) = (response.get("hdtvlogo"), response.get("clearlogo"));
        Ok(vec![
            ("logo", logo_url(mapping, hd, clear)?.map(Value::String)),
            ("logoLang", logo_lang(mapping, hd, clear)?),
            ("poster", first_url(response.get("tvposter"))?.map(Value::String)),
            ("background", first_url(response.get("showbackground"))?.map(Value::String)),
            ("thumbnail", thumbnail_url(response.get("seasonthumb"), response.get("tvthumb"))?.map(Value::String)),
        ])
    }

    async fn movie_images(&self, mapping: &MappingInfo) -> Result<Fields, String> {
        let id = if has(mapping.tmdb_id.as_ref()) { mapping.tmdb_id.as_ref() } else { mapping.imdb_id.as_ref() };
        let response = self.fanart(&format!("movies/{}", text(id))).await?;
        prop(&response, "hdmovielogo")?;
        let empty = Map::new();
        let response = response.as_object().unwrap_or(&empty);
        let (hd, logo) = (response.get("hdmovielogo"), response.get("movielogo"));
        Ok(vec![
            ("logo", logo_url(mapping, hd, logo)?.map(Value::String)),
            ("logoLang", logo_lang(mapping, hd, logo)?),
            ("poster", first_url(response.get("movieposter"))?.map(Value::String)),
            ("background", first_url(response.get("moviebackground"))?.map(Value::String)),
        ])
    }

    async fn fanart(&self, endpoint: &str) -> Result<Value, String> {
        let request = HttpRequest {
            verb: Verb::Get,
            url: format!("{FANART_URL}{endpoint}?api_key={}", jsonapi::encode_uri_component(&self.api_key)),
            accept: Some("application/json"),
            user_agent: Some(GOT_USER_AGENT),
            timeout: Some(SAFETY_TIMEOUT),
        };
        let started = Instant::now();
        let result = self.got(&request).await.and_then(|response| got_body(&response));
        let outcome = if result.is_ok() { Outcome::Ok } else { Outcome::Error };
        self.observer.observe(UPSTREAM, outcome, started.elapsed());
        result
    }

    async fn got(&self, request: &HttpRequest) -> Result<HttpResponse, String> {
        let mut retry = 0;
        loop {
            match self.http.send(request).await {
                Ok(response) => return Ok(response),
                Err(_) if retry < GOT_RETRIES => {
                    retry += 1;
                    tokio::time::sleep(Duration::from_millis(1000 << retry)).await;
                }
                Err(error) => return Err(error.0),
            }
        }
    }

    async fn exists(&self, url: String) -> Option<String> {
        let request =
            HttpRequest { verb: Verb::Head, url, accept: None, user_agent: None, timeout: Some(SAFETY_TIMEOUT) };
        match self.http.send(&request).await {
            Ok(response) if !(400..500).contains(&response.status) => Some(request.url),
            _ => None,
        }
    }
}

fn got_body(response: &HttpResponse) -> Result<Value, String> {
    if !(response.is_success() || response.status == 304) {
        return Err(format!("Response code {}", response.status));
    }
    if response.body.is_empty() {
        return Ok(Value::String(String::new()));
    }
    jsonapi::parse(&response.body).map_err(|error| error.to_string())
}

pub fn tvdb_series_id(xml: &str) -> Result<String, String> {
    let document = roxmltree::Document::parse(xml).map_err(|error| error.to_string())?;
    let nodes = xml_children(document.root()).ok_or("Cannot read properties of undefined (reading '0')")?;
    let Some(series) = xml_children(nodes[0]) else { return Err("no id found".to_owned()) };
    let mut ids = Vec::with_capacity(series.len());
    for node in series {
        let first = xml_children(node).ok_or("Cannot read properties of undefined (reading '0')")?[0];
        let first = xml_children(first).ok_or("Cannot read properties of undefined (reading '0')")?[0];
        ids.push(first.is_text().then(|| first.text().unwrap_or_default().to_owned()));
    }
    let mut defined: Vec<String> = ids.iter().flatten().cloned().collect();
    js_numeric_sort(&mut defined);
    defined.into_iter().next().filter(|id| !id.is_empty()).ok_or_else(|| "no id found".to_owned())
}

fn xml_children<'a, 'input>(node: roxmltree::Node<'a, 'input>) -> Option<Vec<roxmltree::Node<'a, 'input>>> {
    let children: Vec<_> = node
        .children()
        .filter(|child| !child.is_text() || !child.text().unwrap_or_default().trim().is_empty())
        .collect();
    (!children.is_empty()).then_some(children)
}

fn js_numeric_sort(ids: &mut [String]) {
    for index in 1..ids.len() {
        let pivot = js::to_number(&ids[index]);
        let (mut left, mut right) = (0, index);
        while left < right {
            let middle = left + ((right - left) >> 1);
            if pivot - js::to_number(&ids[middle]) < 0.0 {
                right = middle;
            } else {
                left = middle + 1;
            }
        }
        ids[left..=index].rotate_right(1);
    }
}

fn has(value: Option<&String>) -> bool {
    value.is_some_and(|value| !value.is_empty())
}

fn text(value: Option<&String>) -> &str {
    value.map_or("undefined", String::as_str)
}

fn prop<'a>(value: &'a Value, key: &str) -> Result<Option<&'a Value>, String> {
    match value {
        Value::Null => Err(format!("Cannot read properties of null (reading '{key}')")),
        Value::Object(map) => Ok(map.get(key)),
        _ => Ok(None),
    }
}

fn concat(arrays: [Option<&Value>; 2]) -> Vec<&Value> {
    arrays
        .into_iter()
        .flat_map(|array| match array {
            None => Vec::new(),
            Some(Value::Array(items)) => items.iter().collect(),
            Some(other) => vec![other],
        })
        .collect()
}

fn items(array: Option<&Value>) -> Result<Vec<&Value>, String> {
    match array {
        None => Ok(Vec::new()),
        Some(Value::Array(items)) => Ok(items.iter().collect()),
        Some(_) => Err("array.filter is not a function".to_owned()),
    }
}

fn first_entry<'a>(items: &[&'a Value]) -> Result<Option<&'a Value>, String> {
    let mut preferred = None;
    for item in items {
        let lang = prop(item, "lang")?;
        let accepted =
            !jsonapi::truthy(lang) || lang.and_then(Value::as_str).is_some_and(|lang| PREFERRED_LANGS.contains(&lang));
        if accepted && preferred.is_none() {
            preferred = Some(*item);
        }
    }
    Ok(preferred.filter(|entry| jsonapi::truthy(Some(entry))).or_else(|| items.first().copied()))
}

fn logo_entry<'a>(
    mapping: &MappingInfo,
    first: Option<&'a Value>,
    second: Option<&'a Value>,
) -> Result<Option<&'a Value>, String> {
    let merged = concat([first, second]);
    let mut default_logo = None;
    for item in &merged {
        let matches = match (&mapping.fanart_logo_id, prop(item, "id")?) {
            (None, None) => true,
            (Some(logo_id), Some(Value::String(id))) => id == logo_id,
            _ => false,
        };
        if matches {
            default_logo = Some(*item);
            break;
        }
    }
    match default_logo.filter(|entry| jsonapi::truthy(Some(entry))) {
        Some(entry) => Ok(Some(entry)),
        None => first_entry(&merged),
    }
}

fn logo_url(mapping: &MappingInfo, first: Option<&Value>, second: Option<&Value>) -> Result<Option<String>, String> {
    url_of(logo_entry(mapping, first, second)?)
}

fn logo_lang(mapping: &MappingInfo, first: Option<&Value>, second: Option<&Value>) -> Result<Option<Value>, String> {
    match logo_entry(mapping, first, second)? {
        None | Some(Value::Null) => Ok(None),
        Some(entry) => Ok(prop(entry, "lang")?.cloned()),
    }
}

fn first_url(array: Option<&Value>) -> Result<Option<String>, String> {
    url_of(first_entry(&items(array)?)?)
}

fn url_of(entry: Option<&Value>) -> Result<Option<String>, String> {
    match entry {
        None | Some(Value::Null) => Ok(None),
        Some(entry) => to_https(prop(entry, "url")?),
    }
}

fn to_https(url: Option<&Value>) -> Result<Option<String>, String> {
    match url {
        Some(Value::String(url)) if !url.is_empty() => Ok(Some(url.replacen("http://", "https://", 1))),
        url if jsonapi::truthy(url) => Err("url.replace is not a function".to_owned()),
        _ => Ok(None),
    }
}

fn thumbnail_url(seasonal: Option<&Value>, series: Option<&Value>) -> Result<Option<String>, String> {
    let seasonal = items(seasonal)?;
    if seasonal.iter().any(|entry| entry.is_null()) {
        return Err("Cannot set properties of null (setting 'season')".to_owned());
    }
    // Node's `seasonal.filter(e => e.season = season)` assigns and keeps every entry, and `firstUrl` returns a string
    // whose `.url` is undefined, so the seasonal thumbnail never wins: only its TypeErrors remain observable.
    url_of(first_entry(&seasonal)?)?;
    first_url(series)
}

#[cfg(test)]
mod tests;
