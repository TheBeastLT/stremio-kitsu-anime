use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use mongodb::bson::Document;
use serde_json::json;

use super::*;
use crate::infrastructure::{
    store::{DocumentStore, encode_document, now_ms},
    testing::MemoryDocuments,
};

const HOUR: Duration = Duration::from_secs(60 * 60);
const SECOND: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Working,
    Failing,
    Hanging,
    HangingWrites,
}

struct Documents {
    inner: MemoryDocuments,
    mode: Mutex<Mode>,
    reads: AtomicUsize,
    many: Mutex<Vec<Vec<String>>>,
    writes: AtomicUsize,
}

impl Documents {
    fn new(mode: Mode) -> Arc<Self> {
        Arc::new(Self {
            inner: MemoryDocuments::default(),
            mode: Mutex::new(mode),
            reads: AtomicUsize::new(0),
            many: Mutex::new(Vec::new()),
            writes: AtomicUsize::new(0),
        })
    }

    fn mode(&self) -> Mode {
        *self.mode.lock().unwrap()
    }

    fn put(&self, key: &str, value: &Value, expires: i64) {
        self.inner.insert(encode_document(key, value, expires));
    }

    fn reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }

    async fn gate(&self, writing: bool) -> Result<(), StoreError> {
        let mode = self.mode();
        if mode == Mode::Failing {
            return Err(StoreError("connection refused".to_owned()));
        }
        if mode == Mode::Hanging || (mode == Mode::HangingWrites && writing) {
            std::future::pending::<()>().await;
        }
        Ok(())
    }
}

#[async_trait]
impl DocumentStore for Documents {
    async fn find_one(&self, key: &str) -> Result<Option<Document>, StoreError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.gate(false).await?;
        self.inner.find_one(key).await
    }

    async fn find_many(&self, keys: &[String]) -> Result<Vec<Document>, StoreError> {
        self.many.lock().unwrap().push(keys.to_vec());
        self.gate(false).await?;
        self.inner.find_many(keys).await
    }

    async fn upsert(&self, key: &str, document: Document) -> Result<(), StoreError> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        self.gate(true).await?;
        self.inner.upsert(key, document).await
    }
}

#[derive(Default)]
struct Recorder(Mutex<Vec<String>>);

impl Recorder {
    fn events(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }

    fn counted(&self) -> Vec<String> {
        self.events().into_iter().filter(|event| !event.starts_with("observe")).collect()
    }
}

impl CacheStats for Recorder {
    fn hit(&self, kind: Kind) {
        self.0.lock().unwrap().push(format!("hit:{}", kind.as_str()));
    }

    fn miss(&self, kind: Kind) {
        self.0.lock().unwrap().push(format!("miss:{}", kind.as_str()));
    }

    fn observe(&self, op: StoreOp, _duration: Duration) {
        self.0.lock().unwrap().push(format!("observe:{}", op.as_str()));
    }
}

fn settings() -> CacheSettings {
    CacheSettings { enabled: true, budget_bytes: 64 * 1024 * 1024, meta_ttl: DAY, catalog_ttl: HOUR * 2 }
}

struct Fixture {
    cache: Cache<String>,
    documents: Arc<Documents>,
    stats: Arc<Recorder>,
}

fn with_store(mode: Mode) -> Fixture {
    with_settings(settings(), mode)
}

fn with_settings(settings: CacheSettings, mode: Mode) -> Fixture {
    let documents = Documents::new(mode);
    let stats = Arc::new(Recorder::default());
    let store = Store::new(documents.clone(), Duration::from_secs(30));
    let cache = Cache::new(settings, Some(store), stats.clone());
    Fixture { cache, documents, stats }
}

fn memory_only() -> (Cache<String>, Arc<Recorder>) {
    let stats = Arc::new(Recorder::default());
    (Cache::new(settings(), None, stats.clone()), stats)
}

struct Loader {
    calls: AtomicUsize,
}

impl Loader {
    fn new() -> Self {
        Self { calls: AtomicUsize::new(0) }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    async fn ok(&self, value: Value) -> Result<Value, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(10)).await;
        Ok(value)
    }

    async fn err(&self, error: &str) -> Result<Value, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(10)).await;
        Err(error.to_owned())
    }
}

