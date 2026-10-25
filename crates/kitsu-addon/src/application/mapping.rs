use std::{
    collections::HashMap,
    fs::File,
    io::{BufReader, Read},
    path::Path,
};

use kitsu_search::records::TitleRecord;
use serde::Deserialize;

use super::jsvalue::JsValue;
use crate::infrastructure::MappingInfo;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MappingRow {
    #[serde(rename = "kitsu_id")]
    pub kitsu_id: u64,
    #[serde(rename = "imdb_id")]
    pub imdb_id: Option<String>,
    pub title: Option<String>,
    pub from_season: Option<i64>,
    pub from_episode: Option<i64>,
    pub non_imdb_episodes: Option<Vec<i64>>,
    pub imdb_episodes: Option<Vec<Option<i64>>>,
    #[serde(rename = "tmdb_id")]
    pub tmdb_id: Option<String>,
    #[serde(rename = "tvdb_id")]
    pub tvdb_id: Option<String>,
    pub fanart_logo_id: Option<String>,
}

impl MappingRow {
    pub fn start_season(&self) -> i64 {
        self.from_season.unwrap_or(1)
    }

    pub fn start_episode(&self) -> i64 {
        self.from_episode.unwrap_or(1)
    }

    pub fn has_imdb_id(&self) -> bool {
        self.imdb_id.as_ref().is_some_and(|id| !id.is_empty())
    }

    pub fn mapping_info(&self) -> MappingInfo {
        MappingInfo {
            imdb_id: self.imdb_id.clone(),
            tmdb_id: self.tmdb_id.clone(),
            tvdb_id: self.tvdb_id.clone(),
            fanart_logo_id: self.fanart_logo_id.clone(),
            from_season: self.from_season,
        }
    }

    pub fn is_non_imdb_episode(&self, kitsu_episode: &JsValue) -> bool {
        self.non_imdb_episodes.as_ref().is_some_and(|episodes| {
            episodes.iter().any(|episode| JsValue::Number(*episode as f64).same_value_zero(kitsu_episode))
        })
    }

    pub fn imdb_episode(&self, kitsu_episode: &JsValue) -> Option<Option<i64>> {
        let episodes = self.imdb_episodes.as_ref()?;
        let index = kitsu_episode.to_number() - 1.0;
        if index.fract() != 0.0 || index < 0.0 || index >= episodes.len() as f64 {
            return Some(None);
        }
        Some(episodes[index as usize])
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    pub kitsu_id: String,
    pub kitsu_episode: i64,
}

#[derive(Debug, Default)]
pub struct Mapping {
    rows: HashMap<u64, MappingRow>,
    by_imdb: HashMap<String, Vec<MappingRow>>,
    listed: HashMap<String, HashMap<String, Listed>>,
    caps: HashMap<u64, u32>,
}

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("opening {path}: {source}")]
    Open { path: String, source: std::io::Error },
    #[error("parsing the imdb mapping: {0}")]
    Parse(#[from] serde_json::Error),
}

impl Mapping {
    pub fn load(path: &Path, records: &[TitleRecord]) -> Result<Self, LoadError> {
        let file = File::open(path).map_err(|source| LoadError::Open { path: path.display().to_string(), source })?;
        Self::read(BufReader::new(file), records)
    }

    pub fn read(reader: impl Read, records: &[TitleRecord]) -> Result<Self, LoadError> {
        let rows: Vec<MappingRow> = serde_json::from_reader(reader)?;
        Ok(Self::new(rows, records))
    }

    pub fn new(rows: Vec<MappingRow>, records: &[TitleRecord]) -> Self {
        let rows: HashMap<u64, MappingRow> = rows.into_iter().map(|row| (row.kitsu_id, row)).collect();
        let mut by_kitsu_id: Vec<&MappingRow> = rows.values().filter(|row| row.has_imdb_id()).collect();
        by_kitsu_id.sort_by_key(|row| row.kitsu_id);
        let mut by_imdb: HashMap<String, Vec<MappingRow>> = HashMap::new();
        for row in by_kitsu_id {
            by_imdb.entry(row.imdb_id.clone().unwrap_or_default()).or_default().push(row.clone());
        }
        for entries in by_imdb.values_mut() {
            entries.sort_by_key(|entry| (entry.start_season(), entry.start_episode()));
        }
        let listed = by_imdb
            .iter()
            .filter_map(|(imdb_id, entries)| {
                let mut listed = HashMap::new();
                for entry in entries {
                    for (index, episode) in entry.imdb_episodes.iter().flatten().enumerate() {
                        if let Some(episode) = episode {
                            listed.insert(
                                format!("{}:{episode}", entry.start_season()),
                                Listed { kitsu_id: entry.kitsu_id.to_string(), kitsu_episode: index as i64 + 1 },
                            );
                        }
                    }
                }
                (!listed.is_empty()).then(|| (imdb_id.clone(), listed))
            })
            .collect();
        let caps = records
            .iter()
            .filter(|record| rows.contains_key(&record.id) && record.status == "finished")
            .filter_map(|record| record.episode_count.filter(|count| *count > 0).map(|count| (record.id, count)))
            .collect();
        Self { rows, by_imdb, listed, caps }
    }

    pub fn imdb_mapping(&self, kitsu_id: &str) -> Option<&MappingRow> {
        object_key(kitsu_id).and_then(|id| self.rows.get(&id))
    }

    pub fn imdb_mapping_of(&self, kitsu_id: &JsValue) -> Option<&MappingRow> {
        self.imdb_mapping(&kitsu_id.text())
    }

    pub fn mapping_info(&self, kitsu_id: &str) -> Option<MappingInfo> {
        self.imdb_mapping(kitsu_id).map(MappingRow::mapping_info)
    }

    pub fn has_imdb_mapping(&self, id: &str) -> bool {
        if id.starts_with("tt") { self.by_imdb.contains_key(id) } else { self.imdb_mapping(id).is_some() }
    }

    pub fn kitsu_ids_for_imdb(&self, imdb_id: &str) -> Vec<String> {
        let entries = self.entries(imdb_id);
        entries
            .iter()
            .filter(|entry| entry.start_season() != 0)
            .chain(entries.iter().filter(|entry| entry.start_season() == 0))
            .map(|entry| entry.kitsu_id.to_string())
            .collect()
    }

    pub fn episode_cap(&self, kitsu_id: u64) -> Option<u32> {
        self.caps.get(&kitsu_id).copied()
    }

    pub fn entries(&self, imdb_id: &str) -> &[MappingRow] {
        self.by_imdb.get(imdb_id).map_or(&[], Vec::as_slice)
    }

    pub fn listed(&self, imdb_id: &str, season: &str, episode: &str) -> Option<&Listed> {
        self.listed.get(imdb_id)?.get(&format!("{season}:{episode}"))
    }
}

fn object_key(text: &str) -> Option<u64> {
    let canonical = text == "0" || (text.starts_with(|c: char| ('1'..='9').contains(&c)));
    if !canonical || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

#[cfg(test)]
mod tests;
