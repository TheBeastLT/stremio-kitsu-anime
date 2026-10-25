use std::sync::LazyLock;

use fastjs_regex::{Regex, utf16_len};

use crate::index::{AbsoluteEnd, Entry, Index, Subtype};
use crate::norm::norm;
use crate::query::{
    FINAL_SEASON, Marker, ParsedQuery, Season, compact, ending_extra_word, extra_family, punct_key, roman_numeral,
    season_of,
};
use crate::rank::{Candidate, is_extra_entry, truthy_episode, truthy_part, truthy_season, truthy_year};

pub mod band {
    pub const TIE: f64 = 0.06;
    pub const REACH: f64 = 0.1;
    pub const YEAR: f64 = 0.15;
    pub const EARLIER_PART: f64 = 0.2;
    pub const ORDINAL_FAMILY: usize = 3;
    pub const ORDINAL_SPAN: u32 = 100;
    pub const ORDINAL_EPISODES: u32 = 10;
    pub const ORDINAL: f64 = 0.2;
    pub const DISTINCTIVE: f64 = 0.2;
    pub const SIBLING: f64 = 0.01;
    pub const REQUESTED_KIND: f64 = 0.04;
    pub const REQUESTED_KIND_SPREAD: f64 = 0.03;
}

pub mod bonus {
    pub const SEASON_MATCH: f64 = 0.08;
    pub const SEASON_MISMATCH: f64 = -0.06;
    pub const SPLIT_SEASON: f64 = 0.03;
    pub const SEASON_WORD: f64 = 0.03;
    pub const LATER_SEASON_EXTRA: f64 = -0.03;
    pub const PUNCTUATED_SEQUEL: f64 = 0.08;
    pub const WHOLE_QUERY_TITLE: f64 = 0.03;
    pub const DISTINCTIVE_WORDS: f64 = 0.2;
    pub const MISSING_WORDS: f64 = -0.3;
    pub const EPISODE_IN_RANGE: f64 = 0.12;
    pub const EPISODE_IN_TITLE: f64 = 0.1;
    pub const EPISODE_OVERFLOW: f64 = -0.05;
    pub const MARKER_MATCH: f64 = 0.2;
    pub const MARKER_UNASKED: f64 = -0.04;
    pub const MOVIE_LEAN: f64 = 0.07;
    pub const SUBTYPE_STEP: f64 = -0.02;
    pub const EXTRA_WORD_UNASKED: f64 = -0.04;
    pub const EXTRA: f64 = 0.06;
    pub const EXTRA_WORD_FAMILY: f64 = 0.02;
    pub const EXTRA_WORD_FORM: f64 = 0.002;
    pub const EXTRA_FRANCHISE_NAME: f64 = 0.02;
    pub const EXTRA_NUMBER: f64 = 0.04;
    pub const MOVIE_FOR_EXTRA: f64 = -0.04;
    pub const MOVIE_SUBTYPE: f64 = 0.04;
    pub const MOVIE_NUMBER: f64 = 0.15;
    pub const YEAR: f64 = 0.12;
    pub const YEAR_CONFIRMED: f64 = 0.15;
    pub const YEAR_NEAR: f64 = 0.03;
    pub const YEAR_WITH_SEASON: f64 = 0.03;
    pub const YEAR_NEAR_WITH_SEASON: f64 = 0.01;
    pub const FIRST_SEASON: f64 = 0.02;
    pub const EARLIER_SIBLING: f64 = -0.03;
    pub const PER_YEAR: f64 = -0.0002;
    pub const PER_YEAR_EXTRA: f64 = -0.0003;
}

const TIE_BREAK_YEAR: f64 = 1950.0;
const MARKER_ORDER: [Marker; 2] = [Marker::Recap, Marker::Reedit];

fn rx(source: &str) -> Regex {
    Regex::new(source, "").unwrap_or_else(|error| panic!("invalid pattern {source}: {error}"))
}

static LATER_SEASON_TITLE: LazyLock<Regex> =
    LazyLock::new(|| rx(r"\b(?:\d{1,2}(?:st|nd|rd|th) season|season \d{1,2})\b"));
