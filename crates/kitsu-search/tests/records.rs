use std::fs::File;
use std::path::PathBuf;

use kitsu_search::records::{Error, TitleRecord, read_records};

const LINE: &str = r#"{"id":2,"canonicalTitle":"B","titles":{"zz":"Z","en":"E"},"abbreviatedTitles":["b"],"subtype":"movie","status":"finished","startDate":null,"episodeCount":null,"synonyms":["s"]}"#;

#[test]
fn reads_one_record_per_line() {
    let data = format!("{LINE}\n\n{LINE}\n");
    assert_eq!(read_records(data.as_bytes()).unwrap().len(), 2);
}

#[test]
fn keeps_null_dates_and_counts_as_none() {
    let record = read_records(LINE.as_bytes()).unwrap().remove(0);
    assert_eq!((record.start_date, record.episode_count), (None, None));
}

#[test]
fn keeps_title_keys_in_file_order() {
    let record = read_records(LINE.as_bytes()).unwrap().remove(0);
    assert_eq!(record.titles.keys().collect::<Vec<_>>(), ["zz", "en"]);
}

#[test]
fn reports_the_line_of_a_bad_record() {
    let data = format!("{LINE}\n{{\"id\":");
    assert!(matches!(read_records(data.as_bytes()), Err(Error::Json { line: 2, .. })));
}

#[test]
fn reads_the_search_titles_data_file() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../static/data/search_titles.jsonl");
    let records = read_records(File::open(path).unwrap()).unwrap();
    assert_eq!(records.len(), 19_606);
    assert_eq!((records[0].id, records[0].canonical_title.as_str()), (1, "Cowboy Bebop"));
}

fn converted(anime: &serde_json::Value) -> TitleRecord {
    TitleRecord::from_kitsu(anime).unwrap()
}

#[test]
fn fragments_and_non_ascii_titles_are_dropped() {
    let anime = serde_json::json!({"id": "5", "canonicalTitle": "A, and More", "titles": {"en": "A, and More", "ja_jp": "日本語", "en_jp": "Little Ghosts, There, Here, and Where"}, "abbreviatedTitles": ["Little Ghosts, There, Here, and Where", "and Where", "12", "x, 123", "Café, é", "AB, ab", "Ok"], "subtype": "TV", "status": "current", "startDate": "2026-01-01", "episodeCount": 12});
    let node: TitleRecord = serde_json::from_value(serde_json::json!({"id": 5, "canonicalTitle": "A, and More", "titles": {"en": "A, and More", "en_jp": "Little Ghosts, There, Here, and Where"}, "abbreviatedTitles": ["Little Ghosts, There, Here, and Where", "12", "x, 123", "Café, é", "AB, ab", "Ok"], "subtype": "TV", "status": "current", "startDate": "2026-01-01", "episodeCount": 12, "synonyms": []})).unwrap();
    assert_eq!(converted(&anime), node);
}

#[test]
fn missing_lists_and_nulls_give_empty_fields() {
    let anime = serde_json::json!({"id": "6", "canonicalTitle": "Foo", "titles": {}, "abbreviatedTitles": null, "subtype": "movie", "status": "upcoming", "startDate": null, "episodeCount": null});
    let node: TitleRecord = serde_json::from_value(serde_json::json!({"id": 6, "canonicalTitle": "Foo", "titles": {}, "abbreviatedTitles": [], "subtype": "movie", "status": "upcoming", "startDate": null, "episodeCount": null, "synonyms": []})).unwrap();
    assert_eq!(converted(&anime), node);
}

#[test]
fn number_only_and_short_pieces_are_fragments() {
    let anime = serde_json::json!({"id": "7", "canonicalTitle": "X, 2023", "titles": {"en": "X, 2023"}, "abbreviatedTitles": ["2023", "1 & 2", "and 2", "7, and 9"], "subtype": "ONA", "status": "finished"});
    let node: TitleRecord = serde_json::from_value(serde_json::json!({"id": 7, "canonicalTitle": "X, 2023", "titles": {"en": "X, 2023"}, "abbreviatedTitles": ["1 & 2", "and 2", "7, and 9"], "subtype": "ONA", "status": "finished", "synonyms": []})).unwrap();
    assert_eq!(converted(&anime), node);
}

#[test]
fn an_entry_without_a_numeric_id_gives_no_record() {
    assert!(TitleRecord::from_kitsu(&serde_json::json!({ "id": "abc", "canonicalTitle": "A" })).is_none());
}
