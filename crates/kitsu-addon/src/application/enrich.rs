use std::{cmp::Ordering, collections::HashMap, future::Future, sync::LazyLock};

use fastjs_regex::Regex;
use futures::future::try_join_all;
use serde_json::Value;

use super::{
    jsvalue::{JsObject, JsValue, add, date, number_text},
    mapping::{Mapping, MappingRow},
    metadata::GenreUrl,
};

const JS_ARRAY_INDEX_LIMIT: f64 = 4_294_967_295.0;
const MAX_SEASON_INDEX: f64 = 10_000.0;

static EPISODE_TITLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"Episode \d+", "").expect("valid regex"));

pub async fn enrich_kitsu<F, Fut>(
    metadata: JsObject,
    mapping: &Mapping,
    genre_url: &GenreUrl,
    retrieve_imdb: F,
) -> Value
where
    F: FnOnce(String) -> Fut,
    Fut: Future<Output = Option<Value>>,
{
    let meta = JsValue::Object(metadata);
    let Some(row) = mapping.imdb_mapping_of(meta.get("kitsu_id")).filter(|row| row.has_imdb_id()) else {
        let offset = (!is_movie(&meta)).then_some(0.0);
        let mut enriched = into_object(meta);
        let videos = with_absolute_episodes(enriched.get("videos").unwrap_or(&JsValue::Undefined), offset);
        enriched.insert("videos".to_owned(), videos);
        return JsValue::Object(enriched).to_json();
    };
    let imdb_id = row.imdb_id.clone().unwrap_or_default();
    let imdb = retrieve_imdb(imdb_id.clone()).await.as_ref().map_or(JsValue::Undefined, JsValue::from);
    let offset = if is_movie(&meta) || is_movie(&imdb) { None } else { absolute_offset(row, imdb.get("videos")) };
    let genres = meta.get("genres");
    let has_genres = genres.as_array().is_some_and(|genres| !genres.is_empty());
    let runtime = meta.get("runtime").or(imdb.get("runtime"));
    let imdb_rating = meta.get("imdbRating").or(imdb.get("imdbRating")).or(&JsValue::Undefined);
    let genres = if has_genres {
        genres.clone()
    } else {
        match imdb.get("genres") {
            JsValue::Array(genres) => {
                JsValue::Array(genres.iter().filter(|genre| !genre.is_str("Animation")).cloned().collect())
            }
            _ => JsValue::Undefined,
        }
    };
    let videos = with_absolute_episodes(&enrich_kitsu_episodes(&meta, row, mapping, &imdb), offset);
    let mut links = Vec::new();
    concat(&mut links, meta.get("links"));
    if !meta.get("imdbRating").truthy() {
        links.extend(imdb_links(&imdb));
    }
    if !has_genres {
        links.extend(cinemeta_genres(&imdb, genre_url));
    }
    let mut enriched = into_object(meta);
    enriched.insert("imdb_id".to_owned(), JsValue::String(imdb_id));
    enriched.insert("runtime".to_owned(), runtime);
    enriched.insert("imdbRating".to_owned(), imdb_rating);
    enriched.insert("genres".to_owned(), genres);
    enriched.insert("videos".to_owned(), videos);
    enriched.insert("links".to_owned(), JsValue::Array(links));
    sanitize(&mut enriched);
    JsValue::Object(enriched).to_json()
}

pub async fn enrich_imdb<F, Fut, E>(metadata: Value, mapping: &Mapping, retrieve_kitsu: F) -> Result<Value, E>
where
    F: Fn(String) -> Fut,
    Fut: Future<Output = Result<JsObject, E>>,
{
    let meta = JsValue::from(&metadata);
    let imdb_id = meta.get("id").text();
    let entries = mapping.entries(&imdb_id);
    if entries.is_empty() {
        return Ok(metadata);
    }
    let mut kitsu_ids: Vec<JsValue> = entries
        .iter()
        .filter(|entry| entry.start_season() > 0)
        .map(|entry| JsValue::String(entry.kitsu_id.to_string()))
        .collect();
    let kitsu_id = if kitsu_ids.len() == 1 { kitsu_ids.remove(0) } else { JsValue::Array(kitsu_ids) };
    let videos = enrich_imdb_episodes(&meta, &imdb_id, entries, mapping, retrieve_kitsu).await?;
    let id = meta.get("id").clone();
    let mut enriched = into_object(meta);
    enriched.insert("imdb_id".to_owned(), id);
    enriched.insert("kitsu_id".to_owned(), kitsu_id);
    enriched.insert("videos".to_owned(), videos);
    sanitize(&mut enriched);
    Ok(JsValue::Object(enriched).to_json())
}