async fn wrap(cache: &Cache<String>, kind: Kind, loader: &Loader, value: &Value) -> Result<Arc<Value>, String> {
    cache.wrap(kind, format!("key:{}", kind.as_str()), || loader.ok(value.clone())).await
}

async fn assert_lives_for(kind: Kind, value: Value, ttl: Duration) {
    let (cache, _) = memory_only();
    let loader = Loader::new();
    wrap(&cache, kind, &loader, &value).await.unwrap();
    tokio::time::advance(ttl.saturating_sub(SECOND)).await;
    wrap(&cache, kind, &loader, &value).await.unwrap();
    assert_eq!(loader.calls(), 1, "expired before {ttl:?}");
    tokio::time::advance(SECOND * 2).await;
    wrap(&cache, kind, &loader, &value).await.unwrap();
    assert_eq!(loader.calls(), 2, "still cached after {ttl:?}");
}

#[tokio::test(start_paused = true)]
async fn meta_lives_for_the_meta_ttl() {
    assert_lives_for(Kind::Meta, json!({ "meta": { "id": "kitsu:1" }, "cacheMaxAge": 43_200 }), DAY).await;
}

#[tokio::test(start_paused = true)]
async fn catalog_lives_for_the_catalog_ttl() {
    assert_lives_for(Kind::Catalog, json!({ "metas": [], "cacheMaxAge": 43_200 }), HOUR * 2).await;
}

#[tokio::test(start_paused = true)]
async fn english_logo_images_live_for_14_days() {
    assert_lives_for(Kind::Images, json!({ "logo": "l", "logoLang": "en" }), DAY * 14).await;
}

#[tokio::test(start_paused = true)]
async fn non_english_logo_images_live_for_4_days() {
    assert_lives_for(Kind::Images, json!({ "logo": "l", "logoLang": "ja" }), DAY * 4).await;
}

#[tokio::test(start_paused = true)]
async fn logo_without_language_lives_for_4_days() {
    assert_lives_for(Kind::Images, json!({ "logo": "l", "logoLang": null }), DAY * 4).await;
}

#[tokio::test(start_paused = true)]
async fn images_without_logo_live_for_2_days() {
    assert_lives_for(Kind::Images, json!({ "logo": null, "logoLang": "", "poster": "p" }), DAY * 2).await;
}

#[tokio::test(start_paused = true)]
async fn id_mapping_lives_for_14_days() {
    assert_lives_for(Kind::IdMapping, json!({ "kitsu_id": 1 }), DAY * 14).await;
}

#[tokio::test(start_paused = true)]
async fn mongo_hit_is_served_and_counted_as_a_hit() {
    let fixture = with_store(Mode::Working);
    fixture.documents.put("stremio-kitsu|meta:1", &json!({ "meta": { "id": "kitsu:1" } }), now_ms() + 3_600_000);
    let loader = Loader::new();
    let value = fixture.cache.wrap_meta("1", || loader.ok(json!("fresh"))).await.unwrap();
    assert_eq!(*value, json!({ "meta": { "id": "kitsu:1" } }));
    assert_eq!(loader.calls(), 0);
    assert_eq!(fixture.stats.counted(), ["hit:meta"]);
}

#[tokio::test(start_paused = true)]
async fn mongo_hit_is_promoted_to_memory_for_the_remaining_ttl() {
    let fixture = with_store(Mode::Working);
    fixture.documents.put("stremio-kitsu|meta:1", &json!("stored"), now_ms() + 3_600_000);
    let loader = Loader::new();
    fixture.cache.wrap_meta("1", || loader.ok(json!("fresh"))).await.unwrap();
    tokio::time::advance(HOUR.saturating_sub(SECOND * 5)).await;
    let value = fixture.cache.wrap_meta("1", || loader.ok(json!("fresh"))).await.unwrap();
    assert_eq!((value.as_ref().clone(), fixture.documents.reads()), (json!("stored"), 1));
    tokio::time::advance(SECOND * 10).await;
    fixture.cache.wrap_meta("1", || loader.ok(json!("fresh"))).await.unwrap();
    assert_eq!(fixture.documents.reads(), 2);
}

