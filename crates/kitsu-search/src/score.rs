use std::cmp::Reverse;

use indexmap::{IndexMap, IndexSet};

use crate::index::{Index, Variant, VariantSource, trigrams_of};
use crate::query::{ParsedQuery, Season, compact, glue, has_ending_extra_word};

// How well a query matches an entry's titles, 0 to 1.
pub const EXACT: f64 = 1.0;
// the imdb mapping's English title
pub const EXACT_MAPPING: f64 = 0.985;
// an anime-offline-database synonym, a little below Kitsu's own titles
pub const EXACT_SYNONYM: f64 = 0.95;
// a partial match on a synonym
pub const SYNONYM_SCALE: f64 = 0.95;
// the franchise name inside a movie title with the queried number ("dragon ball z movie 01")
pub const NUMBERED_MOVIE: f64 = 0.97;
// a partial match: idf-weighted word overlap, the rest jaro-winkler similarity
pub const WORD_WEIGHT: f64 = 0.6;
// a query word missing from a title costs more than an extra title word (shortened titles)
pub const COVERAGE_WEIGHT: f64 = 0.35;
// a word matched as initials (glue) is a guess: worth less than the written word ("sd" is "SD Gundam" before "Gundam
// SEED Destiny")
pub const INITIALS_WEIGHT: f64 = 0.5;
// jaro-winkler similarity of two spellings of one word
pub const CLOSE_WORD: f64 = 0.85;
// a word in more entries than this only fetches candidates when the query has no rarer word
pub const COMMON_WORD_ENTRIES: usize = 400;
// candidates sharing the most query words that get their titles scored
pub const MAX_SCORED: usize = 1000;
pub const MAX_TRIGRAM: usize = 200;
// of the query's trigrams an entry needs to be a trigram candidate
pub const MIN_TRIGRAM_SHARE: f64 = 0.6;
pub const MIN_TRIGRAM_HITS: f64 = 2.0;

pub struct Query<'p> {
    pub parsed: &'p ParsedQuery,
    pub words: Vec<&'p str>,
    pub(crate) unique: Vec<(&'p str, f64)>,
    chars: Vec<char>,
    word_starts: Vec<usize>,
    compact_base: String,
    compact_text: String,
    compact_full: String,
}

impl<'p> Query<'p> {
    pub fn new(index: &Index, parsed: &'p ParsedQuery) -> Self {
        let words: Vec<&str> = parsed.base.split(' ').collect();
        let unique = weighted(index, &words);
        let chars: Vec<char> = parsed.base.chars().collect();
        let word_starts = std::iter::once(0)
            .chain(chars.iter().enumerate().filter(|(_, c)| **c == ' ').map(|(i, _)| i + 1))
            .collect();
        Self {
            parsed,
            words,
            unique,
            chars,
            word_starts,
            compact_base: compact(&parsed.base),
            compact_text: compact(&parsed.text),
            compact_full: compact(&parsed.full),
        }
    }

    fn word_chars(&self, i: usize) -> &[char] {
        let end = self.word_starts.get(i + 1).map_or(self.chars.len(), |next| next - 1);
        &self.chars[self.word_starts[i]..end]
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

pub(crate) fn weighted<'w>(index: &Index, words: &[&'w str]) -> Vec<(&'w str, f64)> {
    words.iter().copied().collect::<IndexSet<&str>>().into_iter().map(|word| (word, index.idf(word))).collect()
}

