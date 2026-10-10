- A push to master deploys to production (`.github/workflows/deploy.yml`, gated by `npm test`).
- Running locally: modules throw at load without `FANART_APIKEY`; `NO_CACHE=true` skips Mongo/memory caching.
  Load env with `export $(grep -E '^(FANART_APIKEY|TRAKT_CLIENT_ID)=' .env | xargs)`; never print `MONGODB_URI`.
- Caches: in-memory LRU → Mongo (`kitsu_collection`) → Cloudflare (12h). Check the backend with `?<timestamp>` on the URL;
  keyv stays on 5 (cache-manager 7 can't read keyv 6 entries, see `test/cache.test.js`).
- Mapping rows (`static/data/imdb_mapping.json`, `new_mappings.json`): read `mapping/README.md` before researching or adding any.
