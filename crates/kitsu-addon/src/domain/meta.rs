use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Trailer {
    pub source: String,
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Link {
    pub name: String,
    pub category: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Video {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub released: Option<String>,
    pub season: u32,
    pub episode: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Meta {
    pub id: String,
    pub kitsu_id: u64,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(rename = "animeType", default, skip_serializing_if = "Option::is_none")]
    pub anime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    pub aliases: Vec<String>,
    pub genres: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poster: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "releaseInfo", default, skip_serializing_if = "Option::is_none")]
    pub release_info: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<String>,
    #[serde(rename = "imdbRating", default, skip_serializing_if = "Option::is_none")]
    pub imdb_rating: Option<String>,
    #[serde(rename = "userCount", default, skip_serializing_if = "Option::is_none")]
    pub user_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,
    pub trailers: Vec<Trailer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub videos: Option<Vec<Video>>,
    pub links: Vec<Link>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imdb_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogMeta {
    pub id: String,
    pub kitsu_id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imdb_id: Option<String>,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(rename = "animeType", default, skip_serializing_if = "Option::is_none")]
    pub anime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub aliases: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "releaseInfo", default, skip_serializing_if = "Option::is_none")]
    pub release_info: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,
    #[serde(rename = "imdbRating", default, skip_serializing_if = "Option::is_none")]
    pub imdb_rating: Option<String>,
    pub genres: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poster: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    pub trailers: Vec<Trailer>,
    pub links: Vec<Link>,
}

impl From<&Meta> for CatalogMeta {
    fn from(meta: &Meta) -> Self {
        Self {
            id: meta.id.clone(),
            kitsu_id: meta.kitsu_id,
            imdb_id: meta.imdb_id.clone(),
            kind: meta.kind.clone(),
            anime_type: meta.anime_type.clone(),
            name: meta.name.clone(),
            aliases: meta.aliases.clone(),
            description: meta.description.clone(),
            release_info: meta.release_info.clone(),
            runtime: meta.runtime.clone(),
            imdb_rating: meta.imdb_rating.clone(),
            genres: meta.genres.clone(),
            logo: meta.logo.clone(),
            poster: meta.poster.clone(),
            background: meta.background.clone(),
            trailers: meta.trailers.clone(),
            links: meta.links.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn meta() -> Meta {
        serde_json::from_value(json!({
            "id": "kitsu:1", "kitsu_id": 1, "type": "series", "animeType": "TV", "name": "Cowboy Bebop",
            "slug": "cowboy-bebop", "aliases": ["Cowboy Bebop"], "genres": ["Action"], "description": "d",
            "releaseInfo": "1998-1999", "imdbRating": "8.7", "trailers": [], "links": [], "imdb_id": "tt0213338"
        }))
        .unwrap()
    }

    #[test]
    fn meta_omits_absent_fields_and_keeps_empty_lists() {
        let value = serde_json::to_value(meta()).unwrap();
        assert!(value.get("poster").is_none());
        assert!(value.get("videos").is_none());
        assert_eq!(value["trailers"], json!([]));
    }

    #[test]
    fn catalog_meta_follows_the_catalog_field_order() {
        let keys: Vec<String> =
            serde_json::to_value(CatalogMeta::from(&meta())).unwrap().as_object().unwrap().keys().cloned().collect();
        assert_eq!(
            keys,
            [
                "id",
                "kitsu_id",
                "imdb_id",
                "type",
                "animeType",
                "name",
                "aliases",
                "description",
                "releaseInfo",
                "imdbRating",
                "genres",
                "trailers",
                "links"
            ]
        );
    }
}