#[tokio::test(start_paused = true)]
async fn mongo_entry_without_expiry_is_promoted_for_the_kind_ttl() {
    let fixture = with_store(Mode::Working);
    let mut document = encode_document("stremio-kitsu|meta:1", &json!("stored"), 0);
    document.get_document_mut("value").unwrap().insert("expires", mongodb::bson::Bson::Null);
    fixture.documents.inner.insert(document);
    let loader = Loader::new();
    fixture.cache.wrap_meta("1", || loader.ok(json!("fresh"))).await.unwrap();
    tokio::time::advance(DAY.saturating_sub(SECOND)).await;
    fixture.cache.wrap_meta("1", || loader.ok(json!("fresh"))).await.unwrap();
    assert_eq!(fixture.documents.reads(), 1);
}

#[tokio::test(start_paused = true)]
async fn a_load_is_written_to_memory_and_mongo() {
    let fixture = with_store(Mode::Working);
    let loader = Loader::new();
    let value = fixture.cache.wrap_meta("1", || loader.ok(json!({ "meta": 1 }))).await.unwrap();
    fixture.cache.drain().await;
    let stored = fixture.documents.inner.document("stremio-kitsu|meta:1").unwrap();
    assert_eq!(crate::infrastructure::store::decode_payload(&stored, now_ms()), Some((*value).clone()));
    fixture.cache.wrap_meta("1", || loader.ok(json!("fresh"))).await.unwrap();
    assert_eq!((loader.calls(), fixture.documents.reads()), (1, 1));
    assert_eq!(fixture.stats.counted(), ["miss:meta", "hit:meta"]);
}

#[tokio::test(start_paused = true)]
async fn the_mongo_write_does_not_delay_the_response() {
    let fixture = with_store(Mode::HangingWrites);
    let loader = Loader::new();
    let value = fixture.cache.wrap_meta("1", || loader.ok(json!("fresh"))).await.unwrap();
    assert_eq!(*value, json!("fresh"));
    tokio::task::yield_now().await;
    assert_eq!(fixture.documents.writes.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.documents.inner.document("stremio-kitsu|meta:1"), None);
}

#[tokio::test(start_paused = true)]
async fn two_concurrent_callers_share_one_load() {
    let (cache, stats) = memory_only();
    let loader = Loader::new();
    let (first, second) = tokio::join!(
        cache.wrap_meta("1", || loader.ok(json!("loaded"))),
        cache.wrap_meta("1", || loader.ok(json!("other"))),
    );
    assert_eq!((first.unwrap().as_ref().clone(), second.unwrap().as_ref().clone()), (json!("loaded"), json!("loaded")));
    assert_eq!(loader.calls(), 1);
    let mut counted = stats.counted();
    counted.sort();
    assert_eq!(counted, ["hit:meta", "miss:meta"]);
}

#[tokio::test(start_paused = true)]
async fn concurrent_callers_waiting_on_mongo_share_one_read() {
    let fixture = with_store(Mode::Working);
    fixture.documents.put("stremio-kitsu|meta:1", &json!("stored"), now_ms() + 3_600_000);
    let loader = Loader::new();
    let (first, second) = tokio::join!(
        fixture.cache.wrap_meta("1", || loader.ok(json!("fresh"))),
        fixture.cache.wrap_meta("1", || loader.ok(json!("fresh"))),
    );
    assert_eq!((first.unwrap().as_ref().clone(), second.unwrap().as_ref().clone()), (json!("stored"), json!("stored")));
    assert_eq!((fixture.documents.reads(), loader.calls()), (1, 0));
    assert_eq!(fixture.stats.counted(), ["hit:meta", "hit:meta"]);
}

#[tokio::test(start_paused = true)]
async fn a_loader_error_is_shared_with_waiting_callers() {
    let (cache, stats) = memory_only();
    let loader = Loader::new();
    let (first, second) =
        tokio::join!(cache.wrap_meta("1", || loader.err("boom")), cache.wrap_meta("1", || loader.err("other")));
    assert_eq!((first, second), (Err("boom".to_owned()), Err("boom".to_owned())));
    assert_eq!(loader.calls(), 1);
    assert_eq!(stats.counted(), Vec::<String>::new());
}

