use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use mongodb::bson::{Document, doc};

use super::{
    cache::{Cache, CacheSettings, NoStats},
    http::{HttpRequest, HttpResponse, HttpTransport, TransportError, Verb},
    store::{DocumentStore, StoreError},
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
        cache_max_age: 43_200,
    };
    Arc::new(Cache::new(settings, None, Arc::new(NoStats)))
}

#[derive(Default)]
pub struct MemoryDocuments {
    documents: Mutex<HashMap<String, Document>>,
}

impl MemoryDocuments {
    pub fn insert(&self, document: Document) {
        let key = document.get_str("key").expect("a stored document has a key").to_owned();
        self.documents.lock().unwrap().insert(key, document);
    }

    pub fn document(&self, key: &str) -> Option<Document> {
        self.documents.lock().unwrap().get(key).cloned()
    }
}

#[async_trait]
impl DocumentStore for MemoryDocuments {
    async fn find_one(&self, key: &str) -> Result<Option<Document>, StoreError> {
        Ok(self.document(key))
    }

    async fn find_many(&self, keys: &[String]) -> Result<Vec<Document>, StoreError> {
        let documents = self.documents.lock().unwrap();
        Ok(documents.iter().filter(|(key, _)| keys.contains(key)).map(|(_, document)| document.clone()).collect())
    }

    async fn upsert(&self, key: &str, document: Document) -> Result<(), StoreError> {
        let mut documents = self.documents.lock().unwrap();
        let stored = documents.entry(key.to_owned()).or_insert_with(|| doc! { "key": key });
        stored.extend(document);
        Ok(())
    }
}
