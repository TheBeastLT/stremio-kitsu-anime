use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;

use super::{
    cache::{Cache, CacheSettings, NoStats},
    http::{HttpRequest, HttpResponse, HttpTransport, TransportError, Verb},
    upstream::{Outcome, UpstreamObserver},
};

type Reply = Result<HttpResponse, TransportError>;

#[derive(Default)]
pub struct FakeHttp {
    replies: Mutex<HashMap<(Verb, String), VecDeque<Reply>>>,
    requests: Mutex<Vec<HttpRequest>>,
}

impl FakeHttp {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn reply(&self, verb: Verb, url: &str, status: u16, body: &str) {
        self.push(verb, url, Ok(HttpResponse { status, body: body.as_bytes().to_vec() }));
    }

    pub fn fail(&self, verb: Verb, url: &str, message: &str) {
        self.push(verb, url, Err(TransportError(message.to_owned())));
    }

    fn push(&self, verb: Verb, url: &str, reply: Reply) {
        self.replies.lock().unwrap().entry((verb, url.to_owned())).or_default().push_back(reply);
    }

    pub fn requests(&self) -> Vec<HttpRequest> {
        self.requests.lock().unwrap().clone()
    }

    pub fn urls(&self) -> Vec<String> {
        self.requests().into_iter().map(|request| request.url).collect()
    }
}

#[async_trait]
impl HttpTransport for FakeHttp {
    async fn send(&self, request: &HttpRequest) -> Result<HttpResponse, TransportError> {
        self.requests.lock().unwrap().push(request.clone());
        let mut replies = self.replies.lock().unwrap();
        let Some(queue) = replies.get_mut(&(request.verb, request.url.clone())) else {
            return Ok(HttpResponse { status: 404, body: Vec::new() });
        };
        if queue.len() > 1 { queue.pop_front().unwrap() } else { queue.front().cloned().unwrap() }
    }
}

#[derive(Default)]
pub struct Observed(Mutex<Vec<(&'static str, Outcome)>>);

impl Observed {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn outcomes(&self) -> Vec<(&'static str, Outcome)> {
        self.0.lock().unwrap().clone()
    }
}

impl UpstreamObserver for Observed {
    fn observe(&self, upstream: &'static str, outcome: Outcome, _duration: Duration) {
        self.0.lock().unwrap().push((upstream, outcome));
    }
}

pub fn memory_cache<E: Clone>() -> Arc<Cache<E>> {
    let settings = CacheSettings {
        enabled: true,
        budget_bytes: 1024 * 1024,
        meta_ttl: Duration::from_secs(60),
        catalog_ttl: Duration::from_secs(60),
    };
    Arc::new(Cache::new(settings, None, Arc::new(NoStats)))
}