fn into_object(value: JsValue) -> JsObject {
    match value {
        JsValue::Object(object) => object,
        _ => JsObject::new(),
    }
}

fn sanitize(object: &mut JsObject) {
    object.retain(|_, value| !value.is_nullish());
}

fn concat(target: &mut Vec<JsValue>, value: &JsValue) {
    match value {
        JsValue::Array(items) => target.extend(items.iter().cloned()),
        other => target.push(other.clone()),
    }
}

fn is_movie(meta: &JsValue) -> bool {
    meta.get("type").is_str("movie")
}

fn episode_of(video: &JsValue) -> &JsValue {
    let episode = video.get("episode");
    if episode.is_integer() { episode } else { video.get("number") }
}

fn absolute_offset(entry: &MappingRow, imdb_videos: &JsValue) -> Option<f64> {
    let from_season = entry.start_season();
    let from_episode = entry.start_episode() as f64;
    if from_season == 0 {
        return None;
    }
    match imdb_videos {
        JsValue::Array(videos) if !videos.is_empty() => Some(
            videos
                .iter()
                .filter(|video| {
                    let season = video.get("season");
                    season.to_number() > 0.0
                        && (season.to_number() < from_season as f64
                            || season.strict_equals(&JsValue::Number(from_season as f64))
                                && episode_of(video).to_number() < from_episode)
                })
                .count() as f64,
        ),
        _ => (from_season == 1).then_some(from_episode - 1.0),
    }
}

fn absolute_episode(offset: Option<f64>, kitsu_episode: &JsValue) -> JsValue {
    offset.map_or(JsValue::Undefined, |offset| add(offset, kitsu_episode))
}

fn with_absolute_episodes(videos: &JsValue, offset: Option<f64>) -> JsValue {
    match videos {
        JsValue::Array(videos) => JsValue::Array(
            videos
                .iter()
                .map(|video| {
                    let mut video = spread(video);
                    let absolute = absolute_episode(offset, video.get("episode").unwrap_or(&JsValue::Undefined));
                    video.insert("absoluteEpisode".to_owned(), absolute);
                    JsValue::Object(video)
                })
                .collect(),
        ),
        _ => JsValue::Undefined,
    }
}

fn spread(value: &JsValue) -> JsObject {
    value.as_object().cloned().unwrap_or_default()
}

fn enrich_kitsu_episodes(meta: &JsValue, row: &MappingRow, mapping: &Mapping, imdb: &JsValue) -> JsValue {
    let Some(videos) = meta.get("videos").as_array().filter(|videos| !videos.is_empty()) else {
        return meta.get("videos").clone();
    };
    if row.imdb_episodes.is_some() {
        return listed_videos(videos, row, imdb);
    }
    let imdb_videos: Option<Vec<JsValue>> = if imdb.truthy() && imdb.get("videos").truthy() {
        imdb.get("videos").as_array().map(|videos| videos.iter().map(with_episode_of).collect())
    } else {
        None
    };
    match imdb_videos.as_ref().map(|imdb_videos| season_counts(row, mapping, imdb_videos)) {
        Some(counts) if !counts.is_empty() => {
            run_videos(videos, row, mapping, &counts, imdb_videos.as_deref().unwrap_or_default())
        }
        _ => {
            let imdb_id = row.imdb_id.as_deref().unwrap_or_default();
            let start_episode = row.start_episode() as f64;
            JsValue::Array(
                videos
                    .iter()
                    .map(|video| {
                        let mut video = spread(video);
                        let imdb_episode =
                            add(start_episode - 1.0, video.get("episode").unwrap_or(&JsValue::Undefined));
                        video.insert("imdb_id".to_owned(), imdb_id.into());
                        video.insert("imdbSeason".to_owned(), JsValue::Number(row.start_season() as f64));
                        video.insert("imdbEpisode".to_owned(), imdb_episode);
                        JsValue::Object(video)
                    })
                    .collect(),
            )
        }
    }
}