static NAMES_SEASON: LazyLock<Regex> = LazyLock::new(|| rx(r"\b(?:season \d|\d(?:st|nd|rd|th) season)\b"));
static NTH_SEASON: LazyLock<Regex> = LazyLock::new(|| rx(r"\b\d{1,2}(?:st|nd|rd|th) season\b"));
static FINAL: LazyLock<Regex> = LazyLock::new(|| rx("final"));
static SEQUEL_TAIL: LazyLock<Regex> = LazyLock::new(|| rx("[!+◎☆★']+$"));
static SEQUEL_PUNCTUATION_ONLY: LazyLock<Regex> = LazyLock::new(|| rx("^[!+◎☆★']*$"));

#[derive(Clone, Debug)]
pub struct Ranked {
    pub candidate: Candidate,
    pub final_score: f64,
}

#[allow(clippy::struct_excessive_bools)]
struct Context<'a> {
    top: f64,
    leader: &'a Entry,
    scored: &'a [Candidate],
    season_around: bool,
    imdb_season_around: bool,
    split_around: bool,
    sequel_by_punctuation: Option<usize>,
    best_coverage: f64,
    tied: bool,
    year_around: bool,
    episode_from_start: bool,
    recap_around: bool,
    season_names: Vec<Option<String>>,
}

#[allow(clippy::struct_excessive_bools)]
struct Rule<'a> {
    at: usize,
    candidate: &'a Candidate,
    entry: &'a Entry,
    parsed: &'a ParsedQuery,
    context: &'a Context<'a>,
    in_band: bool,
    in_reach: bool,
    season_match: bool,
}

pub fn gt(value: Option<u32>, than: u32) -> bool {
    value.is_some_and(|value| value > than)
}

pub fn same_year(entry_year: Option<u32>, parsed_year: Option<u32>) -> bool {
    matches!((entry_year, parsed_year), (Some(a), Some(b)) if a == b)
}

pub fn year_near(entry_year: Option<u32>, year: u32) -> bool {
    entry_year.unwrap_or(0).abs_diff(year) <= 1
}

pub fn extra_number_is(digits: Option<&str>, number: u32) -> bool {
    digits.and_then(|digits| digits.parse::<u32>().ok()) == Some(number)
}

pub fn subtype_rank(subtype: Subtype) -> f64 {
    match subtype {
        Subtype::Tv => 0.0,
        Subtype::Ona => 1.0,
        Subtype::Ova => 2.0,
        Subtype::Movie => 3.0,
        Subtype::Special => 4.0,
    }
}

pub fn age_tie_break(entry: &Entry, per_year: f64) -> f64 {
    match entry.year.filter(|year| *year != 0) {
        Some(year) => (f64::from(year) - TIE_BREAK_YEAR) * per_year,
        None => 0.0,
    }
}

pub fn earliest_of<'e, T>(entries: impl IntoIterator<Item = (T, &'e Entry)>) -> Option<T> {
    fn order(entry: &Entry) -> (Season, &str) {
        (entry.title_season.or(entry.imdb_season).unwrap_or(1), entry.start_date.as_deref().unwrap_or("9999"))
    }
    let mut best: Option<(T, &Entry)> = None;
    for (key, entry) in entries {
        if best.as_ref().is_none_or(|(_, best)| order(entry) < order(best)) {
            best = Some((key, entry));
        }
    }
    best.map(|(key, _)| key)
}

fn entry_of<'i>(index: &'i Index, candidate: &Candidate) -> &'i Entry {
    &index.entries()[candidate.title.entry as usize]
}

fn is_part_of_matched_season(entry: &Entry, parsed: &ParsedQuery, context: &Context<'_>) -> bool {
    gt(parsed.part, 1)
        && entry.part_number == parsed.part
        && entry.group_key.as_deref().is_some_and(|key| {
            !key.is_empty()
                && context
                    .season_names
                    .iter()
                    .flatten()
                    .any(|name| !name.is_empty() && key.starts_with(&format!("{name} ")))
        })
}

fn split_season_around(index: &Index, scored: &[Candidate], near: &[usize], season: Option<Season>) -> bool {
    let entry = |i: &usize| entry_of(index, &scored[*i]);
    if near.iter().any(|i| entry(i).imdb_season == season) {
        return false;
    }
    near.iter().map(entry).any(|split| {
        split.split_season == season
            && near.iter().map(entry).any(|other| {
                other.subtype.is_series()
                    && other.title_season == season
                    && other.imdb_season == Some(1)
                    && other.franchise != split.franchise
            })
    })
}

