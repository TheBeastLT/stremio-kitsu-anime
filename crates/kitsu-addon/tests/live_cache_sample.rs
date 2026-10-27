use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use kitsu_addon::infrastructure::{
    Cache, CacheSettings, DocumentStore, Store, StoreError,
    cache::NoStats,
    store::{self, decode_stored, encode_document},
};
use mongodb::bson::{Bson, DateTime, Document};
use serde_json::Value;

const LINE_PREFIX: &str = "OUT doc ";
const SPLICE_RISK_BYTES: usize = 16_000;

struct OneDocument(Document);

#[async_trait]
impl DocumentStore for OneDocument {
    async fn find_one(&self, key: &str) -> Result<Option<Document>, StoreError> {
        Ok((self.0.get_str("key").ok() == Some(key)).then(|| self.0.clone()))
    }

    async fn find_many(&self, keys: &[String]) -> Result<Vec<Document>, StoreError> {
        let key = self.0.get_str("key").unwrap_or_default();
        Ok(if keys.iter().any(|wanted| wanted == key) { vec![self.0.clone()] } else { Vec::new() })
    }

    async fn upsert(&self, _key: &str, _document: Document) -> Result<(), StoreError> {
        Ok(())
    }
}

fn family(key: &str) -> String {
    let (kind, id) = key.split_once(':').unwrap_or((key, ""));
    let kind = kind.trim_start_matches("stremio-kitsu|");
    let id_kind = if id.starts_with("tt") {
        "tt"
    } else if !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()) {
        "kitsu"
    } else {
        id.split([':', '|']).next().unwrap_or("")
    };
    match kind {
        "catalog" => "catalog".to_owned(),
        _ => format!("{kind}:{id_kind}"),
    }
}

fn type_name(value: &Bson) -> &'static str {
    match value {
        Bson::Null => "null",
        Bson::Boolean(_) => "bool",
        Bson::Int32(_) => "int32",
        Bson::Int64(_) => "int64",
        Bson::Double(number) if number.fract() == 0.0 => "double(whole)",
        Bson::Double(_) => "double",
        Bson::String(_) => "string",
        Bson::DateTime(_) => "date",
        Bson::Array(_) => "array",
        Bson::Document(_) => "object",
        _ => "other",
    }
}

fn types_seen(value: &Bson, seen: &mut BTreeSet<&'static str>) {
    seen.insert(type_name(value));
    match value {
        Bson::Array(items) => {
            if items.iter().any(|item| matches!(item, Bson::Array(_))) {
                seen.insert("nested array");
            }
            for item in items {
                types_seen(item, seen);
            }
        }
        Bson::Document(entries) => {
            for item in entries.values() {
                types_seen(item, seen);
            }
        }
        Bson::Int64(number) if i32::try_from(*number).is_err() => {
            seen.insert("int beyond int32");
        }
        Bson::Double(number) if number.fract() == 0.0 && number.abs() > f64::from(i32::MAX) => {
            seen.insert("int beyond int32");
        }
        _ => {}
    }
}

fn number(value: &Bson) -> Option<f64> {
    match value {
        Bson::Int32(number) => Some(f64::from(*number)),
        Bson::Int64(number) => Some(*number as f64),
        Bson::Double(number) => Some(*number),
        _ => None,
    }
}

fn is_iso(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 24 && [4, 7, 10, 13, 16, 19, 23].map(|index| bytes[index]) == *b"--T::.Z"
}

fn served_like_node(stored: &Bson, served: &Value, path: &str) -> Result<(), String> {
    let mismatch = || Err(format!("{path}: stored {} served {}", type_name(stored), json_type(served)));
    match (stored, served) {
        (Bson::Null | Bson::Undefined, Value::Null) => Ok(()),
        (Bson::Boolean(left), Value::Bool(right)) if left == right => Ok(()),
        (Bson::String(left), Value::String(right)) if left == right => Ok(()),
        (Bson::DateTime(date), Value::String(text)) => {
            let parsed = DateTime::parse_rfc3339_str(text).ok().map(DateTime::timestamp_millis);
            if is_iso(text) && parsed == Some(date.timestamp_millis()) { Ok(()) } else { mismatch() }
        }
        (Bson::Array(left), Value::Array(right)) if left.len() == right.len() => left
            .iter()
            .zip(right)
            .enumerate()
            .try_for_each(|(index, (left, right))| served_like_node(left, right, &format!("{path}[{index}]"))),
        (Bson::Document(left), Value::Object(right)) => {
            if !left.keys().eq(right.keys()) {
                return Err(format!("{path}: key order or set differs"));
            }
            left.iter()
                .zip(right.values())
                .try_for_each(|((key, left), right)| served_like_node(left, right, &format!("{path}.{key}")))
        }
        (stored, Value::Number(served)) if number(stored).is_some_and(|stored| Some(stored) == served.as_f64()) => {
            Ok(())
        }
        _ => mismatch(),
    }
}

