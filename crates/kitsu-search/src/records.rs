use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use serde::Deserialize;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("reading records: {0}")]
    Io(#[from] io::Error),
    #[error("record on line {line}: {source}")]
    Json { line: usize, source: serde_json::Error },
    #[error("opening {path}: {source}")]
    Open { path: PathBuf, source: io::Error },
    #[error("parsing {path}: {source}")]
    Parse { path: PathBuf, source: serde_json::Error },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TitleRecord {
    pub id: u64,
    pub canonical_title: String,
    pub titles: IndexMap<String, String>,
    pub abbreviated_titles: Vec<String>,
    pub subtype: String,
    pub status: String,
    pub start_date: Option<String>,
    pub episode_count: Option<u32>,
    pub synonyms: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ImdbMapping {
    pub kitsu_id: u64,
    pub imdb_id: Option<String>,
    pub title: Option<String>,
    #[serde(rename = "fromSeason")]
    pub from_season: Option<u32>,
    #[serde(rename = "fromEpisode")]
    pub from_episode: Option<u32>,
}

pub type ImdbMappings = HashMap<u64, ImdbMapping>;

pub type Aliases = HashMap<String, Vec<String>>;

pub struct Sources {
    pub records: Vec<TitleRecord>,
    pub mapping: ImdbMappings,
    pub aliases: Aliases,
}

impl Sources {
    pub fn read(titles: &Path, aliases: &Path, mapping: &Path) -> Result<Self, Error> {
        Ok(Self {
            records: read_records(open(titles)?)?,
            mapping: read_imdb_mapping(open(mapping)?).map_err(|source| parse_error(mapping, source))?,
            aliases: read_aliases(open(aliases)?).map_err(|source| parse_error(aliases, source))?,
        })
    }
}

fn open(path: &Path) -> Result<BufReader<File>, Error> {
    File::open(path).map(BufReader::new).map_err(|source| Error::Open { path: path.to_owned(), source })
}

fn parse_error(path: &Path, source: serde_json::Error) -> Error {
    Error::Parse { path: path.to_owned(), source }
}

pub fn read_records(reader: impl Read) -> Result<Vec<TitleRecord>, Error> {
    let mut records = Vec::new();
    for (index, line) in BufReader::new(reader).lines().enumerate() {
        let line = line?;
        if line.is_empty() {
            continue;
        }
        let record = serde_json::from_str(&line).map_err(|source| Error::Json { line: index + 1, source })?;
        records.push(record);
    }
    Ok(records)
}

pub fn read_imdb_mapping(reader: impl Read) -> Result<ImdbMappings, serde_json::Error> {
    let rows: Vec<ImdbMapping> = serde_json::from_reader(reader)?;
    Ok(rows.into_iter().map(|row| (row.kitsu_id, row)).collect())
}

pub fn read_aliases(reader: impl Read) -> Result<Aliases, serde_json::Error> {
    serde_json::from_reader(reader)
}
