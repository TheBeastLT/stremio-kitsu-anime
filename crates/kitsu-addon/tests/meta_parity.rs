use std::{
    collections::HashSet,
    fs::File,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock},
};

use async_trait::async_trait;
use kitsu_addon::{
    application::{EntryOptions, GenreUrl, ImageLookup, Mapping, Metadata, enrich_imdb, enrich_kitsu},
    infrastructure::{Images, config::DEFAULT_ADDON_URL, kitsu::jsonapi},
};
use kitsu_search::records::read_records;
use serde_json::Value;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/meta").join(name)
}

fn zstd_json(name: &str) -> Value {
    let bytes = zstd::decode_all(File::open(fixture(name)).unwrap()).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

static RESPONSES: LazyLock<Value> = LazyLock::new(|| zstd_json("responses.json.zst"));
static GOLDENS: LazyLock<Value> = LazyLock::new(|| zstd_json("goldens.json.zst"));
static DEVIATIONS: LazyLock<Value> =
    LazyLock::new(|| serde_json::from_reader(File::open(fixture("rust_deviations.json")).unwrap()).unwrap());
static MAPPING: LazyLock<Arc<Mapping>> = LazyLock::new(|| {
    let records = read_records(File::open(fixture("records.jsonl")).unwrap()).unwrap();
    Arc::new(Mapping::load(&fixture("mapping_rows.json"), &records).unwrap())
});

struct SyntheticImages;

#[async_trait]
impl ImageLookup for SyntheticImages {
    async fn images(&self, kitsu_id: &str) -> Images {
        let base = format!("https://images.test/{kitsu_id}");
        match kitsu_id.parse::<u64>().unwrap() % 4 {
            0 => Images {
                logo: Some(format!("{base}/logo.png")),
                logo_lang: Some("en".to_owned()),
                poster: Some(format!("{base}/poster.jpg")),
                background: Some(format!("{base}/background.jpg")),
                thumbnail: None,
            },
            2 => Images { background: Some(format!("{base}/background.jpg")), ..Images::default() },
            _ => Images::default(),
        }
    }
}

fn metadata() -> Metadata {
    Metadata::new(MAPPING.clone(), Arc::new(SyntheticImages), GenreUrl::new(DEFAULT_ADDON_URL))
}

fn anime(kitsu_id: &str) -> Value {
    let body = RESPONSES["kitsu"][kitsu_id].as_str().unwrap();
    let document = jsonapi::deserialise(jsonapi::parse(body.as_bytes()).unwrap()).unwrap();
    document["data"].clone()
}

fn cinemeta(imdb_id: &str) -> Value {
    GOLDENS["imdb"][imdb_id]["cinemeta"].clone()
}

fn assert_json_eq(case: &str, actual: &Value, expected: &Value) {
    let actual = serde_json::to_string(actual).unwrap();
    let expected = serde_json::to_string(expected).unwrap();
    if actual != expected {
        let position = actual.bytes().zip(expected.bytes()).position(|(left, right)| left != right).unwrap_or(0);
        let from = position.saturating_sub(200);
        panic!(
            "{case} differs at byte {position}\n rust: {}\n node: {}",
            &actual[from..(position + 200).min(actual.len())],
            &expected[from..(position + 200).min(expected.len())]
        );
    }
}

fn kitsu_cases() -> Vec<(&'static String, &'static Value)> {
    GOLDENS["kitsu"].as_object().unwrap().iter().collect()
}

#[tokio::test]
async fn entry_metas_equal_node() {
    let metadata = metadata();
    for (kitsu_id, golden) in kitsu_cases() {
        let meta = metadata.to_entry_meta(&anime(kitsu_id), EntryOptions::default()).await.unwrap();
        let actual = kitsu_addon::application::JsValue::Object(meta).to_json();
        assert_json_eq(&format!("entry {kitsu_id}"), &actual, &golden["entry"]);
    }
}

#[tokio::test]
async fn entry_metas_with_relations_and_no_images_equal_node() {
    let metadata = metadata();
    let options = EntryOptions { images: false, relations: true, ..EntryOptions::default() };
    let cases: Vec<_> = kitsu_cases().into_iter().filter(|(_, golden)| golden.get("relations").is_some()).collect();
    assert_eq!(cases.len(), 3);
    for (kitsu_id, golden) in cases {
        let meta = metadata.to_entry_meta(&anime(kitsu_id), options).await.unwrap();
        let actual = kitsu_addon::application::JsValue::Object(meta).to_json();
        assert_json_eq(&format!("relations {kitsu_id}"), &actual, &golden["relations"]);
    }
}

#[tokio::test]
async fn catalog_metas_equal_node() {
    let metadata = metadata();
    for (kitsu_id, golden) in kitsu_cases() {
        let actual = metadata.to_catalog_meta(&anime(kitsu_id)).await.unwrap();
        assert_json_eq(&format!("catalog {kitsu_id}"), &actual, &golden["catalog"]);
    }
}

#[tokio::test]
async fn enriched_kitsu_metas_equal_node() {
    let metadata = metadata();
    for (kitsu_id, golden) in kitsu_cases() {
        let meta = metadata.to_entry_meta(&anime(kitsu_id), EntryOptions::default()).await.unwrap();
        let actual =
            enrich_kitsu(meta, &MAPPING, metadata.genre_url(), |imdb_id| async move { Some(cinemeta(&imdb_id)) }).await;
        assert_json_eq(&format!("enriched {kitsu_id}"), &actual, &golden["enriched"]);
        assert_json_eq(
            &format!("cached catalog {kitsu_id}"),
            &metadata.catalog_fields(&actual),
            &golden["cachedCatalog"],
        );
    }
}

#[test]
fn mapping_lookups_equal_node() {
    for (kitsu_id, golden) in kitsu_cases() {
        assert_eq!(MAPPING.has_imdb_mapping(kitsu_id), golden["hasImdbMapping"], "{kitsu_id}");
        let cap = MAPPING.episode_cap(kitsu_id.parse().unwrap());
        assert_eq!(cap.map_or(Value::Null, Value::from), golden["episodeCap"], "{kitsu_id}");
    }
    for (imdb_id, golden) in GOLDENS["imdb"].as_object().unwrap() {
        assert_eq!(MAPPING.has_imdb_mapping(imdb_id), golden["hasImdbMapping"], "{imdb_id}");
        assert_eq!(serde_json::to_value(MAPPING.kitsu_ids_for_imdb(imdb_id)).unwrap(), golden["kitsuIds"], "{imdb_id}");
    }
}

#[tokio::test]
async fn enriched_imdb_metas_equal_node() {
    let metadata = metadata();
    let metadata = &metadata;
    for (imdb_id, golden) in GOLDENS["imdb"].as_object().unwrap() {
        let actual = enrich_imdb(cinemeta(imdb_id), &MAPPING, |kitsu_id| async move {
            metadata.to_entry_meta(&anime(&kitsu_id), EntryOptions::default()).await
        })
        .await
        .unwrap();
        assert_json_eq(&format!("imdb {imdb_id}"), &actual, &golden["enriched"]);
    }
}

#[test]
fn recorded_cinemeta_responses_parse_to_the_metas_node_read() {
    let mut checked = HashSet::new();
    for (url, recorded) in RESPONSES["cinemeta"].as_object().unwrap() {
        let imdb_id = url.rsplit('/').next().unwrap().trim_end_matches(".json");
        let Ok(Value::Object(document)) = jsonapi::parse(recorded["body"].as_str().unwrap().as_bytes()) else {
            continue;
        };
        if let Some(meta) = document.get("meta").filter(|meta| meta.get("imdb_id").is_some()) {
            assert_json_eq(url, meta, &cinemeta(imdb_id));
            checked.insert(imdb_id.to_owned());
        }
    }
    assert_eq!(checked.len(), GOLDENS["imdb"].as_object().unwrap().len() - 1);
}

fn with_dates(meta: &Value) -> kitsu_addon::application::JsObject {
    use kitsu_addon::application::JsValue;
    let JsValue::Object(mut meta) = JsValue::from(meta) else { panic!("an object") };
    if let Some(JsValue::Array(videos)) = meta.get_mut("videos") {
        for video in videos.iter_mut() {
            if let JsValue::Object(video) = video {
                let released = kitsu_addon::infrastructure::js::date_parse(&video["released"].text());
                video.insert("released".to_owned(), JsValue::Date(released));
            }
        }
    }
    meta
}

#[tokio::test]
async fn synthetic_enrichments_equal_node() {
    let metadata = metadata();
    let metadata = &metadata;
    let cases = GOLDENS["synthetic"].as_object().unwrap();
    assert_eq!(cases.len(), 8);
    for (name, case) in cases {
        let expected = |field: &str| DEVIATIONS[name].get(field).unwrap_or(&case[field]);
        let imdb = case["imdb"].clone();
        let actual =
            enrich_kitsu(with_dates(&case["kitsu"]), &MAPPING, metadata.genre_url(), |_| async move { Some(imdb) })
                .await;
        assert_json_eq(&format!("{name} kitsu"), &actual, expected("enrichedKitsu"));
        let actual = enrich_imdb(case["imdb"].clone(), &MAPPING, |kitsu_id| async move {
            metadata.to_entry_meta(&anime(&kitsu_id), EntryOptions::default()).await
        })
        .await
        .unwrap();
        assert_json_eq(&format!("{name} imdb"), &actual, expected("enrichedImdb"));
    }
}

#[test]
fn rust_deviations_differ_from_the_node_goldens() {
    for (name, fields) in DEVIATIONS.as_object().unwrap() {
        for (field, expected) in fields.as_object().unwrap() {
            assert_ne!(expected, &GOLDENS["synthetic"][name][field], "{name} {field} equals Node's");
        }
    }
}
