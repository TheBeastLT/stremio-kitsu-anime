use std::sync::LazyLock;

use fastjs_regex::Regex;

use crate::index::{Entry, Index, Subtype};
use crate::norm::norm;
use crate::query::{
    FINAL_SEASON, Marker, ParsedQuery, SEQUEL_MARKS, Season, compact, ending_extra_word, extra_family, punct_key,
    roman_numeral, season_of,
};
use crate::rank::{Candidate, is_extra_entry, truthy_episode, truthy_part, truthy_season, truthy_year};
use crate::rx;

// How far below the best title score a candidate still counts for a rule.
pub mod band {
    pub const TIE: f64 = 0.06; // about as good as the best: type, season and age tie-breaks
    pub const REACH: f64 = 0.1; // season, numeral and episode rules
    pub const YEAR: f64 = 0.15;
    pub const EARLIER_PART: f64 = 0.2;
    pub const ORDINAL_FAMILY: usize = 3; // more series sharing the name are a long franchise: its count says nothing
    pub const ORDINAL_SPAN: u32 = 100; // episodes of an earlier series that count as one season
    pub const ORDINAL_EPISODES: u32 = 10; // episodes of a series counted as a season
    pub const ORDINAL: f64 = 0.2; // a series entry counted as the queried season, below the best title match
    pub const DISTINCTIVE: f64 = 0.2; // a candidate holding the query's distinctive words
    pub const SIBLING: f64 = 0.01; // an earlier season matching as well
    // an extra / movie of the requested kind holding the franchise name lands this far below the best, and up to
    // REQUESTED_KIND_SPREAD higher the closer its title is
    pub const REQUESTED_KIND: f64 = 0.04;
    pub const REQUESTED_KIND_SPREAD: f64 = 0.03;
}

pub mod bonus {
    pub const SEASON_MATCH: f64 = 0.08;
    pub const SEASON_MISMATCH: f64 = -0.06; // a known season other than the queried one, when another candidate has it
    pub const SPLIT_SEASON: f64 = 0.03; // the split-numbered season, when no entry has the queried season itself
    pub const SEASON_WORD: f64 = 0.03; // "S2" for a title saying "season 2" (not "part 2")
    pub const LATER_SEASON_EXTRA: f64 = -0.03; // "2nd Season Special" for an extra request without a season
    pub const PUNCTUATED_SEQUEL: f64 = 0.08;
    pub const WHOLE_QUERY_TITLE: f64 = 0.03; // the whole query, extra words included, is a title
    pub const DISTINCTIVE_WORDS: f64 = 0.2; // band::DISTINCTIVE
    // per share of the telling query words a near-equal match lacks, against the best covering one
    pub const MISSING_WORDS: f64 = -0.3;
    pub const EPISODE_IN_RANGE: f64 = 0.12;
    // the query's trailing number is a word of a title: its name, not an episode ("yamato 2205")
    pub const EPISODE_IN_TITLE: f64 = 0.1;
    pub const EPISODE_OVERFLOW: f64 = -0.05; // an episode number past the entry's episode count
    pub const MARKER_MATCH: f64 = 0.2; // a recap / re-edit request and title
    pub const MARKER_UNASKED: f64 = -0.04; // a recap / re-edit title for a plain query
    pub const MOVIE_LEAN: f64 = 0.07; // a movie / compilation / theatrical request leans to movies
    pub const SUBTYPE_STEP: f64 = -0.02; // per Subtype::rank step below TV, among equal matches
    pub const EXTRA_WORD_UNASKED: f64 = -0.04; // a title ending in an extra word for a query not asking for one
    pub const EXTRA: f64 = 0.06; // an OVA / special for an extra request
    pub const EXTRA_WORD_FAMILY: f64 = 0.02; // the title ends in the queried extra word's kind ("... Specials")
    pub const EXTRA_WORD_FORM: f64 = 0.002; // and in its exact form ("special" vs "specials")
    pub const EXTRA_FRANCHISE_NAME: f64 = 0.02; // an extra titled exactly the franchise name ("Mazinkaiser", an OVA)
    pub const EXTRA_NUMBER: f64 = 0.04; // "ova 2" for "OVA 2" (another number: the same against)
    pub const MOVIE_FOR_EXTRA: f64 = -0.04;
    pub const MOVIE_SUBTYPE: f64 = 0.04; // a movie for "movie N"
    pub const MOVIE_NUMBER: f64 = 0.15; // "movie N" for the N-th movie (another number: the same against)
    pub const YEAR: f64 = 0.12;
    pub const YEAR_CONFIRMED: f64 = 0.15; // when a candidate close by started that year
    pub const YEAR_NEAR: f64 = 0.03; // a year off by one
    // next to a season the year is usually the series premiere ("Show 2018 S03")
    pub const YEAR_WITH_SEASON: f64 = 0.03;
    pub const YEAR_NEAR_WITH_SEASON: f64 = 0.01;
    pub const FIRST_SEASON: f64 = 0.02; // the leading franchise's first season (imdb mapping), for a plain query
    pub const EARLIER_SIBLING: f64 = -0.03; // a later season when an earlier one matches as well
    pub const PER_YEAR: f64 = -0.0002; // per year since TIE_BREAK_YEAR: the earlier entry among equals
    pub const PER_YEAR_EXTRA: f64 = -0.0003;
}

