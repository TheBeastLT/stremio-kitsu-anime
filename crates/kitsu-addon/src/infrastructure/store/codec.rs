use mongodb::bson::{Bson, DateTime, Document, doc};
use serde_json::{Map, Number, Value};

use super::META_KEY_PREFIX;

const JS_MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
const JS_MAX_DATE_MILLIS: i64 = 8_640_000_000_000_000;
const MILLIS_PER_DAY: i64 = 86_400_000;

pub fn encode_document(key: &str, value: &Value, expires_ms: i64) -> Document {
    doc! {
        "key": key,
        "value": { "value": payload_to_bson(key, value), "expires": Bson::Double(expires_ms as f64) },
        "expiresAt": DateTime::from_millis(expires_ms),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stored {
    pub value: Value,
    pub expires: Option<i64>,
}

pub fn decode_payload(document: &Document, now_ms: i64) -> Option<Value> {
    decode_stored(document, now_ms).map(|stored| stored.value)
}

pub fn decode_stored(document: &Document, now_ms: i64) -> Option<Stored> {
    let Some(Bson::Document(wrapper)) = document.get("value") else {
        return None;
    };
    let expires = expires_of(wrapper.get("expires"));
    if expires.is_some_and(|expires| now_ms as f64 > expires) {
        return None;
    }
    match wrapper.get("value") {
        None | Some(Bson::Undefined) => None,
        Some(value) => Some(Stored { value: bson_to_json(value), expires: expires.map(|expires| expires as i64) }),
    }
}

fn expires_of(expires: Option<&Bson>) -> Option<f64> {
    match expires {
        Some(Bson::Double(value)) if !value.is_nan() => Some(*value),
        Some(Bson::Int32(value)) => Some(f64::from(*value)),
        Some(Bson::Int64(value)) => Some(*value as f64),
        _ => None,
    }
}

pub fn bson_to_json(value: &Bson) -> Value {
    match value {
        Bson::Null | Bson::Undefined => Value::Null,
        Bson::Boolean(flag) => Value::Bool(*flag),
        Bson::Int32(number) => Value::from(*number),
        Bson::Int64(number) => js_number(*number as f64),
        Bson::Double(number) => js_number(*number),
        Bson::String(text) => Value::String(text.clone()),
        Bson::DateTime(date) => iso_string(date.timestamp_millis()).map_or(Value::Null, Value::String),
        Bson::Array(items) => Value::Array(items.iter().map(bson_to_json).collect()),
        Bson::Document(entries) => {
            Value::Object(entries.iter().map(|(key, item)| (key.clone(), bson_to_json(item))).collect::<Map<_, _>>())
        }
        other => other.clone().into_relaxed_extjson(),
    }
}

fn js_number(number: f64) -> Value {
    if !number.is_finite() {
        return Value::Null;
    }
    if number.fract() == 0.0 && number.abs() <= JS_MAX_SAFE_INTEGER {
        return Value::from(number as i64);
    }
    Number::from_f64(number).map_or(Value::Null, Value::Number)
}

pub fn json_to_bson(value: &Value) -> Bson {
    match value {
        Value::Null => Bson::Null,
        Value::Bool(flag) => Bson::Boolean(*flag),
        Value::Number(number) => number_to_bson(number),
        Value::String(text) => Bson::String(text.clone()),
        Value::Array(items) => Bson::Array(items.iter().map(json_to_bson).collect()),
        Value::Object(entries) => {
            Bson::Document(entries.iter().map(|(key, item)| (key.clone(), json_to_bson(item))).collect())
        }
    }
}

fn number_to_bson(number: &Number) -> Bson {
    let value = number.as_f64().unwrap_or(f64::NAN);
    let is_int32 = value.fract() == 0.0
        && value >= f64::from(i32::MIN)
        && value <= f64::from(i32::MAX)
        && !(value == 0.0 && value.is_sign_negative());
    if is_int32 { Bson::Int32(value as i32) } else { Bson::Double(value) }
}

fn payload_to_bson(key: &str, value: &Value) -> Bson {
    let mut bson = json_to_bson(value);
    if is_kitsu_meta_key(key)
        && let Some(Bson::Array(videos)) = bson
            .as_document_mut()
            .and_then(|wrapper| wrapper.get_document_mut("meta").ok())
            .and_then(|meta| meta.get_mut("videos"))
    {
        for video in videos.iter_mut().filter_map(Bson::as_document_mut) {
            if let Some(released) = video.get_mut("released")
                && let Some(millis) = released.as_str().and_then(parse_iso)
            {
                *released = Bson::DateTime(DateTime::from_millis(millis));
            }
        }
    }
    bson
}

fn is_kitsu_meta_key(key: &str) -> bool {
    key.strip_prefix(META_KEY_PREFIX)
        .and_then(|rest| rest.strip_prefix(':'))
        .is_some_and(|id| !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()))
}

pub fn iso_string(millis: i64) -> Option<String> {
    if millis.abs() > JS_MAX_DATE_MILLIS {
        return None;
    }
    let days = millis.div_euclid(MILLIS_PER_DAY);
    let time = millis.rem_euclid(MILLIS_PER_DAY);
    let (year, month, day) = civil_from_days(days);
    let year = if (0..=9999).contains(&year) { format!("{year:04}") } else { format!("{year:+07}") };
    Some(format!(
        "{year}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        time / 3_600_000,
        time / 60_000 % 60,
        time / 1000 % 60,
        time % 1000
    ))
}

pub fn parse_iso(text: &str) -> Option<i64> {
    let bytes = text.as_bytes();
    if bytes.len() != 24 || [4, 7, 10, 13, 16, 19, 23].map(|index| bytes[index]) != *b"--T::.Z" {
        return None;
    }
    let field = |from: usize, to: usize| -> Option<i64> {
        let digits = text.get(from..to)?;
        digits.bytes().all(|byte| byte.is_ascii_digit()).then(|| digits.parse().ok())?
    };
    let days = days_from_civil(field(0, 4)?, field(5, 7)?, field(8, 10)?);
    let millis = days * MILLIS_PER_DAY
        + field(11, 13)? * 3_600_000
        + field(14, 16)? * 60_000
        + field(17, 19)? * 1000
        + field(20, 23)?;
    (iso_string(millis).as_deref() == Some(text)).then_some(millis)
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 { shifted_month + 3 } else { shifted_month - 9 };
    (year_of_era + era * 400 + i64::from(month <= 2), month, day)
}

pub fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const NOW: i64 = 1_700_000_000_000;

    fn stored(wrapper: Document) -> Document {
        doc! { "key": "k", "value": wrapper }
    }

    #[test]
    fn whole_numbers_in_int32_range_are_int32() {
        assert_eq!(json_to_bson(&json!(150_000)), Bson::Int32(150_000));
    }

    #[test]
    fn whole_floats_are_int32_like_a_js_number() {
        assert_eq!(json_to_bson(&json!(3.0)), Bson::Int32(3));
    }

    #[test]
    fn int32_bounds_are_int32() {
        assert_eq!(json_to_bson(&json!(-2_147_483_648.0)), Bson::Int32(i32::MIN));
    }

    #[test]
    fn whole_numbers_past_int32_are_doubles() {
        assert_eq!(json_to_bson(&json!(5_000_000_000_i64)), Bson::Double(5e9));
    }

    #[test]
    fn fractions_are_doubles() {
        assert_eq!(json_to_bson(&json!(82.25)), Bson::Double(82.25));
    }

    #[test]
    fn negative_zero_is_a_double() {
        let bson = json_to_bson(&json!(-0.0));
        assert!(matches!(bson, Bson::Double(value) if value == 0.0 && value.is_sign_negative()));
    }

    #[test]
    fn whole_doubles_read_as_integers() {
        assert_eq!(serde_json::to_string(&bson_to_json(&Bson::Double(5e9))).unwrap(), "5000000000");
    }

    #[test]
    fn negative_zero_reads_as_zero() {
        assert_eq!(serde_json::to_string(&bson_to_json(&Bson::Double(-0.0))).unwrap(), "0");
    }

    #[test]
    fn non_finite_doubles_read_as_null() {
        assert_eq!(bson_to_json(&Bson::Double(f64::NAN)), Value::Null);
    }

    #[test]
    fn int64_reads_as_a_number() {
        assert_eq!(bson_to_json(&Bson::Int64(7)), json!(7));
    }

    #[test]
    fn dates_read_as_iso_strings_with_milliseconds() {
        assert_eq!(
            bson_to_json(&Bson::DateTime(DateTime::from_millis(909_187_200_000))),
            json!("1998-10-24T00:00:00.000Z")
        );
    }

    #[test]
    fn dates_before_the_epoch_read_as_iso_strings() {
        assert_eq!(iso_string(-220_838_400_001).as_deref(), Some("1963-01-01T23:59:59.999Z"));
    }

    #[test]
    fn years_past_9999_use_the_expanded_format() {
        assert_eq!(iso_string(253_402_300_800_000).as_deref(), Some("+010000-01-01T00:00:00.000Z"));
    }

    #[test]
    fn years_before_zero_use_the_expanded_format() {
        assert_eq!(iso_string(-62_167_219_200_001).as_deref(), Some("-000001-12-31T23:59:59.999Z"));
    }

    #[test]
    fn dates_outside_the_js_range_read_as_null() {
        assert_eq!(bson_to_json(&Bson::DateTime(DateTime::from_millis(JS_MAX_DATE_MILLIS + 1))), Value::Null);
    }

    #[test]
    fn iso_strings_parse_back_to_millis() {
        assert_eq!(parse_iso("1998-10-31T13:05:09.123Z"), Some(909_839_109_123));
    }

    #[test]
    fn iso_strings_without_milliseconds_do_not_parse() {
        assert_eq!(parse_iso("1998-10-31T13:05:09Z"), None);
    }

    #[test]
    fn impossible_dates_do_not_parse() {
        assert_eq!(parse_iso("1998-02-30T00:00:00.000Z"), None);
    }

    #[test]
    fn nulls_are_kept() {
        assert_eq!(bson_to_json(&Bson::Document(doc! { "thumbnail": Bson::Null })), json!({ "thumbnail": null }));
    }

    #[test]
    fn key_order_is_kept() {
        let value = json!({ "z": [1, "a", null, true], "a": { "k": "v" } });
        assert_eq!(bson_to_json(&json_to_bson(&value)), value);
    }

    #[test]
    fn kitsu_meta_released_strings_are_written_as_dates() {
        let value = json!({ "meta": { "videos": [{ "released": "1998-10-24T00:00:00.000Z" }] } });
        let document = encode_document("stremio-kitsu|meta:1", &value, NOW);
        let released = document.get_document("value").unwrap().get_document("value").unwrap().get_document("meta");
        let video = released.unwrap().get_array("videos").unwrap()[0].as_document().unwrap().clone();
        assert_eq!(video.get("released"), Some(&Bson::DateTime(DateTime::from_millis(909_187_200_000))));
    }

    #[test]
    fn imdb_meta_released_strings_stay_strings() {
        let value = json!({ "meta": { "videos": [{ "released": "1998-10-24T00:00:00.000Z" }] } });
        let document = encode_document("stremio-kitsu|meta:tt0213338", &value, NOW);
        let payload = document.get_document("value").unwrap().get("value").unwrap();
        assert_eq!(payload, &json_to_bson(&value));
    }

    #[test]
    fn documents_carry_key_wrapper_and_expiry() {
        let document = encode_document("stremio-kitsu|images:1", &json!({ "logo": "l" }), NOW);
        assert_eq!(
            document,
            doc! {
                "key": "stremio-kitsu|images:1",
                "value": { "value": { "logo": "l" }, "expires": Bson::Double(NOW as f64) },
                "expiresAt": DateTime::from_millis(NOW),
            }
        );
    }

    #[test]
    fn payload_is_read_before_expiry() {
        let document = stored(doc! { "value": 1, "expires": Bson::Double((NOW + 1) as f64) });
        assert_eq!(decode_payload(&document, NOW), Some(json!(1)));
    }

    #[test]
    fn payload_is_read_at_the_expiry_millisecond() {
        let document = stored(doc! { "value": 1, "expires": Bson::Double(NOW as f64) });
        assert_eq!(decode_payload(&document, NOW), Some(json!(1)));
    }

    #[test]
    fn expired_payload_is_a_miss() {
        let document = stored(doc! { "value": 1, "expires": Bson::Double((NOW - 1) as f64) });
        assert_eq!(decode_payload(&document, NOW), None);
    }

    #[test]
    fn missing_expiry_never_expires() {
        let document = stored(doc! { "value": 1, "expires": Bson::Null });
        assert_eq!(decode_payload(&document, NOW), Some(json!(1)));
    }

    #[test]
    fn a_stored_null_is_a_null_hit() {
        let document = stored(doc! { "value": Bson::Null, "expires": Bson::Null });
        assert_eq!(decode_payload(&document, NOW), Some(Value::Null));
    }

    #[test]
    fn a_wrapper_without_value_is_a_miss() {
        assert_eq!(decode_payload(&stored(doc! { "expires": Bson::Null }), NOW), None);
    }

    #[test]
    fn a_legacy_string_value_is_a_miss() {
        let document = doc! { "key": "k", "value": "{\"value\":1,\"expires\":null}" };
        assert_eq!(decode_payload(&document, NOW), None);
    }
}
