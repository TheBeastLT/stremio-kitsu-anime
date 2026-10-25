use std::{
    collections::{HashMap, HashSet},
    fs::File,
};

use kitsu_search::records::read_records;

use super::*;
use crate::application::testing::{REAL_MAPPING, data_path, fixture_mapping};

fn rows() -> Vec<MappingRow> {
    serde_json::from_reader(BufReader::new(File::open(data_path("imdb_mapping.json")).unwrap())).unwrap()
}

fn row(kitsu_id: u64, imdb_id: &str, season: i64, episode: i64) -> MappingRow {
    MappingRow {
        kitsu_id,
        imdb_id: Some(imdb_id.to_owned()),
        title: None,
        from_season: Some(season),
        from_episode: Some(episode),
        non_imdb_episodes: None,
        imdb_episodes: None,
        tmdb_id: None,
        tvdb_id: None,
        fanart_logo_id: None,
    }
}

#[test]
fn imdb_mapping_has_one_row_per_kitsu_id() {
    let mut seen = HashSet::new();
    let duplicates: Vec<u64> = rows().iter().map(|row| row.kitsu_id).filter(|id| !seen.insert(*id)).collect();
    assert_eq!(duplicates, Vec::<u64>::new());
}

#[test]
fn imdb_mapping_rows_have_a_tt_imdb_id_and_both_start_fields_or_neither() {
    let tt = |id: &str| {
        id.strip_prefix("tt").is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
    };
    let invalid: Vec<u64> = rows()
        .iter()
        .filter(|row| {
            row.imdb_id.as_deref().is_some_and(|id| !tt(id)) || row.from_season.is_some() != row.from_episode.is_some()
        })
        .map(|row| row.kitsu_id)
        .collect();
    assert_eq!(invalid, Vec::<u64>::new());
}

#[test]
fn imdb_mapping_has_no_two_rows_starting_at_the_same_imdb_episode() {
    let mut starts: HashMap<String, Vec<u64>> = HashMap::new();
    for row in rows().iter().filter(|row| row.imdb_id.is_some() && row.from_season.is_some()) {
        let start = format!("{} S{}E{}", row.imdb_id.as_deref().unwrap(), row.start_season(), row.start_episode());
        starts.entry(start).or_default().push(row.kitsu_id);
    }
    let shared: Vec<_> = starts.into_iter().filter(|(_, ids)| ids.len() > 1).collect();
    assert_eq!(shared, Vec::new());
}

#[test]
fn imdb_mapping_has_no_finished_entry_running_into_the_next_row_of_its_imdb_season() {
    let records = read_records(File::open(data_path("search_titles.jsonl")).unwrap()).unwrap();
    let counts: HashMap<u64, u32> = records
        .iter()
        .filter(|record| record.status == "finished")
        .filter_map(|record| record.episode_count.filter(|count| *count > 0).map(|count| (record.id, count)))
        .collect();
    let mut seasons: HashMap<String, Vec<MappingRow>> = HashMap::new();
    for row in
        rows().into_iter().filter(|row| row.imdb_id.is_some() && row.from_season.is_some_and(|season| season > 0))
    {
        seasons.entry(format!("{} S{}", row.imdb_id.as_deref().unwrap(), row.start_season())).or_default().push(row);
    }
    let mut overlaps = Vec::new();
    for (season, mut season_rows) in seasons {
        season_rows.sort_by_key(MappingRow::start_episode);
        for pair in season_rows.windows(2) {
            let Some(count) = counts.get(&pair[0].kitsu_id) else { continue };
            let skipped = pair[0].non_imdb_episodes.as_ref().map_or(0, Vec::len) as i64;
            if pair[0].start_episode() + i64::from(*count) - skipped > pair[1].start_episode() {
                overlaps.push(format!("{season}: {}", pair[0].kitsu_id));
            }
        }
    }
    assert_eq!(overlaps, Vec::<String>::new());
}

#[test]
fn imdb_mapping_lists_imdb_episodes_as_season_0_episodes_each_claimed_by_one_row() {
    let rows = rows();
    let invalid: Vec<u64> = rows
        .iter()
        .filter_map(|row| row.imdb_episodes.as_ref().map(|episodes| (row, episodes)))
        .filter(|(row, episodes)| {
            row.from_season != Some(0)
                || row.non_imdb_episodes.is_some()
                || !episodes.iter().all(|episode| episode.is_none_or(|episode| episode > 0))
                || row.from_episode != episodes.iter().flatten().next().copied()
        })
        .map(|(row, _)| row.kitsu_id)
        .collect();
    assert_eq!(invalid, Vec::<u64>::new());
    let mut claims: HashMap<String, Vec<u64>> = HashMap::new();
    for row in rows.iter().filter(|row| row.imdb_id.is_some() && row.from_season == Some(0)) {
        let episodes = row.imdb_episodes.clone().unwrap_or_else(|| vec![row.from_episode]);
        for episode in episodes.into_iter().flatten() {
            claims.entry(format!("{} S0E{episode}", row.imdb_id.as_deref().unwrap())).or_default().push(row.kitsu_id);
        }
    }
    let shared: Vec<_> = claims.into_iter().filter(|(_, ids)| ids.len() > 1).collect();
    assert_eq!(shared, Vec::new());
}

#[test]
fn the_real_mapping_file_loads() {
    assert_ne!(rows().len(), 0);
    assert!(REAL_MAPPING.imdb_mapping("1").is_some());
}

