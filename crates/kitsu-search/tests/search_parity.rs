use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::time::Instant;

use kitsu_search::index::Index;
use kitsu_search::records::Sources;
use kitsu_search::{MIN_CONFIDENCE, Searcher};
use serde::Deserialize;
use serde_json::value::RawValue;

mod common;

use common::golden;

fn data(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../static/data").join(name)
}

#[derive(Deserialize)]
struct GoldenHit {
    id: u32,
    score: Box<RawValue>,
}

#[derive(Deserialize)]
struct Row {
    query: String,
    ids: Vec<GoldenHit>,
    confidence: Option<Box<RawValue>>,
}

fn number(raw: &RawValue) -> f64 {
    raw.get().parse().unwrap_or_else(|error| panic!("not a number {}: {error}", raw.get()))
}

fn round_up(digits: &mut [u8]) {
    for digit in digits.iter_mut().rev() {
        if *digit == b'.' {
            continue;
        }
        if *digit == b'9' {
            *digit = b'0';
        } else {
            *digit += 1;
            return;
        }
    }
    panic!("carry past the leading digit");
}

fn js_fixed_12(value: f64) -> f64 {
    let magnitude = value.abs();
    assert!(magnitude < 10.0, "{value}");
    let exact = format!("{magnitude:.1100}");
    let point = exact.find('.').unwrap();
    let mut kept = exact.as_bytes()[..point + 13].to_vec();
    if exact.as_bytes()[point + 13] >= b'5' {
        round_up(&mut kept);
    }
    let rounded: f64 = String::from_utf8(kept).unwrap().parse().unwrap();
    if value.is_sign_negative() { -rounded } else { rounded }
}

#[allow(clippy::float_cmp)]
fn differs(got: f64, expected: f64) -> bool {
    js_fixed_12(got) != expected
}

fn searcher() -> Searcher {
    let sources =
        Sources::read(&data("search_titles.jsonl"), &data("search_aliases.json"), &data("imdb_mapping.json")).unwrap();
    Searcher::new(Index::build(&sources.records, &sources.mapping, &sources.aliases))
}

fn compare(searcher: &Searcher, row: &Row) -> Option<String> {
    let ranking = searcher.search_with(&row.query, 20, 0.0);
    let confidence = if ranking.hits.is_empty() { None } else { ranking.confidence };
    let hits =
        if confidence.is_some_and(|confidence| confidence >= MIN_CONFIDENCE) { ranking.hits } else { Vec::new() };
    let expected_confidence = row.confidence.as_deref().map(number);
    let confidence_differs = match (confidence, expected_confidence) {
        (Some(got), Some(expected)) => differs(got, expected),
        (None, None) => false,
        _ => true,
    };
    let ids_differ = hits.len() != row.ids.len()
        || hits.iter().zip(&row.ids).any(|(hit, want)| hit.id != want.id || differs(hit.score, number(&want.score)));
    (confidence_differs || ids_differ).then(|| {
        let got: Vec<(u32, f64)> = hits.iter().map(|hit| (hit.id, hit.score)).collect();
        let want: Vec<(u32, f64)> = row.ids.iter().map(|hit| (hit.id, number(&hit.score))).collect();
        format!(
            "{:?}\n  confidence got {confidence:?} want {expected_confidence:?}\n  got  {got:?}\n  want {want:?}",
            row.query
        )
    })
}

#[test]
fn every_golden_query_ranks_as_node() {
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
