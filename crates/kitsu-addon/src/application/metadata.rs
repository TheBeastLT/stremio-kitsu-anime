use std::{
    cmp::Ordering,
    collections::HashSet,
    sync::{Arc, LazyLock},
};

use async_trait::async_trait;
use fastjs_regex::Regex;
use serde_json::{Map, Value};
use tokio::time::Instant;

use super::{
    jsvalue::{JsObject, JsValue, TypeError, date},
    mapping::Mapping,
};
use crate::infrastructure::{Images, kitsu::jsonapi::encode_uri_component};

const ALLOWED_RELATIONSHIPS: [&str; 2] = ["prequel", "sequel"];
const SINGLE_VIDEO_SUBTYPES: [&str; 4] = ["movie", "special", "OVA", "ONA"];
const CATALOG_META_FIELDS: [&str; 17] = [
    "id",
    "kitsu_id",
    "imdb_id",
    "type",
    "animeType",
    "name",
    "aliases",
    "description",
    "releaseInfo",
    "runtime",
    "imdbRating",
    "genres",
    "logo",
    "poster",
    "background",
    "trailers",
    "links",
];

static LEADING_DIGITS: LazyLock<Regex> = LazyLock::new(|| js_regex(r"^\d+"));
static DESCRIPTION_TAIL: LazyLock<Regex> =
    LazyLock::new(|| js_regex(r"\n+(?:[([].+[)\]\n]|Source:.*)?(?:\n+Note(.|\n)+)?$"));

fn js_regex(source: &str) -> Regex {
    Regex::new(source, "").expect("valid regex")
}

#[async_trait]
pub trait ImageLookup: Send + Sync {
    async fn images(&self, kitsu_id: &str) -> Images;
}

pub struct NoImages;

