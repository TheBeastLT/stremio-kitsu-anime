use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use moka::{Expiry, future::Cache as Moka, policy::EvictionPolicy};
use serde_json::Value;
use singleflight_async::SingleFlight;
use tokio::{sync::Semaphore, time::Instant};
use tokio_util::task::TaskTracker;

use super::{
    Config,
    store::{self, Store, StoreError, Stored},
};

pub const ZSTD_LEVEL: i32 = 3;
const MAX_BACKGROUND_WRITES: usize = 256;
const DRAIN_TIMEOUT: Duration = Duration::from_secs(10);
const DAY: Duration = Duration::from_secs(24 * 60 * 60);
const IMAGES_TTL: Duration = DAY.saturating_mul(14);
const IMAGES_NON_EN_TTL: Duration = DAY.saturating_mul(4);
const IMAGES_NULL_TTL: Duration = DAY.saturating_mul(2);
const ID_MAPPING_TTL: Duration = IMAGES_TTL;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Meta,
    Catalog,
    Images,
    IdMapping,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Meta => "meta",
            Self::Catalog => "catalog",
            Self::Images => "images",
            Self::IdMapping => "id_mapping",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StoreOp {
    Get,
    Set,
    Mget,
}

impl StoreOp {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Get => "get",
            Self::Set => "set",
            Self::Mget => "mget",
        }
    }
}

pub trait CacheStats: Send + Sync {
    fn hit(&self, kind: Kind);
    fn miss(&self, kind: Kind);
    fn observe(&self, op: StoreOp, duration: Duration);
}

pub struct NoStats;

impl CacheStats for NoStats {
    fn hit(&self, _kind: Kind) {}
    fn miss(&self, _kind: Kind) {}
    fn observe(&self, _op: StoreOp, _duration: Duration) {}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheSettings {
    pub enabled: bool,
    pub budget_bytes: u64,
    pub meta_ttl: Duration,
    pub catalog_ttl: Duration,
}

impl CacheSettings {
    pub fn from_config(config: &Config) -> Self {
        Self {
            enabled: !config.no_cache,
            budget_bytes: config.memory_cache_mb * 1024 * 1024,
            meta_ttl: Duration::from_millis(config.meta_ttl_ms),
            catalog_ttl: Duration::from_millis(config.catalog_ttl_ms),
        }
    }
}

#[derive(Clone)]
struct Packed(Arc<[u8]>);

impl Packed {
    fn encode(value: &Value) -> anyhow::Result<Self> {
        Ok(Self(zstd::encode_all(serde_json::to_vec(value)?.as_slice(), ZSTD_LEVEL)?.into()))
    }

    fn decode(&self) -> anyhow::Result<Value> {
        Ok(serde_json::from_slice(&zstd::decode_all(self.0.as_ref())?)?)
    }
}

#[derive(Clone)]
struct Entry {
    packed: Packed,
    ttl: Duration,
    deadline: Instant,
}

struct PerEntryTtl;

impl Expiry<String, Entry> for PerEntryTtl {
    fn expire_after_create(&self, _key: &String, entry: &Entry, _created_at: std::time::Instant) -> Option<Duration> {
        Some(entry.ttl)
    }
}

fn memory(settings: &CacheSettings) -> Moka<String, Entry> {
    tracing::info!(memory_mb = settings.budget_bytes / 1024 / 1024, "Cache sizes");
    Moka::builder()
        .max_capacity(settings.budget_bytes)
        .weigher(|key: &String, entry: &Entry| u32::try_from(key.len() + entry.packed.0.len()).unwrap_or(u32::MAX))
        .eviction_policy(EvictionPolicy::lru())
        .expire_after(PerEntryTtl)
        .build()
}

pub struct Cache<E> {
    settings: CacheSettings,
    memory: Moka<String, Entry>,
    store: Option<Store>,
    stats: Arc<dyn CacheStats>,
    flight: SingleFlight<String, Result<Arc<Value>, E>>,
    tasks: TaskTracker,
    write_permits: Arc<Semaphore>,
}

impl<E: Clone> Cache<E> {
    pub fn new(settings: CacheSettings, store: Option<Store>, stats: Arc<dyn CacheStats>) -> Self {
        Self {
            memory: memory(&settings),
            settings,
            store,
            stats,
            flight: SingleFlight::new(),
            tasks: TaskTracker::new(),
            write_permits: Arc::new(Semaphore::new(MAX_BACKGROUND_WRITES)),
        }
    }

