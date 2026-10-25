use serde_json::{Map, Value};

use super::{js, kitsu::jsonapi};
use crate::application::Error;

pub const OPENSUBTITLES_ADDON_URL: &str = "https://opensubtitles-v3.strem.io";

pub fn redirect_url(metadata: &Value, id: &str, extra: &Map<String, Value>) -> Result<String, Error> {
    let episode = id.split(':').nth(2).filter(|part| !part.is_empty()).and_then(|part| js::parse_int(part, Some(10)));
    let Some(video_id) = imdb_video_id(metadata, episode)? else {
        return Err(Error::NotFound("No imdb mapping found".to_owned()));
    };
    let kind = if metadata.get("videos").is_some_and(Value::is_array) { "series" } else { "movie" };
    let extras = if extra.is_empty() {
        String::new()
    } else {
        let pairs: Vec<String> =
            extra.iter().map(|(key, value)| format!("{key}={}", jsonapi::js_text(Some(value)))).collect();
        format!("/{}", pairs.join("&"))
    };
    Ok(format!("{OPENSUBTITLES_ADDON_URL}/subtitles/{kind}/{video_id}{extras}.json"))
}

fn imdb_video_id(metadata: &Value, episode: Option<f64>) -> Result<Option<String>, Error> {
    let Some(episode) = episode.filter(|episode| episode.is_finite()) else {
        return Ok(truthy_text(metadata.get("imdb_id")));
    };
    let Some(videos) = metadata.get("videos").and_then(Value::as_array) else {
        return Err(Error::failed(anyhow::anyhow!("Cannot read properties of undefined (reading 'find')")));
    };
    for video in videos {
        if video.is_null() {
            return Err(Error::failed(anyhow::anyhow!("Cannot read properties of null (reading 'episode')")));
        }
        if video.get("episode").and_then(Value::as_f64) == Some(episode) {
            if !jsonapi::truthy(video.get("imdb_id")) {
                return Ok(None);
            }
            let imdb_id = jsonapi::js_text(video.get("imdb_id"));
            let season = jsonapi::js_text(video.get("imdbSeason"));
            let episode = jsonapi::js_text(video.get("imdbEpisode"));
            return Ok(Some(format!("{imdb_id}:{season}:{episode}")));
        }
    }
    Ok(None)
}

fn truthy_text(value: Option<&Value>) -> Option<String> {
    jsonapi::truthy(value).then(|| jsonapi::js_text(value))
}

#[cfg(test)]
mod tests;