fn is_season_match(candidate: &Candidate, entry: &Entry, parsed: &ParsedQuery, context: &Context<'_>) -> bool {
    if entry.subtype == Subtype::Movie && parsed.tvdb_season {
        return false;
    }
    if entry.subtype.is_extra()
        && parsed.tvdb_season
        && parsed.extra_kind.is_none()
        && truthy_year(parsed).is_some()
        && entry.year != parsed.year
        && entry.title_season != parsed.season
    {
        return false;
    }
    if entry.imdb_season == parsed.season {
        return true;
    }
    if context.split_around && entry.split_season == parsed.season {
        return true;
    }
    if entry.title_season.is_none() && entry.imdb_season.is_none() {
        return parsed.season == Some(1);
    }
    entry.title_season == parsed.season
        && (!candidate.title.matched_by_mapping
            || !context.imdb_season_around
            || candidate.title.season_title_score >= candidate.title.score - band::REACH
            || same_year(entry.year, parsed.year))
}

fn season_bonus(rule: &Rule<'_>) -> f64 {
    let Rule { entry, parsed, context, in_reach, season_match, .. } = *rule;
    let mut bonus = 0.0;
    let season = truthy_season(parsed);
    if parsed.extra_kind.is_some()
        && season.is_none()
        && in_reach
        && gt(entry.title_season, 1)
        && entry.own_titles().any(|title| LATER_SEASON_TITLE.is_match(title))
    {
        bonus += bonus::LATER_SEASON_EXTRA;
    }
    if parsed.extra_kind.is_some() && season.is_some() && in_reach {
        let season = season.unwrap_or_default();
        let named = |word: &str| word == season.to_string() || roman_numeral(season) == Some(word);
        let extra_season =
            is_extra_entry(entry) && (season_match || entry.own_titles().any(|title| title.split(' ').any(named)));
        if extra_season {
            bonus += bonus::SEASON_MATCH;
        }
    } else if let Some(season) = season
        && in_reach
    {
        let final_match = season == FINAL_SEASON && entry.own_titles().any(|title| FINAL.is_match(title));
        if season_match || final_match {
            bonus += bonus::SEASON_MATCH;
        } else if context.season_around && (entry.title_season.is_some() || entry.imdb_season.is_some()) {
            bonus += bonus::SEASON_MISMATCH;
        }
        if season_match && context.split_around && entry.split_season == parsed.season {
            bonus += bonus::SPLIT_SEASON;
        }
        let names_season = |title: &str| {
            NAMES_SEASON.is_match(title) && (season_of(title) == parsed.season || entry.part_number == parsed.season)
        };
        if (parsed.tvdb_season || NTH_SEASON.is_match(&parsed.text))
            && season_match
            && entry.own_titles().any(names_season)
        {
            bonus += bonus::SEASON_WORD;
        }
        if let Some(part) = truthy_part(parsed)
            && season_match
            && entry.part_number.unwrap_or(1) == part
        {
            bonus += bonus::SEASON_WORD;
        }
        bonus += age_tie_break(entry, bonus::PER_YEAR);
    }
    bonus
}