    pub async fn wrap_meta<Fut>(&self, id: &str, loader: impl FnOnce() -> Fut) -> Result<Arc<Value>, E>
    where
        Fut: Future<Output = Result<Value, E>>,
    {
        self.wrap(Kind::Meta, store::meta_key(id), loader).await
    }

    pub async fn wrap_catalog<Fut>(
        &self,
        catalog_id: &str,
        genre: &str,
        skip: &str,
        loader: impl FnOnce() -> Fut,
    ) -> Result<Arc<Value>, E>
    where
        Fut: Future<Output = Result<Value, E>>,
    {
        self.wrap(Kind::Catalog, store::catalog_key(catalog_id, genre, skip), loader).await
    }

    pub async fn wrap_images<Fut>(&self, id: &str, loader: impl FnOnce() -> Fut) -> Result<Arc<Value>, E>
    where
        Fut: Future<Output = Result<Value, E>>,
    {
        self.wrap(Kind::Images, store::images_key(id), loader).await
    }

    pub async fn wrap_id_mapping<Fut>(&self, full_id: &str, loader: impl FnOnce() -> Fut) -> Result<Arc<Value>, E>
    where
        Fut: Future<Output = Result<Value, E>>,
    {
        self.wrap(Kind::IdMapping, store::id_mapping_key(full_id), loader).await
    }

    pub async fn cached_metas(&self, ids: &[String]) -> Vec<Option<Arc<Value>>> {
        if !self.settings.enabled {
            return vec![None; ids.len()];
        }
        let keys: Vec<String> = ids.iter().map(|id| store::meta_key(id)).collect();
        let mut found = Vec::with_capacity(keys.len());
        for key in &keys {
            found.push(self.remembered(key).await);
        }
        let missing: Vec<usize> = (0..keys.len()).filter(|&index| found[index].is_none()).collect();
        let Some(store) = &self.store else { return found };
        if missing.is_empty() {
            return found;
        }
        let missing_keys: Vec<String> = missing.iter().map(|&index| keys[index].clone()).collect();
        let started = std::time::Instant::now();
        let result = store.mget(&missing_keys).await;
        self.stats.observe(StoreOp::Mget, started.elapsed());
        match result {
            Ok(entries) => {
                for (index, entry) in missing.into_iter().zip(entries) {
                    if let Some(stored) = entry {
                        found[index] = Some(self.promote(Kind::Meta, keys[index].clone(), stored).await);
                    }
                }
            }
            Err(error) => tracing::warn!(%error, "Failed to read cached metas"),
        }
        found
    }

    pub fn memory_entries(&self) -> u64 {
        self.memory.entry_count()
    }

    pub fn memory_weight(&self) -> u64 {
        self.memory.weighted_size()
    }

    pub async fn run_pending_tasks(&self) {
        self.memory.run_pending_tasks().await;
    }

    pub async fn drain(&self) {
        self.tasks.close();
        if tokio::time::timeout(DRAIN_TIMEOUT, self.tasks.wait()).await.is_err() {
            tracing::warn!("Timed out waiting for cache writes to finish");
        }
    }

    async fn wrap<Fut>(&self, kind: Kind, key: String, loader: impl FnOnce() -> Fut) -> Result<Arc<Value>, E>
    where
        Fut: Future<Output = Result<Value, E>>,
    {
        if !self.settings.enabled {
            return loader().await.map(Arc::new);
        }
        if let Some(value) = self.remembered(&key).await {
            self.stats.hit(kind);
            return Ok(value);
        }
        let missed = AtomicBool::new(false);
        let result = self.flight.work(key.clone(), || self.lookup_or_load(kind, key, loader, &missed)).await;
        if result.is_ok() {
            if missed.load(Ordering::Relaxed) {
                self.stats.miss(kind);
            } else {
                self.stats.hit(kind);
            }
        }
        result
    }