#[tokio::test(start_paused = true)]
async fn a_loader_error_is_not_cached() {
    let fixture = with_store(Mode::Working);
    let loader = Loader::new();
    let failed = fixture.cache.wrap_meta("1", || loader.err("boom")).await;
    assert_eq!(failed, Err("boom".to_owned()));
    let value = fixture.cache.wrap_meta("1", || loader.ok(json!("fresh"))).await.unwrap();
    fixture.cache.drain().await;
    assert_eq!((value.as_ref().clone(), loader.calls()), (json!("fresh"), 2));
    assert_eq!(fixture.documents.writes.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn a_failing_mongo_only_skips_the_cache() {
    let fixture = with_store(Mode::Failing);
    let loader = Loader::new();
    let value = fixture.cache.wrap_meta("1", || loader.ok(json!("fresh"))).await.unwrap();
    fixture.cache.drain().await;
    assert_eq!(*value, json!("fresh"));
    assert_eq!(fixture.stats.counted(), ["miss:meta"]);
    let again = fixture.cache.wrap_meta("1", || loader.ok(json!("other"))).await.unwrap();
    assert_eq!((again.as_ref().clone(), loader.calls()), (json!("fresh"), 1));
}

#[tokio::test(start_paused = true)]
async fn a_hanging_mongo_times_out_and_loads() {
    let fixture = with_store(Mode::Hanging);
    let loader = Loader::new();
    let started = Instant::now();
    let value = fixture.cache.wrap_meta("1", || loader.ok(json!("fresh"))).await.unwrap();
    assert_eq!(*value, json!("fresh"));
    assert!(started.elapsed() >= Duration::from_secs(30) && started.elapsed() < Duration::from_secs(31));
}

#[tokio::test(start_paused = true)]
async fn failing_cached_metas_returns_nothing() {
    let fixture = with_store(Mode::Failing);
    assert_eq!(fixture.cache.cached_metas(&["1".to_owned()]).await, [None]);
}

#[tokio::test(start_paused = true)]
async fn a_corrupt_memory_entry_is_a_miss() {
    let (cache, stats) = memory_only();
    let garbage = Packed(Arc::from(&b"not zstd"[..]));
    let entry = Entry { packed: garbage, ttl: HOUR, deadline: Instant::now() + HOUR };
    cache.memory.insert("stremio-kitsu|meta:1".to_owned(), entry).await;
    let loader = Loader::new();
    let value = cache.wrap_meta("1", || loader.ok(json!("fresh"))).await.unwrap();
    assert_eq!((value.as_ref().clone(), loader.calls()), (json!("fresh"), 1));
    assert_eq!(stats.counted(), ["miss:meta"]);
    let again = cache.wrap_meta("1", || loader.ok(json!("other"))).await.unwrap();
    assert_eq!(*again, json!("fresh"));
}

#[tokio::test(start_paused = true)]
async fn cached_metas_mixes_memory_mongo_and_missing_in_order() {
    let fixture = with_store(Mode::Working);
    let loader = Loader::new();
    fixture.cache.wrap_meta("a", || loader.ok(json!("A"))).await.unwrap();
    fixture.cache.drain().await;
    fixture.documents.put("stremio-kitsu|meta:b", &json!("B"), now_ms() + 3_600_000);
    let ids = ["b", "a", "c"].map(str::to_owned);
    let found = fixture.cache.cached_metas(&ids).await;
    let found: Vec<Option<Value>> = found.into_iter().map(|value| value.map(|value| (*value).clone())).collect();
    assert_eq!(found, [Some(json!("B")), Some(json!("A")), None]);
    assert_eq!(*fixture.documents.many.lock().unwrap(), [["stremio-kitsu|meta:b", "stremio-kitsu|meta:c"]]);
}

#[tokio::test(start_paused = true)]
async fn cached_metas_promotes_mongo_hits() {
    let fixture = with_store(Mode::Working);
    fixture.documents.put("stremio-kitsu|meta:b", &json!("B"), now_ms() + 3_600_000);
    fixture.cache.cached_metas(&["b".to_owned()]).await;
    let found = fixture.cache.cached_metas(&["b".to_owned()]).await;
    assert_eq!(found[0].as_deref(), Some(&json!("B")));
    assert_eq!(fixture.documents.many.lock().unwrap().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn cached_metas_from_memory_skips_mongo() {
    let fixture = with_store(Mode::Working);
    let loader = Loader::new();
    fixture.cache.wrap_meta("a", || loader.ok(json!("A"))).await.unwrap();
    fixture.cache.cached_metas(&["a".to_owned()]).await;
    assert!(fixture.documents.many.lock().unwrap().is_empty());
}

#[tokio::test(start_paused = true)]
async fn store_operations_are_timed() {
    let fixture = with_store(Mode::Working);
    let loader = Loader::new();
    fixture.cache.wrap_meta("1", || loader.ok(json!(1))).await.unwrap();
    fixture.cache.drain().await;
    fixture.cache.cached_metas(&["2".to_owned()]).await;
    let observed: Vec<String> =
        fixture.stats.events().into_iter().filter(|event| event.starts_with("observe")).collect();
    assert_eq!(observed, ["observe:get", "observe:set", "observe:mget"]);
}

#[tokio::test(start_paused = true)]
async fn no_cache_bypasses_both_layers() {
    let settings = CacheSettings { enabled: false, ..settings() };
    let fixture = with_settings(settings, Mode::Working);
    fixture.documents.put("stremio-kitsu|meta:1", &json!("stored"), now_ms() + 3_600_000);
    let loader = Loader::new();
    fixture.cache.wrap_meta("1", || loader.ok(json!("fresh"))).await.unwrap();
    let value = fixture.cache.wrap_meta("1", || loader.ok(json!("fresh"))).await.unwrap();
    fixture.cache.drain().await;
    assert_eq!((value.as_ref().clone(), loader.calls()), (json!("fresh"), 2));
    assert_eq!(fixture.cache.cached_metas(&["1".to_owned()]).await, [None]);
    assert_eq!((fixture.documents.reads(), fixture.documents.writes.load(Ordering::SeqCst)), (0, 0));
    assert_eq!(fixture.stats.events(), Vec::<String>::new());
}

#[test]
fn settings_follow_the_config() {
    let config = Config::from_lookup(|name| match name {
        "NO_CACHE" => Some("true".to_owned()),
        "MEMORY_CACHE_MB" => Some("7".to_owned()),
        "META_TTL" => Some("1000".to_owned()),
        "CATALOG_TTL" => Some("2000".to_owned()),
        _ => None,
    });
    let expected = CacheSettings {
        enabled: false,
        budget_bytes: 7 * 1024 * 1024,
        meta_ttl: Duration::from_secs(1),
        catalog_ttl: Duration::from_secs(2),
    };
    assert_eq!(CacheSettings::from_config(&config), expected);
}

#[test]
fn the_cache_and_its_futures_are_send_and_sync() {
    fn send<T: Send>(_: T) {}
    fn shared<T: Send + Sync>() {}
    shared::<Cache<String>>();
    let (cache, _) = memory_only();
    send(cache.wrap_meta("1", || async { Ok::<_, String>(json!(1)) }));
    send(cache.cached_metas(&[]));
}

fn incompressible(seed: u64, bytes: usize) -> Value {
    let mut state = seed | 1;
    let text: String = (0..bytes)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            char::from(b'!' + (state % 90) as u8)
        })
        .collect();
    json!(text)
}

#[tokio::test(start_paused = true)]
async fn heavy_entries_evict_older_ones_at_the_byte_budget() {
    let settings = CacheSettings { budget_bytes: 1024 * 1024, ..settings() };
    let cache: Cache<String> = Cache::new(settings, None, Arc::new(NoStats));
    let loader = Loader::new();
    for id in 0..30_u64 {
        cache.wrap_meta(&id.to_string(), || loader.ok(incompressible(id, 100_000))).await.unwrap();
        cache.run_pending_tasks().await;
    }
    assert!(cache.memory_weight() <= 1024 * 1024, "{}", cache.memory_weight());
    let kept = cache.memory_entries();
    assert!((8..30).contains(&kept), "{kept}");
    let calls = loader.calls();
    cache.wrap_meta("29", || loader.ok(json!("reloaded"))).await.unwrap();
    cache.wrap_meta("0", || loader.ok(json!("reloaded"))).await.unwrap();
    assert_eq!(loader.calls(), calls + 1);
}

#[tokio::test(start_paused = true)]
async fn many_tiny_entries_are_all_kept_within_the_budget() {
    let settings = CacheSettings { budget_bytes: 1024 * 1024, ..settings() };
    let cache: Cache<String> = Cache::new(settings, None, Arc::new(NoStats));
    for id in 0..10_000 {
        cache.wrap_meta(&id.to_string(), || async move { Ok::<_, String>(json!(id)) }).await.unwrap();
    }
    cache.run_pending_tasks().await;
    assert_eq!(cache.memory_entries(), 10_000);
    assert!(cache.memory_weight() <= 1024 * 1024, "{}", cache.memory_weight());
}

#[tokio::test(start_paused = true)]
async fn the_weight_is_the_key_and_compressed_bytes() {
    let (cache, _) = memory_only();
    let value = json!({ "meta": { "id": "kitsu:1" } });
    cache.wrap_meta("1", || async { Ok::<_, String>(value.clone()) }).await.unwrap();
    cache.run_pending_tasks().await;
    let packed = Packed::encode(&value).unwrap().0.len() as u64;
    assert_eq!(cache.memory_weight(), "stremio-kitsu|meta:1".len() as u64 + packed);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_waiter_gets_a_value_when_the_leader_is_cancelled() {
    let (cache, _) = memory_only();
    let cache = Arc::new(cache);
    let started = Arc::new(tokio::sync::Notify::new());
    let leader = tokio::spawn({
        let (cache, started) = (Arc::clone(&cache), Arc::clone(&started));
        async move {
            cache
                .wrap_meta("1", || async move {
                    started.notify_one();
                    std::future::pending::<Result<Value, String>>().await
                })
                .await
        }
    });
    started.notified().await;
    let waiter = tokio::spawn({
        let cache = Arc::clone(&cache);
        async move { cache.wrap_meta("1", || async { Ok(json!("waiter")) }).await }
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    leader.abort();
    let value = tokio::time::timeout(Duration::from_secs(5), waiter).await.unwrap().unwrap().unwrap();
    assert_eq!(*value, json!("waiter"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_waiter_gets_a_value_when_the_leader_panics() {
    let (cache, _) = memory_only();
    let cache = Arc::new(cache);
    let release = Arc::new(tokio::sync::Notify::new());
    let started = Arc::new(tokio::sync::Notify::new());
    let leader = tokio::spawn({
        let (cache, release, started) = (Arc::clone(&cache), Arc::clone(&release), Arc::clone(&started));
        async move {
            cache
                .wrap_meta("1", || async move {
                    started.notify_one();
                    release.notified().await;
                    panic!("loader broken")
                })
                .await
        }
    });
    started.notified().await;
    let waiter = tokio::spawn({
        let cache = Arc::clone(&cache);
        async move { cache.wrap_meta("1", || async { Ok(json!("waiter")) }).await }
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    release.notify_one();
    assert!(leader.await.unwrap_err().is_panic());
    let value = tokio::time::timeout(Duration::from_secs(5), waiter).await.unwrap().unwrap().unwrap();
    assert_eq!(*value, json!("waiter"));
}

#[tokio::test(start_paused = true)]
async fn drain_gives_up_on_a_hanging_write_after_ten_seconds() {
    let fixture = with_store(Mode::HangingWrites);
    let loader = Loader::new();
    fixture.cache.wrap_meta("1", || loader.ok(json!("loaded"))).await.unwrap();
    let started = tokio::time::Instant::now();
    fixture.cache.drain().await;
    assert_eq!(started.elapsed(), DRAIN_TIMEOUT);
}
