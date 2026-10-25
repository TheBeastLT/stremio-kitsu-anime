use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use kitsu_search::index::Index;
use kitsu_search::records::{Aliases, ImdbMappings, TitleRecord};
use kitsu_search::{Searcher, SharedIndex, merge_new_records, refresh_index};
use serde_json::json;

fn record(id: u64, title: &str) -> TitleRecord {
    serde_json::from_value(json!({
        "id": id, "canonicalTitle": title, "titles": {}, "abbreviatedTitles": [], "subtype": "TV",
        "status": "current", "startDate": "2026-01-01", "episodeCount": 12, "synonyms": [],
    }))
    .unwrap()
}

fn base() -> Vec<TitleRecord> {
    vec![record(1, "Cowboy Bebop"), record(2, "Trigun"), record(3, "Monster")]
}

fn refresh(records: &mut Vec<TitleRecord>, fresh: Vec<TitleRecord>) -> Option<(usize, Index)> {
    refresh_index(records, fresh, &ImdbMappings::new(), &Aliases::new())
}

fn top(index: Index, query: &str) -> Option<u32> {
    SharedIndex::new(index).search(query).first().map(|hit| hit.id)
}

#[test]
fn appends_unknown_records_in_fetch_order() {
    let mut records = base();
    let added = merge_new_records(&mut records, vec![record(9, "Nine"), record(8, "Eight")]);
    assert_eq!((added, records.iter().map(|r| r.id).collect::<Vec<_>>()), (2, vec![1, 2, 3, 9, 8]));
}

#[test]
fn skips_records_the_index_already_has() {
    let mut records = base();
    assert_eq!(merge_new_records(&mut records, vec![record(2, "Trigun Again")]), 0);
    assert_eq!(records[1].canonical_title, "Trigun");
}

#[test]
fn keeps_the_last_of_a_record_fetched_twice_at_its_first_position() {
    let mut records = base();
    let fresh = vec![record(9, "Nine"), record(8, "Eight"), record(9, "Nine Revised")];
    assert_eq!(merge_new_records(&mut records, fresh), 2);
    assert_eq!(records[3].canonical_title, "Nine Revised");
    assert_eq!(records[4].id, 8);
}

#[test]
fn builds_no_index_when_nothing_is_new() {
    let mut records = base();
    assert!(refresh(&mut records, vec![record(1, "Cowboy Bebop")]).is_none());
}

#[test]
fn rebuilds_the_index_with_the_new_record_searchable() {
    let mut records = base();
    let old = Index::build(&records, &ImdbMappings::new(), &Aliases::new());
    assert_eq!(top(old, "Frieren Beyond Journeys End"), None);
    let (added, index) = refresh(&mut records, vec![record(50, "Frieren Beyond Journeys End")]).unwrap();
    assert_eq!((added, index.entries().len(), records.len()), (1, 4, 4));
    assert_eq!(top(index, "Frieren Beyond Journeys End"), Some(50));
}

#[test]
fn keeps_old_records_searchable_after_a_refresh() {
    let mut records = base();
    let (_, index) = refresh(&mut records, vec![record(50, "Frieren")]).unwrap();
    assert_eq!(top(index, "Trigun"), Some(2));
}

#[test]
fn readers_keep_working_while_the_index_is_swapped() {
    let mut records = base();
    let shared = SharedIndex::new(Index::build(&records, &ImdbMappings::new(), &Aliases::new()));
    let done = AtomicBool::new(false);
    thread::scope(|scope| {
        let readers: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    let mut searches = 0;
                    while !done.load(Ordering::Acquire) {
                        let hits = shared.search("Trigun");
                        assert_eq!(hits.first().map(|hit| hit.id), Some(2));
                        searches += 1;
                    }
                    searches
                })
            })
            .collect();
        for id in 100..110 {
            let (_, index) = refresh(&mut records, vec![record(id, &format!("Fresh Title {id}"))]).unwrap();
            let previous = shared.swap(index);
            assert_eq!(previous.search("Trigun").first().map(|hit| hit.id), Some(2));
        }
        done.store(true, Ordering::Release);
        for reader in readers {
            assert!(reader.join().unwrap() > 0);
        }
    });
    assert_eq!(shared.load().index().entries().len(), 13);
    assert_eq!(shared.search("Fresh Title 109").first().map(|hit| hit.id), Some(109));
}

#[test]
fn builds_the_trigrams_before_publishing_a_swapped_index() {
    let mut records = base();
    let shared = SharedIndex::new(Index::build(&records, &ImdbMappings::new(), &Aliases::new()));
    let (_, index) = refresh(&mut records, vec![record(50, "Frieren")]).unwrap();
    assert!(!index.trigrams_built());
    shared.swap(index);
    assert!(shared.load().index().trigrams_built());
}

#[test]
fn a_limit_of_zero_returns_no_hits() {
    let searcher = Searcher::new(Index::build(&base(), &ImdbMappings::new(), &Aliases::new()));
    assert_eq!(searcher.search_with("Trigun", 0, 0.0).hits.len(), 0);
}

#[test]
fn a_limit_of_one_returns_the_top_hit_only() {
    let searcher = Searcher::new(Index::build(&base(), &ImdbMappings::new(), &Aliases::new()));
    assert_eq!(searcher.search_with("Trigun", 1, 0.0).hits.iter().map(|hit| hit.id).collect::<Vec<_>>(), [2]);
}
