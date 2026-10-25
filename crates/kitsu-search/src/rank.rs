use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use fastjs_regex::{Regex, utf16_len};
use indexmap::{IndexMap, IndexSet};

use crate::index::{Entry, Index};
use crate::query::{EXTRA_RX, ExtraKind, Numeral, ParsedQuery, Season, glue, numerals_of};
use crate::rules::{Ranked, apply_rules, band, earliest_of, gt};
use crate::score::{EXACT_SYNONYM, Query, TitleScore, base_for, candidates, title_score};

pub const MIN_CONFIDENCE: f64 = 0.65;
pub const MAX_CANDIDATES: usize = 80;
pub const NAME_WITH_EXTRA_WORD: f64 = 0.8;
pub const TYPO_MIN_LENGTH: usize = 4;
pub const TYPO_CORRECTION_ENTRIES: usize = 5;
pub const TYPO_MAX_ENTRIES: usize = 2;
pub const DISTINCTIVE_ENTRIES: usize = 10;
pub const NAME_WORD_LENGTH: usize = 3;
const FILLER_WORDS: [&str; 12] = ["the", "a", "an", "of", "and", "no", "to", "in", "wa", "ga", "wo", "ni"];

pub mod confidence {
    pub const PARTIAL_COVERAGE: f64 = 0.15;
    pub const GLUED_COVERAGE: f64 = 0.85;
    pub const GLUED_MIN_SCORE: f64 = 0.4;
    pub const GLUED_BASE: f64 = 0.6;
    pub const SPLIT_AGREEMENT_SCORE: f64 = 0.9;
    pub const SPLIT_AGREEMENT: f64 = 0.01;
}

static YEAR_SUFFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\(\d{4}\)$", "").unwrap_or_else(|error| panic!("year suffix: {error}")));

#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub title: TitleScore,
    pub coverage: Option<f64>,
    pub holds_distinctive_words: bool,
}

impl Candidate {
    fn new(title: TitleScore) -> Self {
        Self { title, coverage: None, holds_distinctive_words: false }
    }

