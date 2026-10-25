use std::collections::BTreeSet;
use std::fs::File;
use std::io::{BufRead, BufReader};

use kitsu_search::query::parse_query;
use serde::Deserialize;
use serde_json::{Map, Value};

mod common;

use common::golden;

#[derive(Deserialize)]
struct Row {
    query: String,
    parsed: Map<String, Value>,
}

fn field_differences(expected: &Map<String, Value>, got: &Map<String, Value>) -> Vec<String> {
    let keys: BTreeSet<&String> = expected.keys().chain(got.keys()).collect();
    keys.into_iter()
        .filter(|key| expected.get(*key) != got.get(*key))
        .map(|key| format!("{key}: got {:?}, expected {:?}", got.get(key), expected.get(key)))
        .collect()
}

#[test]
fn parse_query_matches_every_golden_line() {
    let Some(path) = golden("parse_query.jsonl") else { return };
    let mut total = 0;
    let mut differences = Vec::new();
    for line in BufReader::new(File::open(path).unwrap()).lines() {
        let row: Row = serde_json::from_str(&line.unwrap()).unwrap();
        total += 1;
        let Value::Object(got) = serde_json::to_value(parse_query(&row.query)).unwrap() else { unreachable!() };
        let fields = field_differences(&row.parsed, &got);
        if !fields.is_empty() {
            differences.push(format!("{:?}: {}", row.query, fields.join("; ")));
        }
    }
    assert_eq!(total, 8_496);
    assert!(
        differences.is_empty(),
        "{} differences, first: {:#?}",
        differences.len(),
        &differences[..differences.len().min(20)]
    );
}