fn read_alike_by_node(sample: &Bson, written: &Bson, path: &str) -> Result<(), String> {
    let mismatch = || Err(format!("{path}: sample {} written {}", type_name(sample), type_name(written)));
    match (sample, written) {
        (Bson::Document(left), Bson::Document(right)) => {
            if !left.keys().eq(right.keys()) {
                return Err(format!("{path}: key order or set differs"));
            }
            left.iter()
                .zip(right.values())
                .try_for_each(|((key, left), right)| read_alike_by_node(left, right, &format!("{path}.{key}")))
        }
        (Bson::Array(left), Bson::Array(right)) if left.len() == right.len() => left
            .iter()
            .zip(right)
            .enumerate()
            .try_for_each(|(index, (left, right))| read_alike_by_node(left, right, &format!("{path}[{index}]"))),
        (Bson::DateTime(left), Bson::DateTime(right)) if left == right => Ok(()),
        (Bson::Null, Bson::Null) => Ok(()),
        (left, right) if number(left).is_some() || number(right).is_some() => {
            if number(left) == number(right) {
                Ok(())
            } else {
                mismatch()
            }
        }
        (left, right) if left == right => Ok(()),
        _ => mismatch(),
    }
}

fn rust_number_rule(written: &Bson, path: &str) -> Result<(), String> {
    match written {
        Bson::Double(number) if number.fract() == 0.0 && i32::try_from(*number as i64).is_ok() => {
            Err(format!("{path}: whole int32-range number written as double"))
        }
        Bson::Int64(_) => Err(format!("{path}: int64 written")),
        Bson::Array(items) => {
            items.iter().enumerate().try_for_each(|(index, item)| rust_number_rule(item, &format!("{path}[{index}]")))
        }
        Bson::Document(entries) => {
            entries.iter().try_for_each(|(key, item)| rust_number_rule(item, &format!("{path}.{key}")))
        }
        _ => Ok(()),
    }
}

fn json_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn settings() -> CacheSettings {
    CacheSettings {
        enabled: true,
        budget_bytes: 256 * 1024 * 1024,
        meta_ttl: Duration::from_secs(86_400),
        catalog_ttl: Duration::from_secs(86_400),
        cache_max_age: 43_200,
    }
}

async fn served_by_cache(document: &Document) -> Result<Arc<Value>, String> {
    let key = document.get_str("key").map_err(|error| error.to_string())?;
    let store = Store::new(Arc::new(OneDocument(document.clone())), Duration::from_secs(5));
    let cache: Cache<String> = Cache::new(settings(), Some(store), Arc::new(NoStats));
    let missing = || async { Err("not served from the stored document".to_owned()) };
    let (kind, id) = key.split_once(':').ok_or("key without a kind")?;
    match kind {
        "stremio-kitsu|meta" => cache.wrap_meta(id, missing).await,
        "stremio-kitsu|images" => cache.wrap_images(id, missing).await,
        "stremio-kitsu|id_mapping" => cache.wrap_id_mapping(id, missing).await,
        "stremio-kitsu|catalog" => {
            let mut parts = id.splitn(3, '|');
            let (catalog, genre, skip) = (parts.next(), parts.next(), parts.next());
            cache.wrap_catalog(catalog.unwrap_or(""), genre.unwrap_or(""), skip.unwrap_or(""), missing).await
        }
        _ => Err(format!("unknown kind {kind}")),
    }
}

#[derive(Default)]
struct Family {
    count: usize,
    decoded: usize,
    served: usize,
    round_trip: usize,
    live: usize,
    expired: usize,
    max_expiry_gap_ms: i64,
    types: BTreeSet<&'static str>,
    wrapper_types: BTreeSet<String>,
    mismatches: Vec<String>,
}

#[derive(Default)]
struct Totals {
    lines: usize,
    unparseable: usize,
    splice_risk: usize,
    duplicate_ids: usize,
}