fn listed_videos(videos: &[JsValue], row: &MappingRow, imdb: &JsValue) -> JsValue {
    let imdb_id = row.imdb_id.as_deref().unwrap_or_default();
    let season = row.start_season() as f64;
    let imdb_videos = videos_by_id(imdb.get("videos").as_array().map_or(&[][..], Vec::as_slice));
    JsValue::Array(
        videos
            .iter()
            .map(|video| match row.imdb_episode(video.get("episode")).flatten() {
                Some(episode) => {
                    let episode = episode as f64;
                    let key = format!("{imdb_id}:{}:{}", number_text(season), number_text(episode));
                    with_imdb_video(video, imdb_id, season, episode, imdb_videos.get(&key).copied())
                }
                None => video.clone(),
            })
            .collect(),
    )
}

fn with_episode_of(video: &JsValue) -> JsValue {
    let mut video = video.clone();
    let episode = episode_of(&video).clone();
    if let JsValue::Object(object) = &mut video {
        object.insert("episode".to_owned(), episode);
    }
    video
}

fn starts_by(entry: &MappingRow, season: f64, episode: f64) -> bool {
    (entry.start_season() as f64, entry.start_episode() as f64) <= (season, episode)
}

fn season_counts(row: &MappingRow, mapping: &Mapping, imdb_videos: &[JsValue]) -> Vec<Option<u64>> {
    let imdb_id = row.imdb_id.as_deref().unwrap_or_default();
    let start_season = row.start_season() as f64;
    let start_episode = row.start_episode() as f64;
    let next_entry = mapping.entries(imdb_id).iter().find(|entry| {
        entry.imdb_episodes.is_none()
            && (entry.start_season(), entry.start_episode()) > (row.start_season(), row.start_episode())
    });
    let mut counts: Vec<Option<u64>> = Vec::new();
    for video in imdb_videos.iter().filter(|video| video.get("episode").truthy()) {
        let season = video.get("season");
        let episode = video.get("episode");
        let in_run = ((season.strict_equals(&JsValue::Number(start_season)) && episode.to_number() >= start_episode)
            || season.to_number() > start_season)
            && next_entry.is_none_or(|next| !starts_by(next, season.to_number(), episode.to_number()));
        if !in_run || mapping.listed(imdb_id, &season.text(), &episode.text()).is_some() {
            continue;
        }
        let index = season.to_number() - start_season;
        if index.fract() != 0.0 || index < 0.0 {
            continue;
        }
        if index >= JS_ARRAY_INDEX_LIMIT {
            continue;
        }
        if index > MAX_SEASON_INDEX {
            tracing::warn!("Ignored imdb season {} of {imdb_id}: beyond {MAX_SEASON_INDEX} seasons", season.text());
            continue;
        }
        let index = index as usize;
        if counts.len() <= index {
            counts.resize(index + 1, None);
        }
        counts[index] = Some(counts[index].map_or(1, |count| count + 1));
    }
    counts
}

