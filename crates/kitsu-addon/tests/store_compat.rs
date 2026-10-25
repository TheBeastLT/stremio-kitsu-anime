use std::path::PathBuf;

use base64::{Engine, engine::general_purpose::STANDARD};
use kitsu_addon::{
    domain::Meta,
    infrastructure::store::{decode_payload, encode_document, meta_key},
};
use mongodb::bson::{Bson, Document};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const RUST_EXPIRES: i64 = 4_102_444_800_000;

#[derive(Deserialize)]
struct NodeFixture {
    #[serde(rename = "recordedAt")]
    recorded_at: i64,
    cases: Vec<NodeCase>,
}

#[derive(Deserialize)]
struct NodeCase {
    key: String,
    expired: bool,
    bson: String,
    served: Option<String>,
}

#[derive(Serialize)]
struct RustCase {
    key: String,
    bson: String,
    served: String,
}

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

fn node_fixture() -> NodeFixture {
    serde_json::from_str(&std::fs::read_to_string(fixture_path("store_node_written.json")).unwrap()).unwrap()
}

fn document(base64: &str) -> Document {
    Document::from_reader(STANDARD.decode(base64).unwrap().as_slice()).unwrap()
}

fn bytes(document: &Document) -> Vec<u8> {
    let mut bytes = Vec::new();
    document.to_writer(&mut bytes).unwrap();
    bytes
}

fn wrapper(document: &Document) -> &Document {
    document.get_document("value").unwrap()
}

fn expires(document: &Document) -> i64 {
    match wrapper(document).get("expires") {
        Some(Bson::Double(expires)) => *expires as i64,
        other => panic!("expires is {other:?}"),
    }
}

fn rust_native_meta() -> Value {
    let meta: Meta = serde_json::from_value(json!({
        "id": "kitsu:3", "kitsu_id": 3, "type": "series", "name": "Trigun", "aliases": ["Trigun"], "genres": ["Action"],
        "userCount": 90000, "trailers": [], "links": [],
        "videos": [
            { "id": "kitsu:3:1", "title": "July", "released": "1998-04-01T15:30:00.250Z", "season": 1, "episode": 1 },
            { "id": "kitsu:3:2", "title": "Episode 2", "season": 1, "episode": 2 }
        ]
    }))
    .unwrap();
    json!({ "meta": meta, "cacheMaxAge": 43200 })
}

fn served(document: &Document) -> String {
    serde_json::to_string(&decode_payload(document, RUST_EXPIRES).unwrap()).unwrap()
}

#[test]
fn node_documents_are_served_as_node_serves_them() {
    let fixture = node_fixture();
    for case in fixture.cases.iter().filter(|case| !case.expired) {
        let payload = decode_payload(&document(&case.bson), fixture.recorded_at).unwrap();
        assert_eq!(Some(serde_json::to_string(&payload).unwrap()), case.served, "{}", case.key);
    }
}

#[test]
fn expired_node_documents_are_misses() {
    let fixture = node_fixture();
    let expired: Vec<&NodeCase> = fixture.cases.iter().filter(|case| case.expired).collect();
    assert!(!expired.is_empty());
    for case in expired {
        assert_eq!(decode_payload(&document(&case.bson), fixture.recorded_at), None, "{}", case.key);
    }
}

#[test]
fn rust_rewrites_node_values_byte_for_byte() {
    let fixture = node_fixture();
    for case in fixture.cases.iter().filter(|case| !case.expired) {
        let node = document(&case.bson);
        let payload = decode_payload(&node, fixture.recorded_at).unwrap();
        let rust = encode_document(&case.key, &payload, expires(&node));
        assert_eq!(bytes(wrapper(&rust)), bytes(wrapper(&node)), "{}", case.key);
        assert_eq!(rust.get_str("key").unwrap(), case.key);
    }
}

#[test]
fn rust_written_fixture_is_current() {
    let fixture = node_fixture();
    let rewritten = fixture.cases.iter().filter(|case| !case.expired).map(|case| {
        let payload = decode_payload(&document(&case.bson), fixture.recorded_at).unwrap();
        (case.key.clone(), payload)
    });
    let cases: Vec<RustCase> = rewritten
        .chain([(meta_key("3"), rust_native_meta())])
        .map(|(key, payload)| {
            let document = encode_document(&key, &payload, RUST_EXPIRES);
            RustCase { served: served(&document), bson: STANDARD.encode(bytes(&document)), key }
        })
        .collect();
    let text =
        format!("{}\n", serde_json::to_string_pretty(&json!({ "expires": RUST_EXPIRES, "cases": cases })).unwrap());
    let path = fixture_path("store_rust_written.json");
    if std::env::var_os("UPDATE_FIXTURES").is_some() {
        std::fs::write(&path, &text).unwrap();
    }
    assert_eq!(std::fs::read_to_string(&path).unwrap_or_default(), text, "rerun with UPDATE_FIXTURES=1");
}
