use std::sync::{Arc, LazyLock};

use async_trait::async_trait;
use fastjs_regex::Regex;
use futures::future::{join_all, try_join_all};
use kitsu_search::SharedIndex;
use serde_json::{Map, Value, json};

use super::{
    EntryOptions, Error, ExtraValue, Extras, GenreUrl, ImageLookup, Mapping, Metadata, SearchGate, enrich_imdb,
    enrich_kitsu,
};
use crate::infrastructure::{
    Cache, CacheSettings, Cinemeta, Config, EntriesOptions, Fanart, HttpTransport, Ids, Images, KitsuApi,
    KitsuTransport, Metrics, Store, UpstreamObserver,
    kitsu::{PAGE_SIZE, jsonapi},
    metrics::{CatalogKind, IdType, MetaOutcome, SearchResult, catalog_label},
    opensubtitles,
};

const LAST_VIDEOS_IDS_LIMIT: usize = 20;
const LAST_VIDEOS: usize = 20;
const TRENDING: &str = "kitsu-anime-trending";

static KITSU_ID: LazyLock<Regex> = LazyLock::new(|| js_regex(r"^(?:kitsu|mal|anilist|anidb):\d+$", ""));
static IMDB_ID: LazyLock<Regex> = LazyLock::new(|| js_regex(r"^tt\d+$", ""));
static SUBTITLES_ID: LazyLock<Regex> = LazyLock::new(|| js_regex(r"^(?:kitsu|mal|anilist|anidb):\d+(?::\d+)?$", ""));
static SEARCH_IMDB_ID: LazyLock<Regex> = LazyLock::new(|| js_regex(r"^tt\d+$", "i"));
static URL_TERM: LazyLock<Regex> = LazyLock::new(|| js_regex(r"(?:https?|stremio):\/\/", ""));

fn js_regex(source: &str, flags: &str) -> Regex {
    Regex::new(source, flags).expect("valid regex")
}

#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    Json(Arc<Value>),
    Redirect(String),
}

pub trait TitleSearch: Send + Sync {
    fn ids(&self, query: &str) -> Vec<String>;
}

pub struct IndexSearch {
    pub index: Arc<SharedIndex>,
    pub mapping: Arc<Mapping>,
}

impl TitleSearch for IndexSearch {
    fn ids(&self, query: &str) -> Vec<String> {
        search_ids(&self.index, &self.mapping, query)
    }
}

pub struct Parts {
    pub kitsu: KitsuApi,
    pub cinemeta: Cinemeta,
    pub ids: Ids<Error>,
    pub cache: Arc<Cache<Error>>,
    pub metadata: Metadata,
    pub mapping: Arc<Mapping>,
    pub search: Arc<dyn TitleSearch>,
    pub gate: SearchGate,
    pub metrics: Metrics,
    pub cache_max_age: u64,
}

pub struct Upstreams {
    pub kitsu: Arc<dyn KitsuTransport>,
    pub http: Arc<dyn HttpTransport>,
}

pub struct Addon {
    kitsu: KitsuApi,
    cinemeta: Cinemeta,
    ids: Ids<Error>,
    cache: Arc<Cache<Error>>,
    metadata: Metadata,
    mapping: Arc<Mapping>,
    search: Arc<dyn TitleSearch>,
    gate: Arc<SearchGate>,
    metrics: Metrics,
    cache_max_age: u64,
}

struct FanartImages {
    fanart: Fanart<Error>,
    mapping: Arc<Mapping>,
}

#[async_trait]
impl ImageLookup for FanartImages {
    async fn images(&self, kitsu_id: &str) -> Images {
        self.fanart.images(kitsu_id, self.mapping.mapping_info(kitsu_id).as_ref()).await
    }
}

impl Addon {
    pub fn new(parts: Parts) -> Self {
        Self {
            kitsu: parts.kitsu,
            cinemeta: parts.cinemeta,
            ids: parts.ids,
            cache: parts.cache,
            metadata: parts.metadata,
            mapping: parts.mapping,
            search: parts.search,
            gate: Arc::new(parts.gate),
            metrics: parts.metrics,
            cache_max_age: parts.cache_max_age,
        }
    }

