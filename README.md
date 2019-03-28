# Stremio Kitsu Anime

Unofficial Kitsu anime catalog fro Stremio platform.

## Usage

This addon provides several anime catalogs (All, Top Rated, Most Popular, Trending) and metadata for them. 

Series id follows convention `kitsu:{kitsu_id}` (ex.: `kitsu:1`) and episode id follows convention `kitsu:{kitsu_id}:{episode_number}` (ex.: `kitsu:1:1`).

Metadata for a specific anime can be retrieved via url `/meta/series/kitsu:{kitsu_id}` .

## Configuration

#### General

 - CACHE_MAX_AGE

#### Cache

 - MONGO_URI
 - NO_CACHE
 - META_TTL
 - CATALOG_TTL