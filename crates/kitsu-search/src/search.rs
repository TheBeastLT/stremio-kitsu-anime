use std::sync::LazyLock;

use fastjs_regex::Regex;

use crate::index::Index;
use crate::norm::norm;
use crate::query::{parse_query, split_titles};
use crate::rank::{MIN_CONFIDENCE, Ranker, Run, confidence};
use crate::rx;
use crate::score::{EXACT_SYNONYM, Query};

// Torrent titles stay well under this (the longest of 51k real queries is 188); longer ones only cost scoring time.
pub const MAX_QUERY_LENGTH: usize = 250;
pub const LIMIT: usize = 20;

static SPACES: LazyLock<Regex> = LazyLock::new(|| rx(r"(\s+)", ""));

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub id: u32,
    pub score: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Ranking {
    pub hits: Vec<Hit>,
    pub confidence: Option<f64>,
}

pub struct Searcher {
    index: Index,
}

pub fn trim_query(query: &str) -> &str {
    let Some((cut, _)) = query.char_indices().nth(MAX_QUERY_LENGTH) else { return query };
    let cut = &query[..cut];
    cut.rfind(char::is_whitespace).map_or(cut, |at| cut[..at].trim_end())
}

impl Searcher {
    pub fn new(index: Index) -> Self {
        Self { index }
    }

    pub fn index(&self) -> &Index {
        &self.index
    }

    pub fn search(&self, query: &str) -> Vec<Hit> {
        self.search_with(query, LIMIT, MIN_CONFIDENCE).hits
    }

    fn rank(&self, query: &str, limit: usize) -> Option<Run> {
        let parsed = parse_query(query);
        if parsed.base.is_empty() {
            return None;
        }
        let query = Query::new(&self.index, &parsed);
        Ranker::new(&self.index, &query).rank(limit)
    }

    fn split_rerun(&self, trimmed: &str, limit: usize, results: Option<Run>) -> Option<Run> {
        // the whole name matched weakly, or confidently while no title equals it (its titles glued: "tensei shitara
        // slime datta ken (that time i got reincarnated as a slime)"): it may hold several alternative titles; a part
        // only wins by matching better, against a confident whole name only with every part naming that entry
        let weak = !results.as_ref().is_some_and(|run| run.confidence >= MIN_CONFIDENCE);
        let parts = match &results {
            Some(run) if !weak && run.top().candidate.title.score >= EXACT_SYNONYM => return results,
            _ => split_titles(trimmed),
        };
        if parts.len() <= 1 {
            return results;
        }
        let mut runs: Vec<Run> = parts.iter().filter_map(|part| self.rank(part, limit)).collect();
        let entry_of = |run: &Run| run.top().candidate.title.entry;
        let agreeing = |entry: u32| {
            runs.iter()
                .filter(|run| {
                    entry_of(run) == entry && run.top().candidate.title.score >= confidence::SPLIT_AGREEMENT_SCORE
                })
                .count() as f64
        };
        let support: Vec<f64> =
            runs.iter().map(|run| run.confidence + confidence::SPLIT_AGREEMENT * agreeing(entry_of(run))).collect();
        let mut best: Option<usize> = None;
        for (i, value) in support.iter().enumerate() {
            if best.is_none_or(|best| *value > support[best]) {
                best = Some(i);
            }
        }
        let Some(best) = best else { return results };
        let unanimous = runs.len() == parts.len() && runs.iter().all(|run| entry_of(run) == entry_of(&runs[best]));
        let chosen = &runs[best];
        let replaces = match &results {
            _ if weak => chosen.confidence > results.as_ref().map_or(0.0, |run| run.confidence),
            Some(run) => {
                unanimous && chosen.confidence >= MIN_CONFIDENCE && chosen.top().final_score > run.top().final_score
            }
            None => false,
        };
        if replaces { Some(runs.swap_remove(best)) } else { results }
    }

    // the last fallbacks, for query words one letter off a word of the top entry's titles (an uploader's "Koufu
    // Collector"): the query with those words spelt as the title does, ranked anew when that is confident (a misspelt
    // word scores low: "hosizora kiseki"); else the top entry counting them as found
    fn typo_rerun(&self, trimmed: &str, limit: usize, results: Option<Run>) -> Option<Run> {
        let Some(corrections) = results.as_ref().and_then(|run| run.typo_corrections.as_ref()) else {
            return results;
        };
        if corrections.is_empty() {
            return results;
        }
        let corrected: String = SPACES
            .split(trimmed)
            .into_iter()
            .map(|token| {
                let token = token.unwrap_or_default();
                corrections.get(&norm(token)).map_or(token, String::as_str)
            })
            .collect();
        match self.rank(&corrected, limit) {
            Some(rerun) if rerun.confidence >= MIN_CONFIDENCE => Some(rerun),
            _ => results,
        }
    }

    pub fn search_with(&self, query: &str, limit: usize, min_confidence: f64) -> Ranking {
        let trimmed = trim_query(query);
        let ranked_limit = limit.max(1);
        let results = self.rank(trimmed, ranked_limit);
        let results = self.split_rerun(trimmed, ranked_limit, results);
        let Some(mut run) = self.typo_rerun(trimmed, ranked_limit, results) else {
            return Ranking { hits: Vec::new(), confidence: None };
        };
        if let Some(typo) = run.typo_confidence
            && typo > run.confidence
        {
            run.confidence = typo;
        }
        // a name without letters ("2", all a scraper keeps of "乱马 1/2") names only an entry titled so ("86", "22/7"): a
        // partial match on its numbers is chance ("2 koi")
        let letterless = || !parse_query(trimmed).base.chars().any(char::is_alphabetic);
        let hits = if run.confidence >= min_confidence
            && (run.top().candidate.title.score >= EXACT_SYNONYM || !letterless())
        {
            run.results
                .iter()
                .take(limit)
                .map(|result| Hit {
                    id: self.index.entries()[result.candidate.title.entry as usize].id as u32,
                    score: result.final_score,
                })
                .collect()
        } else {
            Vec::new()
        };
        Ranking { hits, confidence: Some(run.confidence) }
    }
}
