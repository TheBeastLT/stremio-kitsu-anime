# Kitsu addon: Rust port evaluation

_2026-10-25. Evaluated kitsu-anime at `c0498ef`. `703ed24` (metric prefix, 404 for unknown catalogs) landed during the
evaluation and changes no search code. Measured on an Apple M4 Pro with Node 26.4 (production runs `node:22-alpine`). All
benchmarks ran offline from a scratch copy, with upstreams stubbed: 3 live fixture fetches in total._

## 1. Recommendation

- **Don't port now.** Spend 1-2 days on Node (option 1) and reconsider a full runtime port with the search core in Rust
  (option 3, scripts stay in Node) once the search rules settle.
- **Search is the service's CPU.** In a production-like replay it took 60% of busy CPU (title ranking 39%, jaro-winkler
  21%). At today's rate of 1.2-2.3 searches/s, that is most of the 0.05-0.09 cores the process uses.
- **A Rust search is about 3-4x cheaper,** measured with a prototype of the hot loop: 14.1 → 3.7 ms per query, and it
  produced the same top-5 ids and scores to 12 decimals on all 3,143 queries. That saves an estimated 0.03-0.05 cores
  and, with a Rust runtime, about 150-200 MB of RSS on the shared production host. The absolute gain is small.
- **The cost is parity and a moving target.** The search changed in 20 commits over 12 days (1,356 lines added). A port
  either freezes that tuning or makes every rule change twice. The helper scripts import `lib/`, so they would keep a
  second implementation alive.
- **Go when** the rule churn drops to under about 1 commit a week for a month, or when the production host's CPU or RAM becomes the
  binding constraint. Then port in the order of section 8, with a byte-level parity gate on search.

## 2. How Torrentio and catalogs were ported, and what carries over

**What was ported.**
- The addon: a Rust workspace under `addon/crates/{addon,debrid}`, laid out as domain / application / providers /
  infrastructure / http, built on axum, tokio and moka (`addon/README.md`).
- Catalogs: a standalone Cargo project, `catalogs/src/{domain,application,infrastructure,http}`.

**Timeline** (history in `stremio-torrentio-rust`, which was the staging tree):
- `d1b7b30` (2026-10-23) added the Rust addon.
- `a98c074`, `32dff15` and `b766488` fixed the cache, the rate limiter and the 404 mapping while it ran in shadow.
- `852ecd6` (2026-10-24) replaced the Node addon.
- `6cc1617` (the same day) replaced Node catalogs.
- It took about 2 calendar days of AI-assisted work. The logic being ported was stable and mostly I/O.

**How parity was verified.**
- A shadow service (removed in `852ecd6`) ran next to the Node addon. It listened on loopback only, used a separate
  Mongo database (`MONGODB_DATABASE`), had no public routing and capped resources.
- `addon-rs/shadow/compare.mjs` diffed the stream lists and resolved the same cached streams on both, then compared the
  chosen file.
- Expected differences were whitelisted: Node's 3-day-old cache, ties at the row limit, and the newer title parser.
- There was no corpus-level diff of a ranking algorithm; the addon had none to port. PTT parsing went to the published
  `parse-torrent-title` crate, and a changed label was accepted ("labels differ, parser version").

**Gains and costs** (`addon/README.md`):

| Benchmark | Node, 1 worker | Rust |
|---|---|---|
| Cold Postgres path | 635 req/s | 3,004 req/s |
| Hot path with TorBox | 5.1k req/s | 30.2k req/s |
| Production RSS | 0.8-1.7 GB | ~0.25 GB plus cache |

Costs:
- A restart empties the in-memory cache, and CPU runs about 10% high for 1-2 hours.
- Some deliberate 404-vs-500 behaviour changes .
- A CPU A/B test on another service was abandoned unfinished. Its pinned one-off service blocked that host's deploys
  until it was removed.

**Metrics.**
- The addon kept the Node metric names "for everything that still applies".
- Catalogs did not keep them: it adds `app=`, uses `resource` rather than `route`, and adds an `outcome` label
  (`catalogs/README.md`, Metrics table).
- So the precedent is "same names where they still apply", not strict parity.

**Cache.**
- The addon dropped keyv and moved to its own zstd collections, accepting a cold cache.
- Catalogs kept keyv-compatible documents in `torrentio_catalog_collection` (`catalogs/src/infrastructure/store.rs`:
  `{key, value, expiresAt}`, a unique `key` index, a TTL index on `expiresAt`, and Node-driver-compatible
  int32/double number encoding).

**What carries over to kitsu.**
- The catalogs skeleton: axum, tower-http gzip and CORS, an entrypoint that exports secrets, a musl Dockerfile with
  cargo-chef and zigbuild, a deploy workflow (fmt, clippy and test, then `docker service update --image`), and a
  single-replica stack with stop-first updates. The addon's stack is already shaped like it.
- The shadow-service-plus-compare approach.
- The keyv-compatible store code.

**What does not carry over.** Nothing in the precedent ported a hand-tuned ranking engine whose exact output feeds
stored data. The scraper stores the kitsu ids that search returns (a reprocess after a rule change can
return different ids than at ingestion).

