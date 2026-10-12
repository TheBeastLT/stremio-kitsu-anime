# Stremio Kitsu Anime

Unofficial Kitsu anime catalog fro Stremio platform.

## Usage

This addon provides several anime catalogs (All, Top Rated, Most Popular, Trending) and metadata for them. 

Series id follows convention `kitsu:{kitsu_id}` (ex.: `kitsu:1`) and episode id follows convention `kitsu:{kitsu_id}:{episode_number}` (ex.: `kitsu:1:1`).

Metadata for a specific anime can be retrieved via url `/meta/series/kitsu:{kitsu_id}` .

Search (`/catalog/anime/kitsu-anime-list/search={query}.json`) ranks an in-memory index of every Kitsu anime's titles,
built from `static/data/search_titles.jsonl` (rebuilt weekly by `.github/workflows/search-titles.yml`), and returns
nothing when no title matches well enough. `/search/{query}.json` returns only the matching kitsu ids, best first
(`{ "ids": ["41174", ...] }`), without fetching metas. The index adds title synonyms from
[anime-offline-database](https://github.com/cedya77/anime-offline-database) (forked from
[manami-project](https://github.com/manami-project/anime-offline-database)), made available under the
[Open Database License](https://opendatacommons.org/licenses/odbl/1-0/).

## Configuration

#### General

 - CACHE_MAX_AGE

#### Cache

 - MONGO_URI
 - NO_CACHE
 - META_TTL
 - CATALOG_TTL