#[test]
fn has_imdb_mapping_looks_up_imdb_ids_by_imdb_and_others_by_kitsu_id() {
    assert!(fixture_mapping().has_imdb_mapping("tt2560140"));
    assert!(!fixture_mapping().has_imdb_mapping("tt0111161"));
    assert!(fixture_mapping().has_imdb_mapping("7442"));
    assert!(!fixture_mapping().has_imdb_mapping("99999999"));
}

#[test]
fn a_row_without_an_imdb_id_still_counts_as_a_kitsu_mapping() {
    assert!(fixture_mapping().has_imdb_mapping("156"));
    assert!(fixture_mapping().imdb_mapping("156").is_some_and(|row| !row.has_imdb_id()));
}

#[test]
fn kitsu_ids_for_imdb_lists_the_seasons_in_order_then_the_specials() {
    assert_eq!(
        fixture_mapping().kitsu_ids_for_imdb("tt2560140"),
        ["7442", "8671", "13569", "41982", "42422", "44240", "46038", "47132", "7838", "7889", "7694", "8674", "13718"]
    );
}

#[test]
fn kitsu_ids_for_an_unmapped_imdb_are_empty() {
    assert_eq!(fixture_mapping().kitsu_ids_for_imdb("tt0111161"), Vec::<String>::new());
}

#[test]
fn episode_cap_is_the_episode_count_of_a_finished_entry() {
    assert_eq!(fixture_mapping().episode_cap(72), Some(50));
}

#[test]
fn an_airing_entry_has_no_episode_cap() {
    assert_eq!(fixture_mapping().episode_cap(12), None);
}

#[test]
fn an_unmapped_entry_has_no_episode_cap() {
    let record = r#"{"id":5,"canonicalTitle":"x","titles":{},"abbreviatedTitles":[],"subtype":"TV","status":"finished","startDate":null,"episodeCount":12,"synonyms":[]}"#;
    let records = read_records(record.as_bytes()).unwrap();
    let mapping = Mapping::new(vec![row(6, "tt1", 1, 1)], &records);
    assert_eq!(mapping.episode_cap(5), None);
}

#[test]
fn rows_of_an_imdb_sort_by_start_then_kitsu_id() {
    let rows = vec![row(9, "tt1", 2, 1), row(5, "tt1", 1, 5), row(3, "tt1", 1, 5), row(1, "tt1", 0, 1)];
    let mapping = Mapping::new(rows, &[]);
    let order: Vec<u64> = mapping.entries("tt1").iter().map(|row| row.kitsu_id).collect();
    assert_eq!(order, [1, 3, 5, 9]);
}

#[test]
fn a_second_row_of_a_kitsu_id_replaces_the_first() {
    let mapping = Mapping::new(vec![row(1, "tt1", 1, 1), row(1, "tt2", 1, 1)], &[]);
    assert_eq!(mapping.imdb_mapping("1").and_then(|row| row.imdb_id.as_deref()), Some("tt2"));
    assert_eq!(mapping.entries("tt1"), &[]);
}

#[test]
fn kitsu_id_lookup_coerces_like_a_js_object_key() {
    assert!(REAL_MAPPING.imdb_mapping_of(&JsValue::Number(7442.0)).is_some());
    assert!(REAL_MAPPING.imdb_mapping_of(&JsValue::from("7442")).is_some());
    assert!(REAL_MAPPING.imdb_mapping("07442").is_none());
    assert!(REAL_MAPPING.imdb_mapping_of(&JsValue::Undefined).is_none());
    assert!(REAL_MAPPING.imdb_mapping_of(&JsValue::Array(vec![JsValue::from("7442")])).is_some());
}

#[test]
fn listed_episodes_name_the_row_and_kitsu_episode_listing_them() {
    assert_eq!(
        REAL_MAPPING.listed("tt0388629", "0", "49"),
        Some(&Listed { kitsu_id: "45690".to_owned(), kitsu_episode: 3 })
    );
    assert_eq!(REAL_MAPPING.listed("tt0388629", "0", "42"), None);
}

#[test]
fn a_null_imdb_episode_lists_nothing() {
    assert_eq!(REAL_MAPPING.listed("tt0434694", "0", "7").map(|listed| listed.kitsu_episode), Some(1));
    let row = REAL_MAPPING.imdb_mapping("1092").unwrap();
    assert_eq!(row.imdb_episode(&JsValue::Number(2.0)), Some(None));
    assert_eq!(row.imdb_episode(&JsValue::Number(4.0)), Some(None));
}

#[test]
fn mapping_info_carries_the_fanart_fields() {
    let row =
        MappingRow { tmdb_id: Some("1".to_owned()), fanart_logo_id: Some("99".to_owned()), ..row(1, "tt1", 2, 1) };
    let mapping = Mapping::new(vec![row], &[]);
    assert_eq!(
        mapping.mapping_info("1"),
        Some(MappingInfo {
            imdb_id: Some("tt1".to_owned()),
            tmdb_id: Some("1".to_owned()),
            tvdb_id: None,
            fanart_logo_id: Some("99".to_owned()),
            from_season: Some(2),
        })
    );
}

#[test]
fn rows_parse_the_camel_case_fields_and_ignore_others() {
    let json = r#"[{"kitsu_id":1,"imdb_id":"tt1","fromSeason":0,"fromEpisode":4,"imdbEpisodes":[4,null],"fanartLogoId":"7","tvdbId":"x"}]"#;
    let mapping = Mapping::read(json.as_bytes(), &[]).unwrap();
    let row = mapping.imdb_mapping("1").unwrap();
    assert_eq!((row.from_season, row.from_episode), (Some(0), Some(4)));
    assert_eq!(row.imdb_episodes, Some(vec![Some(4), None]));
    assert_eq!(row.fanart_logo_id.as_deref(), Some("7"));
    assert_eq!(row.tvdb_id, None);
}
