use std::path::PathBuf;
use std::sync::LazyLock;

use kitsu_search::Searcher;
use kitsu_search::index::Index;
use kitsu_search::records::{Aliases, ImdbMappings, Sources, TitleRecord};
use serde_json::json;

fn record(id: u64, title: &str) -> TitleRecord {
    serde_json::from_value(json!({
        "id": id, "canonicalTitle": title, "titles": {}, "abbreviatedTitles": [], "subtype": "TV",
        "status": "finished", "startDate": "2020-01-01", "episodeCount": 12, "synonyms": [],
    }))
    .unwrap()
}

static NUMBERED: LazyLock<Searcher> = LazyLock::new(|| {
    let records = vec![
        record(1, "2 Koi"),
        record(2, "86"),
        record(3, "22/7"),
        record(4, "Cowboy Bebop"),
        record(5, "Trigun"),
        record(6, "12 Kingdoms Story"),
    ];
    Searcher::new(Index::build(&records, &ImdbMappings::new(), &Aliases::new()))
});

fn top(query: &str) -> Option<u32> {
    NUMBERED.search(query).first().map(|hit| hit.id)
}

#[test]
fn a_name_without_letters_finds_only_an_entry_titled_so() {
    assert_eq!((top("86"), top("22/7")), (Some(2), Some(3)));
}

#[test]
fn a_name_without_letters_matching_partly_finds_nothing() {
    assert_eq!((top("2"), top("12"), top("2 2024")), (None, None, None));
}

#[test]
fn every_search_alias_finds_its_entry_first() {
    let data = |name: &str| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../static/data").join(name);
    let sources =
        Sources::read(&data("search_titles.jsonl"), &data("search_aliases.json"), &data("imdb_mapping.json")).unwrap();
    let searcher = Searcher::new(Index::build(&sources.records, &sources.mapping, &sources.aliases));
    let wrong: Vec<(String, String, Option<u32>)> = sources
        .aliases
        .iter()
        .flat_map(|(id, titles)| titles.iter().map(move |title| (id, title)))
        .filter_map(|(id, title)| {
            let found = searcher.search(title).first().map(|hit| hit.id);
            (found.map(|found| found.to_string()).as_ref() != Some(id)).then(|| (id.clone(), title.clone(), found))
        })
        .collect();
    assert_eq!(wrong, Vec::new());
}
