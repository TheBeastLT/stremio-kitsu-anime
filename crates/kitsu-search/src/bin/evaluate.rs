use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use indexmap::IndexMap;
use kitsu_search::Searcher;
use kitsu_search::index::Index;
use kitsu_search::records::Sources;
use serde::Deserialize;

#[derive(Deserialize)]
struct Item {
    bucket: String,
    query: String,
    accepted: Vec<u64>,
    preferred: u64,
}

#[derive(Deserialize)]
struct Fix {
    query: String,
    verdict: String,
    #[serde(default)]
    correct: Vec<u64>,
}

#[derive(Default)]
struct Bucket {
    n: u32,
    accepted: u32,
    preferred: u32,
    kitsu: u32,
    none: u32,
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read_jsonl<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Vec<T>, String> {
    let file = File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut rows = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line.map_err(|error| format!("{}: {error}", path.display()))?;
        if !line.is_empty() {
            rows.push(serde_json::from_str(&line).map_err(|error| format!("{}: {error}", path.display()))?);
        }
    }
    Ok(rows)
}

fn pct(count: u32, n: u32) -> String {
    format!("{:>7}", format!("{}%", fixed_1(100.0 * f64::from(count) / f64::from(n))))
}

fn fixed_1(value: f64) -> String {
    let exact = format!("{value:.1100}");
    let point = exact.find('.').unwrap_or(exact.len());
    let rest = &exact[point + 2..];
    let tie = rest.starts_with('5') && rest[1..].bytes().all(|digit| digit == b'0');
    if tie { format!("{:.1}", value + 0.05) } else { format!("{value:.1}") }
}

fn row(name: &str, b: &Bucket) -> String {
    format!(
        "{name:<64} {:>4} {}   {}  {:>9}  {}",
        b.n,
        pct(b.accepted, b.n),
        pct(b.preferred, b.n),
        b.none,
        pct(b.kitsu, b.n)
    )
}

fn run() -> Result<(), String> {
    let raw = std::env::args().any(|arg| arg == "--raw");
    let diff = std::env::args().any(|arg| arg == "--diff");
    let dir = root().join("search_eval");
    let fixes: HashMap<String, Fix> = if raw {
        HashMap::new()
    } else {
        read_jsonl::<Fix>(&dir.join("label_fixes.jsonl"))?.into_iter().map(|fix| (fix.query.clone(), fix)).collect()
    };
    let sample: Vec<Item> = read_jsonl::<Item>(&dir.join("sample.jsonl"))?
        .into_iter()
        .filter(|item| {
            !fixes
                .get(&item.query)
                .is_some_and(|fix| matches!(fix.verdict.as_str(), "garbage" | "absent" | "ambiguous"))
        })
        .map(|mut item| {
            if let Some(fix) = fixes.get(&item.query).filter(|fix| !fix.correct.is_empty()) {
                item.accepted.clone_from(&fix.correct);
                item.preferred = fix.correct[0];
            }
            item
        })
        .collect();
    let tops_file = File::open(dir.join("kitsu_search_tops.json")).map_err(|error| error.to_string())?;
    let kitsu_tops: HashMap<String, u64> =
        serde_json::from_reader(BufReader::new(tops_file)).map_err(|error| error.to_string())?;
    let data = root().join("static/data");
    let sources = Sources::read(
        &data.join("search_titles.jsonl"),
        &data.join("search_aliases.json"),
        &data.join("imdb_mapping.json"),
    )
    .map_err(|error| error.to_string())?;
    let titles: HashMap<u64, &str> =
        sources.records.iter().map(|record| (record.id, record.canonical_title.as_str())).collect();
    let searcher = Searcher::new(Index::build(&sources.records, &sources.mapping, &sources.aliases));

    let mut buckets: IndexMap<&str, Bucket> = IndexMap::new();
    let mut changed = Vec::new();
    for item in &sample {
        let bucket = buckets.entry(item.bucket.as_str()).or_default();
        let top = searcher.search(&item.query).first().map(|hit| u64::from(hit.id));
        let kitsu = kitsu_tops.get(&item.query).copied();
        bucket.n += 1;
        if top.is_some_and(|top| item.accepted.contains(&top)) {
            bucket.accepted += 1;
        }
        if top == Some(item.preferred) {
            bucket.preferred += 1;
        }
        if kitsu.is_some_and(|kitsu| item.accepted.contains(&kitsu)) {
            bucket.kitsu += 1;
        }
        if top.is_none() {
            bucket.none += 1;
        }
        if top != kitsu {
            changed.push((item, top, kitsu));
        }
    }

    println!("{:<64}    n  accepted preferred  no result  kitsu search", "bucket");
    let mut total = Bucket::default();
    for (name, b) in &buckets {
        total.n += b.n;
        total.accepted += b.accepted;
        total.preferred += b.preferred;
        total.kitsu += b.kitsu;
        total.none += b.none;
        println!("{}", row(name, b));
    }
    println!("{}", row("overall", &total));

    if diff {
        let title = |id: Option<u64>| {
            id.map_or_else(|| "-".to_owned(), |id| format!("{id} {}", titles.get(&id).copied().unwrap_or("")))
        };
        println!(
            "\n{} queries with a different top result than the Kitsu search (+ index right, - kitsu right, ~ neither):",
            changed.len()
        );
        for (item, top, kitsu) in changed {
            let right = |id: Option<u64>| id.is_some_and(|id| item.accepted.contains(&id));
            let mark = if right(top) {
                '+'
            } else if right(kitsu) {
                '-'
            } else {
                '~'
            };
            println!("{mark} [{}] {}: {} -> {}", item.bucket, item.query, title(kitsu), title(top));
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::pct;

    #[test]
    fn rounds_by_the_exact_value_below_a_tie() {
        assert_eq!(pct(3, 2000), "   0.1%");
    }

    #[test]
    fn rounds_down_by_the_exact_value_near_a_tie() {
        assert_eq!(pct(249, 2000), "  12.4%");
    }

    #[test]
    fn rounds_an_exact_tie_up() {
        assert_eq!(pct(1, 16), "   6.3%");
        assert_eq!(pct(1, 400), "   0.3%");
    }

    #[test]
    fn pads_to_seven() {
        assert_eq!(pct(1, 1), " 100.0%");
    }
}