pub fn token_score<S: AsRef<str>>(
    index: &Index,
    query: &[(&str, f64)],
    title_words: &[S],
    initials: &IndexSet<String>,
) -> f64 {
    let title = unique_words(title_words);
    let weight = |word: &str, idf: f64| if initials.contains(word) { idf * INITIALS_WEIGHT } else { idf };
    let query_weight = query.iter().fold(0.0, |sum, (_, idf)| sum + idf);
    let mut shared = 0.0;
    for &(word, idf) in query {
        if title.contains(&word) {
            shared += weight(word, idf);
        }
    }
    let mut title_weight = 0.0;
    for word in &title {
        title_weight += weight(word, index.idf(word));
    }
    let dice = if query_weight + title_weight == 0.0 { 0.0 } else { 2.0 * shared / (query_weight + title_weight) };
    let coverage = if query_weight == 0.0 { 0.0 } else { shared / query_weight };
    (1.0 - COVERAGE_WEIGHT) * dice + COVERAGE_WEIGHT * coverage
}

// Jaro-Winkler over chars: transpositions halved as a fraction, the prefix boost (up to 4 chars) only above 0.7
pub fn jaro_winkler(a: &[char], b: &[char]) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    if a == b {
        return 1.0;
    }
    let range = (a.len().max(b.len()) / 2).saturating_sub(1);
    let mut stack = [false; 256];
    let mut heap = Vec::new();
    let used = if a.len() + b.len() <= stack.len() {
        &mut stack[..a.len() + b.len()]
    } else {
        heap.resize(a.len() + b.len(), false);
        &mut heap[..]
    };
    let (a_used, b_used) = used.split_at_mut(a.len());
    for (i, c) in a.iter().enumerate() {
        let (low, high) = (i.saturating_sub(range), (i + range + 1).min(b.len()));
        if low >= high {
            continue;
        }
        let taken = &mut b_used[low..high];
        if let Some(j) = b[low..high].iter().zip(taken.iter()).position(|(other, taken)| other == c && !taken) {
            a_used[i] = true;
            taken[j] = true;
        }
    }
    let matches = a_used.iter().filter(|used| **used).count();
    if matches == 0 {
        return 0.0;
    }
    let m = matches as f64;
    let transpositions = matched(a, a_used).zip(matched(b, b_used)).filter(|(x, y)| x != y).count() as f64;
    let jaro = (m / a.len() as f64 + m / b.len() as f64 + (m - transpositions / 2.0) / m) / 3.0;
    if jaro <= 0.7 {
        return jaro;
    }
    let prefix = a.iter().zip(b).take(4).take_while(|(x, y)| x == y).count();
    jaro + prefix as f64 * 0.1 * (1.0 - jaro)
}

fn matched<'t>(text: &'t [char], used: &'t [bool]) -> impl Iterator<Item = &'t char> {
    text.iter().zip(used).filter(|(_, used)| **used).map(|(c, _)| c)
}

// spelling similarity, from the first query word the title has: leading words it lacks ("mobile suit" before "sd gundam
// force") would only cost the prefix-weighted similarity, and the word score already counts them missing
pub fn char_similarity(query: &Query<'_>, title: &[char]) -> f64 {
    let in_title = |word: &[char]| {
        title.split(|&c| c == ' ').any(|other| other == word || jaro_winkler(word, other) >= CLOSE_WORD)
    };
    let last = query.words.len() - 1;
    let start = (0..last).find(|&i| in_title(query.word_chars(i))).unwrap_or(last);
    jaro_winkler(&query.chars[query.word_starts[start]..], title)
}

fn rare_words<'q>(index: &Index, words: &[&'q str]) -> Vec<&'q str> {
    let rare: Vec<&str> = words.iter().copied().filter(|word| index.entry_count(word) < COMMON_WORD_ENTRIES).collect();
    if rare.is_empty() { words.to_vec() } else { rare }
}

// words the index has never seen ("rehamatora" for "Re: Hamatora"): candidates by character trigrams instead
fn trigram_candidates(index: &Index, base: &str) -> Vec<u32> {
    let grams = trigrams_of(&compact(base));
    let mut hits: IndexMap<u32, u32> = IndexMap::new();
    for gram in &grams {
        for &entry in index.entries_with_trigram(*gram) {
            *hits.entry(entry).or_insert(0) += 1;
        }
    }
    let min_hits = (MIN_TRIGRAM_SHARE * grams.len() as f64).max(MIN_TRIGRAM_HITS);
    let mut kept: Vec<(u32, u32)> = hits.into_iter().filter(|&(_, count)| f64::from(count) >= min_hits).collect();
    kept.sort_by_key(|&(_, count)| Reverse(count));
    kept.into_iter().take(MAX_TRIGRAM).map(|(entry, _)| entry).collect()
}

