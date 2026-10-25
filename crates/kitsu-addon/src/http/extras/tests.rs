use std::{fs::File, path::Path};

use serde_json::Value;

use super::*;
use crate::application::ExtraValue;

fn node_cases() -> Vec<(String, Vec<(String, ExtraValue)>)> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/http/exchanges.json.zst");
    let recorded: Value = serde_json::from_slice(&zstd::decode_all(File::open(path).unwrap()).unwrap()).unwrap();
    recorded["querystring"]
        .as_array()
        .unwrap()
        .iter()
        .map(|case| {
            let entries = case[1]
                .as_array()
                .unwrap()
                .iter()
                .map(|entry| {
                    let value = match &entry[1] {
                        Value::String(text) => ExtraValue::One(text.clone()),
                        Value::Array(values) => {
                            ExtraValue::Many(values.iter().map(|value| value.as_str().unwrap().to_owned()).collect())
                        }
                        other => panic!("unexpected value {other}"),
                    };
                    (entry[0].as_str().unwrap().to_owned(), value)
                })
                .collect();
            (case[0].as_str().unwrap().to_owned(), entries)
        })
        .collect()
}

fn entries(extras: &Extras) -> Vec<(String, ExtraValue)> {
    extras.entries().into_iter().map(|(key, value)| (key.to_owned(), value.clone())).collect()
}

#[test]
fn parses_like_node_querystring() {
    let cases = node_cases();
    assert_eq!(cases.len(), 41);
    for (query, expected) in cases {
        assert_eq!(entries(&parse_query(&query)), expected, "{query}");
    }
}

#[test]
fn keeps_at_most_a_thousand_pairs() {
    let query: Vec<String> = (0..1005).map(|index| format!("k{index}=v")).collect();
    assert_eq!(parse_query(&query.join("&")).entries().len(), 1000);
}

#[test]
fn resource_extras_read_the_last_segment_without_json() {
    let extras = resource_extras("/catalog/anime/kitsu-anime-list/search=one%20piece&skip=20.json");
    assert_eq!(extras.get("search"), Some(&ExtraValue::One("one piece".to_owned())));
    assert_eq!(extras.get("skip"), Some(&ExtraValue::One("20".to_owned())));
}

#[test]
fn resource_extras_of_an_upper_case_suffix_drop_it_too() {
    assert_eq!(resource_extras("/catalog/anime/x/skip=20.JSON").get("skip"), Some(&ExtraValue::One("20".to_owned())));
}

#[test]
fn resource_extras_after_a_trailing_slash_are_empty() {
    assert_eq!(resource_extras("/catalog/anime/x/skip=20.json/"), Extras::default());
}

#[test]
fn resource_extras_of_a_short_segment_are_empty() {
    assert_eq!(resource_extras("/a/.jso"), Extras::default());
}

#[test]
fn encoded_ampersands_stay_inside_their_value() {
    let extras = resource_extras("/catalog/anime/x/search=tom%26jerry&skip=0.json");
    assert_eq!(extras.get("search"), Some(&ExtraValue::One("tom&jerry".to_owned())));
}
