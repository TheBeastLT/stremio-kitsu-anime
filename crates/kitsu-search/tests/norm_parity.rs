use std::fs::File;
use std::io::{BufRead, BufReader};

use kitsu_search::norm::norm;
use serde::Deserialize;

mod common;

use common::golden;

#[derive(Deserialize)]
struct Row {
    title: String,
    norm: String,
}

#[test]
fn norm_matches_every_golden_line() {
    let Some(path) = golden("norm.jsonl") else { return };
    let mut total = 0;
    let mut differences = Vec::new();
    for line in BufReader::new(File::open(path).unwrap()).lines() {
        let row: Row = serde_json::from_str(&line.unwrap()).unwrap();
        total += 1;
        let got = norm(&row.title);
        if got != row.norm {
            differences.push(format!("{:?} -> {:?}, expected {:?}", row.title, got, row.norm));
        }
    }
    assert_ne!(total, 0);
    assert!(
        differences.is_empty(),
        "{} differences, first: {:#?}",
        differences.len(),
        &differences[..differences.len().min(20)]
    );
}
