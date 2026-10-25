use std::{sync::Arc, time::Duration};

use serde_json::Value;

use super::{
    cache::Cache,
    http::{HttpRequest, HttpTransport},
    js,
    kitsu::jsonapi,
};
use crate::application::Error;

pub const YUNA_URL: &str = "https://relations.yuna.moe/api/v2/ids";
pub const YUNA_TIMEOUT: Duration = Duration::from_secs(30);

pub struct Ids<E> {
    http: Arc<dyn HttpTransport>,
    cache: Arc<Cache<E>>,
}

impl<E: Clone + From<Error>> Ids<E> {
    pub fn new(http: Arc<dyn HttpTransport>, cache: Arc<Cache<E>>) -> Self {
        Self { http, cache }
    }

    pub async fn to_kitsu(&self, full_id: &str) -> Result<String, E> {
        let mut parts = full_id.split(':');
        let id_type = parts.next().unwrap_or_default();
        let id = parts.next().unwrap_or("undefined");
        if id_type == "kitsu" {
            return Ok(id.to_owned());
        }
        let kitsu_id = self.cache.wrap_id_mapping(full_id, || self.query(id_type, id)).await?;
        Ok(jsonapi::js_text(Some(&kitsu_id)))
    }

    async fn query(&self, id_type: &str, id: &str) -> Result<Value, E> {
        let not_found = || E::from(Error::NotFound(format!("No kitsu id found for: {id_type}:{id}")));
        if js::parse_int(id, None).is_some_and(|number| number <= 0.0) {
            return Err(not_found());
        }
        let source = if id_type == "mal" { "myanimelist" } else { id_type };
        let url = format!("{YUNA_URL}?source={source}&id={id}&include=kitsu");
        let response = self
            .http
            .send(&HttpRequest::axios(url, Some(YUNA_TIMEOUT)))
            .await
            .map_err(|error| E::from(Error::failed(anyhow::anyhow!(error.0))))?;
        if !response.is_success() {
            let message = format!("Request failed with status code {}", response.status);
            return Err(E::from(Error::failed(anyhow::anyhow!(message))));
        }
        match jsonapi::parse(&response.body) {
            Ok(Value::Object(mut data)) => {
                data.shift_remove("kitsu").filter(|kitsu| jsonapi::truthy(Some(kitsu))).ok_or_else(not_found)
            }
            _ => Err(not_found()),
        }
    }
}

#[cfg(test)]
mod tests;
