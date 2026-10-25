use std::sync::LazyLock;

use kitsu_search::index::Index;
use kitsu_search::query::parse_query;
use kitsu_search::records::{Aliases, TitleRecord, read_imdb_mapping};
use kitsu_search::score::{Query, candidates, title_score};
use serde_json::{Value, json};

fn record(id: u64, canonical_title: &str, extra: &Value) -> TitleRecord {
    let mut record = json!({
        "id": id, "canonicalTitle": canonical_title, "titles": {}, "abbreviatedTitles": [], "subtype": "TV",
        "status": "finished", "startDate": "2000-01-01", "episodeCount": 12, "synonyms": [],
    });
    record.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    serde_json::from_value(record).unwrap()
}

const MAPPING: &str =
    r#"[{"kitsu_id": 1, "imdb_id": "tt1", "title": "The Saga of Tanya the Evil", "fromSeason": 1, "fromEpisode": 1}]"#;

static INDEX: LazyLock<Index> = LazyLock::new(|| {
    let records = [
        record(1, "Saga of Tanya", &json!({"synonyms": ["Youjo Senki Chronicle"]})),
        record(2, "Saga of Tanya Zwei", &json!({"titles": {"en": "Tanya the Evil"}})),
        record(3, "Dragon Ball Z Movie 3: The Tree of Might", &json!({"subtype": "movie"})),
        record(4, "Dragon Ball Z", &json!({"episodeCount": 291})),
        record(5, "Re: Hamatora", &json!({"startDate": "2014-07-01"})),
        record(6, "Hamatora The Animation", &json!({"startDate": "2014-01-01"})),
        record(7, "Mobile Suit Gundam Seed", &json!({"synonyms": ["Gundam SEED TV"]})),
    ];
    Index::build(&records, &read_imdb_mapping(MAPPING.as_bytes()).unwrap(), &Aliases::new())
});

fn scored(query: &str) -> Vec<(u64, f64)> {
    let parsed = parse_query(query);
    let query = Query::new(&INDEX, &parsed);
    candidates(&INDEX, &query)
        .into_iter()
        .map(|entry| (INDEX.entries()[entry as usize].id, title_score(&INDEX, entry, &query).score))
        .collect()
}

fn assert_scores(query: &str, expected: &[(u64, f64)]) {
    let got = scored(query);
    let ids: Vec<u64> = got.iter().map(|(id, _)| *id).collect();
    let expected_ids: Vec<u64> = expected.iter().map(|(id, _)| *id).collect();
    assert_eq!(ids, expected_ids, "{query}");
    for ((id, score), (_, want)) in got.iter().zip(expected) {
        assert!((score - want).abs() < 1e-12, "{query} {id}: got {score}, expected {want}");
    }
}

#[test]
fn an_own_title_equal_to_the_query_scores_one() {
    assert_scores("saga of tanya", &[(1, 1.0), (2, 0.892_972_746_503_371_8), (3, 0.297_852_900_121_447)]);
}

#[test]
fn a_synonym_equal_to_the_query_scores_below_own_titles() {
    assert_scores("youjo senki chronicle", &[(1, 0.95)]);
}

#[test]
fn the_mapping_title_equal_to_the_query_scores_between() {
    assert_scores(
        "the saga of tanya the evil",
        &[(1, 0.985), (2, 0.760_159_852_236_398_8), (3, 0.387_823_725_897_695_87), (6, 0.313_060_432_447_375_8)],
    );
}

#[test]
fn a_movie_with_the_queried_number_holding_the_name_is_floored() {
    assert_scores("dragon ball z movie 3", &[(3, 0.97), (4, 1.0)]);
}

#[test]
fn an_unseen_word_fetches_candidates_by_trigrams() {
    assert_scores("rehamatora", &[(5, 1.0), (6, 0.248_619_528_619_528_63)]);
}

#[test]
fn partial_matches_blend_word_overlap_and_spelling() {
    assert_scores("tanya evil", &[(1, 0.682_473_500_815_987_2), (2, 0.941_921_166_830_024_7)]);
}

#[test]
fn an_unseen_word_without_trigram_hits_has_no_candidates() {
    assert_scores("msgs", &[]);
}

#[test]
fn a_synonym_base_equal_to_the_query_is_exact() {
    assert_scores("gundam seed", &[(7, 0.95)]);
}

#[test]
fn a_numbered_query_scores_partial_matches() {
    assert_scores("saga tanya zwei 2", &[(2, 0.759_069_739_635_715_8), (1, 0.595_294_422_972_202_4)]);
}

#[test]
fn spelling_starts_at_the_first_query_word_the_title_holds() {
    assert_scores("hamatora", &[(5, 0.797_868_430_642_011_6), (6, 0.760_347_412_856_462)]);
}
