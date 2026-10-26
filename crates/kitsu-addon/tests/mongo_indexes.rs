use kitsu_addon::infrastructure::{MongoDocuments, store};
use mongodb::{
    Client, Database,
    bson::{Bson, Document, doc},
};

async fn database() -> Option<(Client, Database)> {
    let uri = std::env::var("KITSU_TEST_MONGODB_URI").ok()?;
    let client = store::connect(&uri).await.unwrap();
    let database = client.default_database().expect("KITSU_TEST_MONGODB_URI names a database");
    Some((client, database))
}

fn collection_name(case: &str) -> String {
    format!("{case}_{}", store::now_ms())
}

async fn indexes(database: &Database, name: &str) -> Vec<Document> {
    let result = database.run_command(doc! { "listIndexes": name }).await.unwrap();
    let batch = result.get_document("cursor").unwrap().get_array("firstBatch").unwrap();
    let mut indexes: Vec<Document> = batch
        .iter()
        .filter_map(Bson::as_document)
        .map(|index| {
            let mut index = index.clone();
            index.remove("v");
            index.remove("background");
            index
        })
        .collect();
    indexes.sort_by_key(|index| index.get_str("name").unwrap().to_owned());
    indexes
}

fn keyv_indexes() -> Vec<Document> {
    vec![
        doc! { "key": { "_id": 1 }, "name": "_id_" },
        doc! { "key": { "expiresAt": 1 }, "name": "expiresAt_1", "expireAfterSeconds": 0 },
        doc! { "key": { "key": 1 }, "name": "key_1", "unique": true },
    ]
}

async fn create_like_keyv(database: &Database, name: &str) {
    database
        .run_command(doc! {
            "createIndexes": name,
            "indexes": [
                { "key": { "key": 1 }, "name": "key_1", "unique": true, "background": true },
                { "key": { "expiresAt": 1 }, "name": "expiresAt_1", "expireAfterSeconds": 0, "background": true },
            ],
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn a_fresh_collection_gets_the_keyv_indexes() {
    let Some((client, database)) = database().await else { return eprintln!("KITSU_TEST_MONGODB_URI unset") };
    let name = collection_name("fresh");
    let documents = MongoDocuments::new(&client, &name);
    assert!(documents.ensure_indexes().await);
    assert!(documents.ensure_indexes().await);
    assert_eq!(indexes(&database, &name).await, keyv_indexes());
    database.collection::<Document>(&name).drop().await.unwrap();
}

#[tokio::test]
async fn a_keyv_created_collection_is_left_as_is() {
    let Some((client, database)) = database().await else { return eprintln!("KITSU_TEST_MONGODB_URI unset") };
    let name = collection_name("keyv");
    create_like_keyv(&database, &name).await;
    MongoDocuments::new(&client, &name).ensure_indexes().await;
    assert_eq!(indexes(&database, &name).await, keyv_indexes());
    database.collection::<Document>(&name).drop().await.unwrap();
}

#[tokio::test]
async fn keyv_accepts_a_rust_created_collection() {
    let Some((client, database)) = database().await else { return eprintln!("KITSU_TEST_MONGODB_URI unset") };
    let name = collection_name("rust");
    MongoDocuments::new(&client, &name).ensure_indexes().await;
    create_like_keyv(&database, &name).await;
    assert_eq!(indexes(&database, &name).await, keyv_indexes());
    database.collection::<Document>(&name).drop().await.unwrap();
}

#[tokio::test]
async fn a_conflicting_index_does_not_stop_the_other() {
    let Some((client, database)) = database().await else { return eprintln!("KITSU_TEST_MONGODB_URI unset") };
    let name = collection_name("conflict");
    database
        .run_command(doc! { "createIndexes": &name, "indexes": [{ "key": { "key": 1 }, "name": "key_1" }] })
        .await
        .unwrap();
    assert!(!MongoDocuments::new(&client, &name).ensure_indexes().await);
    let names: Vec<String> =
        indexes(&database, &name).await.iter().map(|index| index.get_str("name").unwrap().to_owned()).collect();
    assert_eq!(names, ["_id_", "expiresAt_1", "key_1"]);
    database.collection::<Document>(&name).drop().await.unwrap();
}
