use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use serde_json::{Value, json};

use super::{
    http::{HttpRequest, HttpTransport},
    kitsu::jsonapi,
    upstream::{Outcome, UpstreamObserver},
};

pub const CINEMETA_URL: &str = "https://v3-cinemeta.strem.io";
pub const CINEMETA_TIMEOUT: Duration = Duration::from_secs(60);
const UPSTREAM: &str = "cinemeta";
const RETRIES: usize = 3;
const NO_RESULTS: &str = "No search results";

pub struct Cinemeta {
    http: Arc<dyn HttpTransport>,
    observer: Arc<dyn UpstreamObserver>,
}

impl Cinemeta {
    pub fn new(http: Arc<dyn HttpTransport>, observer: Arc<dyn UpstreamObserver>) -> Self {
        Self { http, observer }
    }

    pub async fn get(&self, imdb_id: &str) -> Value {
        let error = match self.metadata(imdb_id, "series").await {
            Ok(meta) => return meta,
            Err(_) => match self.metadata(imdb_id, "movie").await {
                Ok(meta) => return meta,
                Err(error) => error,
            },
        };
        tracing::warn!("failed cinemeta query {imdb_id} due: {error}");
        json!({ "id": imdb_id, "imdb_id": imdb_id })
    }

    async fn metadata(&self, imdb_id: &str, kind: &str) -> Result<Value, String> {
        let url = format!("{CINEMETA_URL}/meta/{kind}/{imdb_id}.json");
        let mut result = self.attempt(&url).await;
        for _ in 0..RETRIES {
            if result.is_ok() {
                break;
            }
            result = self.attempt(&url).await;
        }
        result
    }

    async fn attempt(&self, url: &str) -> Result<Value, String> {
        let started = Instant::now();
        let response = self.http.send(&HttpRequest::axios(url.to_owned(), Some(CINEMETA_TIMEOUT))).await;
        let response = match response {
            Ok(response) if response.is_success() => Ok(response),
            Ok(response) => Err(format!("Request failed with status code {}", response.status)),
            Err(error) => Err(error.0),
        }
        .inspect_err(|_| self.observer.observe(UPSTREAM, Outcome::Error, started.elapsed()))?;
        self.observer.observe(UPSTREAM, Outcome::Ok, started.elapsed());
        let Ok(Value::Object(mut data)) = jsonapi::parse(&response.body) else { return Err(NO_RESULTS.to_owned()) };
        match data.shift_remove("meta") {
            Some(meta) if jsonapi::truthy(meta.get("imdb_id")) => Ok(meta),
            _ => Err(NO_RESULTS.to_owned()),
        }
    }
}

#[cfg(test)]
mod tests;