async fn check(document: &Document, now: i64, family: &mut Family) -> Result<(), String> {
    family.count += 1;
    let key = document.get_str("key").unwrap_or_default().to_owned();
    let (Ok(wrapper), Ok(expires_at)) = (document.get_document("value"), document.get_datetime("expiresAt")) else {
        return Err("document without a value wrapper or a Date expiresAt".to_owned());
    };
    let expires = wrapper.get("expires").and_then(number);
    let sample_value = wrapper.get("value").cloned().unwrap_or(Bson::Undefined);
    family.wrapper_types.insert(format!(
        "expiresAt {}, value.expires {}",
        type_name(&Bson::DateTime(*expires_at)),
        wrapper.get("expires").map_or("absent", type_name)
    ));
    types_seen(&sample_value, &mut family.types);
    let Some(expires) = expires else { return Err("value.expires is not a number".to_owned()) };
    family.max_expiry_gap_ms = family.max_expiry_gap_ms.max((expires as i64 - expires_at.timestamp_millis()).abs());
    let live = (now as f64) <= expires;
    if live {
        family.live += 1;
    } else {
        family.expired += 1;
    }
    let at_expiry = expires as i64;
    match (decode_stored(document, at_expiry), decode_stored(document, at_expiry + 1)) {
        (Some(stored), None) if stored.expires == Some(at_expiry) => {
            if let Err(reason) = served_like_node(&sample_value, &stored.value, "value") {
                return Err(format!("decode: {reason}"));
            }
            family.decoded += 1;
        }
        (Some(_), None) => return Err("decoded expiry differs from value.expires".to_owned()),
        (None, _) => return Err("not decoded while live".to_owned()),
        (Some(_), Some(_)) => return Err("decoded after expiry".to_owned()),
    }
    if decode_stored(document, now).is_some() != live {
        return Err("live/expired decision differs from Node".to_owned());
    }
    if live {
        match served_by_cache(document).await {
            Ok(served) => match served_like_node(&sample_value, &served, "value") {
                Ok(()) => family.served += 1,
                Err(reason) => return Err(format!("cache: {reason}")),
            },
            Err(reason) => return Err(format!("cache: {reason}")),
        }
    }
    let decoded = decode_stored(document, at_expiry).map(|stored| stored.value).unwrap_or_default();
    let written = encode_document(&key, &decoded, at_expiry);
    let written_wrapper = written.get_document("value").cloned().unwrap_or_default();
    let checks = [
        (written.get_str("key").ok() == Some(key.as_str())).then_some(()).ok_or("key differs".to_owned()),
        (written.get_datetime("expiresAt").ok() == Some(&DateTime::from_millis(at_expiry)))
            .then_some(())
            .ok_or("expiresAt differs from value.expires or is not a Date".to_owned()),
        matches!(written_wrapper.get("expires"), Some(Bson::Double(written)) if written.to_bits() == expires.to_bits())
            .then_some(())
            .ok_or("value.expires differs or is not a double".to_owned()),
        read_alike_by_node(&sample_value, written_wrapper.get("value").unwrap_or(&Bson::Undefined), "value"),
        rust_number_rule(written_wrapper.get("value").unwrap_or(&Bson::Undefined), "value"),
    ];
    if let Some(reason) = checks.into_iter().find_map(Result::err) {
        return Err(format!("write: {reason}"));
    }
    family.round_trip += 1;
    Ok(())
}

#[tokio::test]
async fn live_cache_documents_read_and_rewrite_like_node() {
    let Some(path) = std::env::var_os("KITSU_LIVE_CACHE_SAMPLE") else {
        return eprintln!("KITSU_LIVE_CACHE_SAMPLE unset: skipped");
    };
    let text = std::fs::read_to_string(path).unwrap();
    let now = store::now_ms();
    let mut totals = Totals::default();
    let mut families: BTreeMap<String, Family> = BTreeMap::new();
    let mut seen_ids = HashSet::new();
    for line in text.lines().filter(|line| !line.is_empty()) {
        totals.lines += 1;
        let body = line.strip_prefix(LINE_PREFIX).unwrap_or(line);
        let parsed = serde_json::from_str::<Value>(body).ok().and_then(|value| Bson::try_from(value).ok());
        let Some(Bson::Document(document)) = parsed else {
            totals.unparseable += 1;
            continue;
        };
        let key = document.get_str("key").unwrap_or_default().to_owned();
        let name = if body.len() >= SPLICE_RISK_BYTES {
            totals.splice_risk += 1;
            format!("{} (spliced?)", family(&key))
        } else if seen_ids.insert(document.get("_id").cloned().map(|id| id.to_string())) {
            family(&key)
        } else {
            totals.duplicate_ids += 1;
            continue;
        };
        let family = families.entry(name).or_default();
        if let Err(reason) = check(&document, now, family).await {
            family.mismatches.push(reason);
        }
    }
    eprintln!(
        "lines {}, unparseable {}, parseable but >= {SPLICE_RISK_BYTES} bytes (export splices them) {}, duplicate ids {}",
        totals.lines, totals.unparseable, totals.splice_risk, totals.duplicate_ids
    );
    eprintln!(
        "| family | docs | live | expired | decode ok | served ok | round-trip ok | expiresAt - value.expires (max ms) | value types | wrapper types as exported |"
    );
    for (name, family) in &families {
        eprintln!(
            "| {name} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            family.count,
            family.live,
            family.expired,
            family.decoded,
            family.served,
            family.round_trip,
            family.max_expiry_gap_ms,
            family.types.iter().copied().collect::<Vec<_>>().join(", "),
            family.wrapper_types.iter().cloned().collect::<Vec<_>>().join("; ")
        );
        for mismatch in family.mismatches.iter().collect::<BTreeSet<_>>() {
            eprintln!("    {name}: {mismatch}");
        }
    }
    let failures: usize = families.values().map(|family| family.mismatches.len()).sum();
    assert_eq!(failures, 0, "{failures} documents differ from what Node reads");
}
