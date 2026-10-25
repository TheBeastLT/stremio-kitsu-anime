use indexmap::IndexMap;
use serde_json::{Map, Number, Value};

use crate::infrastructure::{js, store::iso_string};

const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

pub type JsObject = IndexMap<String, JsValue>;

#[derive(Debug, Clone, PartialEq, Default)]
pub enum JsValue {
    #[default]
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Date(f64),
    Array(Vec<JsValue>),
    Object(JsObject),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("TypeError: {0}")]
pub struct TypeError(pub String);

pub static UNDEFINED: JsValue = JsValue::Undefined;

impl JsValue {
    pub fn get(&self, key: &str) -> &JsValue {
        match self {
            JsValue::Object(object) => object.get(key).unwrap_or(&UNDEFINED),
            _ => &UNDEFINED,
        }
    }

    pub fn prop(&self, key: &str) -> Result<&JsValue, TypeError> {
        match self {
            JsValue::Undefined | JsValue::Null => {
                Err(TypeError(format!("Cannot read properties of {} (reading '{key}')", self.text())))
            }
            _ => Ok(self.get(key)),
        }
    }

    pub fn truthy(&self) -> bool {
        match self {
            JsValue::Undefined | JsValue::Null => false,
            JsValue::Bool(flag) => *flag,
            JsValue::Number(number) => *number != 0.0 && !number.is_nan(),
            JsValue::String(text) => !text.is_empty(),
            JsValue::Date(_) | JsValue::Array(_) | JsValue::Object(_) => true,
        }
    }

    pub fn is_nullish(&self) -> bool {
        matches!(self, JsValue::Undefined | JsValue::Null)
    }

