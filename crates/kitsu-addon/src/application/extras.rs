use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtraValue {
    One(String),
    Many(Vec<String>),
}

impl ExtraValue {
    pub fn truthy(&self) -> bool {
        match self {
            Self::One(text) => !text.is_empty(),
            Self::Many(_) => true,
        }
    }

    pub fn text(&self) -> String {
        match self {
            Self::One(text) => text.clone(),
            Self::Many(values) => values.join(","),
        }
    }

    fn json(&self) -> Value {
        match self {
            Self::One(text) => Value::String(text.clone()),
            Self::Many(values) => Value::Array(values.iter().cloned().map(Value::String).collect()),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Extras(Vec<(String, ExtraValue)>);

impl Extras {
    pub fn get(&self, key: &str) -> Option<&ExtraValue> {
        self.0.iter().find(|(name, _)| name == key).map(|(_, value)| value)
    }

    pub fn truthy(&self, key: &str) -> Option<&ExtraValue> {
        self.get(key).filter(|value| value.truthy())
    }

    pub fn add(&mut self, key: String, value: String) {
        let Some((_, entry)) = self.0.iter_mut().find(|(name, _)| *name == key) else {
            self.0.push((key, ExtraValue::One(value)));
            return;
        };
        match entry {
            ExtraValue::Many(values) => values.push(value),
            ExtraValue::One(first) => *entry = ExtraValue::Many(vec![std::mem::take(first), value]),
        }
    }

    pub fn entries(&self) -> Vec<(&str, &ExtraValue)> {
        let mut indexes: Vec<(u32, &str, &ExtraValue)> = self
            .0
            .iter()
            .filter_map(|(key, value)| array_index(key).map(|index| (index, key.as_str(), value)))
            .collect();
        indexes.sort_by_key(|(index, _, _)| *index);
        let others = self.0.iter().filter(|(key, _)| array_index(key).is_none());
        indexes
            .into_iter()
            .map(|(_, key, value)| (key, value))
            .chain(others.map(|(key, value)| (key.as_str(), value)))
            .collect()
    }

    pub fn to_json(&self) -> Map<String, Value> {
        self.entries().into_iter().map(|(key, value)| (key.to_owned(), value.json())).collect()
    }
}

fn array_index(key: &str) -> Option<u32> {
    let canonical = key == "0" || (!key.starts_with('0') && !key.is_empty() && key.bytes().all(|b| b.is_ascii_digit()));
    canonical.then(|| key.parse::<u32>().ok()).flatten().filter(|index| *index < u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extras(pairs: &[(&str, &str)]) -> Extras {
        let mut extras = Extras::default();
        for (key, value) in pairs {
            extras.add((*key).to_owned(), (*value).to_owned());
        }
        extras
    }

    #[test]
    fn a_repeated_key_becomes_a_list_in_order() {
        let extras = extras(&[("a", "1"), ("b", "x"), ("a", "2"), ("a", "3")]);
        assert_eq!(extras.get("a"), Some(&ExtraValue::Many(vec!["1".into(), "2".into(), "3".into()])));
    }

    #[test]
    fn a_list_reads_as_its_comma_joined_text() {
        assert_eq!(ExtraValue::Many(vec!["20".into(), "40".into()]).text(), "20,40");
    }

    #[test]
    fn an_empty_value_is_falsy_and_a_list_truthy() {
        assert!(!ExtraValue::One(String::new()).truthy());
        assert!(ExtraValue::Many(vec![String::new(), String::new()]).truthy());
    }

    #[test]
    fn integer_keys_come_first_in_ascending_order_like_a_js_object() {
        let extras = extras(&[("1", "a"), ("b", "2"), ("0", "c"), ("01", "d"), ("4294967295", "e")]);
        let keys: Vec<&str> = extras.entries().into_iter().map(|(key, _)| key).collect();
        assert_eq!(keys, ["0", "1", "b", "01", "4294967295"]);
    }

    #[test]
    fn json_keeps_lists_as_arrays() {
        let extras = extras(&[("a", "1"), ("a", "2"), ("b", "")]);
        assert_eq!(Value::Object(extras.to_json()), serde_json::json!({ "a": ["1", "2"], "b": "" }));
    }
}
