use kitsu_search::index::{AbsoluteEnd, Entry, Index};
use kitsu_search::query::{ParsedQuery, parse_query};
use kitsu_search::rank::{
    Coverage, confidence_of, has_ordinal_episodes, one_edit_apart, truthy_episode, truthy_part, truthy_season,
    truthy_year, within_ordinal_span,
};
use kitsu_search::records::{Aliases, ImdbMappings, TitleRecord};
use kitsu_search::rules::{
    EpisodeSpan, age_tie_break, bonus, earliest_of, episode_in_range, extra_number_is, gt, same_year, year_near,
};
use kitsu_search::search::trim_query;
use serde_json::{Value, json};

const NAMES: [&str; 4] = ["Alpha", "Beta", "Gamma", "Delta"];

fn entries(rows: &[Value]) -> Vec<Entry> {
    let records: Vec<TitleRecord> = rows
        .iter()
        .enumerate()
        .map(|(i, extra)| {
            let mut record = json!({
                "id": i + 1, "canonicalTitle": NAMES[i], "titles": {}, "abbreviatedTitles": [],
                "subtype": "TV", "status": "finished", "startDate": null, "episodeCount": null, "synonyms": [],
            });
            record.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            serde_json::from_value(record).unwrap()
        })
        .collect();
    Index::build(&records, &ImdbMappings::new(), &Aliases::new()).entries().to_vec()
}

#[test]
fn gt_treats_a_missing_value_as_js_undefined() {
    let table = [(None, 1, false), (Some(1), 1, false), (Some(2), 1, true), (Some(0), 0, false), (None, 0, false)];
    for (value, than, expected) in table {
        assert_eq!(gt(value, than), expected, "{value:?} > {than}");
    }
}

#[test]
fn same_year_is_false_for_a_null_start_against_no_queried_year() {
    let table = [
        (None, None, false),
        (Some(2000), None, false),
        (None, Some(2000), false),
        (Some(2000), Some(2000), true),
        (Some(2000), Some(2001), false),
    ];
    for (entry, parsed, expected) in table {
        assert_eq!(same_year(entry, parsed), expected, "{entry:?} === {parsed:?}");
    }
}

#[test]
fn within_ordinal_span_counts_a_null_episode_count_as_zero() {
    let table = [(None, true), (Some(0), true), (Some(100), true), (Some(101), false)];
    for (count, expected) in table {
        assert_eq!(within_ordinal_span(count), expected, "{count:?} <= 100");
    }
}

#[test]
fn has_ordinal_episodes_is_false_for_a_null_episode_count() {
    let table = [(None, false), (Some(9), false), (Some(10), true)];
    for (count, expected) in table {
        assert_eq!(has_ordinal_episodes(count), expected, "{count:?} >= 10");
    }
}

#[test]
fn year_near_puts_a_null_start_year_far_from_any_year() {
    let table = [(None, 2000, false), (Some(1999), 2000, true), (Some(2001), 2000, true), (Some(2002), 2000, false)];
    for (entry, year, expected) in table {
        assert_eq!(year_near(entry, year), expected, "|{entry:?} - {year}| <= 1");
    }
}

#[test]
fn extra_number_is_compares_the_digits_as_a_number() {
    let table = [(None, 0, false), (Some("07"), 7, true), (Some("0"), 0, true), (Some("2"), 3, false)];
    for (digits, number, expected) in table {
        assert_eq!(extra_number_is(digits, number), expected, "+{digits:?} === {number}");
    }
}

#[test]
fn episode_span_is_unbounded_for_a_falsy_count_or_an_open_end() {
    let table = [
        (EpisodeSpan::of_count(None), EpisodeSpan::Unbounded),
        (EpisodeSpan::of_count(Some(0)), EpisodeSpan::Unbounded),
        (EpisodeSpan::of_count(Some(12)), EpisodeSpan::Count(12)),
        (EpisodeSpan::of_absolute(1, AbsoluteEnd::Open), EpisodeSpan::Unbounded),
        (EpisodeSpan::of_absolute(13, AbsoluteEnd::Episode(24)), EpisodeSpan::Count(12)),
    ];
    for (got, expected) in table {
        assert_eq!(got, expected);
    }
}

#[test]
fn episode_in_range_needs_a_start_past_the_first_episode() {
    let table = [
        (5, None, EpisodeSpan::Unbounded, false),
        (5, Some(1), EpisodeSpan::Unbounded, false),
        (11, Some(12), EpisodeSpan::Unbounded, false),
        (500, Some(12), EpisodeSpan::Unbounded, true),
        (13, Some(12), EpisodeSpan::Count(11), true),
        (22, Some(12), EpisodeSpan::Count(11), true),
        (23, Some(12), EpisodeSpan::Count(11), false),
    ];
    for (episode, from, span, expected) in table {
        assert_eq!(episode_in_range(episode, from, span), expected, "{episode} in {from:?} {span:?}");
    }
}