    async fn lookup_or_load<Fut>(
        &self,
        kind: Kind,
        key: String,
        loader: impl FnOnce() -> Fut,
        missed: &AtomicBool,
    ) -> Result<Arc<Value>, E>
    where
        Fut: Future<Output = Result<Value, E>>,
    {
        if let Some(value) = self.remembered(&key).await {
            return Ok(value);
        }
        if let Some(stored) = self.stored(&key).await {
            return Ok(self.promote(kind, key, stored).await);
        }
        missed.store(true, Ordering::Relaxed);
        let value = Arc::new(loader().await?);
        let ttl = self.ttl(kind, &value);
        self.remember(key.clone(), &value, ttl).await;
        self.persist(key, Arc::clone(&value), ttl);
        Ok(value)
    }

    async fn remembered(&self, key: &str) -> Option<Arc<Value>> {
        let entry = self.memory.get(key).await?;
        if entry.deadline <= Instant::now() {
            self.memory.invalidate(key).await;
            return None;
        }
        match entry.packed.decode() {
            Ok(value) => Some(Arc::new(value)),
            Err(error) => {
                tracing::warn!(key, %error, "Failed to decode a memory cache entry");
                self.memory.invalidate(key).await;
                None
            }
        }
    }

    async fn stored(&self, key: &str) -> Option<Stored> {
        let store = self.store.as_ref()?;
        let started = std::time::Instant::now();
        let result = store.get(key).await;
        self.stats.observe(StoreOp::Get, started.elapsed());
        result.unwrap_or_else(|error| {
            tracing::warn!(key, %error, "Failed to read the cache");
            None
        })
    }

    async fn promote(&self, kind: Kind, key: String, stored: Stored) -> Arc<Value> {
        let value = Arc::new(stored.value);
        let ttl = match stored.expires {
            Some(expires) => Duration::from_millis(expires.saturating_sub(store::now_ms()).max(0) as u64),
            None => self.ttl(kind, &value),
        };
        if !ttl.is_zero() {
            self.remember(key, &value, ttl).await;
        }
        value
    }

    async fn remember(&self, key: String, value: &Value, ttl: Duration) {
        match Packed::encode(value) {
            Ok(packed) => self.memory.insert(key, Entry { packed, ttl, deadline: Instant::now() + ttl }).await,
            Err(error) => tracing::warn!(key, %error, "Failed to encode a memory cache entry"),
        }
    }

    fn persist(&self, key: String, value: Arc<Value>, ttl: Duration) {
        let Some(store) = self.store.clone() else { return };
        let Ok(permit) = Arc::clone(&self.write_permits).try_acquire_owned() else {
            tracing::warn!(key, "Cache write dropped, too many writes in flight");
            return;
        };
        let stats = Arc::clone(&self.stats);
        self.tasks.spawn(async move {
            let started = std::time::Instant::now();
            let result: Result<(), StoreError> = store.set(&key, &value, ttl).await;
            stats.observe(StoreOp::Set, started.elapsed());
            if let Err(error) = result {
                tracing::warn!(key, %error, "Failed to write the cache");
            }
            drop(permit);
        });
    }

    fn ttl(&self, kind: Kind, value: &Value) -> Duration {
        match kind {
            Kind::Meta => self.settings.meta_ttl,
            Kind::Catalog => self.settings.catalog_ttl,
            Kind::Images => images_ttl(value),
            Kind::IdMapping => ID_MAPPING_TTL,
        }
    }
}

pub fn images_ttl(images: &Value) -> Duration {
    let logo_lang = images.get("logoLang");
    if logo_lang.and_then(Value::as_str) == Some("en") {
        IMAGES_TTL
    } else if logo_lang.is_some_and(truthy) || images.get("logo").is_some_and(truthy) {
        IMAGES_NON_EN_TTL
    } else {
        IMAGES_NULL_TTL
    }
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|number| number != 0.0 && !number.is_nan()),
        Value::String(text) => !text.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

#[cfg(test)]
mod tests;
