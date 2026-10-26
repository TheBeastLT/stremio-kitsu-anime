use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::sync::LazyLock;

use kitsu_search::index::Index;
use kitsu_search::query::parse_query;
use kitsu_search::records::Sources;
use kitsu_search::score::{Query, candidates, title_score};
use serde::Deserialize;

mod common;

use common::golden;

const SCORE_TOLERANCE: f64 = 1e-9;

fn data(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../static/data").join(name)
}

static INDEX: LazyLock<Index> = LazyLock::new(|| {
    let sources =
        Sources::read(&data("search_titles.jsonl"), &data("search_aliases.json"), &data("imdb_mapping.json")).unwrap();
    Index::build(&sources.records, &sources.mapping, &sources.aliases)
});

#[derive(Deserialize)]
struct Candidate {
    id: u64,
    score: f64,
}

#[derive(Deserialize)]
struct Row {
    query: String,
    candidates: Vec<Candidate>,
}

fn scored(query: &str) -> Vec<(u64, f64)> {
    let parsed = parse_query(query);
    if parsed.base.is_empty() {
        return Vec::new();
    }
    let query = Query::new(&INDEX, &parsed);
    candidates(&INDEX, &query)
        .into_iter()
        .map(|entry| (INDEX.entries()[entry as usize].id, title_score(&INDEX, entry, &query).score))
        .collect()
}

#[test]
fn candidates_and_title_scores_match_every_golden_query() {
    let Some(path) = golden("candidates.jsonl") else { return };
    let rows: Vec<Row> = BufReader::new(File::open(path).unwrap())
        .lines()
        .map(|line| serde_json::from_str(&line.unwrap()).unwrap())
        .collect();
    assert_eq!(rows.len(), 200);
    let mut differences = Vec::new();
    for row in &rows {
        let got = scored(&row.query);
        let ids: Vec<u64> = got.iter().map(|(id, _)| *id).collect();
        let expected: Vec<u64> = row.candidates.iter().map(|candidate| candidate.id).collect();
        if ids != expected {
            differences.push(format!("{:?}: ids differ ({} got, {} expected)", row.query, ids.len(), expected.len()));
            continue;
        }
        for ((id, score), candidate) in got.iter().zip(&row.candidates) {
            if (score - candidate.score).abs() > SCORE_TOLERANCE {
                differences.push(format!("{:?} {id}: got {score}, expected {}", row.query, candidate.score));
            }
        }
    }
    assert!(
        differences.is_empty(),
        "{} differences: {:#?}",
        differences.len(),
        &differences[..differences.len().min(20)]
    );
}
