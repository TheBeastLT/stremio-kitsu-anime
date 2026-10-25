use std::cmp::Reverse;
use std::ops::Range;

use indexmap::{IndexMap, IndexSet};

use crate::index::{Index, Variant, trigrams_of};
use crate::jaro_winkler::distance;
use crate::query::{ParsedQuery, Season, compact, glue, has_ending_extra_word};

pub const EXACT: f64 = 1.0;
pub const EXACT_MAPPING: f64 = 0.985;
pub const EXACT_SYNONYM: f64 = 0.95;
pub const SYNONYM_SCALE: f64 = 0.95;
pub const NUMBERED_MOVIE: f64 = 0.97;
pub const WORD_WEIGHT: f64 = 0.6;
pub const COVERAGE_WEIGHT: f64 = 0.35;
pub const INITIALS_WEIGHT: f64 = 0.5;
pub const CLOSE_WORD: f64 = 0.85;
pub const COMMON_WORD_ENTRIES: usize = 400;
pub const MAX_SCORED: usize = 1000;
pub const MAX_TRIGRAM: usize = 200;
pub const MIN_TRIGRAM_SHARE: f64 = 0.6;
pub const MIN_TRIGRAM_HITS: f64 = 2.0;

pub struct Query<'p> {
    pub parsed: &'p ParsedQuery,
    pub words: Vec<&'p str>,
    unique: Vec<(&'p str, f64)>,
    weight: f64,
    units: Vec<u16>,
    spans: Vec<Range<usize>>,
    compact_base: String,
    compact_text: String,
    compact_full: String,
}

impl<'p> Query<'p> {
    pub fn new(index: &Index, parsed: &'p ParsedQuery) -> Self {
        let words: Vec<&str> = parsed.base.split(' ').collect();
        let unique: Vec<(&str, f64)> =
            words.iter().copied().collect::<IndexSet<&str>>().into_iter().map(|word| (word, index.idf(word))).collect();
        let weight = unique.iter().fold(0.0, |sum, (_, idf)| sum + idf);
        let units: Vec<u16> = parsed.base.encode_utf16().collect();
        let mut spans = Vec::with_capacity(words.len());
        let mut start = 0;
        for word in &words {
            let end = start + word.encode_utf16().count();
            spans.push(start..end);
            start = end + 1;
        }
        Self {
            parsed,
            words,
            unique,
            weight,
            units,
            spans,
            compact_base: compact(&parsed.base),
            compact_text: compact(&parsed.text),
            compact_full: compact(&parsed.full),
        }
    }

    fn word_units(&self, i: usize) -> &[u16] {
        &self.units[self.spans[i].clone()]
    }

    fn units_from(&self, i: usize) -> &[u16] {
        &self.units[self.spans[i].start..]
    }
}

fn unique_words<S: AsRef<str>>(words: &[S]) -> Vec<&str> {
    let mut unique: Vec<&str> = Vec::with_capacity(words.len());
    for word in words {
        if !unique.contains(&word.as_ref()) {
            unique.push(word.as_ref());
        }
    }
    unique
}

pub fn token_score<S: AsRef<str>>(
    index: &Index,
    query: &Query<'_>,
    title_words: &[S],
    initials: &IndexSet<String>,
) -> f64 {
    let title = unique_words(title_words);
    let weight = |word: &str, idf: f64| if initials.contains(word) { idf * INITIALS_WEIGHT } else { idf };
    let mut shared = 0.0;
    for &(word, idf) in &query.unique {
        if title.contains(&word) {
            shared += weight(word, idf);
        }
    }
    let mut title_weight = 0.0;
    for word in &title {
        title_weight += weight(word, index.idf(word));
    }
    let dice = if query.weight + title_weight == 0.0 { 0.0 } else { 2.0 * shared / (query.weight + title_weight) };
    let coverage = if query.weight == 0.0 { 0.0 } else { shared / query.weight };
    (1.0 - COVERAGE_WEIGHT) * dice + COVERAGE_WEIGHT * coverage
}

pub fn char_similarity(query: &Query<'_>, title: &[u16]) -> f64 {
    let in_title = |word: &[u16]| {
        title.split(|&unit| unit == u16::from(b' ')).any(|other| other == word || distance(word, other) >= CLOSE_WORD)
    };
    let mut start = 0;
    while start + 1 < query.words.len() && !in_title(query.word_units(start)) {
        start += 1;
    }
    distance(query.units_from(start), title)
}

fn rare_words<'q>(index: &Index, words: &[&'q str]) -> Vec<&'q str> {
    let rare: Vec<&str> = words.iter().copied().filter(|word| index.entry_count(word) < COMMON_WORD_ENTRIES).collect();
    if rare.is_empty() { words.to_vec() } else { rare }
}

fn trigram_candidates(index: &Index, base: &str) -> Vec<u32> {
    let grams = trigrams_of(&compact(base));
    let mut hits: IndexMap<u32, u32> = IndexMap::new();
    for gram in &grams {
        for &entry in index.entries_with_trigram(*gram) {
            *hits.entry(entry).or_insert(0) += 1;
        }
    }
    let share = MIN_TRIGRAM_SHARE * grams.len() as f64;
    let min_hits = if share > MIN_TRIGRAM_HITS { share } else { MIN_TRIGRAM_HITS };
    let mut kept: Vec<(u32, u32)> = hits.into_iter().filter(|&(_, count)| f64::from(count) >= min_hits).collect();
    kept.sort_by_key(|&(_, count)| Reverse(count));
    kept.into_iter().take(MAX_TRIGRAM).map(|(entry, _)| entry).collect()
}

