use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, OnceLock};

use fastjs_regex::{Regex, utf16_len};
use indexmap::{IndexMap, IndexSet};

use crate::math::ln;
use crate::norm::norm;
use crate::query::{
    Marker, MovieIndex, Season, compact, extra_number_of, has_ending_extra_word, markers_of, movie_prefix_of,
    punct_key, season_of, seasonless_title, title_base_of, title_movie_index_of, title_year_of, without_movie_words,
};
use crate::records::{Aliases, ImdbMapping, ImdbMappings, TitleRecord};
use crate::unicode::{NODE_ONLY_LETTERS, class_members};

const SEARCHED_STATUSES: [&str; 3] = ["finished", "current", "upcoming"];
const SPLIT_SEASON_EPISODES: std::ops::RangeInclusive<u32> = 24..=28;
const UNKNOWN_SEASON: Season = 99;

fn rx(source: &str, flags: &str) -> Regex {
    Regex::new(source, flags).unwrap_or_else(|error| panic!("invalid pattern {source}: {error}"))
}

static DOTTED: LazyLock<Regex> = LazyLock::new(|| {
    let letter = format!(r"[\p{{L}}{}]", class_members(NODE_ONLY_LETTERS));
    rx(&format!(r"\b(?:{letter}\.){{2,}}(?:{letter}\b)?"), "gu")
});
static DOT: LazyLock<Regex> = LazyLock::new(|| rx(r"\.", "g"));
static NUMBERED_SYNONYM: LazyLock<Regex> =
    LazyLock::new(|| rx(r"\b(?:episode|ep|recap|movie|film|pelicula|filme|part|vol|volume) \d", ""));
static PARENTHESISED: LazyLock<Regex> = LazyLock::new(|| rx(r"\([^)]*\)", "g"));
static SEQUEL_PUNCTUATION: LazyLock<Regex> = LazyLock::new(|| rx("[!+◎☆★']$", ""));
static PART: LazyLock<Regex> = LazyLock::new(|| rx(r"\b(?:part|cou?r) (\d)\b", ""));
static PLAIN_BASE: LazyLock<Regex> = LazyLock::new(|| rx("^[a-z0-9 ]+$", ""));

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Subtype {
    Tv,
    Ova,
    Ona,
    Movie,
    Special,
}

impl Subtype {
    fn searched(subtype: &str) -> Option<Self> {
        match subtype {
            "TV" => Some(Self::Tv),
            "OVA" => Some(Self::Ova),
            "ONA" => Some(Self::Ona),
            "movie" => Some(Self::Movie),
            "special" => Some(Self::Special),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tv => "TV",
            Self::Ova => "OVA",
            Self::Ona => "ONA",
            Self::Movie => "movie",
            Self::Special => "special",
        }
    }

    pub fn is_series(self) -> bool {
        matches!(self, Self::Tv | Self::Ona)
    }

    pub fn is_extra(self) -> bool {
        matches!(self, Self::Ova | Self::Special)
    }
}