fn run_videos(
    videos: &[JsValue],
    row: &MappingRow,
    mapping: &Mapping,
    counts: &[Option<u64>],
    imdb_videos: &[JsValue],
) -> JsValue {
    let imdb_id = row.imdb_id.as_deref().unwrap_or_default();
    let start_episode = row.start_episode() as f64;
    let videos_map = videos_by_id(imdb_videos);
    let total = |counts: &[Option<u64>]| counts.iter().flatten().sum::<u64>() as f64;
    let mut skipped = 0.0;
    let mut last_released: Option<f64> = None;
    JsValue::Array(
        videos
            .iter()
            .map(|video| {
                let kitsu_episode = video.get("episode");
                if row.is_non_imdb_episode(kitsu_episode) {
                    skipped += 1.0;
                    return video.clone();
                }
                let target = kitsu_episode.to_number() - skipped;
                let season_index = (0..counts.len())
                    .find(|index| total(&counts[..=*index]) >= target)
                    .map_or(counts.len(), |index| index + 1)
                    - 1;
                let season = (row.start_season() + season_index as i64) as f64;
                let first_episode = if season_index == 0 { start_episode } else { 1.0 };
                let mut episode =
                    first_episode - 1.0 + kitsu_episode.to_number() - skipped - total(&counts[..season_index]);
                let mut next = first_episode;
                while next <= episode {
                    if mapping.listed(imdb_id, &number_text(season), &number_text(next)).is_some() {
                        episode += 1.0;
                    }
                    next += 1.0;
                }
                let key = format!("{imdb_id}:{}:{}", number_text(season), number_text(episode));
                let mut enriched = with_imdb_video(video, imdb_id, season, episode, videos_map.get(&key).copied());
                let released = date(enriched.get("released"));
                let last = match last_released {
                    Some(last) if last > released => last,
                    _ => released,
                };
                last_released = Some(last);
                if let JsValue::Object(object) = &mut enriched {
                    object.insert("released".to_owned(), JsValue::Date(last));
                }
                enriched
            })
            .collect(),
    )
}

fn videos_by_id(videos: &[JsValue]) -> HashMap<String, &JsValue> {
    videos.iter().map(|video| (video.get("id").text(), video)).collect()
}

fn with_imdb_video(video: &JsValue, imdb_id: &str, season: f64, episode: f64, imdb_video: Option<&JsValue>) -> JsValue {
    let imdb_video = imdb_video.unwrap_or(&JsValue::Undefined);
    let title = video.get("title");
    let title = if title.as_str().is_some_and(|text| EPISODE_TITLE.is_match(text)) {
        imdb_video.get("title").or(imdb_video.get("name")).or(title)
    } else {
        title.clone()
    };
    let released = imdb_video.get("released").or(&JsValue::Number(date(video.get("released"))));
    let mut enriched = spread(video);
    enriched.insert("title".to_owned(), title);
    enriched.insert("thumbnail".to_owned(), video.get("thumbnail").or(imdb_video.get("thumbnail")));
    enriched.insert("overview".to_owned(), video.get("overview").or(imdb_video.get("overview")));
    enriched.insert("released".to_owned(), JsValue::Date(date(&released)));
    enriched.insert("imdb_id".to_owned(), imdb_id.into());
    enriched.insert("imdbSeason".to_owned(), JsValue::Number(season));
    enriched.insert("imdbEpisode".to_owned(), JsValue::Number(episode));
    JsValue::Object(enriched)
}

