# Kitsu ↔ IMDb mapping

`static/data/imdb_mapping.json` links Kitsu entries to IMDb series, so Kitsu metas carry IMDb episode numbers and
IMDb metas (`/meta/series/tt….json`) carry `kitsu_id` + `kitsuEpisode` on each video. The runtime is
`lib/metadataEnrich.js`.

## Rows

```json
{ "kitsu_id": 49134, "imdb_id": "tt18311412", "title": "NieR: Automata Ver 1.1a", "fromSeason": 0, "fromEpisode": 1 }
```

- `title` is the IMDb series title, the same on every row of that imdb.
- `fromSeason`/`fromEpisode`: the IMDb episode the entry's Kitsu episode 1 lands on. Omitted for movies (and read
  as S1E1 for series). Season 0 is IMDb's specials.
- `nonImdbEpisodes`: Kitsu episode numbers IMDb doesn't list in the row's run (also ones it lists after another
  row's episodes, which one row can't reach); rare.
- The row format is fixed: solve mapping problems with rows or code, not new fields.

## How the runtime assigns episodes

- **One row per Kitsu id.** Rows are keyed by `kitsu_id`; a second row silently replaces the first. A special can
  only be mapped through Kitsu's own entry for it, never as an episode of an already mapped series entry.
- **A row runs until the next row starts.** For each IMDb video, the last row (by season, then episode) with
  `fromSeason <= season && fromEpisode <= episode` wins, and that row's Kitsu episodes are handed out one after
  another, up to the episode count of an entry that finished airing (from `search_titles.jsonl`; an airing entry has
  no cap). A season 0 row of an airing entry therefore also claims every later special up to the next season 0 row.
  Stop it with a **stopper row**: a row for the next special's own Kitsu entry. When that special has no unmapped
  Kitsu entry, the spill stays.
- **`fromEpisode` is compared without its season.** A row at S1E7 also catches S2E7+ when S2 has no row of its own,
  so map every season.
- Kitsu metas keep Kitsu numbering (`season: 1`, Kitsu episode) and add `imdbSeason`/`imdbEpisode` on each video.

## Adding rows

- **Importer**: `mapping/mal_seasonal_import.js` (CI: "Import MAL seasons") auto-merges verified rows and queues the
  rest in `static/data/new_mappings.json`, keyed by Kitsu id.
- **Review page**: `mapping/mapping_index.html` works through that queue and writes both files on every action. It
  re-reads the live files first, but a write landing in between can still be lost, so ask whether it is open before
  writing either file.
- **Reviewed rows from anywhere else**: `node mapping/add_rows.js <rows.json>`. It puts each row where the page and
  importer would, takes the imdb's title, and refuses Kitsu ids that already have a row.
- Write the data files only through these, and only rows the user approved.

## Checks

- `npm test` (gates every deploy) checks one row per Kitsu id, no two rows starting at the same IMDb episode, the file
  format and the row shape.
- `mapping/offset_check.js` (weekly CI, results in an issue) re-checks the start episodes of recent rows against
  Cinemeta, and flags season 0 rows of those imdbs whose run covers more specials than Kitsu has episodes (airing
  entries only: a finished one's run is capped).

## After a change

The deploy clears the Mongo cache of changed rows (`mapping/clear_mapping_cache.js`). Cloudflare still serves old
metas for up to 12h, so verify on the backend with a cache-busting query: `/meta/series/<id>.json?<timestamp>`.

## Research sources

- Cinemeta: `https://v3-cinemeta.strem.io/meta/series/<tt>.json` (season 0 lists the specials with air dates). It
  rate-limits bursts with HTML responses.
- Kitsu: `https://kitsu.io/api/edge/anime/<id>` (`startDate`, `episodeCount`, `subtype`),
  `anime/<id>/media-relationships?include=destination`, `anime?filter[text]=<title>`. Episode airdates are mostly
  missing, so the entry's `startDate` is the date to match.