    fn bare(entry: u32, score: f64) -> Self {
        Self::new(TitleScore {
            entry,
            score,
            plain_exact: false,
            by_synonym: false,
            matched_as_extra: false,
            whole_query_is_title: false,
            matched_without_number: false,
            part_of_best: None,
            best_title_movie: None,
            matched_by_mapping: false,
            season_title_score: 0.0,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Coverage {
    pub single: f64,
    pub all: f64,
}

#[derive(Clone, Debug)]
pub struct Run {
    pub results: Vec<Ranked>,
    pub confidence: f64,
    pub typo_confidence: Option<f64>,
    pub typo_corrections: Option<IndexMap<String, String>>,
}

impl Run {
    pub fn top(&self) -> &Ranked {
        &self.results[0]
    }
}

pub fn truthy_season(parsed: &ParsedQuery) -> Option<Season> {
    parsed.season.filter(|season| *season != 0)
}

pub fn truthy_episode(parsed: &ParsedQuery) -> Option<u32> {
    parsed.episode.filter(|episode| *episode != 0)
}

pub fn truthy_year(parsed: &ParsedQuery) -> Option<u32> {
    parsed.year.filter(|year| *year != 0)
}

pub fn truthy_part(parsed: &ParsedQuery) -> Option<u32> {
    parsed.part.filter(|part| *part != 0)
}

pub fn is_extra_entry(entry: &Entry) -> bool {
    entry.subtype.is_extra() || entry.has_extra_word
}

pub fn has_ordinal_episodes(episode_count: Option<u32>) -> bool {
    episode_count.is_some_and(|count| count >= band::ORDINAL_EPISODES)
}

pub fn within_ordinal_span(episode_count: Option<u32>) -> bool {
    episode_count.is_none_or(|count| count <= band::ORDINAL_SPAN)
}

fn is_digits(word: &str) -> bool {
    !word.is_empty() && word.bytes().all(|byte| byte.is_ascii_digit())
}

fn is_name_word(word: &str, min_length: usize) -> bool {
    utf16_len(word) >= min_length && !FILLER_WORDS.contains(&word) && !is_digits(word)
}

fn sort_by_score(scored: &mut [Candidate]) {
    scored.sort_by(|a, b| b.title.score.partial_cmp(&a.title.score).unwrap_or(std::cmp::Ordering::Equal));
}

pub fn one_edit_apart(a: &str, b: &str) -> bool {
    let a: Vec<u16> = a.encode_utf16().collect();
    let b: Vec<u16> = b.encode_utf16().collect();
    if a == b || a.len().abs_diff(b.len()) > 1 {
        return false;
    }
    let mut i = 0;
    while i < a.len() && a.get(i) == b.get(i) {
        i += 1;
    }
    let from = |text: &[u16], at: usize| text.get(at..).unwrap_or(&[]).to_vec();
    if a.len() == b.len() {
        return from(&a, i + 1) == from(&b, i + 1)
            || (a.get(i) == b.get(i + 1) && a.get(i + 1) == b.get(i) && from(&a, i + 2) == from(&b, i + 2));
    }
    let (longer, shorter) = if a.len() > b.len() { (&a, &b) } else { (&b, &a) };
    from(longer, i + 1) == from(shorter, i)
}

pub fn confidence_of(score: f64, coverage: Coverage) -> f64 {
    use confidence::{GLUED_BASE, GLUED_COVERAGE, GLUED_MIN_SCORE, PARTIAL_COVERAGE};
    let glued_share = (coverage.all - GLUED_COVERAGE) / (1.0 - GLUED_COVERAGE);
    let glued = if coverage.all >= GLUED_COVERAGE && score >= GLUED_MIN_SCORE {
        GLUED_BASE + (1.0 - GLUED_BASE) * glued_share
    } else {
        0.0
    };
    let partial = score - PARTIAL_COVERAGE * (1.0 - coverage.single);
    if glued > partial { glued } else { partial }
}

pub fn token_score_of(index: &Index, query_words: &[&str], title_words: &[&str]) -> f64 {
    let title: IndexSet<&str> = title_words.iter().copied().collect();
    let (mut shared, mut query_weight, mut title_weight) = (0.0, 0.0, 0.0);
    for word in query_words.iter().copied().collect::<IndexSet<&str>>() {
        query_weight += index.idf(word);
        if title.contains(word) {
            shared += index.idf(word);
        }
    }
    for word in title {
        title_weight += index.idf(word);
    }
    let dice = if query_weight + title_weight == 0.0 { 0.0 } else { 2.0 * shared / (query_weight + title_weight) };
    let coverage = if query_weight == 0.0 { 0.0 } else { shared / query_weight };
    (1.0 - crate::score::COVERAGE_WEIGHT) * dice + crate::score::COVERAGE_WEIGHT * coverage
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RequestedKind {
    Extra,
    Movie,
    Marker,
}

#[derive(Clone, PartialEq, Eq, Hash)]
enum SeriesKey<'e> {
    Franchise(&'e str),
    Id(u64),
}

pub struct Ranker<'a> {
    index: &'a Index,
    parsed: &'a ParsedQuery,
    query: &'a Query<'a>,
    held: RefCell<HashMap<u32, HashSet<String>>>,
}

impl<'a> Ranker<'a> {
    pub fn new(index: &'a Index, query: &'a Query<'a>) -> Self {
        Self { index, parsed: query.parsed, query, held: RefCell::new(HashMap::new()) }
    }

    fn entry(&self, candidate: &Candidate) -> &'a Entry {
        &self.index.entries()[candidate.title.entry as usize]
    }

    fn words(&self) -> &[&'a str] {
        &self.query.words
    }

    pub fn holds_word(&self, entry: u32, word: &str) -> bool {
        let mut held = self.held.borrow_mut();
        let words = held.entry(entry).or_insert_with(|| {
            self.index.entries()[entry as usize]
                .titles
                .iter()
                .flat_map(|variant| glue(&variant.title.split(' ').collect::<Vec<_>>(), self.words(), false).words)
                .collect()
        });
        let plural = format!("{word}s");
        let singular = (utf16_len(word) > 3 && word.ends_with('s')).then(|| &word[..word.len() - 1]);
        words.contains(word) || words.contains(&plural) || singular.is_some_and(|singular| words.contains(singular))
    }

    fn holds_all_words(&self, candidate: &Candidate) -> bool {
        self.words().iter().all(|word| self.holds_word(candidate.title.entry, word))
    }

    pub fn score(&self) -> Vec<Candidate> {
        let mut scored: Vec<Candidate> = candidates(self.index, self.query)
            .into_iter()
            .map(|entry| Candidate::new(title_score(self.index, entry, self.query)))
            .collect();
        sort_by_score(&mut scored);
        scored.truncate(MAX_CANDIDATES);
        scored
    }

    pub fn keep_exact_ahead(&self, scored: &mut [Candidate]) {
        let exact =
            scored.iter().filter(|candidate| candidate.title.plain_exact).map(|candidate| candidate.title.score);
        let Some(lowest) = exact.reduce(f64::min) else { return };
        if self.parsed.base != self.parsed.full {
            return;
        }
        let ceiling = lowest - 0.001;
        for candidate in scored.iter_mut().filter(|candidate| !candidate.title.plain_exact) {
            candidate.title.score = candidate.title.score.min(ceiling);
        }
        sort_by_score(scored);
    }

    pub fn prefer_series_over_synonym_extra(&self, scored: &mut [Candidate]) {
        let parsed = self.parsed;
        let top = &scored[0];
        if truthy_season(parsed).is_some()
            || truthy_year(parsed).is_some()
            || parsed.wants_extra
            || parsed.movie.is_some()
            || self.entry(top).subtype.is_series()
            || !top.title.by_synonym
            || top.title.score < EXACT_SYNONYM
        {
            return;
        }
        let top_score = top.title.score;
        let series = scored.iter().find(|candidate| {
            let entry = self.entry(candidate);
            entry.subtype.is_series()
                && !entry.has_extra_word
                && candidate.title.score >= top_score - band::REACH
                && self.holds_all_words(candidate)
        });
        let Some(series) = series.map(|series| series.title.score) else { return };
        scored[0].title.score = series - 0.001;
        sort_by_score(scored);
    }

    pub fn prefer_requested_kind(&self, scored: &mut [Candidate]) {
        let parsed = self.parsed;
        let movie_request = (parsed.bare_movie || parsed.theater) && parsed.movie.is_none();
        let kind = if parsed.extra_kind.is_some() {
            RequestedKind::Extra
        } else if movie_request {
            RequestedKind::Movie
        } else if !parsed.markers.is_empty() {
            RequestedKind::Marker
        } else {
            return;
        };
        let top = scored[0].title.score;
        let of_kind = |entry: &Entry| match kind {
            RequestedKind::Extra => is_extra_entry(entry),
            RequestedKind::Movie => entry.subtype == crate::index::Subtype::Movie,
            RequestedKind::Marker => parsed.markers.iter().all(|marker| entry.markers.contains(marker)),
        };
        let words = self.words();
        for candidate in scored.iter_mut() {
            let entry = self.entry(candidate);
            if !of_kind(entry) {
                continue;
            }
            let closest = entry
                .titles
                .iter()
                .filter_map(|variant| {
                    let base: Vec<&str> = variant.base.split(' ').collect();
                    let glued = glue(&base, words, true).words;
                    words
                        .iter()
                        .all(|word| glued.iter().any(|title_word| title_word == word))
                        .then(|| crate::score::token_score(self.index, self.query, &base, &IndexSet::new()))
                })
                .reduce(f64::max);
            if let Some(closest) = closest {
                let floor = top - band::REQUESTED_KIND + band::REQUESTED_KIND_SPREAD * closest;
                if floor > candidate.title.score {
                    candidate.title.score = floor;
                }
            }
        }
        sort_by_score(scored);
    }

    pub fn prefer_nth_of_franchise(&self, scored: &mut [Candidate]) {
        let parsed = self.parsed;
        if !parsed.tvdb_season
            || parsed.season.is_some_and(|season| season < 2)
            || truthy_year(parsed).is_some()
            || parsed.wants_extra
            || parsed.movie.is_some()
        {
            return;
        }
        let top = scored[0].title.score;
        let series: Vec<usize> = (0..scored.len())
            .filter(|&i| {
                let entry = self.entry(&scored[i]);
                entry.subtype.is_series()
                    && !entry.has_extra_word
                    && !YEAR_SUFFIX.is_match(&entry.canonical_title)
                    && self.holds_all_words(&scored[i])
            })
            .collect();
        if series
            .iter()
            .map(|&i| self.entry(&scored[i]))
            .any(|entry| entry.title_season == parsed.season || entry.imdb_season == parsed.season)
        {
            return;
        }
        let mut first_of_series: IndexMap<SeriesKey<'_>, usize> = IndexMap::new();
        for &i in &series {
            let entry = self.entry(&scored[i]);
            let Some(start) = entry.start_date.as_deref().filter(|date| !date.is_empty()) else { continue };
            if scored[i].title.score < top - band::ORDINAL || !has_ordinal_episodes(entry.episode_count) {
                continue;
            }
            let key = entry.franchise().map_or(SeriesKey::Id(entry.id), SeriesKey::Franchise);
            let earlier = first_of_series.get(&key).is_none_or(|&first| {
                self.entry(&scored[first]).start_date.as_deref().is_some_and(|first| start < first)
            });
            if earlier {
                first_of_series.insert(key, i);
            }
        }
        let mut family: Vec<usize> = first_of_series.into_values().collect();
        family.sort_by(|&a, &b| self.entry(&scored[a]).start_date.cmp(&self.entry(&scored[b]).start_date));
        if family.len() < 2 || family.len() > band::ORDINAL_FAMILY || parsed.season != Some(family.len() as Season) {
            return;
        }
        if family[..family.len() - 1].iter().any(|&i| !within_ordinal_span(self.entry(&scored[i]).episode_count)) {
            return;
        }
        let nth = family[family.len() - 1];
        if self.entry(&scored[nth]).imdb_season.is_none() {
            return;
        }
        let score = scored[nth].title.score;
        scored[nth].title.score = (if top > score { top } else { score }) + band::SIBLING;
        sort_by_score(scored);
    }

    fn position(scored: &[Candidate], entry: u32) -> Option<usize> {
        scored.iter().position(|candidate| candidate.title.entry == entry)
    }

    fn raise(scored: &mut [Candidate], at: usize, to: f64) {
        if to > scored[at].title.score {
            scored[at].title.score = to;
        }
    }

    pub fn prefer_first_of_franchise(&self, scored: &mut Vec<Candidate>) {
        let parsed = self.parsed;
        if truthy_season(parsed).is_some() || parsed.wants_extra || parsed.movie.is_some() {
            return;
        }
        let entries = self.index.entries();
        let top = scored[0].title.score;
        let stripped = scored
            .iter()
            .filter(|candidate| candidate.title.matched_without_number && candidate.title.score >= top - band::TIE);
        let later_named = scored.iter().filter(|candidate| {
            let entry = self.entry(candidate);
            candidate.title.plain_exact
                && !candidate.title.matched_without_number
                && gt(entry.title_season.or(entry.season), 1)
                && candidate.title.score >= top - band::TIE
        });
        let named: Vec<u32> = stripped.chain(later_named).map(|candidate| candidate.title.entry).collect();
        for entry_index in named {
            let at = Self::position(scored, entry_index).unwrap_or_else(|| unreachable!("named candidates stay"));
            let entry = &entries[entry_index as usize];
            let group = entry.group_key.as_deref().map_or(&[][..], |key| self.index.season_group(key));
            let Some(first_index) = earliest_of(group.iter().map(|&i| (i, &entries[i as usize]))) else { continue };
            if first_index == entry_index {
                continue;
            }
            let present = Self::position(scored, first_index);
            let candidate = &scored[at];
            let same_franchise = present.is_some_and(|present| {
                scored[present].title.plain_exact
                    && entry.franchise().is_some()
                    && entries[first_index as usize].franchise == entry.franchise
            });
            if !candidate.title.matched_without_number && !same_franchise {
                continue;
            }
            let score = candidate.title.score;
            match present {
                Some(present) => Self::raise(scored, present, score),
                None => scored.push(Candidate::bare(first_index, score)),
            }
        }
        if truthy_year(parsed).is_none() && numerals_of(&parsed.base).is_empty() {
            self.prefer_earlier_part(scored);
        }
        sort_by_score(scored);
    }

    fn prefer_earlier_part(&self, scored: &mut [Candidate]) {
        let parsed = self.parsed;
        let current_top = scored[0].title.score;
        let long_name = parsed.base.split(' ').count() >= 3;
        let part_of = |candidate: &Candidate| {
            if candidate.title.matched_without_number {
                Some(self.entry(candidate).title_season.unwrap_or(1))
            } else {
                candidate.title.part_of_best
            }
        };
        let numbered: Vec<u32> = scored
            .iter()
            .filter(|candidate| {
                (candidate.title.matched_without_number || (gt(candidate.title.part_of_best, 1) && long_name))
                    && candidate.title.score >= current_top - band::TIE
            })
            .map(|candidate| candidate.title.entry)
            .collect();
        for entry_index in numbered {
            let at = Self::position(scored, entry_index).unwrap_or_else(|| unreachable!("numbered candidates stay"));
            let candidate = &scored[at];
            let part = part_of(candidate);
            let subtype = self.entry(candidate).subtype;
            let earlier: Vec<usize> = (0..scored.len())
                .filter(|&i| {
                    let other = &scored[i];
                    let other_entry = self.entry(other);
                    i != at
                        && other_entry.subtype == subtype
                        && other.title.score >= current_top - band::EARLIER_PART
                        && part.is_some_and(|part| other_entry.title_season.unwrap_or(1) < part)
                        && self.coverage_of(other_entry, false).all >= 1.0
                })
                .collect();
            let Some(first) = earliest_of(earlier.iter().map(|&i| (i, self.entry(&scored[i])))) else { continue };
            let score = scored[at].title.score;
            Self::raise(scored, first, score);
            scored[at].title.score -= band::SIBLING;
        }
    }

    pub fn extra_word_in_name(&self, scored: &[Candidate]) -> bool {
        let parsed = self.parsed;
        if parsed.extra_kind.is_none() {
            return false;
        }
        let word =
            EXTRA_RX.find(&parsed.full).and_then(|m| m.group(1)).map_or("undefined", |range| &parsed.full[range]);
        let ending = Regex::new(&format!(r"\b{word}(?: \d{{1,2}})?$"), "")
            .unwrap_or_else(|error| panic!("ending {word}: {error}"));
        let numbered = parsed.extra_number.map(|number| {
            Regex::new(&format!(r"\b{word} 0?{number}\b"), "")
                .unwrap_or_else(|error| panic!("numbered {word}: {error}"))
        });
        let full_words: Vec<&str> = parsed.full.split(' ').collect();
        scored.iter().take(10).any(|candidate| {
            let entry = self.entry(candidate);
            !entry.subtype.is_extra()
                && !entry.has_extra_word
                && entry.titles.iter().any(|variant| {
                    let title = variant.title.as_str();
                    !variant.from_mapping()
                        && !variant.from_synonym()
                        && ((numbered.as_ref().is_some_and(|numbered| numbered.is_match(title))
                            && !ending.is_match(title))
                            || (title.split(' ').any(|title_word| title_word == word)
                                && !ending.is_match(title)
                                && variant.base.split(' ').count() > 1
                                && token_score_of(self.index, &full_words, &title.split(' ').collect::<Vec<_>>())
                                    >= NAME_WITH_EXTRA_WORD))
                })
        })
    }

    pub fn name_words(&self, scored: &[Candidate]) -> Vec<&'a str> {
        let words = self.words();
        let query: IndexSet<&'a str> = words.iter().copied().collect();
        let top = &scored[0];
        let top_title_in_query = self.entry(top).titles.iter().any(|variant| {
            glue(&variant.title.split(' ').collect::<Vec<_>>(), words, false)
                .words
                .iter()
                .all(|word| query.contains(word.as_str()))
        });
        query
            .into_iter()
            .filter(|word| {
                is_name_word(word, NAME_WORD_LENGTH) && (!top_title_in_query || self.holds_word(top.title.entry, word))
            })
            .collect()
    }

    pub fn mark_distinctive_words(&self, scored: &mut [Candidate], words: &[&str]) {
        let distinctive: Vec<&str> = words
            .iter()
            .copied()
            .filter(|word| (1..=DISTINCTIVE_ENTRIES).contains(&self.index.entry_count(word)))
            .collect();
        if distinctive.is_empty() {
            return;
        }
        let top = scored[0].title.score;
        for candidate in scored.iter_mut() {
            candidate.holds_distinctive_words = candidate.title.score >= top - band::DISTINCTIVE
                && distinctive.iter().all(|word| self.holds_word(candidate.title.entry, word));
        }
    }

    pub fn mark_telling_coverage(&self, scored: &mut [Candidate], words: &[&str]) {
        let top = scored[0].title.score;
        let near: Vec<usize> = (0..scored.len()).filter(|&i| scored[i].title.score >= top - band::TIE).collect();
        let telling: Vec<&str> = words
            .iter()
            .copied()
            .filter(|word| {
                let holders = near.iter().filter(|&&i| self.holds_word(scored[i].title.entry, word)).count();
                holders > 0 && holders as f64 <= near.len() as f64 / 2.0
            })
            .collect();
        if telling.is_empty() {
            return;
        }
        for i in near {
            let held = telling.iter().filter(|word| self.holds_word(scored[i].title.entry, word)).count();
            scored[i].coverage = Some(held as f64 / telling.len() as f64);
        }
    }

    pub fn coverage_of(&self, entry: &Entry, typos: bool) -> Coverage {
        let parsed = self.parsed;
        let words: Vec<&str> =
            parsed.base.split(' ').filter(|word| !word.is_empty() && is_name_word(word, 0)).collect();
        if words.is_empty() {
            return Coverage { single: 1.0, all: 1.0 };
        }
        let season = entry.title_season.or(entry.season);
        let season_words: Vec<&str> = words
            .iter()
            .copied()
            .filter(|word| {
                utf16_len(word) > 1
                    && season.is_some_and(|season| numerals_of(word).first() == Some(&Numeral::Value(season)))
            })
            .collect();
        let glued: Vec<Vec<String>> = entry
            .titles
            .iter()
            .map(|variant| {
                let base: Vec<&str> = base_for(variant, parsed).split(' ').collect();
                let mut title = glue(&base, &words, !variant.from_synonym()).words;
                title.extend(season_words.iter().map(|word| (*word).to_owned()));
                title
            })
            .collect();
        let typo = |word: &str, other: &str| {
            typos
                && utf16_len(word) >= TYPO_MIN_LENGTH
                && utf16_len(other) >= TYPO_MIN_LENGTH
                && one_edit_apart(word, other)
        };
        let count = words.len() as f64;
        let single = glued
            .iter()
            .map(|title| {
                let set: HashSet<&str> = title.iter().map(String::as_str).collect();
                let held = words
                    .iter()
                    .filter(|word| set.contains(*word) || title.iter().any(|other| typo(word, other)))
                    .count();
                held as f64 / count
            })
            .fold(0.0, |max, share| if share > max { share } else { max });
        let all: HashSet<&str> = glued.iter().flatten().map(String::as_str).collect();
        Coverage { single, all: words.iter().filter(|word| all.contains(*word)).count() as f64 / count }
    }

    pub fn typo_corrections_of(&self, entry: &Entry) -> IndexMap<String, String> {
        let parsed = self.parsed;
        let words: Vec<&str> = parsed
            .base
            .split(' ')
            .filter(|word| utf16_len(word) >= TYPO_MIN_LENGTH && self.index.entry_count(word) <= TYPO_MAX_ENTRIES)
            .collect();
        let typo = |word: &str, other: &str| {
            utf16_len(other) >= TYPO_MIN_LENGTH
                && word.encode_utf16().next() == other.encode_utf16().next()
                && one_edit_apart(word, other)
        };
        let mut best: IndexMap<String, String> = IndexMap::new();
        for variant in &entry.titles {
            let title: IndexSet<&str> = base_for(variant, parsed).split(' ').collect();
            let mut corrections: IndexMap<String, String> = IndexMap::new();
            for word in words.iter().copied().filter(|word| !title.contains(word)) {
                if let Some(to) = title.iter().find(|other| typo(word, other)) {
                    corrections.insert(word.to_owned(), (*to).to_owned());
                }
            }
            if corrections.len() > best.len() {
                best = corrections;
            }
        }
        let name_words = parsed.base.split(' ').filter(|word| is_name_word(word, 3));
        let anchored = name_words.into_iter().any(|word| !best.contains_key(word));
        if anchored { best } else { IndexMap::new() }
    }

    pub fn rank(&self, limit: usize) -> Option<Run> {
        let mut scored = self.score();
        if scored.is_empty() {
            return None;
        }
        self.keep_exact_ahead(&mut scored);
        self.prefer_series_over_synonym_extra(&mut scored);
        self.prefer_requested_kind(&mut scored);
        self.prefer_first_of_franchise(&mut scored);
        self.prefer_nth_of_franchise(&mut scored);
        let words = self.name_words(&scored);
        self.mark_distinctive_words(&mut scored, &words);
        self.mark_telling_coverage(&mut scored, &words);
        let asked;
        let parsed = if self.extra_word_in_name(&scored) {
            asked = ParsedQuery { extra_kind: None::<ExtraKind>, wants_extra: false, ..self.parsed.clone() };
            &asked
        } else {
            self.parsed
        };
        let ranked = apply_rules(self.index, &scored, parsed);
        let top_entry = &self.index.entries()[ranked[0].candidate.title.entry as usize];
        let confidence = confidence_of(ranked[0].candidate.title.score, self.coverage_of(top_entry, false));
        let mut run = Run { results: Vec::new(), confidence, typo_confidence: None, typo_corrections: None };
        if confidence < MIN_CONFIDENCE {
            run.typo_confidence =
                Some(confidence_of(ranked[0].candidate.title.score, self.coverage_of(top_entry, true)));
            run.typo_corrections = ranked
                .iter()
                .take(TYPO_CORRECTION_ENTRIES)
                .map(|result| self.typo_corrections_of(&self.index.entries()[result.candidate.title.entry as usize]))
                .find(|corrections| !corrections.is_empty());
        }
        run.results = ranked;
        run.results.truncate(limit);
        Some(run)
    }
}