async fn enrich_imdb_episodes<F, Fut, E>(
    meta: &JsValue,
    imdb_id: &str,
    entries: &[MappingRow],
    mapping: &Mapping,
    retrieve_kitsu: F,
) -> Result<JsValue, E>
where
    F: Fn(String) -> Fut,
    Fut: Future<Output = Result<JsObject, E>>,
{
    if is_movie(meta) {
        return Ok(meta.get("videos").clone());
    }
    let videos = meta.get("videos");
    let offsets: HashMap<u64, Option<f64>> =
        entries.iter().map(|entry| (entry.kitsu_id, absolute_offset(entry, videos))).collect();
    let imdb_videos = videos.as_array().filter(|videos| !videos.is_empty());
    let Some(imdb_videos) = imdb_videos.filter(|_| !matches!(meta.get("type"), JsValue::Undefined) && videos.truthy())
    else {
        let metas = try_join_all(entries.iter().map(|entry| retrieve_kitsu(entry.kitsu_id.to_string()))).await?;
        let mut episodes = Vec::new();
        for (entry, kitsu_meta) in entries.iter().zip(metas) {
            let kitsu_videos = kitsu_meta.get("videos").filter(|videos| videos.truthy()).and_then(JsValue::as_array);
            for video in kitsu_videos.into_iter().flatten() {
                let kitsu_episode = video.get("episode");
                let listed = entry.imdb_episode(kitsu_episode);
                if listed.is_some_and(|episode| episode.is_none()) {
                    continue;
                }
                let episode = match listed {
                    Some(Some(episode)) => JsValue::Number(episode as f64),
                    _ => JsValue::Number(add(entry.start_episode() as f64, kitsu_episode).to_number() - 1.0),
                };
                episodes.push(super::metadata::object([
                    ("title", video.get("title").clone()),
                    ("season", JsValue::Number(entry.start_season() as f64)),
                    ("episode", episode),
                    ("kitsu_id", JsValue::String(entry.kitsu_id.to_string())),
                    ("kitsuEpisode", kitsu_episode.clone()),
                    ("absoluteEpisode", absolute_episode(offsets[&entry.kitsu_id], kitsu_episode)),
                ]));
            }
        }
        return Ok(JsValue::Array(episodes));
    };
    let mut counters: HashMap<u64, i64> = entries.iter().map(|entry| (entry.kitsu_id, 1)).collect();
    let run_entries: Vec<&MappingRow> = entries.iter().filter(|entry| entry.imdb_episodes.is_none()).collect();
    let mut sorted = imdb_videos.clone();
    sorted.sort_by(|left, right| {
        let seasons = left.get("season").to_number() - right.get("season").to_number();
        let order = if seasons != 0.0 && !seasons.is_nan() {
            seasons
        } else {
            episode_of(left).to_number() - episode_of(right).to_number()
        };
        order.partial_cmp(&0.0).unwrap_or(Ordering::Equal)
    });
    Ok(JsValue::Array(
        sorted
            .iter()
            .map(|video| {
                let season = video.get("season");
                let episode = episode_of(video);
                if let Some(listed) = mapping.listed(imdb_id, &season.text(), &episode.text()) {
                    let mut video = spread(video);
                    video.insert("kitsu_id".to_owned(), listed.kitsu_id.as_str().into());
                    video.insert("kitsuEpisode".to_owned(), JsValue::Number(listed.kitsu_episode as f64));
                    return JsValue::Object(video);
                }
                let Some(entry) =
                    run_entries.iter().rev().find(|entry| starts_by(entry, season.to_number(), episode.to_number()))
                else {
                    return video.clone();
                };
                let counter = counters.get_mut(&entry.kitsu_id).expect("a counter per entry");
                let mut kitsu_episode = *counter;
                *counter += 1;
                while entry.is_non_imdb_episode(&JsValue::Number(kitsu_episode as f64)) {
                    kitsu_episode = *counter;
                    *counter += 1;
                }
                let kitsu_episode_value = JsValue::Number(kitsu_episode as f64);
                let absolute = absolute_episode(offsets[&entry.kitsu_id], &kitsu_episode_value);
                let mut video = spread(video);
                if mapping.episode_cap(entry.kitsu_id).is_some_and(|cap| kitsu_episode > i64::from(cap)) {
                    video.insert("absoluteEpisode".to_owned(), absolute);
                } else {
                    video.insert("kitsu_id".to_owned(), JsValue::String(entry.kitsu_id.to_string()));
                    video.insert("kitsuEpisode".to_owned(), kitsu_episode_value);
                    video.insert("absoluteEpisode".to_owned(), absolute);
                }
                JsValue::Object(video)
            })
            .collect(),
    ))
}

fn imdb_links(imdb: &JsValue) -> Vec<JsValue> {
    links_of(imdb).into_iter().filter(|link| link.get("category").is_str("imdb")).cloned().collect()
}

fn cinemeta_genres(imdb: &JsValue, genre_url: &GenreUrl) -> Vec<JsValue> {
    links_of(imdb)
        .into_iter()
        .filter(|link| link.get("category").is_str("Genres") && !link.get("name").is_str("Animation"))
        .map(|link| {
            let mut link = spread(link);
            let url = genre_url.url(&link.get("name").unwrap_or(&JsValue::Undefined).text());
            link.insert("url".to_owned(), JsValue::String(url));
            JsValue::Object(link)
        })
        .collect()
}

fn links_of(imdb: &JsValue) -> Vec<&JsValue> {
    let links = imdb.get("links");
    if links.truthy() { links.as_array().map(|links| links.iter().collect()).unwrap_or_default() } else { Vec::new() }
}

#[cfg(test)]
mod tests;
