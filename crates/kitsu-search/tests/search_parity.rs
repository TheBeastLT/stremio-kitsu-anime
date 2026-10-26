use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::time::Instant;

use kitsu_search::index::Index;
use kitsu_search::records::Sources;
use kitsu_search::{MIN_CONFIDENCE, Searcher};
use serde::Deserialize;

mod common;

use common::golden;

fn data(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../static/data").join(name)
}

const CONFIDENCE_TOLERANCE: f64 = 1e-9;

#[derive(Deserialize)]
struct GoldenHit {
    id: u32,
}

#[derive(Deserialize)]
struct Row {
    query: String,
    ids: Vec<GoldenHit>,
    confidence: Option<f64>,
}

fn searcher() -> Searcher {
    let sources =
        Sources::read(&data("search_titles.jsonl"), &data("search_aliases.json"), &data("imdb_mapping.json")).unwrap();
    Searcher::new(Index::build(&sources.records, &sources.mapping, &sources.aliases))
}

fn compare(searcher: &Searcher, row: &Row) -> Option<String> {
    let ranking = searcher.search_with(&row.query, 20, 0.0);
    let confidence = if ranking.hits.is_empty() { None } else { ranking.confidence };
    let ids: Vec<u32> = if confidence.is_some_and(|confidence| confidence >= MIN_CONFIDENCE) {
        ranking.hits.iter().map(|hit| hit.id).collect()
    } else {
        Vec::new()
    };
    let expected: Vec<u32> = row.ids.iter().map(|hit| hit.id).collect();
    let confidence_differs = match (confidence, row.confidence) {
        (Some(got), Some(expected)) => (got - expected).abs() > CONFIDENCE_TOLERANCE,
        (None, None) => false,
        _ => true,
    };
    (confidence_differs || ids != expected).then(|| {
        format!(
            "{:?}\n  confidence got {confidence:?} want {:?}\n  got  {ids:?}\n  want {expected:?}",
            row.query, row.confidence
        )
    })
}

#[test]
fn every_golden_query_ranks_the_same_ids_with_the_same_confidence() {
    let Some(path) = golden("search.jsonl") else { return };
    let rows: Vec<Row> = BufReader::new(File::open(path).unwrap())
        .lines()
        .map(|line| serde_json::from_str(&line.unwrap()).unwrap())
        .collect();
    assert_ne!(rows.len(), 0);
    let searcher = searcher();
    let threads = std::thread::available_parallelism().map_or(4, usize::from);
    let started = Instant::now();
    let differences: Vec<String> = std::thread::scope(|scope| {
        let chunks: Vec<_> = rows
            .chunks(rows.len().div_ceil(threads))
            .map(|chunk| scope.spawn(|| chunk.iter().filter_map(|row| compare(&searcher, row)).collect::<Vec<_>>()))
            .collect();
        chunks.into_iter().flat_map(|chunk| chunk.join().unwrap()).collect()
    });
    eprintln!("{} queries in {:?} on {threads} threads", rows.len(), started.elapsed());
    assert!(
        differences.is_empty(),
        "{} of {} queries differ:\n{}",
        differences.len(),
        rows.len(),
        differences[..differences.len().min(15)].join("\n")
    );
}

fn fingerprint(searcher: &Searcher, rows: &[Row]) -> u64 {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let threads = std::thread::available_parallelism().map_or(4, usize::from);
    let parts: Vec<u64> = std::thread::scope(|scope| {
        let chunks: Vec<_> = rows
            .chunks(rows.len().div_ceil(threads))
            .map(|chunk| {
                scope.spawn(move || {
                    let mut hasher = DefaultHasher::new();
                    for row in chunk {
                        let ranking = searcher.search_with(&row.query, 20, 0.0);
                        ranking.confidence.map(f64::to_bits).hash(&mut hasher);
                        for hit in &ranking.hits {
                            (hit.id, hit.score.to_bits()).hash(&mut hasher);
                        }
                    }
                    hasher.finish()
                })
            })
            .collect();
        chunks.into_iter().map(|chunk| chunk.join().unwrap()).collect()
    });
    let mut hasher = DefaultHasher::new();
    parts.hash(&mut hasher);
    hasher.finish()
}

#[test]
#[ignore = "slow: rebuilds the index and searches the whole golden corpus ten times"]
fn ten_runs_over_the_corpus_are_identical() {
    let Some(path) = golden("search.jsonl") else { return };
    let rows: Vec<Row> = BufReader::new(File::open(path).unwrap())
        .lines()
        .map(|line| serde_json::from_str(&line.unwrap()).unwrap())
        .collect();
    let fingerprints: Vec<u64> = (0..10).map(|_| fingerprint(&searcher(), &rows)).collect();
    eprintln!("{fingerprints:x?}");
    assert!(fingerprints.iter().all(|fingerprint| *fingerprint == fingerprints[0]), "{fingerprints:x?}");
}