const TIE_BREAK_YEAR: f64 = 1950.0;
const MARKER_ORDER: [Marker; 2] = [Marker::Recap, Marker::Reedit];

static LATER_SEASON_TITLE: LazyLock<Regex> =
    LazyLock::new(|| rx(r"\b(?:\d{1,2}(?:st|nd|rd|th) season|season \d{1,2})\b", ""));
static NAMES_SEASON: LazyLock<Regex> = LazyLock::new(|| rx(r"\b(?:season \d|\d(?:st|nd|rd|th) season)\b", ""));
static NTH_SEASON: LazyLock<Regex> = LazyLock::new(|| rx(r"\b\d{1,2}(?:st|nd|rd|th) season\b", ""));

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

pub fn age_tie_break(entry: &Entry, per_year: f64) -> f64 {
    match entry.year.filter(|year| *year != 0) {
        Some(year) => (f64::from(year) - TIE_BREAK_YEAR) * per_year,
        None => 0.0,
    }
}

pub fn earliest_of<'e, T>(entries: impl IntoIterator<Item = (T, &'e Entry)>) -> Option<T> {
    let order = |entry: &'e Entry| {
        (entry.title_season.or(entry.imdb_season).unwrap_or(1), entry.start_date.as_deref().unwrap_or("9999"))
    };
    entries.into_iter().min_by_key(|(_, entry)| order(entry)).map(|(key, _)| key)
}

fn entry_of<'i>(index: &'i Index, candidate: &Candidate) -> &'i Entry {
    &index.entries()[candidate.title.entry as usize]
}

// A later part of the queried season, titled after the season's own entry ("BLEACH: Sennen Kessen-hen -
// Ketsubetsu-tan", part 2 of season 17's "BLEACH: Sennen Kessen-hen", for "(cour 2) S17")
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

// The split-numbered season counts only against a series that names the season in its own title yet is another imdb
// series' first season ("Hanma Baki: Son of Ogre", titled "Baki 3"): where the title-numbered season is a movie or the
// franchise's own ("Initial D Third Stage"), the imdb seasons are what they say.
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