    pub fn build(
        config: &Config,
        upstreams: Upstreams,
        store: Option<Store>,
        mapping: Arc<Mapping>,
        search: Arc<SharedIndex>,
        metrics: Metrics,
    ) -> Self {
        let observer: Arc<dyn UpstreamObserver> = Arc::new(metrics.clone());
        let cache = Arc::new(Cache::new(CacheSettings::from_config(config), store, Arc::new(metrics.clone())));
        let api_key = config.fanart_apikey.clone().unwrap_or_default();
        let fanart = Fanart::new(Arc::clone(&upstreams.http), Arc::clone(&observer), Arc::clone(&cache), api_key);
        let images = Arc::new(FanartImages { fanart, mapping: Arc::clone(&mapping) });
        Self::new(Parts {
            kitsu: KitsuApi::new(upstreams.kitsu, Arc::clone(&observer)),
            cinemeta: Cinemeta::new(Arc::clone(&upstreams.http), observer),
            ids: Ids::new(upstreams.http, Arc::clone(&cache)),
            metadata: Metadata::new(Arc::clone(&mapping), images, GenreUrl::new(&config.addon_url)),
            cache,
            search: Arc::new(IndexSearch { index: search, mapping: Arc::clone(&mapping) }),
            mapping,
            gate: SearchGate::new(config.search_concurrency, config.search_queue),
            metrics,
            cache_max_age: config.cache_max_age,
        })
    }

    pub fn cache(&self) -> &Cache<Error> {
        &self.cache
    }

    pub fn cache_max_age(&self) -> u64 {
        self.cache_max_age
    }

    pub async fn catalog(&self, id: &str, extras: &Extras) -> Result<Arc<Value>, Error> {
        let Some(catalog) = catalog_label(id) else {
            return Err(Error::NotFound(format!("Unknown catalog: {id}")));
        };
        if let Some(search) = extras.truthy("search") {
            self.metrics.catalog_request(catalog, CatalogKind::Search);
            return self.search_catalog(search).await.map(Arc::new);
        }
        if let Some(ids) = extras.truthy("lastVideosIds") {
            self.metrics.catalog_request(catalog, CatalogKind::LastVideos);
            return self.last_videos(ids).await.map(Arc::new);
        }
        self.metrics.catalog_request(catalog, CatalogKind::Browse);
        let skip = extras.truthy("skip").map(ExtraValue::text);
        let genre = extras.truthy("genre").map(ExtraValue::text);
        let options = EntriesOptions {
            offset: skip.clone(),
            genre: genre.clone(),
            sort: sort_value(id).map(str::to_owned),
            status: status_value(id).map(str::to_owned),
            trending: id == TRENDING,
            limit: None,
        };
        let skip = skip.unwrap_or_else(|| "0".to_owned());
        let genre = genre.unwrap_or_else(|| "All".to_owned());
        self.cache
            .wrap_catalog(id, &genre, &skip, || async {
                let entries = self.kitsu.anime_entries(&options).await.map_err(Error::into_failed)?;
                let metas = self.catalog_metas(&entries).await?;
                Ok(json!({ "metas": metas, "cacheMaxAge": self.cache_max_age }))
            })
            .await
    }

    pub async fn meta(&self, id: &str) -> Result<Arc<Value>, Error> {
        if KITSU_ID.is_match(id) {
            return self.observed(IdType::Kitsu, self.kitsu_meta(id).await);
        }
        if IMDB_ID.is_match(id) {
            if !self.mapping.has_imdb_mapping(id) {
                return self.observed(IdType::Imdb, Err(Error::NotFound(format!("No imdb mapping for: {id}"))));
            }
            return self.observed(IdType::Imdb, self.imdb_meta(id).await).map_err(Error::into_failed);
        }
        Err(Error::NotFound(format!("Invalid id: {id}")))
    }

    pub async fn subtitles(&self, id: &str, extras: &Extras) -> Result<Answer, Error> {
        if !SUBTITLES_ID.is_match(id) {
            return Err(Error::NotFound(format!("Invalid id: {id}")));
        }
        let redirect = match self.kitsu_meta(id).await {
            Ok(response) => {
                let meta = response.get("meta").unwrap_or(&Value::Null);
                opensubtitles::redirect_url(meta, id, &extras.to_json())
            }
            Err(error) => Err(error),
        };
        Ok(redirect.map_or_else(|_| Answer::Json(Arc::new(json!({ "subtitles": [] }))), Answer::Redirect))
    }

