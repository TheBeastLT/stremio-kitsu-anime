use std::fs::File;
use std::path::PathBuf;

use kitsu_search::records::{Error, read_records};

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
