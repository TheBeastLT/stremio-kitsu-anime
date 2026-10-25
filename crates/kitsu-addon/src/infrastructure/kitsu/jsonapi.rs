use std::{cmp::Ordering, collections::HashMap, fmt::Write};

use serde_json::{Map, Number, Value};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct DeserialiseError(String);

const I64_LIMIT: f64 = 9_223_372_036_854_775_807.0;
const MAX_ARRAY_INDEX: u64 = 4_294_967_294;

pub fn parse(body: &[u8]) -> serde_json::Result<Value> {
    serde_json::from_slice::<Value>(body).map(js_value)
}

pub fn js_value(value: Value) -> Value {
    match value {
        Value::Number(number) => Value::Number(js_number(&number)),
        Value::Array(items) => Value::Array(items.into_iter().map(js_value).collect()),
        Value::Object(map) => Value::Object(js_object(map)),
        other => other,
    }
}

fn js_number(number: &Number) -> Number {
    match number.as_f64() {
        Some(float) if float.fract() == 0.0 && float.abs() < I64_LIMIT => Number::from(float as i64),
        Some(float) => Number::from_f64(float).unwrap_or_else(|| number.clone()),
        None => number.clone(),
    }
}

fn js_object(map: Map<String, Value>) -> Map<String, Value> {
    if !map.keys().any(|key| array_index(key).is_some()) {
        return map.into_iter().map(|(key, value)| (key, js_value(value))).collect();
    }
    let mut entries: Vec<(Option<u64>, String, Value)> =
        map.into_iter().map(|(key, value)| (array_index(&key), key, js_value(value))).collect();
    entries.sort_by(|left, right| match (left.0, right.0) {
        (Some(left), Some(right)) => left.cmp(&right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    });
    entries.into_iter().map(|(_, key, value)| (key, value)).collect()
}

fn array_index(key: &str) -> Option<u64> {
    let canonical = key == "0" || (key.starts_with(|c: char| ('1'..='9').contains(&c)) && key.len() <= 10);
    if !canonical || !key.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    key.parse().ok().filter(|index| *index <= MAX_ARRAY_INDEX)
}

pub fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => number.as_f64().is_some_and(|float| float != 0.0 && !float.is_nan()),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(_) | Value::Object(_)) => true,
    }
}

pub fn js_text(value: Option<&Value>) -> String {
    match value {
        None => "undefined".to_owned(),
        Some(Value::Null) => "null".to_owned(),
        Some(Value::Bool(flag)) => flag.to_string(),
        Some(Value::Number(number)) => number.to_string(),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| if item.is_null() { String::new() } else { js_text(Some(item)) })
            .collect::<Vec<_>>()
            .join(","),
        Some(Value::Object(_)) => "[object Object]".to_owned(),
    }
}

pub fn query(params: &Map<String, Value>) -> String {
    query_with(params, None)
}

fn query_with(params: &Map<String, Value>, prefix: Option<&str>) -> String {
    params
        .iter()
        .map(|(param, value)| match prefix {
            Some(prefix) => query_format(value, &format!("{prefix}[{param}]")),
            None => query_format(value, param),
        })
        .collect::<Vec<_>>()
        .join("&")
}

fn query_format(value: &Value, key: &str) -> String {
    match value {
        Value::Array(values) => values.iter().map(|value| query_format(value, key)).collect::<Vec<_>>().join("&"),
        Value::Object(map) => query_with(map, Some(key)),
        other => format!("{}={}", encode_uri_component(key), encode_uri_component(&js_text(Some(other)))),
    }
}

pub fn encode_uri_component(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            encoded.push(byte as char);
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

pub fn deserialise(document: Value) -> Result<Value, DeserialiseError> {
    let Value::Object(mut document) = document else {
        return Ok(document);
    };
    let included = document.shift_remove("included");
    let included_resources = match &included {
        Some(Value::Array(resources)) => resources.as_slice(),
        _ => &[],
    };
    let mut state = State::default();
    match document.get_mut("data") {
        Some(Value::Array(items)) => deserialise_array(items, included_resources, &mut state)?,
        Some(Value::Object(data)) => {
            Linker { primary: &[], included: included_resources, state: &mut state }.link_relationships(data)?;
        }
        Some(Value::Null) | None if truthy(included.as_ref()) => {
            return Err(DeserialiseError("Cannot destructure 'data' as it is null or undefined".to_owned()));
        }
        _ => {}
    }
    if let Some(Value::Object(data)) = document.get_mut("data")
        && truthy(data.get("attributes"))
    {
        deattribute(data);
    }
    Ok(Value::Object(document))
}

fn deserialise_array(items: &mut [Value], included: &[Value], state: &mut State) -> Result<(), DeserialiseError> {
    for index in 0..items.len() {
        let mut value = items[index].clone();
        match &mut value {
            Value::Object(data) => {
                Linker { primary: items, included, state }.link_relationships(data)?;
                if truthy(data.get("attributes")) {
                    deattribute(data);
                }
            }
            Value::Null => {
                return Err(DeserialiseError("Cannot read properties of null (reading 'relationships')".into()));
            }
            _ => {}
        }
        items[index] = value;
    }
    Ok(())
}

#[derive(Default)]
struct State {
    previously_linked: HashMap<String, Value>,
    relationship_cache: HashMap<String, Value>,
}

struct Linker<'a> {
    primary: &'a [Value],
    included: &'a [Value],
    state: &'a mut State,
}

