use std::str::FromStr;

pub const DEFAULT_PORT: u16 = 7000;
pub const DEFAULT_CACHE_MAX_AGE: u64 = 12 * 60 * 60;
pub const DEFAULT_TTL_MS: u64 = 24 * 60 * 60 * 1000;
pub const DEFAULT_MEMORY_CACHE_MB: u64 = 384;
pub const DEFAULT_STORE_TIMEOUT_MS: u64 = 30_000;
pub const DEFAULT_KITSU_COLLECTION: &str = "kitsu_collection";
pub const DEFAULT_ADDON_URL: &str = "https://anime-kitsu.strem.fun";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub port: u16,
    pub mongodb_uri: Option<String>,
    pub metrics_credentials: Option<(String, String)>,
    pub fanart_apikey: Option<String>,
    pub cache_max_age: u64,
    pub no_cache: bool,
    pub meta_ttl_ms: u64,
    pub catalog_ttl_ms: u64,
    pub memory_cache_mb: u64,
    pub store_timeout_ms: u64,
    pub kitsu_collection: String,
    pub addon_url: String,
}

impl Config {
    pub fn from_env() -> Self {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        let text = |name: &str| lookup(name).filter(|value| !value.is_empty());
        let number = |name: &str| -> Option<u64> { text(name).and_then(|value| parsed(&value)).filter(|n| *n > 0) };
        Self {
            port: text("PORT").and_then(|value| parsed(&value)).unwrap_or(DEFAULT_PORT),
            mongodb_uri: text("MONGODB_URI"),
            metrics_credentials: text("METRICS_USER").zip(text("METRICS_PASSWORD")),
            fanart_apikey: text("FANART_APIKEY"),
            cache_max_age: number("CACHE_MAX_AGE").unwrap_or(DEFAULT_CACHE_MAX_AGE),
            no_cache: text("NO_CACHE").is_some(),
            meta_ttl_ms: number("META_TTL").unwrap_or(DEFAULT_TTL_MS),
            catalog_ttl_ms: number("CATALOG_TTL").unwrap_or(DEFAULT_TTL_MS),
            memory_cache_mb: number("MEMORY_CACHE_MB").unwrap_or(DEFAULT_MEMORY_CACHE_MB),
            store_timeout_ms: number("STORE_TIMEOUT_MS").unwrap_or(DEFAULT_STORE_TIMEOUT_MS),
            kitsu_collection: text("KITSU_COLLECTION").unwrap_or_else(|| DEFAULT_KITSU_COLLECTION.to_owned()),
            addon_url: text("ADDON_URL").unwrap_or_else(|| DEFAULT_ADDON_URL.to_owned()),
        }
    }
}

fn parsed<T: FromStr>(value: &str) -> Option<T> {
    value.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn config(pairs: &[(&str, &str)]) -> Config {
        let env: HashMap<String, String> = pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
        Config::from_lookup(|name| env.get(name).cloned())
    }

    #[test]
    fn defaults_when_nothing_is_set() {
        let config = config(&[]);
        assert_eq!(config.port, 7000);
        assert_eq!(config.cache_max_age, 43_200);
        assert_eq!(config.meta_ttl_ms, 86_400_000);
        assert_eq!(config.catalog_ttl_ms, 86_400_000);
        assert_eq!(config.memory_cache_mb, 384);
        assert_eq!(config.store_timeout_ms, 30_000);
        assert_eq!(config.kitsu_collection, "kitsu_collection");
        assert_eq!(config.addon_url, "https://anime-kitsu.strem.fun");
        assert!(!config.no_cache);
        assert_eq!(config.mongodb_uri, None);
        assert_eq!(config.metrics_credentials, None);
    }

    #[test]
    fn reads_overrides() {
        let config = config(&[
            ("PORT", "8080"),
            ("MONGODB_URI", "mongodb://db"),
            ("METRICS_USER", "u"),
            ("METRICS_PASSWORD", "p"),
            ("FANART_APIKEY", "key"),
            ("CACHE_MAX_AGE", "60"),
            ("NO_CACHE", "true"),
            ("META_TTL", "1000"),
            ("CATALOG_TTL", "2000"),
            ("MEMORY_CACHE_MB", "64"),
            ("STORE_TIMEOUT_MS", "500"),
            ("KITSU_COLLECTION", "kitsu_collection_rs"),
            ("ADDON_URL", "https://example.test"),
        ]);
        assert_eq!(config.port, 8080);
        assert_eq!(config.mongodb_uri.as_deref(), Some("mongodb://db"));
        assert_eq!(config.metrics_credentials, Some(("u".to_owned(), "p".to_owned())));
        assert_eq!(config.fanart_apikey.as_deref(), Some("key"));
        assert_eq!(config.cache_max_age, 60);
        assert!(config.no_cache);
        assert_eq!((config.meta_ttl_ms, config.catalog_ttl_ms), (1000, 2000));
        assert_eq!(config.memory_cache_mb, 64);
        assert_eq!(config.store_timeout_ms, 500);
        assert_eq!(config.kitsu_collection, "kitsu_collection_rs");
        assert_eq!(config.addon_url, "https://example.test");
    }

    #[test]
    fn invalid_or_zero_numbers_fall_back_to_defaults() {
        let config = config(&[("PORT", "abc"), ("CACHE_MAX_AGE", "0"), ("META_TTL", "x"), ("MEMORY_CACHE_MB", "-1")]);
        assert_eq!(config.port, 7000);
        assert_eq!(config.cache_max_age, 43_200);
        assert_eq!(config.meta_ttl_ms, 86_400_000);
        assert_eq!(config.memory_cache_mb, 384);
    }

    #[test]
    fn metrics_credentials_need_both_values() {
        assert_eq!(config(&[("METRICS_USER", "u")]).metrics_credentials, None);
    }

    #[test]
    fn empty_values_count_as_unset() {
        let config = config(&[("MONGODB_URI", ""), ("NO_CACHE", ""), ("ADDON_URL", "")]);
        assert_eq!(config.mongodb_uri, None);
        assert!(!config.no_cache);
        assert_eq!(config.addon_url, "https://anime-kitsu.strem.fun");
    }
}