    #[must_use]
    pub fn with_search(mut self, search: Arc<dyn TitleSearch>) -> Self {
        self.search = search;
        self
    }

    pub fn search_gate(&self) -> &SearchGate {
        &self.gate
    }

    pub async fn search(&self, query: &str) -> Result<Vec<String>, Error> {
        let search = Arc::clone(&self.search);
        let query = query.to_owned();
        let ids = self.gate.run(move || search.ids(&query)).await?;
        self.metrics.search_request(if ids.is_empty() { SearchResult::Empty } else { SearchResult::Found });
        Ok(ids)
    }

    async fn search_catalog(&self, search: &ExtraValue) -> Result<Value, Error> {
        let ExtraValue::One(search) = search else {
            return Err(Error::NotFound("args.extra.search.match is not a function".to_owned()));
        };
        if URL_TERM.is_match(search) {
            return Err(Error::failed(anyhow::anyhow!("Invalid search term: {search}")));
        }
        let ids: Vec<String> = self.search(search).await?.iter().map(|id| format!("kitsu:{id}")).collect();
        let (cached, missing) = self.cached_metas(&ids).await;
        let kitsu_ids: Vec<String> = missing.iter().map(|&index| ids[index].replacen("kitsu:", "", 1)).collect();
        let resources = self.kitsu.anime_catalog_metas(&kitsu_ids).await.map_err(Error::into_failed)?;
        let fetched = try_join_all(resources.iter().map(|resource| async move {
            match resource {
                Some(resource) => self.metadata.to_catalog_meta(resource).await.map(Some),
                None => Ok(None),
            }
        }))
        .await
        .map_err(Error::from)?;
        let metas: Vec<Value> = merged(cached, &missing, fetched)
            .into_iter()
            .flatten()
            .filter(|meta| jsonapi::truthy(Some(meta)))
            .map(|meta| self.metadata.catalog_fields(&meta))
            .collect();
        Ok(json!({ "metas": metas, "cacheMaxAge": self.cache_max_age }))
    }

    async fn last_videos(&self, ids: &ExtraValue) -> Result<Value, Error> {
        let ExtraValue::One(ids) = ids else {
            return Err(Error::NotFound("args.extra.lastVideosIds.split is not a function".to_owned()));
        };
        let ids: Vec<String> = ids.split(',').take(LAST_VIDEOS_IDS_LIMIT).map(str::to_owned).collect();
        let (cached, missing) = self.cached_metas(&ids).await;
        let ids = &ids;
        let fetched = join_all(missing.iter().map(|&index| async move {
            self.kitsu_meta(&ids[index]).await.ok().and_then(|response| response.get("meta").cloned())
        }))
        .await;
        let detailed: Vec<Value> = merged(cached, &missing, fetched)
            .into_iter()
            .zip(ids)
            .filter_map(|(meta, id)| {
                meta.filter(|meta| jsonapi::truthy(Some(meta))).map(|meta| self.detailed(&meta, id))
            })
            .collect();
        Ok(json!({ "metasDetailed": detailed, "cacheMaxAge": self.cache_max_age }))
    }

    fn detailed(&self, meta: &Value, id: &str) -> Value {
        let mut detailed = Map::new();
        detailed.insert("id".to_owned(), Value::String(id.to_owned()));
        if let Value::Object(fields) = self.metadata.catalog_fields(meta) {
            detailed.extend(fields.into_iter().filter(|(name, _)| name != "id"));
        }
        if let Some(videos) = meta.get("videos").and_then(Value::as_array) {
            let last = videos[videos.len().saturating_sub(LAST_VIDEOS)..].to_vec();
            detailed.insert("videos".to_owned(), Value::Array(last));
        }
        Value::Object(detailed)
    }