fn title_form_bonus(rule: &Rule<'_>) -> f64 {
    (if rule.context.sequel_by_punctuation == Some(rule.at) { bonus::PUNCTUATED_SEQUEL } else { 0.0 })
        + (if rule.candidate.title.whole_query_is_title { bonus::WHOLE_QUERY_TITLE } else { 0.0 })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EpisodeSpan {
    Unbounded,
    Count(u32),
}

impl EpisodeSpan {
    pub fn of_count(count: Option<u32>) -> Self {
        match count {
            Some(count) if count != 0 => Self::Count(count),
            _ => Self::Unbounded,
        }
    }

    pub fn of_absolute(from: u32, to: AbsoluteEnd) -> Self {
        match to {
            AbsoluteEnd::Open => Self::Unbounded,
            AbsoluteEnd::Episode(to) => Self::of_count(Some(to - from + 1)),
        }
    }
}

pub fn episode_in_range(episode: u32, from: Option<u32>, span: EpisodeSpan) -> bool {
    from.is_some_and(|from| {
        from > 1
            && episode >= from
            && match span {
                EpisodeSpan::Unbounded => true,
                EpisodeSpan::Count(count) => u64::from(episode) < u64::from(from) + u64::from(count),
            }
    })
}

fn episode_bonus(rule: &Rule<'_>) -> f64 {
    let Rule { entry, parsed, context, in_reach, .. } = *rule;
    let Some(episode) = truthy_episode(parsed) else { return 0.0 };
    if !in_reach {
        return 0.0;
    }
    let mut bonus = 0.0;
    let later_cour = !context.episode_from_start
        && gt(entry.imdb_season, 0)
        && episode_in_range(episode, entry.imdb_from_episode, EpisodeSpan::of_count(entry.episode_count));
    let in_absolute = entry.absolute.is_some_and(|absolute| {
        episode_in_range(episode, Some(absolute.from), EpisodeSpan::of_absolute(absolute.from, absolute.to))
    });
    if truthy_season(parsed).is_none() && (in_absolute || later_cour) {
        bonus += bonus::EPISODE_IN_RANGE;
    }
    let number = episode.to_string();
    let name_words: Vec<&str> = parsed.base.split(' ').filter(|word| *word != number).collect();
    let mut with_number =
        entry.titles.iter().filter(|variant| variant.title.split(' ').any(|word| word == number)).peekable();
    let number_in_title = with_number.peek().is_some();
    let needed = f64::max(1.0, name_words.len() as f64 / 2.0);
    let name_in_title = with_number.any(|variant| {
        let words: Vec<&str> = variant.title.split(' ').collect();
        name_words.iter().filter(|word| words.contains(word)).count() as f64 >= needed
    });
    if name_in_title && episode >= 1000 {
        bonus += bonus::EPISODE_IN_TITLE;
    }
    if let Some(count) = entry.episode_count.filter(|count| *count != 0)
        && u64::from(episode) > u64::from(count) + 1
        && !number_in_title
    {
        bonus += bonus::EPISODE_OVERFLOW;
    }
    bonus
}

fn marker_bonus(rule: &Rule<'_>) -> f64 {
    let Rule { entry, parsed, context, in_band, .. } = *rule;
    if !in_band {
        return 0.0;
    }
    let mut bonus = 0.0;
    for marker in MARKER_ORDER {
        if parsed.markers.contains(&marker) {
            bonus += if entry.markers.contains(&marker) { bonus::MARKER_MATCH } else { 0.0 };
        } else if entry.markers.contains(&marker) {
            bonus += bonus::MARKER_UNASKED;
        }
    }
    let recap_request = parsed.markers.contains(&Marker::Recap) && !context.recap_around;
    if (recap_request || parsed.theater || parsed.bare_movie) && parsed.extra_kind.is_none() {
        bonus += if entry.subtype == Subtype::Movie { bonus::MOVIE_LEAN } else { 0.0 };
    }
    bonus
}

fn coverage_bonus(rule: &Rule<'_>) -> f64 {
    match rule.candidate.coverage {
        Some(coverage) if rule.in_band => bonus::MISSING_WORDS * (rule.context.best_coverage - coverage),
        _ => 0.0,
    }
}

fn distinctive_word_bonus(rule: &Rule<'_>) -> f64 {
    if rule.candidate.holds_distinctive_words { bonus::DISTINCTIVE_WORDS } else { 0.0 }
}

fn season_query_type_bonus(rule: &Rule<'_>) -> f64 {
    let Rule { entry, parsed, context, in_band, .. } = *rule;
    if !in_band || !context.tied || truthy_season(parsed).is_none() || parsed.wants_extra {
        return 0.0;
    }
    bonus::SUBTYPE_STEP * subtype_rank(entry.subtype)
        + if entry.has_extra_word { bonus::EXTRA_WORD_UNASKED } else { 0.0 }
}

fn extra_bonus(rule: &Rule<'_>) -> f64 {
    let Rule { entry, parsed, in_band, .. } = *rule;
    if parsed.extra_kind.is_none() || !in_band {
        return 0.0;
    }
    let mut bonus = 0.0;
    if is_extra_entry(entry) {
        let endings: Vec<&str> = entry.own_titles().filter_map(ending_extra_word).collect();
        let asked = parsed.extra_word.as_deref().filter(|word| !word.is_empty());
        if let Some(word) = asked
            && endings.iter().any(|ending| extra_family(ending) == extra_family(word))
        {
            bonus += bonus::EXTRA_WORD_FAMILY;
        }
        if let Some(word) = asked
            && endings.contains(&word)
        {
            bonus += bonus::EXTRA_WORD_FORM;
        }
        if compact(&norm(&entry.canonical_title)) == compact(&parsed.base) {
            bonus += bonus::EXTRA_FRANCHISE_NAME;
        }
        bonus += bonus::EXTRA + age_tie_break(entry, bonus::PER_YEAR_EXTRA);
    }
    if let Some(number) = parsed.extra_number
        && entry.titles.iter().any(|variant| variant.extra_number.is_some())
    {
        let same_number = entry.titles.iter().any(|variant| extra_number_is(variant.extra_number.as_deref(), number));
        bonus += if same_number { bonus::EXTRA_NUMBER } else { -bonus::EXTRA_NUMBER };
    }
    if entry.subtype == Subtype::Movie {
        bonus += bonus::MOVIE_FOR_EXTRA;
    }
    bonus
}

fn movie_bonus(rule: &Rule<'_>) -> f64 {
    let Rule { candidate, entry, parsed, context, .. } = *rule;
    let Some(movie) = parsed.movie else { return 0.0 };
    let index = entry.movie.or(entry.derived_movie);
    let numbered_by_best_title =
        candidate.title.best_title_movie.is_some() || candidate.title.score < context.top - band::REACH;
    let mut bonus = if entry.subtype == Subtype::Movie { bonus::MOVIE_SUBTYPE } else { 0.0 };
    if index == Some(movie) {
        bonus += bonus::MOVIE_NUMBER;
    } else if index.is_some() && numbered_by_best_title {
        bonus -= bonus::MOVIE_NUMBER;
    }
    bonus
}

fn year_bonus(rule: &Rule<'_>) -> f64 {
    let Rule { candidate, entry, parsed, context, .. } = *rule;
    let Some(year) = truthy_year(parsed) else { return 0.0 };
    let entry_year = entry.year.filter(|year| *year != 0);
    if (entry_year.is_none() && entry.title_years.is_empty()) || candidate.title.score < context.top - band::YEAR {
        return 0.0;
    }
    if entry.year == Some(year) || entry.title_years.contains(&year) {
        return if truthy_season(parsed).is_some() {
            bonus::YEAR_WITH_SEASON
        } else if context.year_around {
            bonus::YEAR_CONFIRMED
        } else {
            bonus::YEAR
        };
    }
    if year_near(entry.year, year) {
        if truthy_season(parsed).is_some() { bonus::YEAR_NEAR_WITH_SEASON } else { bonus::YEAR_NEAR }
    } else {
        0.0
    }
}

fn plain_query_bonus(rule: &Rule<'_>, index: &Index) -> f64 {
    let Rule { at, candidate, entry, parsed, context, in_band, .. } = *rule;
    if !in_band || !context.tied || truthy_season(parsed).is_some() || parsed.wants_extra {
        return 0.0;
    }
    let mut bonus = if entry.imdb_season == Some(1) && entry.franchise == context.leader.franchise {
        bonus::FIRST_SEASON
    } else {
        0.0
    };
    let earlier_sibling = gt(entry.season, 1)
        && context.scored.iter().enumerate().any(|(j, other)| {
            let other_entry = entry_of(index, other);
            j != at
                && other.title.score >= candidate.title.score - band::SIBLING
                && !gt(other_entry.season, 1)
                && (other_entry.subtype.is_series()
                    || other_entry.subtype == Subtype::Ova
                        && gt(other_entry.episode_count, 1)
                        && other_entry.start_date.as_deref().unwrap_or("9999")
                            < entry.start_date.as_deref().unwrap_or(""))
        });
    bonus += (if earlier_sibling { bonus::EARLIER_SIBLING } else { 0.0 })
        + bonus::SUBTYPE_STEP * subtype_rank(entry.subtype);
    if parsed.extra_kind.is_none() && candidate.title.matched_as_extra {
        bonus += bonus::EXTRA_WORD_UNASKED;
    }
    bonus + age_tie_break(entry, bonus::PER_YEAR)
}

fn punctuated_sequel(index: &Index, scored: &[Candidate], near: &[usize], parsed: &ParsedQuery) -> Option<usize> {
    if parsed.wants_extra {
        return None;
    }
    let mut winner = None;
    let mut length = 0;
    for &at in near {
        for key in &entry_of(index, &scored[at]).sequel_keys {
            let key_length = utf16_len(key);
            if !parsed.punct_key.contains(key.as_str()) || key_length <= length {
                continue;
            }
            let bare = SEQUEL_TAIL.replace(key, "");
            let less_punctuated = |other: &str| {
                utf16_len(other) < key_length
                    && other.starts_with(bare.as_ref())
                    && SEQUEL_PUNCTUATION_ONLY.is_match(&other[bare.len()..])
            };
            let sibling = near.iter().any(|&other| {
                other != at
                    && entry_of(index, &scored[other])
                        .main_titles
                        .iter()
                        .any(|title| less_punctuated(&punct_key(title)))
            });
            if sibling {
                winner = Some(at);
                length = key_length;
            }
        }
    }
    winner
}

pub fn apply_rules(index: &Index, scored: &[Candidate], parsed: &ParsedQuery) -> Vec<Ranked> {
    let top = scored[0].title.score;
    let entry = |candidate: &Candidate| entry_of(index, candidate);
    let near: Vec<usize> = (0..scored.len()).filter(|&i| scored[i].title.score >= top - band::REACH).collect();
    let season = truthy_season(parsed);
    let year = truthy_year(parsed);
    let mut context = Context {
        top,
        leader: entry(&scored[0]),
        scored,
        season_around: season.is_some()
            && near.iter().map(|&i| entry(&scored[i])).any(|e| e.title_season == season || e.imdb_season == season),
        imdb_season_around: season.is_some() && near.iter().any(|&i| entry(&scored[i]).imdb_season == season),
        split_around: parsed.tvdb_season && split_season_around(index, scored, &near, parsed.season),
        sequel_by_punctuation: punctuated_sequel(index, scored, &near, parsed),
        best_coverage: scored.iter().map(|candidate| candidate.coverage.unwrap_or(0.0)).fold(0.0, f64::max),
        tied: scored.iter().filter(|candidate| candidate.title.score >= top - band::TIE).count() > 1,
        year_around: year.is_some_and(|year| {
            season.is_none()
                && scored.iter().any(|candidate| {
                    candidate.title.score >= top - band::YEAR
                        && (entry(candidate).year == Some(year) || entry(candidate).title_years.contains(&year))
                })
        }),
        episode_from_start: truthy_episode(parsed).is_some_and(|episode| {
            scored.iter().any(|candidate| {
                let e = entry(candidate);
                candidate.title.score >= top - band::REACH
                    && e.subtype.is_series()
                    && e.imdb_from_episode.unwrap_or(1) == 1
                    && e.episode_count.is_some_and(|count| count >= episode)
            })
        }),
        recap_around: scored.iter().any(|candidate| {
            candidate.title.score >= top - band::TIE && entry(candidate).markers.contains(&Marker::Recap)
        }),
        season_names: Vec::new(),
    };
    if gt(parsed.part, 1) {
        context.season_names = near
            .iter()
            .filter(|&&i| is_season_match(&scored[i], entry(&scored[i]), parsed, &context))
            .map(|&i| entry(&scored[i]).group_key.clone())
            .collect();
    }
    let mut ranked: Vec<Ranked> = scored
        .iter()
        .enumerate()
        .map(|(at, candidate)| {
            let entry = entry(candidate);
            let rule = Rule {
                at,
                candidate,
                entry,
                parsed,
                context: &context,
                in_band: candidate.title.score >= top - band::TIE,
                in_reach: candidate.title.score >= top - band::REACH,
                season_match: is_season_match(candidate, entry, parsed, &context)
                    || is_part_of_matched_season(entry, parsed, &context),
            };
            let mut sum = 0.0;
            sum += season_bonus(&rule);
            sum += title_form_bonus(&rule);
            sum += episode_bonus(&rule);
            sum += marker_bonus(&rule);
            sum += season_query_type_bonus(&rule);
            sum += extra_bonus(&rule);
            sum += movie_bonus(&rule);
            sum += year_bonus(&rule);
            sum += plain_query_bonus(&rule, index);
            sum += distinctive_word_bonus(&rule);
            sum += coverage_bonus(&rule);
            Ranked { candidate: candidate.clone(), final_score: candidate.title.score + sum }
        })
        .collect();
    ranked.sort_by(|a, b| b.final_score.partial_cmp(&a.final_score).unwrap_or(std::cmp::Ordering::Equal));
    ranked
}
