use std::sync::LazyLock;

use kitsu_search::index::{AbsoluteEnd, AbsoluteEpisodes, Entry, Index, VariantSource};
use kitsu_search::records::{TitleRecord, read_aliases, read_imdb_mapping};
use serde_json::{Value, json};

fn record(id: u64, canonical_title: &str, extra: &Value) -> TitleRecord {
    let mut record = json!({
        "id": id, "canonicalTitle": canonical_title, "titles": {}, "abbreviatedTitles": [], "subtype": "TV",
        "status": "finished", "startDate": "2000-01-01", "episodeCount": 12, "synonyms": [],
    });
    record.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    serde_json::from_value(record).unwrap()
}

const MAPPING: &str = r#"[
    {"kitsu_id": 2, "imdb_id": "tt1", "title": "Saga", "fromSeason": 1, "fromEpisode": 1},
    {"kitsu_id": 3, "imdb_id": "tt1", "title": "Saga", "fromSeason": 2, "fromEpisode": 1},
    {"kitsu_id": 4, "imdb_id": "tt1", "title": "Saga", "fromSeason": 3, "fromEpisode": 1},
    {"kitsu_id": 9, "imdb_id": "tt2", "title": "X", "fromSeason": 2},
    {"kitsu_id": 10, "imdb_id": "tt2", "title": "Kaiju", "fromSeason": 0, "fromEpisode": 1}
]"#;

static INDEX: LazyLock<Index> = LazyLock::new(|| {
    let records = [
        record(
            1,
            "Ghost in the Shell: S.A.C.",
            &json!({"titles": {"en": "GitS SAC"}, "abbreviatedTitles": ["G.I.T.S."]}),
        ),
        record(2, "Saga", &json!({"startDate": "2001-01-01", "episodeCount": 26})),
        record(3, "Saga Next", &json!({"startDate": "2003-01-01", "episodeCount": null})),
        record(4, "Saga Last", &json!({"startDate": "2005-01-01"})),
        record(5, "Pocket Monsters: Mewtwo", &json!({"subtype": "movie", "startDate": "1998-07-18"})),
        record(6, "Pocket Monsters: Lugia", &json!({"subtype": "movie", "startDate": "1998-07-18"})),
        record(7, "Pocket Monsters: Celebi", &json!({"subtype": "movie", "startDate": "1997-07-18"})),
        record(8, "Hero Chapter 3", &json!({"subtype": "movie", "startDate": "2010-01-01"})),
        record(9, "Kaiju 2", &json!({"synonyms": ["Kaiju Episode 5", "kaiju 2", "Kaiju Two", "Kaiju Season 3"]})),
        record(10, "Kaiju", &json!({"titles": {"en_jp": "Kaijuu"}})),
        record(11, "Kaiju Recap", &json!({"subtype": "music"})),
        record(12, "Ending", &json!({"titles": {"en": "Ending"}, "status": "tba"})),
    ];
    let mapping = read_imdb_mapping(MAPPING.as_bytes()).unwrap();
    let aliases = read_aliases(r#"{"1": ["Koukaku S2", "Koukaku S2"]}"#.as_bytes()).unwrap();
    Index::build(&records, &mapping, &aliases)
});

fn entry(id: u64) -> &'static Entry {
    INDEX.entries().iter().find(|entry| entry.id == id).unwrap()
}

fn titles(id: u64) -> Vec<(&'static str, VariantSource)> {
    entry(id).titles.iter().map(|variant| (variant.title.as_str(), variant.source)).collect()
}

const OWN: VariantSource = VariantSource::Own { abbreviated: false };
const ABBREVIATED: VariantSource = VariantSource::Own { abbreviated: true };

