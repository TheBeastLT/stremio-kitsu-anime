use std::fmt::Write;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::sync::LazyLock;

use kitsu_search::index::{AbsoluteEnd, Entry, Index, Trigram, Variant, VariantSource};
use kitsu_search::records::Sources;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

mod common;

use common::golden;

fn data(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../static/data").join(name)
}

static INDEX: LazyLock<Index> = LazyLock::new(|| {
    let sources =
        Sources::read(&data("search_titles.jsonl"), &data("search_aliases.json"), &data("imdb_mapping.json")).unwrap();
    Index::build(&sources.records, &sources.mapping, &sources.aliases)
});

#[derive(Deserialize)]
struct Posting {
    word: String,
    entries: Vec<u32>,
    ids: Vec<u64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IndexGolden {
    entries: usize,
    variants: usize,
    words: usize,
    postings_sha256: String,
    first: Vec<Posting>,
    last: Vec<Posting>,
}

#[derive(Deserialize)]
struct GramPosting {
    gram: String,
    entries: Vec<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TrigramsGolden {
    grams: usize,
    postings_sha256: String,
    first: Vec<GramPosting>,
    last: Vec<GramPosting>,
}

fn read_json<T: serde::de::DeserializeOwned>(name: &str) -> Option<T> {
    golden(name).map(|path| serde_json::from_reader(BufReader::new(File::open(path).unwrap())).unwrap())
}

fn joined(list: &[u32]) -> String {
    list.iter().map(u32::to_string).collect::<Vec<_>>().join(",")
}

fn sorted_words(index: &Index) -> Vec<u32> {
    let mut ids: Vec<u32> = (0..index.word_count() as u32).collect();
    ids.sort_by(|&a, &b| index.word(a).encode_utf16().cmp(index.word(b).encode_utf16()));
    ids
}

fn code_units(gram: Trigram) -> String {
    gram.iter().fold(String::new(), |mut out, unit| {
        write!(out, "{unit:04x}").unwrap();
        out
    })
}

fn sorted_grams(index: &Index) -> Vec<Trigram> {
    let mut grams: Vec<Trigram> = index.trigrams().keys().copied().collect();
    grams.sort_unstable();
    grams
}

#[test]
fn counts_match_the_golden() {
    let Some(golden) = read_json::<IndexGolden>("index.json") else { return };
    assert_eq!(
        (INDEX.entries().len(), INDEX.variants(), INDEX.word_count()),
        (golden.entries, golden.variants, golden.words)
    );
}

#[test]
fn every_posting_list_matches_the_golden_hash() {
    let Some(golden) = read_json::<IndexGolden>("index.json") else { return };
    let mut hash = Sha256::new();
    for id in sorted_words(&INDEX) {
        hash.update(format!("{}\t{}\n", INDEX.word(id), joined(INDEX.posting(id))));
    }
    assert_eq!(format!("{:x}", hash.finalize()), golden.postings_sha256);
}

#[test]
fn first_and_last_posting_lists_match_the_golden() {
    let Some(golden) = read_json::<IndexGolden>("index.json") else { return };
    let words = sorted_words(&INDEX);
    let sampled = words[..5].iter().chain(&words[words.len() - 5..]);
    let expected = golden.first.iter().chain(&golden.last);
    for (&id, posting) in sampled.zip(expected) {
        let ids: Vec<u64> = INDEX.posting(id).iter().map(|&i| INDEX.entries()[i as usize].id).collect();
        assert_eq!(
            (INDEX.word(id), INDEX.posting(id), ids),
            (posting.word.as_str(), &posting.entries[..], posting.ids.clone())
        );
    }
}

#[test]
fn the_first_word_is_the_empty_word_of_entry_134() {
    let words = sorted_words(&INDEX);
    assert_eq!(INDEX.word(words[0]), "");
    let entries: Vec<u64> = INDEX.entries_with("").iter().map(|&i| INDEX.entries()[i as usize].id).collect();
    assert_eq!(entries, [134]);
}

#[test]
fn trigrams_match_the_golden() {
    let Some(golden) = read_json::<TrigramsGolden>("trigrams.json") else { return };
    let grams = sorted_grams(&INDEX);
    let mut hash = Sha256::new();
    for &gram in &grams {
        hash.update(format!("{}\t{}\n", code_units(gram), joined(INDEX.entries_with_trigram(gram))));
    }
    assert_eq!((grams.len(), format!("{:x}", hash.finalize())), (golden.grams, golden.postings_sha256));
    let sampled = grams[..5].iter().chain(&grams[grams.len() - 5..]);
    for (&gram, posting) in sampled.zip(golden.first.iter().chain(&golden.last)) {
        assert_eq!((code_units(gram), INDEX.entries_with_trigram(gram)), (posting.gram.clone(), &posting.entries[..]));
    }
}

fn insert<T: Into<Value>>(object: &mut Map<String, Value>, key: &str, value: Option<T>) {
    if let Some(value) = value {
        object.insert(key.to_owned(), value.into());
    }
}

fn variant_json(variant: &Variant) -> Value {
    let mut object = Map::new();
    object.insert("title".into(), json!(variant.title));
    object.insert("base".into(), json!(variant.base));
    insert(&mut object, "movieBase", variant.movie_base.clone());
    insert(&mut object, "titleYear", variant.title_year);
    insert(&mut object, "movie", variant.movie);
    insert(&mut object, "season", variant.season);
    object.insert("markers".into(), json!(variant.markers));
    insert(&mut object, "extraNumber", variant.extra_number.clone());
    match variant.source {
        VariantSource::Own { abbreviated } => {
            object.insert("abbreviated".into(), json!(abbreviated));
        }
        VariantSource::Synonym { stripped_season } => {
            object.insert("fromSynonym".into(), json!(true));
            insert(&mut object, "strippedSeason", stripped_season);
        }
        VariantSource::Mapping => {
            object.insert("fromMapping".into(), json!(true));
        }
    }
    Value::Object(object)
}

fn entry_json(entry: &Entry) -> Value {
    let mut object = Map::new();
    object.insert("id".into(), json!(entry.id));
    object.insert("canonicalTitle".into(), json!(entry.canonical_title));
    object.insert("mainTitles".into(), json!(entry.main_titles));
    object.insert("subtype".into(), json!(entry.subtype.as_str()));
    object.insert("startDate".into(), json!(entry.start_date));
    object.insert("year".into(), json!(entry.year));
    object.insert("episodeCount".into(), json!(entry.episode_count));
    object.insert("titles".into(), Value::Array(entry.titles.iter().map(variant_json).collect()));
    object.insert("ownTitles".into(), json!(entry.own_titles().collect::<Vec<_>>()));
    object.insert("sequelKeys".into(), json!(entry.sequel_keys));
    object.insert("hasExtraWord".into(), json!(entry.has_extra_word));
    insert(&mut object, "titleSeason", entry.title_season);
    insert(&mut object, "partNumber", entry.part_number);
    insert(&mut object, "season", entry.season);
    object.insert("titleYears".into(), json!(entry.title_years));
    insert(&mut object, "movie", entry.movie);
    object.insert("markers".into(), json!(entry.markers));
    insert(&mut object, "franchise", entry.franchise.clone());
    insert(&mut object, "imdbSeason", entry.imdb_season);
    insert(&mut object, "imdbFromEpisode", entry.imdb_from_episode);
    insert(&mut object, "derivedMovie", entry.derived_movie);
    insert(&mut object, "groupKey", entry.group_key.clone());
    if let Some(absolute) = entry.absolute {
        object.insert("absFrom".into(), json!(absolute.from));
        let to = match absolute.to {
            AbsoluteEnd::Episode(episode) => json!(episode),
            AbsoluteEnd::Open => json!("Infinity"),
        };
        object.insert("absTo".into(), to);
    }
    insert(&mut object, "splitSeason", entry.split_season);
    Value::Object(object)
}

#[test]
fn every_entry_matches_the_golden() {
    let Some(path) = golden("entries.jsonl") else { return };
    let mut total = 0;
    let mut differences = Vec::new();
    for (line, entry) in BufReader::new(File::open(path).unwrap()).lines().zip(INDEX.entries()) {
        let expected: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let got = entry_json(entry);
        total += 1;
        if got != expected {
            differences.push(format!("{got}\nexpected {expected}"));
        }
    }
    assert_eq!(total, INDEX.entries().len());
    assert!(
        differences.is_empty(),
        "{} differences, first: {:#?}",
        differences.len(),
        &differences[..differences.len().min(5)]
    );
}

#[test]
fn season_groups_list_their_entries_in_index_order() {
    for (key, list) in INDEX.season_groups() {
        assert!(list.windows(2).all(|pair| pair[0] < pair[1]), "{key}");
        assert!(list.iter().all(|&i| INDEX.entries()[i as usize].group_key.as_deref() == Some(key.as_str())));
    }
    let grouped: usize = INDEX.season_groups().values().map(Vec::len).sum();
    assert_eq!(grouped, INDEX.entries().iter().filter(|entry| entry.group_key.is_some()).count());
}

#[test]
fn every_start_date_is_an_iso_date() {
    let iso = |date: &str| {
        date.len() == 10
            && date
                .bytes()
                .enumerate()
                .all(|(i, byte)| if i == 4 || i == 7 { byte == b'-' } else { byte.is_ascii_digit() })
    };
    let odd: Vec<&str> =
        INDEX.entries().iter().filter_map(|entry| entry.start_date.as_deref()).filter(|date| !iso(date)).collect();
    assert!(odd.is_empty(), "localeCompare equals byte order only for ISO dates: {odd:?}");
}