#[test]
fn zero_season_episode_part_and_year_are_falsy() {
    let base = parse_query("show");
    let table: [(ParsedQuery, bool); 5] = [
        (base.clone(), false),
        (ParsedQuery { season: Some(0), episode: Some(0), part: Some(0), year: Some(0), ..base.clone() }, false),
        (ParsedQuery { season: Some(2), episode: Some(2), part: Some(2), year: Some(2000), ..base.clone() }, true),
        (ParsedQuery { season: Some(1), episode: Some(1), part: Some(1), year: Some(1990), ..base.clone() }, true),
        (ParsedQuery { season: Some(99), episode: Some(1000), part: Some(3), year: Some(2030), ..base }, true),
    ];
    for (parsed, expected) in table {
        let got = [
            truthy_season(&parsed).is_some(),
            truthy_episode(&parsed).is_some(),
            truthy_part(&parsed).is_some(),
            truthy_year(&parsed).is_some(),
        ];
        assert_eq!(got, [expected; 4], "{parsed:?}");
    }
}

#[test]
fn earliest_of_defaults_season_one_and_a_late_date_and_keeps_the_first_of_equals() {
    let list = entries(&[
        json!({"startDate": null}),
        json!({"startDate": "2005-01-01"}),
        json!({"canonicalTitle": "Gamma Season 2", "startDate": "1990-01-01"}),
        json!({"startDate": "2005-01-01"}),
    ]);
    let table: [(&[usize], Option<usize>); 5] =
        [(&[], None), (&[0, 1], Some(1)), (&[2, 0], Some(0)), (&[1, 3], Some(1)), (&[3, 1], Some(3))];
    for (members, expected) in table {
        assert_eq!(earliest_of(members.iter().map(|&i| (i, &list[i]))), expected, "{members:?}");
    }
}

#[test]
#[allow(clippy::float_cmp)]
fn age_tie_break_is_zero_without_a_start_year() {
    let list =
        entries(&[json!({"startDate": null}), json!({"startDate": "2000-05-01"}), json!({"startDate": "1998-01-01"})]);
    let table = [(0, bonus::PER_YEAR, 0.0), (1, bonus::PER_YEAR, -0.01), (2, bonus::PER_YEAR_EXTRA, -0.0144)];
    for (i, per_year, expected) in table {
        assert_eq!(age_tie_break(&list[i], per_year), expected, "{:?}", list[i].start_date);
    }
}

#[test]
fn one_edit_apart_reads_past_the_end_as_js_undefined() {
    let table = [
        ("koufu", "kyoufu", true),
        ("abc", "abc", false),
        ("", "", false),
        ("a", "", true),
        ("ab", "abc", true),
        ("ab", "ba", true),
        ("abc", "abd", true),
        ("abc", "abde", false),
        ("abcd", "ab", false),
        ("abcd", "acbe", false),
        ("😀", "😁", true),
    ];
    for (a, b, expected) in table {
        assert_eq!(one_edit_apart(a, b), expected, "{a:?} {b:?}");
    }
}

#[test]
#[allow(clippy::float_cmp)]
fn confidence_takes_the_better_of_partial_and_glued_coverage() {
    let table = [
        (1.0, 1.0, 1.0, 1.0),
        (0.5, 0.0, 0.9, 0.733_333_333_333_333_4),
        (0.3, 0.0, 1.0, 0.15),
        (0.8, 0.5, 0.5, 0.725_000_000_000_000_1),
        (0.1, 0.0, 0.0, 0.0),
    ];
    for (score, single, all, expected) in table {
        assert_eq!(confidence_of(score, Coverage { single, all }), expected, "{score} {single} {all}");
    }
}

#[test]
fn trim_query_cuts_at_250_utf16_units_and_drops_the_last_partial_word() {
    let long = format!("{}tail", "word ".repeat(52));
    let pair_cut = format!("{}😀x", "ab ".repeat(83));
    let lone_word = format!("{}😀 rest", "a".repeat(249));
    let table = [
        ("short query".to_owned(), "short query".to_owned()),
        (long, "word ".repeat(50).trim_end().to_owned()),
        (pair_cut, "ab ".repeat(83).trim_end().to_owned()),
        (lone_word, format!("{}\u{FFFD}", "a".repeat(249))),
    ];
    for (query, expected) in table {
        assert_eq!(trim_query(&query), expected, "{query:?}");
    }
}

#[test]
fn trim_query_keeps_the_episode_of_a_cut_inside_a_surrogate_pair() {
    let query = format!("{}(01-03😀", "a".repeat(243));
    assert_eq!(parse_query(&trim_query(&query)).episode, Some(3));
}