## 3. Search algorithm

### Structure

| Part | Where | Size |
|---|---|---|
| Query and title parsing: `norm` (NFKD, strip combining marks, lowercase, `\p{L}\p{N}` cleanup, split letters and digits, fold macron vowels); `seasonOf` (S/season/ordinal/roman/part/cour/final, 12 rules in priority order); `parseQuery` (year, movie N, recap/re-edit markers, theatrical, extra word and number, season and part, pack words, episode); `splitTitles`; `glue` (glued words and initials) | `lib/title_query.js` 57-265 | 272 lines, about 81 regexes, 4 lookbehinds, 6 lookaheads |
| Index build: entries from records plus imdb mapping, title variants (own, synonyms, the mapping's English title), movie numbering, season groups, absolute-episode layout, split seasons, word → entries posting lists, lazy trigram index | `lib/title_search.js` 133-160, 864-1011 | about 250 lines |
| Candidate generation: entries sharing the query's rarer words (<400 entries), idf-weighted, top 1,000; a trigram fallback for unseen words | `title_search.js` 189-220 | |
| Title scoring: exact tiers (1 / 0.985 mapping / 0.95 synonym); otherwise 0.6 × idf token score (dice + coverage, with glue and initials) + 0.4 × jaro-winkler from the first shared word; numbered-movie floor | `title_search.js` 164-186, 260-322 | |
| Ranking rules: keepExactAhead, preferSeriesOverSynonymExtra, preferRequestedKind, preferFirstOfFranchise, preferNthOfFranchise, distinctive and telling words, then 11 bonus rules in `applyRules` (season, title form, episode, marker, type, extra, movie, year, plain query, distinctive, coverage) with about 40 tuned constants | `title_search.js` 324-538, 600-862 | about 550 lines |
| Confidence (`MIN_CONFIDENCE` 0.65), split-title reruns, typo coverage and typo-corrected rerun | `title_search.js` 540-598 | |
| Data: `static/data/search_titles.jsonl` (11.2 MB, 19,606 records), `search_aliases.json`, `imdb_mapping.json` (0.87 MB) | `lib/title_records.js`, `lib/title_index.js` | |
| Refresh: hourly fetch of up to 5 pages of Kitsu's newest entries, then a full index rebuild | `lib/title_index.js` 51-66 | |

**Per-query complexity.**
- Search does not scan every entry. It looks up the posting lists of the query's words.
- Measured over the eval corpus: candidates per query p50 142, p90 464, p99 870. 17 of 3,143 queries hit the 1,000 cap.
- Each candidate scores all of its title variants (6.7 per entry on average, 127,786 in total). Each variant costs a
  `glue`, a Set-based token score and 1 + (query words × title words) jaro-winkler calls.
- `search()` can run `rank()` up to 2 + N times: the whole query, each split part and the typo rerun.
- 110 of 3,143 queries contain a word the index has never seen. They take the trigram path, which builds a trigram index
  over every entry on first use.

**Index scale.** 19,160 searchable entries, 127,786 variants, 61,773 distinct words, 3.25 M characters.

### Node baseline

Commands, run in `scratchpad/kitsu-rust-eval`: `app/` is a copy of `lib/`, `static/`, `search_eval/` and the entry files,
with `node_modules` symlinked.

```
node search_eval/evaluate.js                      # in app/
node --expose-gc bench.js                         # loads the index, runs every sample.jsonl query through searchIds
node proto/dump.js && node proto/kernel.js        # hot-loop kernel, JS
cargo build --release --offline && ./target/release/kernel .. && ./target/release/kernel2 ..   # in proto/rs
```

**Eval (`evaluate.js`).**
- 3,147 sample queries; 3,102 scored after the `label_fixes.jsonl` exclusions.
- Overall: 96.3% accepted, 92.4% preferred, 34 with no result. The old Kitsu text search scored 84.6%.
- The weakest buckets are "curated miss: unknown season" (71.9%) and "real: names a recap" (83.3%).
- The whole run takes 50 s, almost all of it in search.

**Load.**

| Measurement | Value |
|---|---|
| `require` (mapping plus a `readRecords` for `episodeCap`) | 178 ms |
| First search (builds the index) | 1,076 ms (load and build CPU 1.41 s) |
| RSS | 44 MB → 169 MB after require → 275 MB after the index |
| Heap used after the index | 83 MB |

Production RSS is 260-280 MB, which matches.

**Per query**, warm, single thread, `searchIds` = what `/search` runs:

| Corpus | n | p50 | p95 | p99 | max | CPU per query | queries/s |
|---|---|---|---|---|---|---|---|
| eval queries (scraper-cleaned titles) | 3,147 | 7.8 ms | 53.9 ms | 110 ms | 471 ms | 15.5 ms | 64 |
| raw torrent names (`examples` in sample.jsonl, uncleaned) | 5,351 | 48.9 ms | 301 ms | 522 ms | 875 ms | 84.2 ms | 12 |

**What is slow.**
- The slowest queries are long romaji names with a bracketed English title, such as "loop 7-kaime no akuyaku reijou ...
  (7th time loop ...)" at 471 ms. They run twice through `splitTitles`, and long strings make jaro-winkler quadratic.
- Queries over 20 ms account for 70% of all search time.
- Raw names are 5x slower, so callers must keep cleaning queries.

**Production** (Grafana `grafanacloud-prom`; only about 1.5 hours of `job="torrentio-kitsu"` data exist since the move to
the current deployment, so this is a short window):

| Measurement | Value |
|---|---|
| HTTP traffic | 3.1-5.3 req/s |
| Searches (`search_requests_total`) | 1.2-2.3 /s, almost all catalog searches (`catalog_requests_total{kind="search"}` 1.22/s) |
| Scraper's `/search` route | about 3 per hour (Cloudflare caches it for 12 h) |
| `process_cpu_seconds_total` | 0.05-0.09 cores |
| RSS | 260-280 MB |
| Event-loop lag p99 | 22-37 ms |
| `/search` route latency | p50 20 ms, p95 48 ms (few samples) |
| Catalog latency (includes the Kitsu fetch) | p50 49 ms, p95 1.99 s |

0.048 cores ÷ 1.26 searches/s ≈ 38 ms of CPU per search, counting all CPU. The local model of the same mix predicts
0.029 cores. **The production vCPU is therefore about 1.7x slower than the M4 core (derived estimate),** and a production
search costs about 25-30 ms of CPU.

### Rust estimate (prototype, labelled)

**What the prototype is.**
- `proto/rs` ports the hot loop exactly: candidate generation, `glue`, the idf token score, `charSimilarity`, and a line
  for line port of the npm `jaro-winkler` (UTF-16 code units, its 0.7 prefix-boost threshold and range quirk).
- It runs over the same title bases (`titleBaseOf(norm(title))`) and query bases (`parseQuery(q).base`).
- It leaves out parsing, the ranking rules and confidence. In JS the same kernel costs 14.1 of the full 15.5 ms, so it
  covers about 90% of search CPU.

| Kernel | mean CPU per query | p50 | p95 | p99 | max | load + index | RSS |
|---|---|---|---|---|---|---|---|
| JS (as shipped: npm jaro-winkler) | 14.1 ms | 7.4 | 49.1 | 91.2 | 257 | 89 ms (data pre-normalised) | 198 MB |
| JS with a typed-array jaro-winkler | 13.3 ms | 7.0 | 46.2 | 87.7 | 226 | | |
| JS with that plus precomputed words and memoised word-pair similarity | 13.5 ms | 6.5 | 48.6 | 95.1 | 290 | | 703 MB |
| Rust, direct port | 4.6 ms | 2.4 | 15.8 | 29.7 | 82 | 88 ms | 110 MB |
| Rust, stack buffers and precomputed UTF-16 words | **3.7 ms** | 1.9 | 12.6 | 23.7 | 61 | 112 ms | 144 MB (UTF-16 copies kept; a tuned layout needs less) |

All three JS variants and both Rust variants produce byte-identical top-5 `id:score` lines (12 decimals) for all 3,143
queries.

**Estimate for a full Rust search** (extrapolated, not measured): about 4-5 ms per query on the M4 and about 7-9 ms on
the production vCPU, against 25-30 ms in Node today.

**What that means in production.**
- At 1.2-2.3 searches/s, Rust saves about 0.03-0.05 cores, roughly 3-5% of the 1-core production host.
- Event-loop blocking from search (p99 about 110 ms locally, about 190 ms estimated in production) disappears on a
  multi-threaded runtime.
- Micro-optimising the JS hot loop gains only 5-6%. The JS cost is spread over Sets, splits and allocations, not one
  hotspot.
- Index startup in a real port includes `norm`, which the prototype skipped (estimated 0.2-0.4 s against Node's 1.25 s).

### How a Rust search would be built

**Layout.** A `kitsu-search` library crate with no I/O:
- `query.rs`: a port of `title_query.js`.
- `index.rs`: entries, posting lists and the trigram index.
- `rank.rs`: scoring, the rules and confidence.
- `bin/evaluate.rs`: a port of `search_eval/evaluate.js`.

The server crate holds the index in an `ArcSwap<Index>`, so the hourly refresh swaps it in without blocking reads.

**Data structures.**
- Interned words (`u32` ids).
- Posting lists as `Vec<u32>` in insertion order. JS `Map` and `Set` iteration order feeds the float sums and the stable
  sorts, so this order must be kept, using `indexmap` where it matters.
- Precomputed per-variant word ids and UTF-16 buffers.
- No FST or trie is needed. The candidate set is already small, the cost is in scoring, and fuzzy index structures would
  change which candidates are scored.

**Parity risks**, roughly in order:
1. **Regex semantics.** JS `\b`, `\d` and `\w` are ASCII-only, while Rust `regex` makes them Unicode by default. This
   matters because 17,050 variants keep non-ASCII letters. Every regex needs `(?-u:\b)` and ASCII classes, with `\p{L}`
   kept where JS uses the `u` flag.
   - The `regex` crate has no lookaround. Ten patterns use it (`title_query.js` 18-19, 106, 122, 149 and others).
   - `(?<!\b\d{1,4})` and the alternation lookbehind on line 18 are variable-length, which even `fancy-regex` rejects.
     They must be rewritten as code.
   - `String.replace` with `$1`/`$2` templates and global regexes with `lastIndex` must be reproduced.
2. **String units.** `length`, `slice`, `[0]`, `charCodeAt` and jaro-winkler all work on UTF-16 code units. A Rust port
   on `char` or bytes changes scores for any non-BMP character and for every non-ASCII length (UTF-8). Use UTF-16 or
   `char` counts consistently, and test astral characters.
3. **Unicode normalisation.** `normalize('NFKD')` uses V8's ICU tables; the `unicode-normalization` crate pins its own
   Unicode version. `toLowerCase()` and Rust `to_lowercase()` both do full mapping, with edge cases in final sigma and
   `İ`. Diff all 127k titles through both `norm`s as part of the gate.
4. **Floating point and ordering.**
   - Sums follow Set and Map insertion order.
   - `Math.log` is V8's fdlibm. A musl build uses musl `log`. They were identical on macOS here; CI runs the release
     tests on the musl target (production's libm), gating ids, order and confidence.
   - Ties decide outcomes: `score === best`, `keepExactAhead`'s `-0.001`, and the stable sorts (`sortByScore` has no
     tie-break and relies on V8 TimSort over insertion order).
5. **JS `undefined` semantics.** Logic such as `!(entry.episodeCount <= BAND.ordinalSpan)`, `undefined > 1` and the
   `??`/`||` chains on optional numbers is relied on implicitly. Each one becomes an explicit `Option` decision that has
   to match.
6. **The jaro-winkler implementation.** Use the npm algorithm, not `strsim`: the 0.7 boost threshold, the range formula
   and code-unit comparison all matter. The prototype's port matched exactly.
7. **A moving target.** 20 search commits between 2026-10-12 and 2026-10-23, 1,356 lines added and 71 deleted, plus 11
   `search_aliases.json` edits. During and after a port every rule change has to land in both implementations, or the
   tuning loop (`evaluate.js --diff`) moves to Rust for good.

## 4. Where Node CPU actually goes

**Setup.** The app copy ran with `NO_CACHE=true`.
- `axios` adapters were stubbed (both the root copy and the private `kitsu/node_modules/axios` 0.27). They serve
  recorded Kitsu `anime/7442?include=genres,episodes,mediaRelationships.destination`, a 20-entry `anime?filter[id]`
  page and Cinemeta `tt2560140`.
- `url-exists` and fanart were stubbed. Gzip was requested.

**CPU per request type** (`drive.mjs`, 300 sequential requests each, server-side `process.cpuUsage`):

| Request | CPU ms/req | Notes |
|---|---|---|
| catalog search, eval-corpus queries | 26.7 | search plus building 20 catalog metas (uncached) |
| catalog search, plain alias names | 22.8 | |
| `/search/{q}.json` | 15.2 | search only |
| catalog browse | 12.2 | 42 KB response |
| meta, imdb (mapped, uncached) | 8.0 | 92 KB response |
| meta, kitsu (uncached build) | 6.9 | |
| subtitles (uncached meta, then 307) | 5.7 | |
| manifest | 0.4 | |
| meta, imdb not found | 0.15 | |

**Profile of a production-like mix.** `node --cpu-prof server.js` with 2,000 requests weighted like production: 45%
catalog search, 28% kitsu meta, 21% subtitles, 17% imdb 404s. All metas were cache misses, so this overstates the
non-search share. Startup is excluded.

| Category | Share of busy CPU |
|---|---|
| Search: ranking and parsing | 38.8% |
| Search: jaro-winkler | 21.4% |
| axios `transformResponse` (JSON parse of upstream bodies) | 13.1% |
| node http/net | 7.2% |
| Kitsu JSON:API deserialise | 5.3% |
| native/unattributed | 3.9% |
| GC | 3.4% |
| Meta building and enrichment | 2.7% |
| gzip | 1.4% |
| express / SDK routing | 0.9% |
| metrics | 0.4% |

**Top self time:** `jaro-winkler distance` 21.4%, title_search anonymous closures 9.7%, `glue` 8.7%, `scoreTitles`
5.0%, `charSimilarity` 4.3%, `compact` 4.2%.

**Conclusion.** Search is about 60% of CPU even with every meta uncached. Production serves about 60% of metas from cache
(`cache_requests_total` meta hit 1.16/s against 0.76/s misses), so search's real share is higher. Gzip, Mongo
serialisation and routing are noise. In a Rust runtime the next items would be JSON parse and the JSON:API deserialise,
together about 18% of today's CPU.

## 5. Helper scripts

| script | purpose | trigger | depends on | recommendation |
|---|---|---|---|---|
| `mapping/update_search_titles.js` | rebuilds `search_titles.jsonl`: Kitsu crawl (~20 min, 1 req/s) plus anime-offline-database synonyms | weekly (`search-titles.yml`, Mon 07:00), then deploy | `title_records.toTitleRecord`, `title_search.SEARCHED_SUBTYPES`, `kitsu_api.animeTitlePage` | **Keep in Node.** The data file is the contract: Rust only reads the JSONL. `toTitleRecord` (fragment and ASCII filters) must not be duplicated, and the hourly refresh in a Rust runtime needs the same function, so port it once and test it against the file with a round-trip diff. 1 commit. |
| `mapping/mal_seasonal_import.js` (+ `fribb_mapping`, `imdb_search`, `mal_seasons`, `mapping_file`) | weekly MAL season import → verified rows auto-merged, the rest queued for review | weekly (`mapping-import.yml`, Mon 05:00) | `kitsu_api` (searchIds, animeData, episode counts), `title_index.loadCurrentTitleIndex`, `metadataEnrich.getImdbMapping`, `id_convert`, `cinemeta_api` fresh, `fanart.getTvdbId`, cheerio | **Keep in Node.** 481 lines, 26 commits, heuristic and I/O-bound. It uses search (`searchIds(malEntry.title)`), so after a Rust port it must call the Rust `/search` (local container or deployed URL) or results drift. |
| `mapping/kitsu_recent_import.js` | same pipeline for entries Kitsu added late | same job | `kitsu_api`, `metadataEnrich`, the importer | **Keep in Node** with the importer. |
| `mapping/mapping_verifier.js` | pure decision rules: is a mapping safe to auto-merge | library of the importers and the offset check | none (pure) | **Keep in Node.** It doesn't run in the addon; porting buys nothing. |
| `mapping/offset_check.js` | weekly re-check of recent rows' IMDb offsets against fresh Cinemeta, kept in an issue | weekly (`mapping-offset-check.yml`, Thu 05:00) | `kitsu_api.animeStartEntries`, `cinemeta_api.getFreshCinemetaMetadata`, `metadataEnrich.episodeCap` | **Keep in Node.** It uses `episodeCap`, which must match the runtime's cap: a shared rule, documented in `mapping/README.md`. A Rust runtime duplicates it, so cover it with a test fixture. |
| `mapping/clear_mapping_cache.js` | deletes `kitsu_collection` keys of changed mapping rows after a deploy | `deploy.yml`, when `imdb_mapping.json` changed | `mongodb`, git | **Keep in Node** (or as a 10-line `mongosh` step). It encodes the cache key format `stremio-kitsu|meta:<id>`, which a Rust store must keep (section 6). |
| `mapping/add_rows.js`, `mapping_file.js` (`formatJson`, `insertMappingEntry`) | manual row insertion, keeping the exact file format | manual | `mapping_file` | **Keep in Node.** The file format is the contract, and the review page `mapping_index.html` (browser, File System Access API) writes the same files. |
| `search_eval/evaluate.js` | offline search accuracy gate | manual, on every search change | `title_index`, `title_records` | **Port to Rust if search is ported** (`cargo run --bin evaluate`), plus a parity mode that diffs Node and Rust over the corpus. Otherwise keep. |
| `npm test` (1,811 test lines; `title_search.test.js` 10 cases) | deploy gate | `deploy.yml` | everything | Search and runtime tests move to `cargo test`. Mapping-file tests stay in Node, run in a separate CI job. |

**Keeping a Node toolchain.**
- After a runtime port, Node would remain only in CI for the 3 scheduled workflows and the cache-clear step. That costs
  nothing at runtime: none of it ships in the Rust image.
- The real cost is duplicated logic. `toTitleRecord`, `episodeCap`/`getImdbMapping` (mapping indexing), the cache key
  format and search itself would each exist twice.
- Share through data files (the JSONL and mapping files are already the contracts) and through HTTP or CLI calls: the
  importer calling `/search`, or a Rust `kitsu-search` CLI the scripts shell out to. Don't port the scripts.

**Must match the runtime exactly:**
- The title records the hourly refresh builds against what the weekly rebuild wrote.
- `episodeCap` in `offset_check`.
- Search results used by the importer.
- Cache keys used by the cache clear.

## 6. The rest of the runtime, cache and metrics compatibility

**Surface** (`addon.js` 194 lines, `serverless.js` 41, `lib/*` about 1,800 lines excluding search):
- The `stremio-addon-sdk` router: manifest; catalog with SDK extra parsing (`search=`, `skip=`, `genre=`,
  `lastVideosIds=`); meta; subtitles returning `{redirect}` → **307** to opensubtitles-v3; and the landing page from the
  SDK template.
- The SDK sets CORS `*` and `cache-control` from `cacheMaxAge` (12 h).
- `req.url` drops `?…` before the SDK parses extras (`serverless.js` 34).
- `/search/{q}.json` (`serverless.js` 16-31) returns `{ids}`: the router decodes the query (a malformed one is a 400),
  and a search exception is a 500.
- Unknown ids, missing imdb mappings, Kitsu 404s and (since `703ed24`) unknown catalogs are 404 (`noHandler`). Other
  rejections are 500.
- `/metrics` uses basic auth with timing-safe comparison.
- `compression()` gzip.
- SIGTERM closes the server and idle connections (`index.js`).

**Kitsu client.**
- The `kitsu` npm library: JSON:API with includes and deserialisation, its own axios 0.27, a 30 s timeout and no retries.
  A Rust port needs a small JSON:API deserialiser covering `included` resolution for genres, episodes and
  `mediaRelationships.destination`.
- Cinemeta: series, then movie, 3 retries, 60 s timeout. Also a "fresh" variant with a cache-break query carried across
  307s, which only scripts use.
- TMDB fallback: dead code (`IMDB_IDS_FOR_TMDB` is empty).
- Fanart through `fanart.tv-api`/`got`, a TVDB XML lookup (`xml-js`), metahub HEAD probes (`url-exists`).
- yuna.moe id mapping for `mal:`/`anilist:`/`anidb:`.

**Meta building and episode mapping.** `metadata.js` (190 lines) and `metadataEnrich.js` (278 lines) hold
absolute-episode offsets, `imdbEpisodes` lists, run hand-out stepping over listed specials, `nonImdbEpisodes` and
`episodeCap`. These rules are documented in `mapping/README.md`. They are about as intricate as search but change less:
22 commits across all runtime files since 2026-10-01.

**Mongo cache compatibility.** Read from `@keyv/mongo` 3.1.0 `dist/index.cjs` 60-250, `keyv` 5.6.0 `get`/`getMany`/`set`,
`cache-manager` 7.2.9 `wrap`/`mget`/`set`, `mongodb` 7.7.0 (bson 7.3.3) and `lib/cache.js`; verified in T2.2 with the
real keyv/@keyv/mongo/cache-manager code over an in-memory collection (`crates/kitsu-addon/tests/fixtures/record_store.js`).
- Collection: `kitsu_collection`, in the URI's default database.
- Document: `{ _id, key, value: { value: <payload>, expires: <epoch ms as BSON double> }, expiresAt: Date }`. `expires`
  is a JS number above the int32 range, so the driver writes a double. `expiresAt` comes from a second `Date.now()` in
  `@keyv/mongo`, so it can be a few ms after `expires`. A non-number TTL (for example `META_TTL` set in the env, which
  stays a string) writes `null` for both, and the document never expires.
- Keys: `stremio-kitsu|meta:<kitsuId>` (the bare number, not `kitsu:<id>`) or `|meta:tt<id>`,
  `|catalog:<catalogId>|<genre or All>|<skip or 0>`, `|images:<kitsuId or imdbId>`, `|id_mapping:<type>:<id>` (the full
  id, for example `mal:1`).
- Payloads: meta `{ meta, cacheMaxAge }`, catalog `{ metas, cacheMaxAge }`, images an object, id_mapping the kitsu id.
- `useKeyPrefix: false` and `serialize: undefined`: payloads are stored as raw BSON, not JSON strings, and keyv wraps
  them in `{value, expires}`. A string `value` (an old keyv JSON string) reads as a miss.
- Indexes: unique `key`, and TTL on `expiresAt`. `@keyv/mongo` creates both on every connect, so they are not
  hand-made; the Rust store doesn't create them.
- keyv checks `value.expires` on read (`Date.now() > expires`) and deletes the expired keys it reads. The Rust store only
  treats them as misses; the TTL index removes them.

**What a Rust store must replicate** (catalogs' `store.rs` is the template, plus the inner wrapper):
- Read `value.value` and honour `value.expires`.
- Write the same 3 fields, upserted on `key`, with `expires` as a double.
- **Dates:** only the kitsu metas' `videos[].released` are `Date` objects, stored as BSON Date; Cinemeta-built `tt`
  metas keep `released` strings. Node serves dates as `toISOString()` (`YYYY-MM-DDTHH:mm:ss.sssZ`, always with
  milliseconds), so Rust must render BSON dates the same way, and write `released` as a Date for kitsu meta keys only.
- **Nulls:** the Node driver's default `ignoreUndefined=false` stores `undefined` fields as `null` (checked). Cached hits
  therefore carry `null`s where fresh responses omit fields. The Rust store keeps them, and a cached value is served as
  read.
- **Numbers:** the driver writes a JS number as int32 when it is a safe integer in the int32 range and not `-0`, and as
  a double otherwise (never int64). Catalogs' rule differs at `i32::MIN` written as a float and at `-0`. On read, Node
  prints whole doubles without a fraction (`5000000000`, not `5000000000.0`).
- `mget` uses `{key: {$in}}`.

With this, there is no cold cache at cutover. Accepting a cold cache instead is also reasonable: production Mongo hits
are ~1.2/s, and a cold cache means ~2/s extra Kitsu and Cinemeta calls for a day. Kitsu rate limits are the risk there,
not CPU.

**Metrics parity.**
- The dashboard (`gen-kitsu-dashboard.mjs`) and the vmagent keep-list (`vmagent-scrape.yml` 47-66) use `kitsu_`-prefixed
  names (since `703ed24`).
- The keep-list includes `kitsu_nodejs_eventloop_lag_seconds`, `_p99_seconds`, `_heap_size_used_bytes` and
  `_active_handles_total`, which have no Rust equivalent. Map them to `runtime_*` like the addon did and update the
  keep-list and dashboard in the same change.
- `process_cpu_seconds_total` and `process_resident_memory_bytes` come from the `prometheus` crate's `process` feature
  (Linux).
- Keep `route` label values (manifest, catalog, meta, subtitles, search, metrics, landing, other), the histogram buckets
  `[0.01 … 10]`, and the labels of `cache_requests_total{kind,result}`, `upstream_request_duration_seconds{upstream,outcome}`,
  `catalog_requests_total{catalog,kind}`, `search_requests_total{result}` and `meta_requests_total{id_type,outcome}`.

**Container, stack and CI.**
- Copy the catalogs Dockerfile, entrypoint (secrets `mongodb_uri`, `metrics_user`, `metrics_password`, `fanart_apikey`)
  and deploy workflow pattern.
- The addon's stack file would only change its image and limits: 768 MB → about 256 MB, matching catalogs.
- Data files: either baked into the image (as Node does now, where weekly data commits trigger deploys) or mounted. Keep
  baking: the workflows already redeploy on data commits.

**Rollout.**
- Precedent: a shadow service on loopback, pinned to the production host, with a separate Mongo
  collection (for example `kitsu_collection_rs`) so the two caches don't cross-contaminate.
- A compare script replays URLs against both.
- **Lesson from an abandoned A/B test:** remove the pinned one-off right after measuring.

## 7. Options

Effort is in engineer-days, assuming AI-assisted work like the precedent. Ranges are estimates.

| Option | Effort | Expected gain | Risks |
|---|---|---|---|
| **1. Stay on Node, optimise hotspots** | 1-2 | Small. JS hot-loop micro-optimisation measured only 5-6%. The levers are elsewhere (below). | Low. Every change is checked with `evaluate.js --diff`. |
| **2. Rust search core only**, as a napi-rs module or a sidecar | 5-8 (port 1,300 lines and 100+ regexes, parity harness, napi build in the Node image) | About 0.03-0.05 cores; event loop unblocked. With napi the scripts keep working through it. | Two toolchains in one image (napi on musl and arm64 builds). Every rule change is made in Rust only from then on, so the tuning loop must move. A sidecar adds a hop and a second service for 1-2 req/s. |
| **3. Full runtime port in Rust, scripts stay in Node** | 12-19 (search 5-8; SDK surface, Kitsu JSON:API, meta and episode mapping, enrichment, keyv-compatible store, metrics 5-8; shadow and cutover 2-3) | 0.04-0.07 cores (search plus JSON parsing). RSS about 275 → 60-100 MB (estimate). No event-loop stalls. Consistent with Torrentio and catalogs. | Duplicated `toTitleRecord` and `episodeCap`. The importer must call Rust search. The episode-mapping port carries its own parity risk. Rule churn during the port. |
| **4. Full port including scripts** | 18-29 (option 3 plus about 2,000 lines of heuristic, I/O-bound script code with 26+ commits on the importer alone) | Removes Node from CI only. No runtime gain beyond option 3. | Highest. The scripts are the fastest-changing part, and the file formats (`formatJson`) and the browser review page stay JS anyway. |

**Option 1 items**, cheapest first:
1. A process-local LRU of search results keyed by the trimmed query, cleared on index rebuild. Cloudflare caches per
   full URL, but whether queries repeat at the origin is unknown; see section 9.
2. A guard so the search catalog doesn't recompute for `skip>0`. The search handler ignores `skip` (`addon.js` catalog
   handler), so a paged search reruns the full search and returns the same page. Its frequency in production is
   unmeasured.
3. Cap `splitTitles` and typo reruns for very long queries; the >200 ms tail is all long bracketed names.
4. Optionally move search into a `worker_thread`. This doesn't save CPU but removes event-loop stalls.

**Recommended:** option 1 now. Option 3 later, behind the go criteria in section 1. Not 2, because it carries the full
parity cost of search while leaving the runtime in Node. Not 4.

## 8. Phased plan and parity verification (when the go criteria are met)

1. **Freeze window and corpus.**
   - Collect real queries. Production doesn't log them today: add a sampled log line, or a `search_queries` counter dump,
     for a week.
   - Build the corpus from `search_eval/sample.jsonl` (3,147), its raw `examples` (5,351) and the sampled production
     queries.
   - Record the Node outputs as golden files: full id lists in order, scores and confidence, at a pinned commit.
2. **`kitsu-search` crate.**
   - Port `title_query` and `title_search` behind a `norm` parity test over all 127k titles plus the corpus.
   - Gate: identical id lists and scores to 1e-12 on the whole corpus, built and run **on the musl target** (to check
     `Math.log` against musl `log`). Since 2026-10-26: identical ids, order and confidence, still on the musl target.
   - Port `evaluate.js` and get identical bucket numbers.
   - From then on, rule changes land in Rust first, and Node is frozen.
3. **Runtime.**
   - Routes and SDK semantics (extras parsing, 307, CORS, cache headers, 404 vs 500).
   - The JSON:API client.
   - Meta and episode mapping, with a golden test: build every mapped kitsu id's meta from recorded fixtures in Node and
     in Rust and diff the JSON.
   - The keyv-compatible store (Date and null rules above) and the `kitsu_` metrics.
4. **Shadow.**
   - Run a shadow service on loopback with its own collection.
   - A compare script replays a day of URLs (from a sampled access log) against both. Diff `/search` exactly. Diff
     meta/catalog JSON with `null` and absent normalised and dates compared as instants.
   - Expect differences only from cache age.
5. **Cutover.**
   - Swap the `kitsu_addon` image. Keep the Node image tag for rollback.
   - Update the vmagent keep-list and the dashboard for `runtime_*`.
   - Point the importer's search at the Rust `/search`.
   - Remove the A/B service the same day.

**Success metrics:**
- Zero search diffs on the corpus.
- `kitsu_process_cpu_seconds_total` −50% or more at equal traffic.
- RSS under 120 MB.
- `/search` p99 under 30 ms in production.
- No change in `meta_requests_total` outcomes or the 5xx rate for a week.

**What would change the recommendation:**
- Search traffic growing about 10x (for example, the scraper switching to uncached `/search` for reprocess runs).
- Production host CPU steal or RAM pressure becoming an incident cause.
- Search rules reaching a stable state.
- The production search tail (p99 about 190 ms estimated) causing user-visible timeouts.

## 9. Open questions

1. Do catalog search queries repeat at the origin, behind Cloudflare? This decides whether a result LRU (option 1) or
   Rust matters. A one-week sampled query log would answer it.
2. Does Stremio send `skip>0` for the search catalog, and how often? The handler ignores `skip`.
3. Is the scraper meant to use the cached public `/search` (about 3/h now), or will reprocess and backfill runs hit an
   uncached origin in bursts?
4. Is a cold Mongo cache acceptable at cutover (simpler), or should the Rust store read the keyv documents?
5. Should the hand-tuned search stay JS for good, as the tuning workbench with `evaluate.js`? If yes, option 3 should
   keep search out of scope, running the Rust runtime with search behind a Node sidecar. That inverts option 2, and its
   cost-benefit is weak.

_Scratch artefacts (not in the repo): `scratchpad/kitsu-rust-eval/{bench.js,server.js,drive.mjs,analyze.js,proto/}`,
results in `out/` and `prof/`._

## Addendum: the PTT regex layer against the kitsu search patterns

Probe (2026-10-25), scratch crate outside both repos: every regex literal of `lib/title_query.js`, `title_search.js`,
`title_index.js` and `title_records.js` (86 unique; 9 with lookaround, 3 of those lookbehind, 42 with `\b`, 6 with
`\p{}`, 6 with the `u` flag) was run on the 3,668 strings of the eval set (queries plus their NFKD-lowercased forms)
in Node and through `parse_torrent_title::Regex` (`regress` fallback + translated `regex` fast path), comparing the
first match's UTF-16 index, text and every capture group.

- All 86 compile, including the variable-length lookbehinds that `fancy-regex` rejects.
- 315,448 comparisons (50,920 real matches), 0 mismatches.
- Not covered: patterns built with `new RegExp(string)` (none inspected), `g`-flag iteration/replace/split semantics
  (the crate has `find_iter`, `replace`, `split`, not exercised here), and the other regex-using files (`lib/metadata*`,
  mapping scripts).

Effect on the evaluation: the "regex semantics" parity risk in section 3 shrinks to "use this crate". The remaining
parity risks (insertion-order dependence, `Math.log` vs musl `log`, UTF-16 scoring, which `utf16_len`/`utf16_index`
help with) and the churn argument are unchanged, so the recommendation stays; the port estimate loses the regex work.

## Rust port results

Measured on an M-series Mac, release build, single thread, after the `kitsu-search` crate was finished.

- Parity: 8,496 of 8,496 golden queries match Node on ids, scores (12 decimals) and confidence. The evaluation table (`evaluate`, plain, `--raw`, `--diff`) is byte-identical to `node search_eval/evaluate.js`: 96.3% accepted, 92.4% preferred, 34 no-result.
- Latency over the golden corpus (8,496 queries, a harder mix than the sample):

| | mean | p50 | p95 | p99 | max |
|---|---|---|---|---|---|
| Rust `search` | 10.9 ms | 5.8 | 39.9 | 77.7 | 137 |
| Node `index.search` | 56.9 ms | 26.3 | 224.0 | 447.5 | 870 |

- Evaluation sample (3,102 queries): 4.36 ms per query in Rust, against the 3.7 ms of the prototype (+18%, inside the 20% budget).
- Index build: 1.05 s and 114 MB peak RSS for the whole process (records, index and trigrams). The trigram index is built once per index; a swapped-in index builds it before it is published.
- Worst case: one fuzzed query took 472 ms (reruns for the split parts and the typo correction, dominated by jaro-winkler).
- Determinism: ten fresh index builds, each searching the whole corpus, produced identical output.
- Since the simplification pass (2026-10-26) the gate is the same ranked ids, order and confidence; scores are no longer
  compared to 12 decimals, and the fdlibm `ln` port, the UTF-16 handling and the `toFixed` rounding are gone.
