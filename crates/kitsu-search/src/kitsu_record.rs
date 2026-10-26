use std::collections::HashSet;
use std::sync::LazyLock;

use fastjs_regex::Regex;
use indexmap::IndexMap;
use serde_json::Value;

use crate::records::TitleRecord;
use crate::rx;

static NUMBERS_ONLY: LazyLock<Regex> = LazyLock::new(|| rx(r"^[\d\s.,&+-]+(?:and[\d\s.,-]+)?$", "i"));

fn ascii_alnum_count(title: &str) -> usize {
    title.chars().filter(char::is_ascii_alphanumeric).count()
}

fn has_ascii_alnum(title: &str) -> bool {
    title.chars().any(|c| c.is_ascii_alphanumeric())
}

fn is_title_fragment(title: &str, pieces: &HashSet<&str>) -> bool {
    let trimmed = title.trim();
    pieces.contains(trimmed)
        && (trimmed.starts_with(char::is_lowercase) || ascii_alnum_count(title) <= 3 || NUMBERS_ONLY.is_match(trimmed))
}

fn strings(value: Option<&Value>) -> Vec<&str> {
    value.and_then(Value::as_array).map_or_else(Vec::new, |items| items.iter().filter_map(Value::as_str).collect())
}

fn id_of(value: Option<&Value>) -> Option<u64> {
    match value? {
        Value::String(text) => text.trim().parse().ok(),
        Value::Number(number) => number.as_u64(),
        _ => None,
    }
}

impl TitleRecord {
    pub fn from_kitsu(anime: &Value) -> Option<Self> {
        let id = id_of(anime.get("id"))?;
        let canonical = anime.get("canonicalTitle").and_then(Value::as_str).unwrap_or_default();
        let titles: IndexMap<String, String> = anime
            .get("titles")
            .and_then(Value::as_object)
            .map(|titles| {
                titles
                    .iter()
                    .filter_map(|(key, title)| Some((key.clone(), title.as_str()?.to_owned())))
                    .filter(|(_, title)| has_ascii_alnum(title))
                    .collect()
            })
            .unwrap_or_default();
        let abbreviated = strings(anime.get("abbreviatedTitles"));
        let pieces: HashSet<&str> = std::iter::once(canonical)
            .chain(titles.values().map(String::as_str))
            .chain(abbreviated.iter().copied())
            .filter(|title| !title.is_empty())
            .flat_map(|title| title.split(',').skip(1).map(str::trim))
            .collect();
        let abbreviated_titles = abbreviated
            .into_iter()
            .filter(|title| has_ascii_alnum(title) && !is_title_fragment(title, &pieces))
            .map(str::to_owned)
            .collect();
        Some(Self {
            id,
            canonical_title: canonical.to_owned(),
            titles,
            abbreviated_titles,
            subtype: anime.get("subtype").and_then(Value::as_str).unwrap_or_default().to_owned(),
            status: anime.get("status").and_then(Value::as_str).unwrap_or_default().to_owned(),
            start_date: anime.get("startDate").and_then(Value::as_str).map(str::to_owned),
            episode_count: anime.get("episodeCount").and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()),
            synonyms: Vec::new(),
        })
    }
}