#[test]
fn leaves_out_unsearched_subtypes_and_statuses() {
    let ids: Vec<u64> = INDEX.entries().iter().map(|entry| entry.id).collect();
    assert_eq!(ids, [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
}

#[test]
fn appends_aliases_to_the_main_titles() {
    assert_eq!(entry(1).main_titles, ["Ghost in the Shell: S.A.C.", "GitS SAC", "Koukaku S2", "Koukaku S2"]);
}

#[test]
fn adds_dotted_abbreviations_undotted() {
    assert_eq!(
        titles(1),
        [
            ("ghost in the shell s a c", OWN),
            ("ghost in the shell sac", OWN),
            ("gits sac", OWN),
            ("kokaku s 2", OWN),
            ("g i t s", ABBREVIATED),
            ("gits", ABBREVIATED),
        ]
    );
}

#[test]
fn takes_the_title_season_from_an_alias() {
    assert_eq!((entry(1).title_season, entry(1).season), (Some(2), Some(2)));
}

#[test]
fn keeps_repeated_mapping_titles() {
    assert_eq!(titles(3), [("saga next", OWN), ("saga", VariantSource::Mapping), ("saga", VariantSource::Mapping)]);
}

#[test]
fn takes_the_season_from_a_mapping_starting_at_episode_one() {
    assert_eq!((entry(4).title_season, entry(4).season), (None, Some(3)));
}

#[test]
fn ignores_a_season_zero_mapping() {
    assert_eq!((entry(10).season, entry(10).imdb_season), (None, Some(0)));
}

#[test]
fn lays_out_absolute_episodes_by_imdb_season() {
    assert_eq!(entry(2).absolute, Some(AbsoluteEpisodes { from: 1, to: AbsoluteEnd::Episode(26) }));
}

#[test]
fn leaves_an_entry_without_episode_count_open() {
    assert_eq!(entry(3).absolute, Some(AbsoluteEpisodes { from: 27, to: AbsoluteEnd::Open }));
}

#[test]
fn stops_the_layout_after_an_open_entry() {
    assert_eq!(entry(4).absolute, None);
}

#[test]
fn sorts_season_zero_first_in_the_layout() {
    assert_eq!(
        (entry(10).absolute, entry(9).absolute),
        (
            Some(AbsoluteEpisodes { from: 1, to: AbsoluteEnd::Episode(12) }),
            Some(AbsoluteEpisodes { from: 13, to: AbsoluteEnd::Episode(24) })
        )
    );
}

#[test]
fn splits_the_season_after_a_two_cour_season() {
    assert_eq!((entry(3).split_season, entry(4).split_season), (Some(3), None));
}

#[test]
fn numbers_unnumbered_movies_by_date_keeping_ties_in_order() {
    let numbers: Vec<Option<u32>> = [7, 5, 6].into_iter().map(|id| entry(id).derived_movie).collect();
    assert_eq!(numbers, [Some(1), Some(2), Some(3)]);
}

#[test]
fn numbers_a_movie_chapter() {
    let variant = &entry(8).titles[0];
    assert_eq!((variant.movie, variant.base.as_str(), entry(8).movie), (Some(3), "hero", Some(3)));
}

#[test]
fn drops_numbered_and_own_synonyms_and_keeps_their_stripped_season() {
    assert_eq!(
        titles(9),
        [
            ("kaiju 2", OWN),
            ("kaiju two", VariantSource::Synonym { stripped_season: None }),
            ("kaiju season 3", VariantSource::Synonym { stripped_season: Some(3) }),
            ("", VariantSource::Mapping),
        ]
    );
}

#[test]
fn indexes_the_empty_word_of_an_empty_mapping_title() {
    assert_eq!(INDEX.entries_with(""), [8]);
}

#[test]
fn groups_seasons_by_seasonless_canonical_title() {
    assert_eq!((INDEX.season_group("kaiju"), entry(9).group_key.as_deref()), (&[8, 9][..], Some("kaiju")));
}

#[test]
fn interns_words_in_first_seen_order() {
    let words: Vec<(&str, &[u32])> = (0..12).map(|id| (INDEX.word(id), INDEX.posting(id))).collect();
    assert_eq!(
        words,
        [
            ("ghost", &[0][..]),
            ("in", &[0]),
            ("the", &[0]),
            ("shell", &[0]),
            ("s", &[0]),
            ("a", &[0]),
            ("c", &[0]),
            ("sac", &[0]),
            ("gits", &[0]),
            ("kokaku", &[0]),
            ("2", &[0, 8]),
            ("g", &[0]),
        ]
    );
}

#[test]
fn builds_trigrams_over_utf16_units() {
    let grams: Vec<[u16; 3]> = kitsu_search::index::trigrams_of("a😀b").into_iter().collect();
    assert_eq!(grams, [[0x61, 0xd83d, 0xde00], [0xd83d, 0xde00, 0x62]]);
}