pub fn candidates(index: &Index, query: &Query<'_>) -> Vec<u32> {
    let mut weights: IndexMap<u32, f64> = IndexMap::new();
    if query.words.iter().any(|word| index.word_id(word).is_none()) {
        for entry in trigram_candidates(index, &query.parsed.base) {
            weights.insert(entry, f64::INFINITY);
        }
    }
    for word in rare_words(index, &query.words) {
        let idf = index.idf(word);
        for &entry in index.entries_with(word) {
            *weights.entry(entry).or_insert(0.0) += idf;
        }
    }
    if weights.len() <= MAX_SCORED {
        return weights.into_keys().collect();
    }
    let mut ranked: Vec<(u32, f64)> = weights.into_iter().collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    ranked.into_iter().take(MAX_SCORED).map(|(entry, _)| entry).collect()
}

#[derive(Clone, Debug, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub struct TitleScore {
    pub entry: u32,
    pub score: f64,
    pub plain_exact: bool,
    pub by_synonym: bool,
    pub matched_as_extra: bool,
    pub whole_query_is_title: bool,
    pub matched_without_number: bool,
    pub part_of_best: Option<Season>,
    pub best_title_movie: Option<u32>,
    pub matched_by_mapping: bool,
    pub season_title_score: f64,
}

pub fn base_for<'v>(variant: &'v Variant, parsed: &ParsedQuery) -> &'v str {
    match &variant.movie_base {
        Some(movie_base) if parsed.bare_movie || parsed.theater => movie_base,
        _ => &variant.base,
    }
}

fn compact_eq(text: &str, compacted: &str) -> bool {
    text.chars().filter(|&c| c != ' ').eq(compacted.chars())
}

fn is_exact(variant: &Variant, base: &str, query: &Query<'_>) -> bool {
    let parsed = query.parsed;
    base == parsed.base
        || variant.title == parsed.base
        || variant.title == parsed.text
        || compact_eq(base, &query.compact_base)
        || (!variant.from_synonym() && compact_eq(&variant.title, &query.compact_full))
}

fn partial_score(index: &Index, variant: &Variant, base: &str, query: &Query<'_>) -> f64 {
    let title_words: Vec<&str> = base.split(' ').collect();
    let glued = glue(&title_words, &query.words, !variant.from_synonym());
    let units: Vec<u16> = base.encode_utf16().collect();
    let score = WORD_WEIGHT * token_score(index, query, &glued.words, &glued.initials)
        + (1.0 - WORD_WEIGHT) * char_similarity(query, &units);
    if variant.from_synonym() { score * SYNONYM_SCALE } else { score }
}

#[allow(clippy::float_cmp)]
pub fn title_score(index: &Index, entry: u32, query: &Query<'_>) -> TitleScore {
    let record = &index.entries()[entry as usize];
    let parsed = query.parsed;
    let mut out = TitleScore {
        entry,
        score: 0.0,
        plain_exact: false,
        by_synonym: false,
        matched_as_extra: false,
        whole_query_is_title: false,
        matched_without_number: false,
        part_of_best: None,
        best_title_movie: None,
        matched_by_mapping: false,
        season_title_score: 0.0,
    };
    let (mut plain_matched, mut extra_matched) = (false, false);
    let numbered_movie = parsed.movie.is_some() && record.movie.or(record.derived_movie) == parsed.movie;
    for variant in &record.titles {
        let base = base_for(variant, parsed);
        let exact = is_exact(variant, base, query);
        let mut score = if exact {
            if variant.from_mapping() {
                EXACT_MAPPING
            } else if variant.from_synonym() {
                EXACT_SYNONYM
            } else {
                EXACT
            }
        } else {
            partial_score(index, variant, base, query)
        };
        if numbered_movie {
            let words: Vec<&str> = base.split(' ').collect();
            if query.words.iter().all(|word| words.contains(word)) && NUMBERED_MOVIE > score {
                score = NUMBERED_MOVIE;
            }
        }
        if !variant.from_mapping()
            && parsed.season.is_some_and(|season| season != 0)
            && variant.season == parsed.season
            && score > out.season_title_score
        {
            out.season_title_score = score;
        }
        out.plain_exact |= exact && variant.title_year.is_none();
        let own = !variant.from_synonym() && !variant.from_mapping();
        let extra = own && has_ending_extra_word(&variant.title);
        let plain = own && !extra && !variant.abbreviated();
        if score == out.score {
            plain_matched |= plain;
            extra_matched |= extra;
            out.by_synonym &= variant.from_synonym();
        }
        if score > out.score {
            out.score = score;
            out.by_synonym = variant.from_synonym();
            plain_matched = plain;
            extra_matched = extra;
            out.best_title_movie = variant.movie;
            out.matched_by_mapping = variant.from_mapping();
            out.part_of_best = variant.stripped_season().or(variant.season);
            out.matched_without_number = exact
                && variant.title != parsed.base
                && variant.title != parsed.text
                && out.part_of_best.is_some_and(|part| part > 1);
        }
    }
    out.whole_query_is_title = record.titles.iter().any(|variant| {
        !variant.from_synonym()
            && !compact_eq(&variant.title, &query.compact_base)
            && (compact_eq(&variant.title, &query.compact_text) || compact_eq(&variant.title, &query.compact_full))
    });
    out.matched_as_extra = !plain_matched && (extra_matched || record.has_extra_word);
    out
}