// The scraper writes any season as S<n>, whether the torrent said "2nd Season" (Kitsu's numbering) or S02 (TVDB's), so
// either counts; an S<n> / "season n" wants a series, not "Naruto Movie 2" for "naruto s2" (a bare number can be a
// movie's: "stand by me doraemon 2"). Matched by the mapping's franchise name next to an entry that is that TVDB
// season, an entry's own season counts only when a title naming it matches about as well or the queried year is its
// own ("sword art online s2" isn't "Alicization - War of Underworld 2nd Season"; "jojo's bizarre adventure 2018 s5" is
// Golden Wind).
fn is_season_match(candidate: &Candidate, entry: &Entry, parsed: &ParsedQuery, context: &Context<'_>) -> bool {
    if entry.subtype == Subtype::Movie && parsed.tvdb_season {
        return false;
    }
    // an OVA / special is a season of a series only by its own title ("Tenchi Muyou! Ryououki 5th Season"), not by an
    // imdb mapping that happens to sit at one while the queried year names another show ("hunter x hunter 2011 s4":
    // "Greed Island Final", mapped at the 1999 series' season 4)
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
    // an extra request without a season wants the first season's extra, not "2nd Season Special"
    if parsed.extra_kind.is_some()
        && season.is_none()
        && in_reach
        && gt(entry.title_season, 1)
        && entry.own_titles().any(|title| LATER_SEASON_TITLE.is_match(title))
    {
        bonus += bonus::LATER_SEASON_EXTRA;
    }
    // "spice and wolf ii specials": the season picks which extra
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
        let final_match = season == FINAL_SEASON && entry.own_titles().any(|title| title.contains("final"));
        if season_match || final_match {
            bonus += bonus::SEASON_MATCH;
        } else if context.season_around && (entry.title_season.is_some() || entry.imdb_season.is_some()) {
            bonus += bonus::SEASON_MISMATCH;
        }
        if season_match && context.split_around && entry.split_season == parsed.season {
            bonus += bonus::SPLIT_SEASON;
        }
        // a season named with a word ("S2", "2nd season"): the entry whose own title names it, not one only mapped
        // there that season, or its part as TVDB counts split cours ("War of Underworld Part 2" for "S2"); not another
        // season ("Steven Universe Season 6", mapped as its own imdb's S1)
        let names_season = |title: &str| {
            NAMES_SEASON.is_match(title) && (season_of(title) == parsed.season || entry.part_number == parsed.season)
        };
        if (parsed.tvdb_season || NTH_SEASON.is_match(&parsed.text))
            && season_match
            && entry.own_titles().any(names_season)
        {
            bonus += bonus::SEASON_WORD;
        }
        // the season's part, from the entry's own titles (no part number: the first)
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

pub fn episode_in_range(episode: u32, from: Option<u32>, count: Option<u32>) -> bool {
    from.is_some_and(|from| {
        from > 1
            && episode >= from
            && count
                .filter(|count| *count != 0)
                .is_none_or(|count| u64::from(episode) < u64::from(from) + u64::from(count))
    })
}

// absolute episode numbers: the franchise's TV entries laid end to end; an episode past an entry's count rules it out
fn episode_bonus(rule: &Rule<'_>) -> f64 {
    let Rule { entry, parsed, context, in_reach, .. } = *rule;
    let Some(episode) = truthy_episode(parsed) else { return 0.0 };
    if !in_reach {
        return 0.0;
    }
    let mut bonus = 0.0;
    // a later cour numbered on within its season ("dr stone new world 16": Part 2, 12-22), when no close match holds
    // the episode from its own start (not "shingeki no kyojin 21": the first season has 25)
    let later_cour = !context.episode_from_start
        && gt(entry.imdb_season, 0)
        && episode_in_range(episode, entry.imdb_from_episode, entry.episode_count);
    let in_absolute =
        entry.absolute.is_some_and(|absolute| episode_in_range(episode, Some(absolute.from), absolute.count));
    if truthy_season(parsed).is_none() && (in_absolute || later_cour) {
        bonus += bonus::EPISODE_IN_RANGE;
    }
    // a title with the number is its name, not an episode; with at least half of the rest of the name too ("one piece
    // 1001" is no "1001 Nights"), it also leads
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
    // compilation movies rarely say so: with no recap entry around, a recap request leans to movies
    let recap_request = parsed.markers.contains(&Marker::Recap) && !context.recap_around;
    if (recap_request || parsed.theater || parsed.bare_movie) && parsed.extra_kind.is_none() {
        bonus += if entry.subtype == Subtype::Movie { bonus::MOVIE_LEAN } else { 0.0 };
    }
    bonus
}

// among near-equal matches, the query words telling them apart weigh more than the type and age tie-breaks ("mobile
// suit sd gundam gaiden": the OVA SD Gundam Gaiden, not the series Gundam SEED Destiny)
fn coverage_bonus(rule: &Rule<'_>) -> f64 {
    match rule.candidate.coverage {
        Some(coverage) if rule.in_band => bonus::MISSING_WORDS * (rule.context.best_coverage - coverage),
        _ => 0.0,
    }
}

// the words naming the show ("igloo" in "mobile suit gundam igloo") over a closer title without them; the same for
// every candidate holding them, so their own order stays
fn distinctive_word_bonus(rule: &Rule<'_>) -> f64 {
    if rule.candidate.holds_distinctive_words { bonus::DISTINCTIVE_WORDS } else { 0.0 }
}

// a season query wants the series itself: TV over other types, not a title ending in an extra word
fn season_query_type_bonus(rule: &Rule<'_>) -> f64 {
    let Rule { entry, parsed, context, in_band, .. } = *rule;
    if !in_band || !context.tied || truthy_season(parsed).is_none() || parsed.wants_extra {
        return 0.0;
    }
    bonus::SUBTYPE_STEP * entry.subtype.rank() + if entry.has_extra_word { bonus::EXTRA_WORD_UNASKED } else { 0.0 }
}

// the title's own extra word decides between extras; Kitsu's OVA/special subtype is too inconsistent for it
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
        let same_number = entry.titles.iter().any(|variant| variant.extra_number == Some(number));
        bonus += if same_number { bonus::EXTRA_NUMBER } else { -bonus::EXTRA_NUMBER };
    }
    if entry.subtype == Subtype::Movie {
        bonus += bonus::MOVIE_FOR_EXTRA;
    }
    bonus
}

