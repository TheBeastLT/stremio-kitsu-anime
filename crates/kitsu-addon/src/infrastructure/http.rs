use std::time::Duration;

use async_trait::async_trait;
use reqwest::{
    Client, Method,
    header::{ACCEPT, USER_AGENT},
    redirect::Policy,
};

pub use super::kitsu::TransportError;

pub const AXIOS_USER_AGENT: &str = "axios/1.8.2";
pub const AXIOS_ACCEPT: &str = "application/json, text/plain, */*";
const MAX_REDIRECTS: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Verb {
    Get,
    Head,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub verb: Verb,
    pub url: String,
    pub accept: Option<&'static str>,
    pub user_agent: Option<&'static str>,
    pub timeout: Option<Duration>,
}

impl HttpRequest {
    pub fn axios(url: String, timeout: Option<Duration>) -> Self {
        Self { verb: Verb::Get, url, accept: Some(AXIOS_ACCEPT), user_agent: Some(AXIOS_USER_AGENT), timeout }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

#[async_trait]
pub trait HttpTransport: Send + Sync {
    async fn send(&self, request: &HttpRequest) -> Result<HttpResponse, TransportError>;
}

pub struct ReqwestHttp {
    http: Client,
}

impl ReqwestHttp {
    pub fn new() -> Result<Self, reqwest::Error> {
        let http = Client::builder().redirect(Policy::limited(MAX_REDIRECTS)).build()?;
        Ok(Self { http })
    }
}

#[async_trait]
impl HttpTransport for ReqwestHttp {
    async fn send(&self, request: &HttpRequest) -> Result<HttpResponse, TransportError> {
        let method = match request.verb {
            Verb::Get => Method::GET,
            Verb::Head => Method::HEAD,
        };
        let mut builder = self.http.request(method, &request.url);
        if let Some(accept) = request.accept {
            builder = builder.header(ACCEPT, accept);
        }
        if let Some(user_agent) = request.user_agent {
            builder = builder.header(USER_AGENT, user_agent);
        }
        if let Some(timeout) = request.timeout {
            builder = builder.timeout(timeout);
        }
        let describe = |error: &reqwest::Error| match request.timeout {
            Some(timeout) if error.is_timeout() => {
                TransportError(format!("timeout of {}ms exceeded", timeout.as_millis()))
            }
            _ => TransportError(error.to_string()),
        };
        let response = builder.send().await.map_err(|error| describe(&error))?;
        let status = response.status().as_u16();
        let body = response.bytes().await.map_err(|error| describe(&error))?;
        Ok(HttpResponse { status, body: body.to_vec() })
    }
}
