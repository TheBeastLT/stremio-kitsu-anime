mod codec;

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use mongodb::{
    Client, Collection,
    bson::{DateTime, Document, doc},
    options::{ClientOptions, Compressor},
};
use serde_json::Value;

pub use codec::{
    Stored, bson_to_json, days_from_civil, decode_payload, decode_stored, encode_document, iso_string, json_to_bson,
    parse_iso,
};

pub const GLOBAL_KEY_PREFIX: &str = "stremio-kitsu";
pub const META_KEY_PREFIX: &str = "stremio-kitsu|meta";
pub const CATALOG_KEY_PREFIX: &str = "stremio-kitsu|catalog";
pub const IMAGES_KEY_PREFIX: &str = "stremio-kitsu|images";
pub const ID_MAPPING_KEY_PREFIX: &str = "stremio-kitsu|id_mapping";

pub fn meta_key(id: &str) -> String {
    format!("{META_KEY_PREFIX}:{id}")
}

pub fn catalog_key(catalog_id: &str, genre: &str, skip: &str) -> String {
    format!("{CATALOG_KEY_PREFIX}:{catalog_id}|{genre}|{skip}")
}

pub fn images_key(id: &str) -> String {
    format!("{IMAGES_KEY_PREFIX}:{id}")
}

pub fn id_mapping_key(full_id: &str) -> String {
    format!("{ID_MAPPING_KEY_PREFIX}:{full_id}")
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct StoreError(pub String);

fn failure(error: &impl ToString) -> StoreError {
    StoreError(error.to_string())
}

#[async_trait]
pub trait DocumentStore: Send + Sync {
    async fn find_one(&self, key: &str) -> Result<Option<Document>, StoreError>;
    async fn find_many(&self, keys: &[String]) -> Result<Vec<Document>, StoreError>;
    async fn upsert(&self, key: &str, document: Document) -> Result<(), StoreError>;
}

#[derive(Clone)]
pub struct Store {
    documents: Arc<dyn DocumentStore>,
    timeout: Duration,
}

impl Store {
    pub fn new(documents: Arc<dyn DocumentStore>, timeout: Duration) -> Self {
        Self { documents, timeout }
    }

    pub async fn get(&self, key: &str) -> Result<Option<Stored>, StoreError> {
        let document = self.bounded(self.documents.find_one(key)).await?;
        Ok(document.and_then(|document| decode_stored(&document, now_ms())))
    }

    pub async fn mget(&self, keys: &[String]) -> Result<Vec<Option<Stored>>, StoreError> {
        let documents = self.bounded(self.documents.find_many(keys)).await?;
        let now = now_ms();
        Ok(keys
            .iter()
            .map(|key| {
                documents
                    .iter()
                    .find(|document| document.get_str("key").is_ok_and(|stored| stored == key))
                    .and_then(|document| decode_stored(document, now))
            })
            .collect())
    }

    pub async fn set(&self, key: &str, value: &Value, ttl: Duration) -> Result<(), StoreError> {
        let expires = now_ms() + ttl.as_millis() as i64;
        self.bounded(self.documents.upsert(key, encode_document(key, value, expires))).await
    }

    async fn bounded<T>(&self, operation: impl Future<Output = Result<T, StoreError>>) -> Result<T, StoreError> {
        tokio::time::timeout(self.timeout, operation).await.map_err(|_| failure(&"cache operation timed out"))?
    }
}

pub fn now_ms() -> i64 {
    DateTime::now().timestamp_millis()
}

pub async fn connect(uri: &str) -> Result<Client, mongodb::error::Error> {
    let mut options = ClientOptions::parse(uri).await?;
    options.compressors = Some(vec![Compressor::Zstd { level: None }]);
    Client::with_options(options)
}

pub struct MongoDocuments {
    collection: Collection<Document>,
}

impl MongoDocuments {
    pub fn new(client: &Client, name: &str) -> Self {
        let database = client.default_database().unwrap_or_else(|| client.database("test"));
        Self { collection: database.collection(name) }
    }
}

async fn mongo<T>(operation: impl IntoFuture<Output = Result<T, mongodb::error::Error>>) -> Result<T, StoreError> {
    operation.await.map_err(|error| failure(&error))
}

#[async_trait]
impl DocumentStore for MongoDocuments {
    async fn find_one(&self, key: &str) -> Result<Option<Document>, StoreError> {
        mongo(
            self.collection
                .find_one(doc! { "key": { "$eq": key } })
                .projection(doc! { "_id": 0, "key": 1, "value": 1 }),
        )
        .await
    }

    async fn find_many(&self, keys: &[String]) -> Result<Vec<Document>, StoreError> {
        mongo(async {
            let mut cursor = self
                .collection
                .find(doc! { "key": { "$in": keys } })
                .projection(doc! { "_id": 0, "key": 1, "value": 1 })
                .await?;
            let mut documents = Vec::new();
            while cursor.advance().await? {
                documents.push(cursor.deserialize_current()?);
            }
            Ok(documents)
        })
        .await
    }

    async fn upsert(&self, key: &str, document: Document) -> Result<(), StoreError> {
        mongo(self.collection.update_one(doc! { "key": { "$eq": key } }, doc! { "$set": document }).upsert(true))
            .await
            .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use crate::infrastructure::testing::MemoryDocuments;
    use mongodb::bson::Bson;
    use serde_json::json;

    use super::*;

    fn store() -> (Arc<MemoryDocuments>, Store) {
        let documents = Arc::new(MemoryDocuments::default());
        (documents.clone(), Store::new(documents, Duration::from_secs(1)))
    }

    async fn value(store: &Store, key: &str) -> Option<Value> {
        store.get(key).await.unwrap().map(|stored| stored.value)
    }

    async fn values(store: &Store, keys: &[String]) -> Vec<Option<Value>> {
        store.mget(keys).await.unwrap().into_iter().map(|stored| stored.map(|stored| stored.value)).collect()
    }

    struct Hanging;

    #[async_trait]
    impl DocumentStore for Hanging {
        async fn find_one(&self, _key: &str) -> Result<Option<Document>, StoreError> {
            std::future::pending().await
        }

        async fn find_many(&self, _keys: &[String]) -> Result<Vec<Document>, StoreError> {
            std::future::pending().await
        }

        async fn upsert(&self, _key: &str, _document: Document) -> Result<(), StoreError> {
            std::future::pending().await
        }
    }

    #[test]
    fn meta_key_matches_node() {
        assert_eq!(meta_key("1"), "stremio-kitsu|meta:1");
    }

    #[test]
    fn imdb_meta_key_matches_node() {
        assert_eq!(meta_key("tt0213338"), "stremio-kitsu|meta:tt0213338");
    }

    #[test]
    fn catalog_key_matches_node() {
        assert_eq!(catalog_key("kitsu-anime-list", "All", "0"), "stremio-kitsu|catalog:kitsu-anime-list|All|0");
    }

    #[test]
    fn images_key_matches_node() {
        assert_eq!(images_key("1"), "stremio-kitsu|images:1");
    }

    #[test]
    fn id_mapping_key_matches_node() {
        assert_eq!(id_mapping_key("mal:1"), "stremio-kitsu|id_mapping:mal:1");
    }

    #[test]
    fn global_prefix_starts_every_kind() {
        let prefixes = [META_KEY_PREFIX, CATALOG_KEY_PREFIX, IMAGES_KEY_PREFIX, ID_MAPPING_KEY_PREFIX];
        assert!(prefixes.iter().all(|prefix| prefix.starts_with(&format!("{GLOBAL_KEY_PREFIX}|"))));
    }

    #[tokio::test]
    async fn set_then_get_returns_the_value() {
        let (_, store) = store();
        store.set("k", &json!({ "a": [1, 2.5, null] }), Duration::from_secs(60)).await.unwrap();
        assert_eq!(value(&store, "k").await, Some(json!({ "a": [1, 2.5, null] })));
    }

    #[tokio::test]
    async fn set_writes_the_keyv_document() {
        let (documents, store) = store();
        store.set("k", &json!(1), Duration::from_secs(60)).await.unwrap();
        let document = documents.document("k").unwrap();
        let keys: Vec<&String> = document.keys().collect();
        assert_eq!(keys, ["key", "value", "expiresAt"]);
        let expires = document.get_document("value").unwrap().get("expires").unwrap();
        let Bson::Double(expires) = expires else { panic!("expires is {expires:?}") };
        assert_eq!(*expires as i64, document.get_datetime("expiresAt").unwrap().timestamp_millis());
    }

    #[tokio::test]
    async fn set_replaces_the_value_on_the_same_key() {
        let (_, store) = store();
        store.set("k", &json!(1), Duration::from_secs(60)).await.unwrap();
        store.set("k", &json!(2), Duration::from_secs(60)).await.unwrap();
        assert_eq!(value(&store, "k").await, Some(json!(2)));
    }

    #[tokio::test]
    async fn get_of_a_missing_key_is_none() {
        let (_, store) = store();
        assert_eq!(value(&store, "missing").await, None);
    }

    #[tokio::test]
    async fn get_of_an_expired_key_is_none() {
        let (documents, store) = store();
        documents.insert(encode_document("k", &json!(1), now_ms() - 1000));
        assert_eq!(value(&store, "k").await, None);
    }

    #[tokio::test]
    async fn mget_keeps_the_requested_order_and_misses() {
        let (_, store) = store();
        store.set("a", &json!("A"), Duration::from_secs(60)).await.unwrap();
        store.set("b", &json!("B"), Duration::from_secs(60)).await.unwrap();
        let keys = ["b".to_owned(), "missing".to_owned(), "a".to_owned(), "b".to_owned()];
        assert_eq!(values(&store, &keys).await, [Some(json!("B")), None, Some(json!("A")), Some(json!("B"))]);
    }

    #[tokio::test]
    async fn mget_skips_expired_keys() {
        let (documents, store) = store();
        documents.insert(encode_document("old", &json!(1), now_ms() - 1000));
        store.set("new", &json!(2), Duration::from_secs(60)).await.unwrap();
        let keys = ["old".to_owned(), "new".to_owned()];
        assert_eq!(values(&store, &keys).await, [None, Some(json!(2))]);
    }

    #[tokio::test]
    async fn get_returns_the_stored_expiry() {
        let (documents, store) = store();
        documents.insert(encode_document("k", &json!(1), now_ms() + 5000));
        let stored = store.get("k").await.unwrap().unwrap();
        assert_eq!(
            stored.expires,
            documents.document("k").unwrap().get_datetime("expiresAt").ok().map(|at| at.timestamp_millis())
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_hanging_store_times_out() {
        let store = Store::new(Arc::new(Hanging), Duration::from_secs(30));
        let error = store.get("k").await.unwrap_err();
        assert_eq!(error, StoreError("cache operation timed out".to_owned()));
    }
}