// franchise-wide numbering ("Pokemon Movie 14") only counts against an entry when it is its best title match
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

// a plain title: the first season, TV over other types, the earlier entry among equals
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
    // an OVA series counts when it came first ("FLCL" before "FLCL Progressive"), not a one-off or a later side story
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
    bonus += (if earlier_sibling { bonus::EARLIER_SIBLING } else { 0.0 }) + bonus::SUBTYPE_STEP * entry.subtype.rank();
    if parsed.extra_kind.is_none() && candidate.title.matched_as_extra {
        bonus += bonus::EXTRA_WORD_UNASKED;
    }
    bonus + age_tie_break(entry, bonus::PER_YEAR)
}

// a sequel marked only by punctuation ("Hataraku Saibou!!" next to "Hataraku Saibou", "Gintama'" next to "Gintama"):
// the longest one in the query
fn punctuated_sequel(index: &Index, scored: &[Candidate], near: &[usize], parsed: &ParsedQuery) -> Option<usize> {
    if parsed.wants_extra {
        return None;
    }
    let mut winner = None;
    let mut length = 0;
    for &at in near {
        for key in &entry_of(index, &scored[at]).sequel_keys {
            let key_length = key.chars().count();
            if !parsed.punct_key.contains(key.as_str()) || key_length <= length {
                continue;
            }
            let bare = key.trim_end_matches(SEQUEL_MARKS);
            let less_punctuated = |other: &str| {
                other.chars().count() < key_length
                    && other.starts_with(bare)
                    && other[bare.len()..].chars().all(|c| SEQUEL_MARKS.contains(&c))
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

// The ranking rules: preferences on top of the title score, for the candidates close to the best match.
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
        // a tie-break needs a tie: alone in the band, the best match keeps its lead ("mobile suit sd gundam mk i")
        tied: scored.iter().filter(|candidate| candidate.title.score >= top - band::TIE).count() > 1,
        year_around: year.is_some_and(|year| {
            season.is_none()
                && scored.iter().any(|candidate| {
                    candidate.title.score >= top - band::YEAR
                        && (entry(candidate).year == Some(year) || entry(candidate).title_years.contains(&year))
                })
        }),
        // the episode fits an entry close to the best match counted from its own first episode
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
    // the franchise names of the season matches, for a later part asked beside the season
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