fn is_series(subtype: &str) -> bool {
    Subtype::searched(subtype).is_some_and(Subtype::is_series)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VariantSource {
    Own { abbreviated: bool },
    Synonym { stripped_season: Option<Season> },
    Mapping,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Variant {
    pub title: String,
    pub base: String,
    pub movie_base: Option<String>,
    pub title_year: Option<u32>,
    pub movie: Option<u32>,
    pub season: Option<Season>,
    pub markers: Vec<Marker>,
    pub extra_number: Option<String>,
    pub source: VariantSource,
}

impl Variant {
    fn new(title: String, subtype: Subtype, source: VariantSource) -> Self {
        let movie = title_movie_index_of(&title, Some(subtype.as_str())).map(|index| match index {
            MovieIndex::Number(number) => number,
            MovieIndex::Text(digits) => digits.parse().unwrap_or_else(|error| panic!("movie {digits}: {error}")),
        });
        let base = title_base_of(&title, Some(subtype.as_str()));
        let movie_base = Some(without_movie_words(&base)).filter(|movie_base| *movie_base != base);
        let mut variant = Self {
            title_year: title_year_of(&title),
            movie,
            season: if movie.is_none() { season_of(&title) } else { None },
            markers: markers_of(&title),
            extra_number: extra_number_of(&title).map(str::to_owned),
            base,
            movie_base,
            title,
            source,
        };
        if let VariantSource::Synonym { .. } = source {
            variant.source = VariantSource::Synonym { stripped_season: season_of(&variant.title) };
            variant.season = None;
            variant.movie = None;
            variant.extra_number = None;
            variant.markers = Vec::new();
        }
        variant
    }

    pub fn from_synonym(&self) -> bool {
        matches!(self.source, VariantSource::Synonym { .. })
    }

    pub fn from_mapping(&self) -> bool {
        self.source == VariantSource::Mapping
    }

    pub fn abbreviated(&self) -> bool {
        self.source == VariantSource::Own { abbreviated: true }
    }

    pub fn stripped_season(&self) -> Option<Season> {
        match self.source {
            VariantSource::Synonym { stripped_season } => stripped_season,
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AbsoluteEnd {
    Episode(u32),
    Open,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AbsoluteEpisodes {
    pub from: u32,
    pub to: AbsoluteEnd,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub id: u64,
    pub canonical_title: String,
    pub main_titles: Vec<String>,
    pub subtype: Subtype,
    pub start_date: Option<String>,
    pub year: Option<u32>,
    pub episode_count: Option<u32>,
    pub titles: Vec<Variant>,
    pub sequel_keys: Vec<String>,
    pub has_extra_word: bool,
    pub title_season: Option<Season>,
    pub part_number: Option<u32>,
    pub season: Option<Season>,
    pub title_years: Vec<u32>,
    pub movie: Option<u32>,
    pub markers: Vec<Marker>,
    pub franchise: Option<String>,
    pub imdb_season: Option<u32>,
    pub imdb_from_episode: Option<u32>,
    pub derived_movie: Option<u32>,
    pub group_key: Option<String>,
    pub absolute: Option<AbsoluteEpisodes>,
    pub split_season: Option<Season>,
}

impl Entry {
    pub fn own_titles(&self) -> impl Iterator<Item = &str> {
        self.titles.iter().filter(|variant| !variant.from_synonym()).map(|variant| variant.title.as_str())
    }

    pub fn franchise(&self) -> Option<&str> {
        self.franchise.as_deref().filter(|franchise| !franchise.is_empty())
    }
}

pub type Trigram = [u16; 3];

pub struct Index {
    entries: Vec<Entry>,
    season_groups: IndexMap<String, Vec<u32>>,
    words: IndexSet<String>,
    postings: Vec<Vec<u32>>,
    idf: Vec<f64>,
    unseen_idf: f64,
    trigrams: OnceLock<HashMap<Trigram, Vec<u32>>>,
}

fn truthy(text: Option<&str>) -> Option<&str> {
    text.filter(|text| !text.is_empty())
}

fn later_season(mapping: &ImdbMapping) -> bool {
    let season = mapping.from_season.unwrap_or(1);
    truthy(mapping.imdb_id.as_deref()).is_some() && season >= 1 && (season > 1 || mapping.from_episode.unwrap_or(1) > 1)
}

fn first_of_imdb(records: &[TitleRecord], mapping: &ImdbMappings) -> HashMap<String, Vec<String>> {
    let mut first = HashMap::new();
    for record in records {
        let Some(row) = mapping.get(&record.id) else { continue };
        let Some(imdb_id) = truthy(row.imdb_id.as_deref()) else { continue };
        if row.from_season.unwrap_or(1) == 1
            && row.from_episode.unwrap_or(1) == 1
            && is_series(&record.subtype)
            && !first.contains_key(imdb_id)
        {
            let titles = [Some(record.canonical_title.as_str()), record.titles.get("en_jp").map(String::as_str)];
            first.insert(imdb_id.to_owned(), titles.into_iter().filter_map(truthy).map(str::to_owned).collect());
        }
    }
    first
}

fn with_aliases<'r>(record: &'r TitleRecord, aliases: &Aliases) -> Cow<'r, IndexMap<String, String>> {
    let Some(list) = aliases.get(&record.id.to_string()) else { return Cow::Borrowed(&record.titles) };
    let mut titles = record.titles.clone();
    for (i, alias) in list.iter().enumerate() {
        titles.insert(format!("alias{i}"), alias.clone());
    }
    Cow::Owned(titles)
}

fn undotted(title: &str) -> String {
    DOTTED.replace_with(title, |m| DOT.replace(&title[m.range()], "").into_owned()).into_owned()
}

fn to_entry(
    record: &TitleRecord,
    titles: &IndexMap<String, String>,
    subtype: Subtype,
    mapping: Option<&ImdbMapping>,
    franchise_titles: &[String],
) -> Entry {
    let main_titles: Vec<String> = std::iter::once(&record.canonical_title)
        .chain(titles.values())
        .filter(|title| !title.is_empty())
        .cloned()
        .collect();
    let with_undotted = |title: &String| [title.clone(), undotted(title)];
    let own: IndexSet<String> = main_titles
        .iter()
        .chain(&record.abbreviated_titles)
        .flat_map(with_undotted)
        .map(|title| norm(&title))
        .filter(|title| !title.is_empty())
        .collect();
    let main_set: HashSet<String> = main_titles.iter().flat_map(with_undotted).map(|title| norm(&title)).collect();
    let mut variants: Vec<Variant> = own
        .iter()
        .map(|title| {
            Variant::new(title.clone(), subtype, VariantSource::Own { abbreviated: !main_set.contains(title) })
        })
        .collect();
    let synonyms: IndexSet<String> =
        record.synonyms.iter().map(|title| norm(title)).filter(|title| !title.is_empty()).collect();
    variants.extend(
        synonyms
            .into_iter()
            .filter(|title| !own.contains(title) && !NUMBERED_SYNONYM.is_match(title))
            .map(|title| Variant::new(title, subtype, VariantSource::Synonym { stripped_season: None })),
    );
    if let Some(row) = mapping
        && let Some(title) = truthy(row.title.as_deref())
        && truthy(row.imdb_id.as_deref()).is_some()
        && subtype.is_series()
    {
        variants.extend(
            std::iter::once(title)
                .chain(franchise_titles.iter().map(String::as_str))
                .map(norm)
                .filter(|title| !own.contains(title))
                .map(|title| Variant::new(title, subtype, VariantSource::Mapping)),
        );
    }
    let kitsu_titles = || variants.iter().filter(|variant| matches!(variant.source, VariantSource::Own { .. }));
    let not_synonyms = || variants.iter().filter(|variant| !variant.from_synonym());
    let title_season = kitsu_titles().find_map(|variant| variant.season.filter(|season| *season != 0));
    let mapped_season = mapping.and_then(|row| match (row.from_season, row.from_episode) {
        (Some(season), Some(1)) if season != 0 => Some(season),
        _ => None,
    });
    let markers: IndexSet<Marker> = kitsu_titles().flat_map(|variant| variant.markers.iter().copied()).collect();
    let extra_word_titles = [
        Some(record.canonical_title.as_str()),
        titles.get("en").map(String::as_str),
        titles.get("en_jp").map(String::as_str),
    ];
    Entry {
        id: record.id,
        canonical_title: record.canonical_title.clone(),
        subtype,
        start_date: record.start_date.clone(),
        year: record.start_date.as_deref().and_then(|date| date.get(..4)).and_then(|year| year.parse().ok()),
        episode_count: record.episode_count,
        sequel_keys: main_titles
            .iter()
            .map(|title| punct_key(&PARENTHESISED.replace(title, " ")))
            .filter(|key| SEQUEL_PUNCTUATION.is_match(key) && utf16_len(key) > 4)
            .collect(),
        has_extra_word: extra_word_titles
            .into_iter()
            .filter_map(truthy)
            .any(|title| has_ending_extra_word(&norm(title))),
        title_season,
        part_number: not_synonyms().find_map(|variant| {
            PART.find(&variant.title)
                .and_then(|m| m.group(1))
                .and_then(|digits| variant.title[digits].parse().ok())
                .filter(|part| *part != 0)
        }),
        season: title_season.or(mapped_season),
        title_years: not_synonyms().filter_map(|variant| variant.title_year).collect(),
        movie: variants.iter().find_map(|variant| variant.movie),
        markers: markers.into_iter().collect(),
        franchise: mapping.and_then(|row| row.imdb_id.clone()),
        imdb_season: mapping.and_then(|row| row.from_season),
        imdb_from_episode: mapping.and_then(|row| row.from_episode),
        derived_movie: None,
        group_key: None,
        absolute: None,
        split_season: None,
        main_titles,
        titles: variants,
    }
}

fn number_unnumbered_movies(entries: &mut [Entry]) {
    let mut groups: IndexMap<String, Vec<usize>> = IndexMap::new();
    for (i, entry) in entries.iter().enumerate().filter(|(_, entry)| entry.subtype == Subtype::Movie) {
        groups.entry(movie_prefix_of(&entry.canonical_title)).or_default().push(i);
    }
    for list in groups.into_values() {
        if list.iter().any(|&i| entries[i].movie.is_some()) {
            continue;
        }
        let mut dated: Vec<usize> =
            list.into_iter().filter(|&i| truthy(entries[i].start_date.as_deref()).is_some()).collect();
        dated.sort_by(|&a, &b| entries[a].start_date.cmp(&entries[b].start_date));
        for (number, i) in (1..).zip(dated) {
            entries[i].derived_movie = Some(number);
        }
    }
}

fn group_seasons(entries: &mut [Entry]) -> IndexMap<String, Vec<u32>> {
    let mut groups: IndexMap<String, Vec<u32>> = IndexMap::new();
    for (i, entry) in entries.iter_mut().enumerate() {
        if entry.subtype.is_series() && !entry.canonical_title.is_empty() {
            let key = seasonless_title(&entry.canonical_title);
            groups.entry(key.clone()).or_default().push(i as u32);
            entry.group_key = Some(key);
        }
    }
    groups
}

fn franchises(entries: &[Entry], member: impl Fn(&Entry) -> bool) -> IndexMap<&str, Vec<usize>> {
    let mut groups: IndexMap<&str, Vec<usize>> = IndexMap::new();
    for (i, entry) in entries.iter().enumerate() {
        if let Some(franchise) = entry.franchise()
            && entry.subtype.is_series()
            && member(entry)
        {
            groups.entry(franchise).or_default().push(i);
        }
    }
    groups
}

fn lay_out_episodes(entries: &mut [Entry]) {
    let groups: Vec<Vec<usize>> =
        franchises(entries, |entry| truthy(entry.start_date.as_deref()).is_some()).into_values().collect();
    for mut list in groups {
        list.sort_by(|&a, &b| {
            let (a, b) = (&entries[a], &entries[b]);
            a.imdb_season
                .unwrap_or(UNKNOWN_SEASON)
                .cmp(&b.imdb_season.unwrap_or(UNKNOWN_SEASON))
                .then_with(|| a.start_date.cmp(&b.start_date))
        });
        let mut from = 1;
        for i in list {
            let entry = &mut entries[i];
            let Some(count) = entry.episode_count.filter(|count| *count != 0) else {
                entry.absolute = Some(AbsoluteEpisodes { from, to: AbsoluteEnd::Open });
                break;
            };
            let to = from + count - 1;
            entry.absolute = Some(AbsoluteEpisodes { from, to: AbsoluteEnd::Episode(to) });
            from = to + 1;
        }
    }
}

fn mark_split_seasons(entries: &mut [Entry]) {
    let mut split = Vec::new();
    for list in franchises(entries, |entry| entry.imdb_season.is_some_and(|season| season > 0)).into_values() {
        for &i in &list {
            let entry = &entries[i];
            let Some(season) = entry.imdb_season.filter(|season| *season >= 2) else { continue };
            if entry.imdb_from_episode != Some(1) || season_of(&norm(&entry.canonical_title)) == Some(season) {
                continue;
            }
            let mut previous = list.iter().map(|&j| &entries[j]).filter(|other| other.imdb_season == Some(season - 1));
            if let (Some(only), None) = (previous.next(), previous.next())
                && only.episode_count.is_some_and(|count| SPLIT_SEASON_EPISODES.contains(&count))
            {
                split.push((i, season + 1));
            }
        }
    }
    for (i, season) in split {
        entries[i].split_season = Some(season);
    }
}

pub fn trigrams_of(key: &str) -> IndexSet<Trigram> {
    let units: Vec<u16> = key.encode_utf16().collect();
    units.windows(3).map(|gram| [gram[0], gram[1], gram[2]]).collect()
}

impl Index {
    pub fn build(records: &[TitleRecord], mapping: &ImdbMappings, aliases: &Aliases) -> Self {
        let first = first_of_imdb(records, mapping);
        let mut entries: Vec<Entry> = records
            .iter()
            .filter(|record| SEARCHED_STATUSES.contains(&record.status.as_str()))
            .filter_map(|record| {
                let subtype = Subtype::searched(&record.subtype)?;
                let row = mapping.get(&record.id);
                let franchise_titles = row
                    .filter(|row| later_season(row))
                    .and_then(|row| row.imdb_id.as_ref().and_then(|imdb_id| first.get(imdb_id)))
                    .map_or(&[][..], Vec::as_slice);
                Some(to_entry(record, &with_aliases(record, aliases), subtype, row, franchise_titles))
            })
            .collect();
        number_unnumbered_movies(&mut entries);
        let season_groups = group_seasons(&mut entries);
        lay_out_episodes(&mut entries);
        mark_split_seasons(&mut entries);

        let mut words = IndexSet::new();
        let mut postings: Vec<Vec<u32>> = Vec::new();
        for (i, entry) in entries.iter().enumerate() {
            let entry_words: IndexSet<&str> =
                entry.titles.iter().flat_map(|variant| variant.title.split(' ')).collect();
            for word in entry_words {
                let (id, added) = match words.get_index_of(word) {
                    Some(id) => (id, false),
                    None => (words.insert_full(word.to_owned()).0, true),
                };
                if added {
                    postings.push(Vec::new());
                }
                postings[id].push(i as u32);
            }
        }
        let idf_of = |count: usize| ln(entries.len() as f64 / (1 + count) as f64);
        let idf = postings.iter().map(|posting| idf_of(posting.len())).collect();
        let unseen_idf = idf_of(0);
        Self { entries, season_groups, words, postings, idf, unseen_idf, trigrams: OnceLock::new() }
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn variants(&self) -> usize {
        self.entries.iter().map(|entry| entry.titles.len()).sum()
    }

    pub fn season_group(&self, key: &str) -> &[u32] {
        self.season_groups.get(key).map_or(&[], Vec::as_slice)
    }

    pub fn season_groups(&self) -> &IndexMap<String, Vec<u32>> {
        &self.season_groups
    }

    pub fn word_count(&self) -> usize {
        self.words.len()
    }

    pub fn word_id(&self, word: &str) -> Option<u32> {
        self.words.get_index_of(word).map(|id| id as u32)
    }

    pub fn word(&self, id: u32) -> &str {
        &self.words[id as usize]
    }

    pub fn posting(&self, id: u32) -> &[u32] {
        &self.postings[id as usize]
    }

    pub fn entries_with(&self, word: &str) -> &[u32] {
        self.word_id(word).map_or(&[], |id| self.posting(id))
    }

    pub fn entry_count(&self, word: &str) -> usize {
        self.entries_with(word).len()
    }

    pub fn idf(&self, word: &str) -> f64 {
        self.word_id(word).map_or(self.unseen_idf, |id| self.idf[id as usize])
    }

    pub fn trigrams_built(&self) -> bool {
        self.trigrams.get().is_some()
    }

    pub fn trigrams(&self) -> &HashMap<Trigram, Vec<u32>> {
        self.trigrams.get_or_init(|| {
            let mut trigrams: HashMap<Trigram, Vec<u32>> = HashMap::new();
            for (i, entry) in self.entries.iter().enumerate() {
                let grams: IndexSet<Trigram> = entry
                    .titles
                    .iter()
                    .filter(|variant| !variant.from_synonym() || PLAIN_BASE.is_match(&variant.base))
                    .flat_map(|variant| trigrams_of(&compact(&variant.base)))
                    .collect();
                for gram in grams {
                    trigrams.entry(gram).or_default().push(i as u32);
                }
            }
            trigrams
        })
    }

    pub fn entries_with_trigram(&self, gram: Trigram) -> &[u32] {
        self.trigrams().get(&gram).map_or(&[], Vec::as_slice)
    }
}