    #[must_use]
    pub fn or(&self, other: &JsValue) -> JsValue {
        if self.truthy() { self.clone() } else { other.clone() }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            JsValue::String(text) => Some(text),
            _ => None,
        }
    }

    pub fn is_str(&self, expected: &str) -> bool {
        self.as_str() == Some(expected)
    }

    pub fn as_array(&self) -> Option<&Vec<JsValue>> {
        match self {
            JsValue::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&JsObject> {
        match self {
            JsValue::Object(object) => Some(object),
            _ => None,
        }
    }

    pub fn is_integer(&self) -> bool {
        matches!(self, JsValue::Number(number) if number.is_finite() && number.fract() == 0.0)
    }

    #[allow(clippy::float_cmp)]
    pub fn strict_equals(&self, other: &JsValue) -> bool {
        match (self, other) {
            (JsValue::Number(left), JsValue::Number(right)) => left == right,
            (JsValue::Date(_) | JsValue::Array(_) | JsValue::Object(_), _) => false,
            _ => self == other,
        }
    }

    #[allow(clippy::float_cmp)]
    pub fn same_value_zero(&self, other: &JsValue) -> bool {
        match (self, other) {
            (JsValue::Number(left), JsValue::Number(right)) => left == right || (left.is_nan() && right.is_nan()),
            _ => self.strict_equals(other),
        }
    }

    pub fn to_number(&self) -> f64 {
        match self {
            JsValue::Undefined | JsValue::Object(_) => f64::NAN,
            JsValue::Null => 0.0,
            JsValue::Bool(flag) => f64::from(u8::from(*flag)),
            JsValue::Number(number) | JsValue::Date(number) => *number,
            JsValue::String(text) => js::to_number(text),
            JsValue::Array(items) => match items.as_slice() {
                [] => 0.0,
                [item] if !matches!(item, JsValue::Array(_)) => item.to_number(),
                _ => f64::NAN,
            },
        }
    }

    pub fn text(&self) -> String {
        match self {
            JsValue::Undefined => "undefined".to_owned(),
            JsValue::Null => "null".to_owned(),
            JsValue::Bool(flag) => flag.to_string(),
            JsValue::Number(number) => number_text(*number),
            JsValue::String(text) => text.clone(),
            JsValue::Date(time) => iso(*time).unwrap_or_else(|| "Invalid Date".to_owned()),
            JsValue::Array(items) => items
                .iter()
                .map(|item| if item.is_nullish() { String::new() } else { item.text() })
                .collect::<Vec<_>>()
                .join(","),
            JsValue::Object(_) => "[object Object]".to_owned(),
        }
    }

    pub fn to_json(&self) -> Value {
        match self {
            JsValue::Undefined | JsValue::Null => Value::Null,
            JsValue::Bool(flag) => Value::Bool(*flag),
            JsValue::Number(number) => number_json(*number),
            JsValue::String(text) => Value::String(text.clone()),
            JsValue::Date(time) => iso(*time).map_or(Value::Null, Value::String),
            JsValue::Array(items) => Value::Array(items.iter().map(JsValue::to_json).collect()),
            JsValue::Object(object) => Value::Object(object_json(object)),
        }
    }
}

pub fn object_json(object: &JsObject) -> Map<String, Value> {
    object
        .iter()
        .filter(|(_, value)| !matches!(value, JsValue::Undefined))
        .map(|(key, value)| (key.clone(), value.to_json()))
        .collect()
}

impl From<&Value> for JsValue {
    fn from(value: &Value) -> Self {
        match value {
            Value::Null => JsValue::Null,
            Value::Bool(flag) => JsValue::Bool(*flag),
            Value::Number(number) => JsValue::Number(number.as_f64().unwrap_or(f64::NAN)),
            Value::String(text) => JsValue::String(text.clone()),
            Value::Array(items) => JsValue::Array(items.iter().map(JsValue::from).collect()),
            Value::Object(map) => JsValue::Object(map.iter().map(|(key, value)| (key.clone(), value.into())).collect()),
        }
    }
}

impl From<&str> for JsValue {
    fn from(text: &str) -> Self {
        JsValue::String(text.to_owned())
    }
}

impl From<String> for JsValue {
    fn from(text: String) -> Self {
        JsValue::String(text)
    }
}

impl From<f64> for JsValue {
    fn from(number: f64) -> Self {
        JsValue::Number(number)
    }
}

impl From<JsObject> for JsValue {
    fn from(object: JsObject) -> Self {
        JsValue::Object(object)
    }
}

pub fn date(value: &JsValue) -> f64 {
    match value {
        JsValue::String(text) => js::date_parse(text),
        JsValue::Date(time) => *time,
        other => js::time_clip(other.to_number()),
    }
}

pub fn add(number: f64, value: &JsValue) -> JsValue {
    match value {
        JsValue::String(text) => JsValue::String(format!("{}{text}", number_text(number))),
        JsValue::Array(_) | JsValue::Object(_) | JsValue::Date(_) => {
            JsValue::String(format!("{}{}", number_text(number), value.text()))
        }
        other => JsValue::Number(number + other.to_number()),
    }
}

pub fn number_text(number: f64) -> String {
    if number.is_nan() {
        "NaN".to_owned()
    } else if number.is_infinite() {
        if number > 0.0 { "Infinity" } else { "-Infinity" }.to_owned()
    } else if number.fract() == 0.0 && number.abs() < 1e21 {
        format!("{number:.0}")
    } else {
        number.to_string()
    }
}

fn number_json(number: f64) -> Value {
    if !number.is_finite() {
        return Value::Null;
    }
    if number.fract() == 0.0 && number.abs() <= MAX_SAFE_INTEGER {
        return Value::from(number as i64);
    }
    Number::from_f64(number).map_or(Value::Null, Value::Number)
}

fn iso(time: f64) -> Option<String> {
    let time = js::time_clip(time);
    if time.is_nan() { None } else { iso_string(time as i64) }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn undefined_object_fields_are_omitted_and_array_items_become_null() {
        let object: JsObject = [
            ("a".to_owned(), JsValue::Undefined),
            ("b".to_owned(), JsValue::Null),
            ("c".to_owned(), JsValue::Array(vec![JsValue::Undefined, JsValue::Number(1.0)])),
        ]
        .into_iter()
        .collect();
        assert_eq!(JsValue::Object(object).to_json(), json!({"b": null, "c": [null, 1]}));
    }

    #[test]
    fn dates_serialise_as_iso_strings_and_invalid_dates_as_null() {
        assert_eq!(JsValue::Date(0.0).to_json(), json!("1970-01-01T00:00:00.000Z"));
        assert_eq!(JsValue::Date(f64::NAN).to_json(), Value::Null);
    }

    #[test]
    fn numbers_print_like_js() {
        assert_eq!(number_text(12.0), "12");
        assert_eq!(number_text(8.5), "8.5");
        assert_eq!(number_text(f64::NAN), "NaN");
        assert_eq!(JsValue::Number(-0.0).to_json(), json!(0));
    }

    #[test]
    fn adding_a_string_concatenates() {
        assert_eq!(add(1.0, &JsValue::from("2")), JsValue::from("12"));
        assert_eq!(add(1.0, &JsValue::Null), JsValue::Number(1.0));
        assert!(matches!(add(1.0, &JsValue::Undefined), JsValue::Number(number) if number.is_nan()));
    }

    #[test]
    fn a_string_kitsu_id_is_not_strictly_equal_to_a_number() {
        assert!(!JsValue::from("12").strict_equals(&JsValue::Number(12.0)));
        assert!(JsValue::from("12").strict_equals(&JsValue::from("12")));
    }

    #[test]
    fn reading_a_property_of_undefined_is_a_type_error() {
        assert!(JsValue::Undefined.prop("data").is_err());
        assert_eq!(JsValue::from("text").prop("data"), Ok(&JsValue::Undefined));
    }
}
