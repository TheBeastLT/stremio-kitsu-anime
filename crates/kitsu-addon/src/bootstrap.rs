use std::{
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::Context as _;
use kitsu_search::{SharedIndex, index::Index, records::Sources};
use tokio::net::TcpListener;

use crate::{
    application::{Addon, Mapping, TitleRefresh, Upstreams},
    http::{AppState, app},
    infrastructure::{
        Config, KitsuApi, Metrics, MongoDocuments, ReqwestHttp, ReqwestTransport, Store,
        kitsu::KITSU_TIMEOUT,
        store::{self},
    },
};

pub const TITLES_FILE: &str = "search_titles.jsonl";
pub const ALIASES_FILE: &str = "search_aliases.json";
pub const IMDB_MAPPING_FILE: &str = "imdb_mapping.json";

pub struct Service {
    pub state: Arc<AppState>,
    pub refresh: TitleRefresh,
}

struct Titles {
    search: Arc<SharedIndex>,
    mapping: Arc<Mapping>,
    sources: Sources,
}

fn load_titles(dir: &Path) -> anyhow::Result<Titles> {
    let started = Instant::now();
    let sources = Sources::read(&dir.join(TITLES_FILE), &dir.join(ALIASES_FILE), &dir.join(IMDB_MAPPING_FILE))
        .context("reading the title search data")?;
    let mapping = Mapping::load(&dir.join(IMDB_MAPPING_FILE), &sources.records).context("reading the imdb mapping")?;
    let search = Arc::new(SharedIndex::new(Index::build(&sources.records, &sources.mapping, &sources.aliases)));
    tracing::info!(
        records = sources.records.len(),
        elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        "Title index built"
    );
    Ok(Titles { search, mapping: Arc::new(mapping), sources })
}

async fn open_store(config: &Config) -> anyhow::Result<Option<Store>> {
    let Some(uri) = config.mongodb_uri.as_deref().filter(|_| !config.no_cache) else {
        return Ok(None);
    };
    let client = store::connect(uri).await.context("connecting to the cache database")?;
    let documents = Arc::new(MongoDocuments::new(&client, &config.kitsu_collection));
    Ok(Some(Store::new(documents, Duration::from_millis(config.store_timeout_ms))))
}

pub async fn build(config: &Config) -> anyhow::Result<Service> {
    let metrics = Metrics::new();
    metrics.spawn_lag_sampler();
    let store = open_store(config).await?;
    let dir = config.data_dir.clone();
    let titles = tokio::task::spawn_blocking(move || load_titles(Path::new(&dir))).await??;
    let kitsu = Arc::new(ReqwestTransport::new(&config.kitsu_url, KITSU_TIMEOUT)?);
    let upstreams = Upstreams { kitsu: kitsu.clone(), http: Arc::new(ReqwestHttp::new()?) };
    let refresh =
        TitleRefresh::new(KitsuApi::new(kitsu, Arc::new(metrics.clone())), Arc::clone(&titles.search), titles.sources);
    let addon = Addon::build(config, upstreams, store, titles.mapping, titles.search, metrics.clone());
    let state = Arc::new(AppState { addon, metrics, metrics_credentials: config.metrics_credentials.clone() });
    Ok(Service { state, refresh })
}

pub async fn bind(port: u16) -> std::io::Result<TcpListener> {
    match TcpListener::bind(SocketAddr::new(Ipv6Addr::UNSPECIFIED.into(), port)).await {
        Ok(listener) => Ok(listener),
        Err(_) => TcpListener::bind(SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), port)).await,
    }
}

pub async fn serve(
    listener: TcpListener,
    state: Arc<AppState>,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    axum::serve(listener, app(Arc::clone(&state))).with_graceful_shutdown(shutdown).await?;
    state.addon.cache().drain().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use async_trait::async_trait;
    use mongodb::bson::Document;
    use serde_json::json;

    use super::*;
    use crate::infrastructure::{
        DocumentStore, KitsuTransport, StoreError,
        http::TransportError,
        kitsu::KitsuResponse,
        testing::{FakeHttp, MemoryDocuments},
    };

    struct NoKitsu;

    #[async_trait]
    impl KitsuTransport for NoKitsu {
        async fn get(&self, _path: &str, _query: &str) -> Result<KitsuResponse, TransportError> {
            Err(TransportError("offline".to_owned()))
        }
    }

    #[derive(Default)]
    struct SlowWrites(MemoryDocuments);

    #[async_trait]
    impl DocumentStore for SlowWrites {
        async fn find_one(&self, key: &str) -> Result<Option<Document>, StoreError> {
            self.0.find_one(key).await
        }

        async fn find_many(&self, keys: &[String]) -> Result<Vec<Document>, StoreError> {
            self.0.find_many(keys).await
        }

        async fn upsert(&self, key: &str, document: Document) -> Result<(), StoreError> {
            tokio::time::sleep(Duration::from_millis(300)).await;
            self.0.upsert(key, document).await
        }
    }

    #[tokio::test]
    async fn serving_drains_the_cache_writes_after_shutdown() {
        let documents = Arc::new(SlowWrites::default());
        let store = Store::new(documents.clone(), Duration::from_secs(5));
        let config = Config::from_lookup(|_| None);
        let mapping = Arc::new(Mapping::new(Vec::new(), &[]));
        let search = Arc::new(SharedIndex::new(Index::build(&[], &HashMap::new(), &HashMap::new())));
        let upstreams = Upstreams { kitsu: Arc::new(NoKitsu), http: FakeHttp::new() };
        let metrics = Metrics::new();
        let addon = Addon::build(&config, upstreams, Some(store), mapping, search, metrics.clone());
        let state = Arc::new(AppState { addon, metrics, metrics_credentials: None });
        state.addon.cache().wrap_meta("1", || async { Ok(json!({ "meta": 1 })) }).await.unwrap();
        assert!(documents.0.document("stremio-kitsu|meta:1").is_none());
        let listener = bind(0).await.unwrap();
        serve(listener, state, async {}).await.unwrap();
        assert!(documents.0.document("stremio-kitsu|meta:1").is_some());
    }
}