#[async_trait]
impl ImageLookup for NoImages {
    async fn images(&self, _kitsu_id: &str) -> Images {
        Images::default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntryOptions {
    pub images: bool,
    pub relations: bool,
    pub deadline: Option<Instant>,
}

impl EntryOptions {
    pub fn within(deadline: Instant) -> Self {
        Self { deadline: Some(deadline), ..Self::default() }
    }
}

impl Default for EntryOptions {
    fn default() -> Self {
        Self { images: true, relations: false, deadline: None }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenreUrl(String);

impl GenreUrl {
    pub fn new(addon_url: &str) -> Self {
        Self(format!(
            "stremio:///discover/{}%2Fmanifest.json/anime/kitsu-anime-popular?genre=",
            encode_uri_component(addon_url)
        ))
    }

    pub fn url(&self, genre: &str) -> String {
        format!("{}{genre}", self.0)
    }
}

pub struct Metadata {
    mapping: Arc<Mapping>,
    images: Arc<dyn ImageLookup>,
    genre_url: GenreUrl,
}

impl Metadata {
    pub fn new(mapping: Arc<Mapping>, images: Arc<dyn ImageLookup>, genre_url: GenreUrl) -> Self {
        Self { mapping, images, genre_url }
    }

    pub fn mapping(&self) -> &Mapping {
        &self.mapping
    }

    pub fn genre_url(&self) -> &GenreUrl {
        &self.genre_url
    }

    pub async fn to_entry_meta(&self, anime: &Value, options: EntryOptions) -> Result<JsObject, TypeError> {
        let anime = JsValue::from(anime);
        let id = anime.get("id").text();
        let images = match (options.images, options.deadline) {
            (false, _) => Images::default(),
            (true, None) => self.images.images(&id).await,
            (true, Some(deadline)) => {
                tokio::time::timeout_at(deadline, self.images.images(&id)).await.unwrap_or_else(|_| {
                    tracing::warn!("Skipped the images of {id} at the meta deadline");
                    Images::default()
                })
            }
        };
        stremio_meta(&anime, &images, options.relations, &self.genre_url)
    }

    pub async fn to_catalog_meta(&self, anime: &Value) -> Result<Value, TypeError> {
        let meta = self.to_entry_meta(anime, EntryOptions::default()).await?;
        Ok(self.catalog_fields(&JsValue::Object(meta).to_json()))
    }

    pub fn catalog_fields(&self, meta: &Value) -> Value {
        catalog_fields(meta, &self.mapping)
    }
}

pub fn catalog_fields(meta: &Value, mapping: &Mapping) -> Value {
    let mut catalog = Map::new();
    for name in CATALOG_META_FIELDS {
        let value = match meta.get(name) {
            Some(imdb_id) if name == "imdb_id" && JsValue::from(imdb_id).truthy() => Some(imdb_id.clone()),
            _ if name == "imdb_id" => {
                let kitsu_id = meta.get("kitsu_id").map_or(JsValue::Undefined, JsValue::from);
                mapping.imdb_mapping_of(&kitsu_id).and_then(|row| row.imdb_id.clone()).map(Value::String)
            }
            value => value.cloned(),
        };
        if let Some(value) = value {
            catalog.insert(name.to_owned(), value);
        }
    }
    Value::Object(catalog)
}

fn stremio_meta(
    anime: &JsValue,
    images: &Images,
    relations: bool,
    genre_url: &GenreUrl,
) -> Result<JsObject, TypeError> {
    let kind = if anime.get("subtype").is_str("movie") { "movie" } else { "series" };
    let release_info = release_info(anime)?;
    let genres = genre_names(anime)?;
    let (videos, behavior_hints) = videos(anime)?;
    let aliases = aliases(anime.get("titles"), anime.get("abbreviatedTitles"))?;
    let id = anime.get("id");
    let fanart = |field: &Option<String>| field.clone().map_or(JsValue::Undefined, JsValue::String);
    let cover = anime.get("coverImage");
    let rating = rounded_rating(anime.get("averageRating"));
    let runtime = match anime.get("episodeLength") {
        length if length.is_integer() => JsValue::String(format!("{} min", length.text())),
        _ => JsValue::Null,
    };
    let youtube = anime.get("youtubeVideoId");
    let trailers = if youtube.truthy() {
        JsValue::Array(vec![object([("source", youtube.clone()), ("type", "Trailer".into())])])
    } else {
        JsValue::Array(Vec::new())
    };
    let links = kitsu_links(anime, kind, &rating, genre_url)?;
    let kitsu_relations = if relations { kitsu_relations(anime)? } else { JsValue::Undefined };
    let fields = [
        ("id", JsValue::String(format!("kitsu:{}", id.text()))),
        ("kitsu_id", id.clone()),
        ("type", kind.into()),
        ("animeType", anime.get("subtype").clone()),
        ("name", anime.get("canonicalTitle").clone()),
        ("slug", anime.get("slug").clone()),
        ("aliases", aliases),
        ("genres", genres.unwrap_or_else(|| JsValue::Array(Vec::new()))),
        ("logo", fanart(&images.logo)),
        ("poster", anime.get("posterImage").get("medium").or(&fanart(&images.poster))),
        (
            "background",
            fanart(&images.background).or(&if cover.truthy() { cover.get("original").clone() } else { cover.clone() }),
        ),
        ("description", clean_description(anime.get("synopsis"))?),
        ("releaseInfo", release_info.clone()),
        ("year", release_info),
        ("imdbRating", if rating.is_nullish() { JsValue::Undefined } else { JsValue::String(rating.text()) }),
        ("userCount", anime.get("userCount").clone()),
        ("status", anime.get("status").clone()),
        ("runtime", runtime),
        ("trailers", trailers),
        ("videos", videos),
        ("links", links),
        ("kitsuRelations", kitsu_relations),
        ("behaviorHints", behavior_hints),
    ];
    Ok(fields
        .into_iter()
        .filter(|(_, value)| !value.is_nullish())
        .map(|(key, value)| (key.to_owned(), value))
        .collect())
}

pub fn object<const N: usize>(fields: [(&str, JsValue); N]) -> JsValue {
    JsValue::Object(fields.into_iter().map(|(key, value)| (key.to_owned(), value)).collect())
}

fn leading_digits(value: &JsValue) -> Result<String, TypeError> {
    let text = value.as_str().ok_or_else(|| TypeError(format!("{} .match is not a function", value.text())))?;
    let found =
        LEADING_DIGITS.find(text).ok_or_else(|| TypeError("Cannot read properties of null (reading '0')".into()))?;
    Ok(text[found.start()..found.end()].to_owned())
}

fn release_info(anime: &JsValue) -> Result<JsValue, TypeError> {
    let start = anime.get("startDate");
    let mut release_info = if start.truthy() { JsValue::String(leading_digits(start)?) } else { start.clone() };
    let end = anime.get("endDate");
    let ends_later = if end.truthy() {
        let end_text = end.as_str().ok_or_else(|| TypeError("endDate.startsWith is not a function".into()))?;
        !end_text.starts_with(&release_info.text())
    } else {
        false
    };
    if ends_later {
        if release_info.truthy() {
            release_info = JsValue::String(format!("{}-{}", release_info.text(), leading_digits(end)?));
        }
    } else if anime.get("status").is_str("current") && release_info.truthy() {
        release_info = JsValue::String(format!("{}-", release_info.text()));
    }
    Ok(release_info)
}

fn list(value: &JsValue) -> Result<Option<&Vec<JsValue>>, TypeError> {
    match value {
        JsValue::Undefined | JsValue::Null => Ok(None),
        JsValue::Array(items) => Ok(Some(items)),
        other => Err(TypeError(format!("{} .map is not a function", other.text()))),
    }
}

fn or_empty(value: &JsValue) -> Result<&[JsValue], TypeError> {
    if value.truthy() { list(value).map(|items| items.map_or(&[][..], Vec::as_slice)) } else { Ok(&[]) }
}

fn genre_names(anime: &JsValue) -> Result<Option<JsValue>, TypeError> {
    let Some(genres) = list(anime.get("genres").prop("data")?)? else { return Ok(None) };
    let names = genres.iter().map(|genre| genre.prop("name").cloned()).collect::<Result<Vec<_>, _>>()?;
    Ok(Some(JsValue::Array(names)))
}

fn videos(anime: &JsValue) -> Result<(JsValue, JsValue), TypeError> {
    let id = anime.get("id").text();
    let subtype = anime.get("subtype");
    let episodes = anime.get("episodes").prop("data")?;
    let listed_episodes = match episodes {
        JsValue::Array(items) if !items.is_empty() => Some(items),
        JsValue::String(text) if !text.is_empty() => {
            return Err(TypeError("episodes.data.map is not a function".into()));
        }
        _ => None,
    };
    let episode_count = anime.get("episodeCount");
    if subtype.is_str("movie")
        && listed_episodes.is_none()
        && episode_count.to_number().partial_cmp(&1.0) != Some(Ordering::Greater)
    {
        return Ok((JsValue::Undefined, JsValue::Undefined));
    }
    let series_start = date(anime.get("startDate"));
    let mut videos = if let Some(episodes) = listed_episodes {
        Some(listed_videos(&id, episodes, episode_count, series_start)?)
    } else if episode_count.truthy() {
        let count = match episode_count {
            JsValue::Number(count) if count.fract() == 0.0 && *count >= 0.0 && *count < 4_294_967_296.0 => {
                *count as u32
            }
            JsValue::Number(_) => return Err(TypeError("RangeError: Invalid array length".into())),
            _ => 1,
        };
        Some(
            (1..=count)
                .map(|episode| {
                    object([
                        ("id", JsValue::String(format!("kitsu:{id}:{episode}"))),
                        ("title", JsValue::String(format!("Episode {episode}"))),
                        ("released", JsValue::Date(series_start)),
                        ("season", JsValue::Number(1.0)),
                        ("episode", JsValue::Number(f64::from(episode))),
                    ])
                })
                .collect(),
        )
    } else {
        None
    };
    let mut behavior_hints = JsValue::Undefined;
    if let Some([video]) = videos.as_deref_mut()
        && SINGLE_VIDEO_SUBTYPES.iter().any(|single| subtype.is_str(single))
        && let JsValue::Object(video) = video
    {
        let video_id = JsValue::String(format!("kitsu:{id}"));
        video.insert("id".to_owned(), video_id.clone());
        behavior_hints = object([("defaultVideoId", video_id)]);
    }
    Ok((videos.map_or(JsValue::Undefined, JsValue::Array), behavior_hints))
}

fn listed_videos(
    id: &str,
    episodes: &[JsValue],
    episode_count: &JsValue,
    series_start: f64,
) -> Result<Vec<JsValue>, TypeError> {
    let mut last_release = series_start;
    let mut videos = Vec::with_capacity(episodes.len());
    for (index, episode) in episodes.iter().enumerate() {
        let number = episode.prop("number")?;
        let titles = episode.prop("titles")?;
        let title = [titles.prop("en_us")?, titles.get("en"), titles.get("en_jp"), episode.get("canonicalTitle")]
            .into_iter()
            .find(|title| title.truthy())
            .cloned()
            .unwrap_or_else(|| JsValue::String(format!("Episode {}", number.text())));
        let released = episode_released(episode, episodes.get(index + 1), &mut last_release);
        let thumbnail = episode.get("thumbnail");
        let length = episode.get("length");
        videos.push(object([
            ("id", JsValue::String(format!("kitsu:{id}:{}", number.text()))),
            ("title", title),
            ("released", JsValue::Date(released)),
            ("season", JsValue::Number(1.0)),
            ("episode", number.clone()),
            ("thumbnail", if thumbnail.is_nullish() { JsValue::Undefined } else { thumbnail.get("original").clone() }),
            ("overview", clean_description(episode.get("synopsis"))?),
            (
                "runtime",
                if length.to_number() > 0.0 {
                    JsValue::String(format!("{} min", length.text()))
                } else {
                    JsValue::Undefined
                },
            ),
        ]));
    }
    fill_missing_episodes(id, &mut videos, episode_count.or(&JsValue::Number(0.0)).to_number(), last_release);
    videos.sort_by(|left, right| {
        let difference = left.get("episode").to_number() - right.get("episode").to_number();
        difference.partial_cmp(&0.0).unwrap_or(Ordering::Equal)
    });
    Ok(videos)
}

fn fill_missing_episodes(id: &str, videos: &mut Vec<JsValue>, limit: f64, released: f64) {
    let listed: HashSet<u64> = videos
        .iter()
        .filter_map(|video| match video.get("episode") {
            JsValue::Number(number) => Some(number.to_bits()),
            _ => None,
        })
        .collect();
    let mut episode = 1.0_f64;
    while episode <= limit {
        if !listed.contains(&episode.to_bits()) {
            let number = JsValue::Number(episode);
            videos.push(object([
                ("id", JsValue::String(format!("kitsu:{id}:{}", number.text()))),
                ("title", JsValue::String(format!("Episode {}", number.text()))),
                ("released", JsValue::Date(released)),
                ("season", JsValue::Number(1.0)),
                ("episode", number),
            ]));
        }
        episode += 1.0;
    }
}

fn episode_released(episode: &JsValue, next: Option<&JsValue>, last_release: &mut f64) -> f64 {
    let airdate = episode.get("airdate");
    let air = airdate.truthy().then(|| date(airdate));
    let next_air = next
        .filter(|next| next.truthy())
        .map(|next| next.get("airdate"))
        .filter(|airdate| airdate.truthy())
        .map(date)
        .or(air);
    let released = match (air, next_air) {
        (Some(air), Some(next_air)) if air > *last_release && air <= next_air => air,
        _ => *last_release,
    };
    *last_release = released;
    released
}

fn aliases(titles: &JsValue, abbreviated: &JsValue) -> Result<JsValue, TypeError> {
    let mut candidates = vec![titles.prop("en_us")?.clone(), titles.get("en").clone(), titles.get("en_jp").clone()];
    match abbreviated {
        JsValue::Array(items) => candidates.extend(items.iter().cloned()),
        other if other.truthy() => candidates.push(other.clone()),
        _ => {}
    }
    let mut aliases: Vec<JsValue> = Vec::new();
    for next in candidates.into_iter().filter(JsValue::truthy) {
        let duplicate = if aliases.is_empty() {
            false
        } else {
            let next_lower = lowercase(&next)?;
            let mut found = false;
            for alias in &aliases {
                if lowercase(alias)? == next_lower {
                    found = true;
                    break;
                }
            }
            found
        };
        if !duplicate {
            aliases.push(next);
        }
    }
    Ok(JsValue::Array(aliases))
}

fn lowercase(value: &JsValue) -> Result<String, TypeError> {
    value
        .as_str()
        .map(str::to_lowercase)
        .ok_or_else(|| TypeError(format!("{} .toLowerCase is not a function", value.text())))
}

fn clean_description(description: &JsValue) -> Result<JsValue, TypeError> {
    match description {
        JsValue::Undefined | JsValue::Null => Ok(JsValue::Undefined),
        JsValue::String(text) => Ok(JsValue::String(DESCRIPTION_TAIL.replace(text, "").into_owned())),
        other => Err(TypeError(format!("{} .replace is not a function", other.text()))),
    }
}

pub fn rounded_rating(rating: &JsValue) -> JsValue {
    if !rating.truthy() {
        return rating.clone();
    }
    let scaled = (rating.to_number() / 10.0 + f64::EPSILON) * 10.0;
    JsValue::String(to_fixed_1(js_round(scaled)))
}

fn js_round(number: f64) -> f64 {
    let floor = number.floor();
    if number - floor >= 0.5 { floor + 1.0 } else { floor }
}

fn to_fixed_1(tenths: f64) -> String {
    if tenths.is_nan() {
        return "NaN".to_owned();
    }
    let sign = if tenths < 0.0 { "-" } else { "" };
    let tenths = tenths.abs() as u64;
    format!("{sign}{}.{}", tenths / 10, tenths % 10)
}

fn kitsu_links(anime: &JsValue, kind: &str, rating: &JsValue, genre_url: &GenreUrl) -> Result<JsValue, TypeError> {
    let mut links = Vec::new();
    if rating.truthy() {
        links.push(object([
            ("name", JsValue::String(rating.text())),
            ("category", "imdb".into()),
            ("url", JsValue::String(format!("https://kitsu.io/anime/{}", anime.get("slug").text()))),
        ]));
    }
    for relationship in or_empty(anime.get("mediaRelationships").prop("data")?)? {
        let role = relationship.prop("role")?;
        if !ALLOWED_RELATIONSHIPS.iter().any(|allowed| role.is_str(allowed)) {
            continue;
        }
        let destination = relationship.prop("destination")?.prop("data")?;
        links.push(object([
            (
                "name",
                JsValue::String(format!(
                    "{}: {}",
                    capitalize(&role.text()),
                    destination.prop("canonicalTitle")?.text()
                )),
            ),
            ("category", "Franchise".into()),
            ("url", JsValue::String(format!("stremio:///detail/{kind}/kitsu:{}", destination.prop("id")?.text()))),
        ]));
    }
    for genre in or_empty(anime.get("genres").prop("data")?)? {
        let name = genre.prop("name")?;
        links.push(object([
            ("name", name.clone()),
            ("category", "Genres".into()),
            ("url", JsValue::String(genre_url.url(&name.text()))),
        ]));
    }
    Ok(JsValue::Array(links))
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| first.to_uppercase().chain(chars).collect())
}

fn kitsu_relations(anime: &JsValue) -> Result<JsValue, TypeError> {
    let mut relations = Vec::new();
    for relationship in or_empty(anime.get("mediaRelationships").prop("data")?)? {
        let destination = relationship.prop("destination")?.get("data");
        if destination.get("type").is_str("anime") {
            relations.push(object([
                ("role", relationship.get("role").clone()),
                ("kitsuId", JsValue::Number(destination.get("id").to_number())),
            ]));
        }
    }
    Ok(JsValue::Array(relations))
}

#[cfg(test)]
mod tests;
