use std::{fs::File, path::Path, sync::LazyLock};

use kitsu_search::{
    index::Index,
    records::{Aliases, read_imdb_mapping, read_records},
};

use super::*;

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/meta").join(name)
}

static DATA: LazyLock<(SharedIndex, Mapping)> = LazyLock::new(|| {
    let records = read_records(File::open(fixture("records.jsonl")).unwrap()).unwrap();
    let imdb = read_imdb_mapping(File::open(fixture("mapping_rows.json")).unwrap()).unwrap();
    let mapping = Mapping::load(&fixture("mapping_rows.json"), &records).unwrap();
    (SharedIndex::new(Index::build(&records, &imdb, &Aliases::new())), mapping)
});

fn search(query: &str) -> Vec<String> {
    search_ids(&DATA.0, &DATA.1, query)
}

#[test]
fn an_imdb_id_names_its_mapped_franchise_cut_at_twenty() {
    let ids = search("tt0388629");
    assert_eq!(ids.len(), 20);
    assert_eq!(ids[0], "12");
}

#[test]
fn an_imdb_id_is_matched_trimmed_and_case_insensitively() {
    assert_eq!(search(" TT0213338 "), ["1"]);
}

#[test]
fn an_unmapped_imdb_id_finds_nothing() {
    assert_eq!(search("tt0000001"), Vec::<String>::new());
}

#[test]
fn a_title_is_searched_in_the_index() {
    assert_eq!(search("cowboy bebop"), ["1", "2"]);
}

#[test]
fn merged_metas_fill_the_missing_positions_in_order() {
    let cached = vec![Some(json!("a")), None, Some(json!("c")), None];
    let fetched = vec![Some(json!("b")), None];
    assert_eq!(merged(cached, &[1, 3], fetched), vec![Some(json!("a")), Some(json!("b")), Some(json!("c")), None]);
}

#[test]
fn browse_catalogs_sort_like_node() {
    assert_eq!(sort_value("kitsu-anime-list"), Some("createdAt"));
    assert_eq!(sort_value("kitsu-anime-rating"), Some("-average_rating"));
    assert_eq!(sort_value("kitsu-anime-popular"), Some("-user_count"));
    assert_eq!(sort_value("kitsu-anime-airing"), Some("-average_rating"));
    assert_eq!(sort_value("kitsu-anime-trending"), None);
}

#[test]
fn only_the_airing_catalog_filters_by_status() {
    assert_eq!(status_value("kitsu-anime-airing"), Some("current"));
    assert_eq!(status_value("kitsu-anime-rating"), None);
}
