use std::{sync::Arc, time::Duration};

use tokio::time::Instant;

use serde_json::{Value, json};

use super::{
    http::{HttpRequest, HttpTransport, TransportError},
    kitsu::jsonapi,
    upstream::{Outcome, UpstreamObserver},
};

pub const CINEMETA_URL: &str = "https://v3-cinemeta.strem.io";
pub const CINEMETA_TIMEOUT: Duration = Duration::from_secs(60);
const UPSTREAM: &str = "cinemeta";
const RETRIES: u32 = 3;
const RETRY_DELAY: Duration = Duration::from_millis(500);
const NO_RESULTS: &str = "No search results";

#[derive(Debug, Clone, PartialEq)]
pub struct Fetched {
    pub meta: Value,
    pub complete: bool,
}

pub struct Cinemeta {
    http: Arc<dyn HttpTransport>,
    observer: Arc<dyn UpstreamObserver>,
}

impl Cinemeta {
    pub fn new(http: Arc<dyn HttpTransport>, observer: Arc<dyn UpstreamObserver>) -> Self {
        Self { http, observer }
    }

    pub async fn get(&self, imdb_id: &str, deadline: Instant) -> Fetched {
        let lookup = async {
            match self.metadata(imdb_id, "series", deadline).await {
                Ok(meta) => Ok(meta),
                Err(_) => self.metadata(imdb_id, "movie", deadline).await,
            }
        };
        let (error, complete) = match tokio::time::timeout_at(deadline, lookup).await {
            Ok(Ok(meta)) => return Fetched { meta, complete: true },
            Ok(Err(Failure::Final(message))) => (message, true),
            Ok(Err(Failure::Transient(message))) => (message, false),
            Err(_) => ("the meta deadline passed".to_owned(), false),
        };
        tracing::warn!("failed cinemeta query {imdb_id} due: {error}");
        Fetched { meta: json!({ "id": imdb_id, "imdb_id": imdb_id }), complete }
    }

    async fn metadata(&self, imdb_id: &str, kind: &str, deadline: Instant) -> Result<Value, Failure> {
        let url = format!("{CINEMETA_URL}/meta/{kind}/{imdb_id}.json");
        let mut retry = 0;
        loop {
            match self.attempt(&url, deadline).await {
                Err(Failure::Transient(_)) if retry < RETRIES => {
                    retry += 1;
                    tokio::time::sleep(RETRY_DELAY * retry).await;
                }
                result => return result,
            }
        }
    }

    async fn attempt(&self, url: &str, deadline: Instant) -> Result<Value, Failure> {
        let started = std::time::Instant::now();
        let timeout = CINEMETA_TIMEOUT.min(deadline.saturating_duration_since(Instant::now()) / 2);
        let request = HttpRequest::axios(url.to_owned(), Some(timeout));
        let response = tokio::time::timeout(timeout, self.http.send(&request))
            .await
            .unwrap_or_else(|_| Err(TransportError(format!("timeout of {}ms exceeded", timeout.as_millis()))));
        let response = match response {
            Ok(response) if response.is_success() => Ok(response),
            Ok(response) => Err(Failure::of_status(response.status)),
            Err(error) => Err(Failure::Transient(error.0)),
        }
        .inspect_err(|_| self.observer.observe(UPSTREAM, Outcome::Error, started.elapsed()))?;
        self.observer.observe(UPSTREAM, Outcome::Ok, started.elapsed());
        let Ok(Value::Object(mut data)) = jsonapi::parse(&response.body) else {
            return Err(Failure::Final(NO_RESULTS.to_owned()));
        };
        match data.shift_remove("meta") {
            Some(meta) if jsonapi::truthy(meta.get("imdb_id")) => Ok(meta),
            _ => Err(Failure::Final(NO_RESULTS.to_owned())),
        }
    }
}

enum Failure {
    Transient(String),
    Final(String),
}

impl Failure {
    fn of_status(status: u16) -> Self {
        let message = format!("Request failed with status code {status}");
        if status == 429 || status >= 500 { Self::Transient(message) } else { Self::Final(message) }
    }
}

#[cfg(test)]
mod tests;