// entries sharing the query's rarer words (all its words when every one is common); past MAX_SCORED, the ones sharing
// the most
pub fn candidates(index: &Index, query: &Query<'_>) -> Vec<u32> {
    let mut weights: IndexMap<u32, f64> = IndexMap::new();
    if query.words.iter().any(|word| !index.knows(word)) {
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

#[derive(Clone, Debug, Default, PartialEq)]
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
        // the whole query equal to a title with spaces ignored ("x1999" for "X/1999"), before a year / season is taken
        // out
        || (!variant.from_synonym() && compact_eq(&variant.title, &query.compact_full))
}

fn partial_score(index: &Index, variant: &Variant, base: &str, query: &Query<'_>) -> f64 {
    let title_words: Vec<&str> = base.split(' ').collect();
    let glued = glue(&title_words, &query.words, !variant.from_synonym());
    let score = WORD_WEIGHT * token_score(index, &query.unique, &glued.words, &glued.initials)
        + (1.0 - WORD_WEIGHT) * char_similarity(query, &base.chars().collect::<Vec<_>>());
    if variant.from_synonym() { score * SYNONYM_SCALE } else { score }
}

#[allow(clippy::float_cmp)]
pub fn title_score(index: &Index, entry: u32, query: &Query<'_>) -> TitleScore {
    let record = &index.entries()[entry as usize];
    let parsed = query.parsed;
    let mut out = TitleScore { entry, ..TitleScore::default() };
    let (mut plain_matched, mut extra_matched) = (false, false);
    let numbered_movie = parsed.movie.is_some() && record.movie.or(record.derived_movie) == parsed.movie;
    for variant in &record.titles {
        let base = base_for(variant, parsed);
        let exact = is_exact(variant, base, query);
        let mut score = match variant.source {
            _ if !exact => partial_score(index, variant, base, query),
            VariantSource::Mapping => EXACT_MAPPING,
            VariantSource::Synonym { .. } => EXACT_SYNONYM,
            VariantSource::Own { .. } => EXACT,
        };
        if numbered_movie && query.words.iter().all(|word| base.split(' ').any(|title_word| title_word == *word)) {
            score = score.max(NUMBERED_MOVIE);
        }
        if !variant.from_mapping()
            && parsed.season.is_some_and(|season| season != 0)
            && variant.season == parsed.season
            && score > out.season_title_score
        {
            out.season_title_score = score;
        }
        // an exact title naming no year of its own ("Black Jack (TV)"; not "Higurashi: When They Cry (2020)"), a
        // synonym's included
        out.plain_exact |= exact && variant.title_year.is_none();
        let own = !variant.from_synonym() && !variant.from_mapping();
        let extra = own && has_ending_extra_word(&variant.title);
        // not an abbreviated title: those often hold the franchise name ("The Ancient Magus' Bride" for its Specials)
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
    // spaces ignored: "actii" for "act II"
    out.whole_query_is_title = record.titles.iter().any(|variant| {
        !variant.from_synonym()
            && !compact_eq(&variant.title, &query.compact_base)
            && (compact_eq(&variant.title, &query.compact_text) || compact_eq(&variant.title, &query.compact_full))
    });
    // matched as an extra: by a title ending in an extra word ("Matsuri Special"), or by a synonym while the entry's
    // main titles end in one; not when a main title without one matched ("Matsuri" next to its "... OVA" English title)
    out.matched_as_extra = !plain_matched && (extra_matched || record.has_extra_word);
    out
}