impl Linker<'_> {
    fn link_relationships(&mut self, data: &mut Map<String, Value>) -> Result<(), DeserialiseError> {
        let Some(Value::Object(relationships)) = data.get("relationships").cloned() else {
            return Ok(());
        };
        for (key, relationship) in &relationships {
            match relationship.get("data") {
                _ if relationship.is_null() => {
                    return Err(DeserialiseError(format!("Cannot read properties of null (reading '{key}')")));
                }
                Some(Value::Array(resources)) => self.link_array(data, key, relationship, resources)?,
                Some(resource) if truthy(Some(resource)) => self.link_object(data, key, relationship, resource)?,
                _ => link_attr(data, key, relationship),
            }
        }
        data.shift_remove("relationships");
        Ok(())
    }

    fn link_array(
        &mut self,
        data: &mut Map<String, Value>,
        key: &str,
        relationship: &Value,
        resources: &[Value],
    ) -> Result<(), DeserialiseError> {
        let mut linked = Map::new();
        copy_truthy(relationship, "links", &mut linked);
        copy_truthy(relationship, "meta", &mut linked);
        let mut items = Vec::with_capacity(resources.len());
        for resource in resources {
            if resource.is_null() {
                return Err(DeserialiseError("Cannot read properties of null (reading 'type')".to_owned()));
            }
            let cache_key = resource_key(resource);
            let mut item = match self.state.previously_linked.get(&cache_key) {
                Some(cached) => cached.clone(),
                None => self.link(resource, cache_key)?,
            };
            set_meta(&mut item, resource.get("meta"));
            items.push(item);
        }
        linked.insert("data".to_owned(), Value::Array(items));
        data.insert(key.to_owned(), Value::Object(linked));
        Ok(())
    }

    fn link_object(
        &mut self,
        data: &mut Map<String, Value>,
        key: &str,
        relationship: &Value,
        resource: &Value,
    ) -> Result<(), DeserialiseError> {
        let cache_key = resource_key(resource);
        let item = match self.state.previously_linked.get(&cache_key) {
            Some(cached) if cached.get("meta") == resource.get("meta") => cached.clone(),
            Some(cached) => {
                let mut item = cached.clone();
                set_meta(&mut item, resource.get("meta"));
                item
            }
            None => self.link(resource, cache_key)?,
        };
        let mut linked = Map::new();
        linked.insert("data".to_owned(), item);
        let relationship_key = format!("{}#{}#{key}", js_text(data.get("type")), js_text(data.get("id")));
        let relationships =
            self.state.relationship_cache.entry(relationship_key).or_insert_with(|| relationship.clone());
        copy_truthy(relationships, "links", &mut linked);
        copy_truthy(relationships, "meta", &mut linked);
        data.insert(key.to_owned(), Value::Object(linked));
        Ok(())
    }

    fn link(&mut self, resource: &Value, cache_key: String) -> Result<Value, DeserialiseError> {
        let id = resource.get("id");
        let kind = resource.get("type");
        let mut filtered = if truthy(id) && truthy(kind) {
            self.find(id, kind).unwrap_or_else(|| identifier(id, kind))
        } else {
            Map::new()
        };
        self.state.previously_linked.insert(cache_key.clone(), Value::Object(identifier(id, kind)));
        if truthy(filtered.get("relationships")) {
            self.link_relationships(&mut filtered)?;
        }
        if let Some(meta) = resource.get("meta").filter(|meta| truthy(Some(meta))) {
            filtered.insert("meta".to_owned(), meta.clone());
        }
        deattribute(&mut filtered);
        let linked = Value::Object(filtered);
        self.state.previously_linked.insert(cache_key, linked.clone());
        Ok(linked)
    }

    fn find(&self, id: Option<&Value>, kind: Option<&Value>) -> Option<Map<String, Value>> {
        self.primary.iter().chain(self.included).find_map(|candidate| match candidate {
            Value::Object(map) if map.get("id") == id && map.get("type") == kind => Some(map.clone()),
            _ => None,
        })
    }
}

fn identifier(id: Option<&Value>, kind: Option<&Value>) -> Map<String, Value> {
    let mut map = Map::new();
    if let Some(id) = id {
        map.insert("id".to_owned(), id.clone());
    }
    if let Some(kind) = kind {
        map.insert("type".to_owned(), kind.clone());
    }
    map
}

fn resource_key(resource: &Value) -> String {
    format!("{}#{}", js_text(resource.get("type")), js_text(resource.get("id")))
}

fn link_attr(data: &mut Map<String, Value>, key: &str, relationship: &Value) {
    let mut linked = Map::new();
    copy_truthy(relationship, "links", &mut linked);
    copy_truthy(relationship, "meta", &mut linked);
    data.insert(key.to_owned(), Value::Object(linked));
}

fn copy_truthy(source: &Value, field: &str, target: &mut Map<String, Value>) {
    if let Some(value) = source.get(field).filter(|value| truthy(Some(value))) {
        target.insert(field.to_owned(), value.clone());
    }
}

fn set_meta(item: &mut Value, meta: Option<&Value>) {
    if let Value::Object(map) = item {
        match meta {
            Some(meta) => {
                map.insert("meta".to_owned(), meta.clone());
            }
            None => {
                map.shift_remove("meta");
            }
        }
    }
}

fn deattribute(data: &mut Map<String, Value>) {
    let nested = match data.get("attributes") {
        Some(Value::Object(attributes)) => attributes.get("attributes").filter(|nested| truthy(Some(nested))).cloned(),
        _ => return,
    };
    if let Some(nested) = nested {
        data.insert("attributes".to_owned(), nested);
    } else if let Some(Value::Object(attributes)) = data.shift_remove("attributes") {
        data.extend(attributes);
    }
}

#[cfg(test)]
mod tests;