    async fn cached_metas(&self, ids: &[String]) -> (Vec<Option<Value>>, Vec<usize>) {
        let kitsu_ids: Vec<String> = join_all(ids.iter().map(|id| self.ids.to_kitsu(id)))
            .await
            .into_iter()
            .map(|kitsu_id| kitsu_id.unwrap_or_else(|_| "undefined".to_owned()))
            .collect();
        let cached: Vec<Option<Arc<Value>>> = self
            .cache
            .cached_metas(&kitsu_ids)
            .await
            .into_iter()
            .map(|response| response.filter(|response| jsonapi::truthy(Some(response))))
            .collect();
        let missing = (0..ids.len()).filter(|&index| cached[index].is_none()).collect();
        let metas = cached.into_iter().map(|response| response.map(|response| meta_of(&response))).collect();
        (metas, missing)
    }

    async fn kitsu_meta(&self, id: &str) -> Result<Arc<Value>, Error> {
        let kitsu_id = self.ids.to_kitsu(id).await?;
        self.cache
            .wrap_meta(&kitsu_id, || async {
                let anime = self.kitsu.anime_data(&kitsu_id).await?;
                let entry = self.metadata.to_entry_meta(&anime, EntryOptions::default()).await.map_err(Error::from)?;
                let meta = enrich_kitsu(entry, &self.mapping, self.metadata.genre_url(), |imdb_id| async move {
                    Some(self.cinemeta.get(&imdb_id).await)
                })
                .await;
                Ok(json!({ "meta": meta, "cacheMaxAge": self.cache_max_age }))
            })
            .await
    }

    async fn imdb_meta(&self, id: &str) -> Result<Arc<Value>, Error> {
        self.cache
            .wrap_meta(id, || async {
                let cinemeta = self.cinemeta.get(id).await;
                let meta = enrich_imdb(cinemeta, &self.mapping, |kitsu_id| async move {
                    let anime = self.kitsu.anime_data(&kitsu_id).await?;
                    self.metadata.to_entry_meta(&anime, EntryOptions::default()).await.map_err(Error::from)
                })
                .await?;
                Ok(json!({ "meta": meta, "cacheMaxAge": self.cache_max_age }))
            })
            .await
    }

    async fn catalog_metas(&self, entries: &[Value]) -> Result<Vec<Value>, Error> {
        try_join_all(entries.iter().map(|entry| self.metadata.to_catalog_meta(entry))).await.map_err(Error::from)
    }

    fn observed(&self, id_type: IdType, result: Result<Arc<Value>, Error>) -> Result<Arc<Value>, Error> {
        let outcome = match &result {
            Ok(_) => MetaOutcome::Ok,
            Err(Error::NotFound(_)) => MetaOutcome::NotFound,
            Err(Error::Failed(_) | Error::Busy(_)) => MetaOutcome::Error,
        };
        self.metrics.meta_request(id_type, outcome);
        result
    }
}

fn search_ids(index: &SharedIndex, mapping: &Mapping, query: &str) -> Vec<String> {
    let trimmed = query.trim();
    if SEARCH_IMDB_ID.is_match(trimmed) {
        let imdb_id = trimmed.to_ascii_lowercase();
        return mapping.kitsu_ids_for_imdb(&imdb_id).into_iter().take(PAGE_SIZE as usize).collect();
    }
    index.search(query).iter().map(|hit| hit.id.to_string()).collect()
}

fn merged(cached: Vec<Option<Value>>, missing: &[usize], fetched: Vec<Option<Value>>) -> Vec<Option<Value>> {
    let mut fetched = fetched.into_iter();
    let mut missing = missing.iter().peekable();
    cached
        .into_iter()
        .enumerate()
        .map(
            |(index, meta)| {
                if missing.next_if(|&&position| position == index).is_some() { fetched.next().flatten() } else { meta }
            },
        )
        .collect()
}

fn meta_of(response: &Value) -> Value {
    response.get("meta").cloned().unwrap_or(Value::Null)
}

fn sort_value(catalog_id: &str) -> Option<&'static str> {
    match catalog_id {
        "kitsu-anime-list" => Some("createdAt"),
        "kitsu-anime-rating" | "kitsu-anime-airing" => Some("-average_rating"),
        "kitsu-anime-popular" => Some("-user_count"),
        _ => None,
    }
}

fn status_value(catalog_id: &str) -> Option<&'static str> {
    (catalog_id == "kitsu-anime-airing").then_some("current")
}

#[cfg(test)]
mod tests;
