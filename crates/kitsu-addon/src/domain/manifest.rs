use std::sync::LazyLock;

use serde::Serialize;

use super::genres::GENRES;

pub const LAST_VIDEOS_IDS_LIMIT: u32 = 20;

#[derive(Debug, Serialize)]
pub struct Extra {
    pub name: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<&'static [&'static str]>,
    #[serde(rename = "isRequired", skip_serializing_if = "Option::is_none")]
    pub is_required: Option<bool>,
    #[serde(rename = "optionsLimit", skip_serializing_if = "Option::is_none")]
    pub options_limit: Option<u32>,
}

impl Extra {
    const fn named(name: &'static str) -> Self {
        Self { name, options: None, is_required: None, options_limit: None }
    }

    const fn genre() -> Self {
        Self { options: Some(&GENRES), ..Self::named("genre") }
    }

    const fn last_videos_ids(is_required: bool) -> Self {
        Self {
            is_required: Some(is_required),
            options_limit: Some(LAST_VIDEOS_IDS_LIMIT),
            ..Self::named("lastVideosIds")
        }
    }
}

#[derive(Debug, Serialize)]
pub struct CatalogDefinition {
    pub id: &'static str,
    pub name: &'static str,
    #[serde(rename = "type")]
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<Vec<Extra>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub genres: Option<&'static [&'static str]>,
}

fn browse_catalog(id: &'static str, name: &'static str) -> CatalogDefinition {
    CatalogDefinition {
        id,
        name,
        kind: "anime",
        extra: Some(vec![Extra::genre(), Extra::named("skip")]),
        genres: Some(&GENRES),
    }
}

#[derive(Debug, Serialize)]
pub struct Manifest {
    pub id: &'static str,
    pub version: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub logo: &'static str,
    pub background: &'static str,
    pub resources: [&'static str; 3],
    pub types: [&'static str; 3],
    pub catalogs: Vec<CatalogDefinition>,
    #[serde(rename = "idPrefixes")]
    pub id_prefixes: [&'static str; 4],
}

pub fn manifest() -> &'static Manifest {
    static MANIFEST: LazyLock<Manifest> = LazyLock::new(|| Manifest {
        id: "community.anime.kitsu",
        version: "0.0.10",
        name: "Anime Kitsu",
        description: "Unofficial Kitsu.io anime catalog addon",
        logo: "https://i.imgur.com/7N6XGoO.png",
        background: "https://i.imgur.com/ym4n96o.png",
        resources: ["catalog", "meta", "subtitles"],
        types: ["anime", "movie", "series"],
        catalogs: vec![
            CatalogDefinition {
                id: "kitsu-anime-trending",
                name: "Kitsu Trending",
                kind: "anime",
                extra: None,
                genres: None,
            },
            browse_catalog("kitsu-anime-airing", "Kitsu Top Airing"),
            browse_catalog("kitsu-anime-popular", "Kitsu Most Popular"),
            browse_catalog("kitsu-anime-rating", "Kitsu Highest Rated"),
            CatalogDefinition {
                id: "kitsu-anime-list",
                name: "Kitsu",
                kind: "anime",
                extra: Some(vec![
                    Extra { is_required: Some(true), ..Extra::named("search") },
                    Extra::last_videos_ids(false),
                    Extra::named("skip"),
                ]),
                genres: None,
            },
            CatalogDefinition {
                id: "kitsu-anime-last-videos",
                name: "Last videos",
                kind: "series",
                extra: Some(vec![Extra::last_videos_ids(true)]),
                genres: None,
            },
        ],
        id_prefixes: ["kitsu", "mal", "anilist", "anidb"],
    });
    &MANIFEST
}

#[cfg(test)]
mod tests {
    use super::*;

    const NODE_MANIFEST: &str = include_str!("../../tests/fixtures/manifest.json");

    #[test]
    fn serialises_to_the_node_manifest_byte_for_byte() {
        assert_eq!(serde_json::to_string(manifest()).unwrap(), NODE_MANIFEST);
    }

    #[test]
    fn serialises_to_the_node_manifest_value() {
        let expected: serde_json::Value = serde_json::from_str(NODE_MANIFEST).unwrap();
        assert_eq!(serde_json::to_value(manifest()).unwrap(), expected);
    }
}
