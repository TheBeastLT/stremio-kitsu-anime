# Kitsu Addon Rust Port Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Replace the Node kitsu addon runtime (search included) with a Rust service that returns the same search results and the same responses, on the existing service, keeping the Node helper scripts.

**Architecture:** A Cargo workspace at the root of the kitsu-anime repo: `crates/kitsu-search` (the ranking engine, no I/O) and `crates/kitsu-addon` (axum runtime, domain / application / infrastructure / http like catalogs). The Node files stay in place for the scripts. Parity is proven offline against golden files recorded from Node at a pinned commit, then online by a shadow service replaying real URLs.

**Tech Stack:** Rust 2024, axum 0.8, tokio, reqwest (rustls), mongodb 3 (zstd), prometheus 0.14, `fastjs-regex` 0.1 (published) for every JS regex, `unicode-normalization`, `indexmap`, `arc-swap`.

**Spec:** `docs/kitsu-rust-port-evaluation.md` (sections 3, 5, 6, 8 and the regex addendum). The evaluation recommended waiting for the search rules to settle; the owner decided to port now, the regex blocker having been removed by the published `fastjs-regex`.

## Rulings (mine; each can be reversed before the task that depends on it)

| # | Ruling | Why | Cost if wrong |
|---|---|---|---|
| R1 | Option 3 of the evaluation: the runtime and search in Rust, `mapping/*` scripts, `search_eval/evaluate.js` and the cache-clear step stay Node. | Scripts are heuristic and I/O-bound, their file formats are the contracts, and porting them yields no runtime gain. | Duplicated logic for `toTitleRecord` and `episodeCap`, covered by tests (T5.3). |
| R2 | The code lives in the kitsu-anime repo, in a Cargo workspace next to the Node files. Deploy keeps `deploy.yml`, GHCR and `docker service update`. | The data files, scripts and workflows are there. | Moving later is a `git mv`. |
| R3 | The Rust store reads and writes keyv-compatible documents in the existing `kitsu_collection` (no cold cache, rollback-safe both ways, `clear_mapping_cache.js` unchanged). | Kitsu rate limits are the cold-cache risk, and catalogs' `store.rs` is a template. | Date and null fidelity work (T2.2). |
| R4 | Search rules are frozen on Node from T0.2 until cutover; a rule change in the window is made in Node and the goldens are re-recorded. | Parity is only provable against a fixed target. | Tuning is blocked for the window (about 2 weeks). |
| R5 | Node search is kept after cutover until the importer calls Rust `/search` (T5.3), then deleted. | The importer uses `searchIds`; results must not drift. | One more cutover step. |
| R6 | Node-only runtime metrics (`kitsu_nodejs_*`) become `kitsu_runtime_*`; every other metric keeps its name, labels and buckets. | The dashboard and keep-list change only in the tiles that read them (T5.2). | Five panels edited. |
| R8 | A memory layer in front of Mongo, as in the torrentio addon: moka holding zstd-compressed JSON bytes, sized only by a byte budget like torrentio's stream cache (`MEMORY_CACHE_MB`, default 384, weigher key + compressed bytes, LRU eviction; Node's was 2,000 entries), per-kind TTLs as in `lib/cache.js`. | Mongo is slow for big metas; compressed bytes keep the resident size small, and a byte budget bounds RSS whatever the meta sizes. | The entry count depends on the meta mix: check in the shadow that 384 MB holds the hot set (T2.2b, T4.1). |
| R7 | The landing page is a static page with the install link, not the SDK template. | The SDK template needs Node. | A cosmetic difference. |

## Global Constraints

- Every JS regex goes through `fastjs_regex::Regex` with the original source and flags; never hand-translate a pattern to the `regex` crate.
- JS strings are UTF-16: lengths, slices, indexes and jaro-winkler run on UTF-16 code units. Use `utf16_len` / `utf16_index` from `fastjs-regex`.
- Order is data: posting lists, `Map`/`Set` iteration and the stable sorts follow insertion order (`Vec` / `indexmap`), floats are summed in the same order as the JS, ties resolve as TimSort does (stable).
- Jaro-winkler is a line-for-line port of the npm `jaro-winkler` 0.2.8, not `strsim`.
- No code comments unless the why is non-obvious; logging through `tracing` in the catalogs style; errors reject and log with the message.
- 404 for "no anime behind this id", unknown catalogs and unknown ids; 500 for every other failure; 307 for subtitles; CORS `*`; `cache-control` from `cacheMaxAge` (12 h, `CACHE_MAX_AGE` override); gzip.
- Metrics keep the `kitsu_` prefix, the `route` label values (manifest, catalog, meta, subtitles, search, metrics, landing, other) and the buckets of `lib/metrics.js`.
- Tracked files carry no host names, IPs or node roles; commit with `git commit -- <paths>`; one-line messages; no commit or push without approval.
- Never write to the production Mongo from the shadow service: it uses `kitsu_collection_rs`.

## Review Focus

1. A query with a character outside the BMP, a combining mark or `İ` / final sigma: `norm`, lengths and scores must equal Node's.
2. Two candidates with an equal score: the order must equal Node's (stable sort over insertion order).
3. A query holding a word the index has never seen (trigram path), and a very long bracketed query (the 471 ms case): same ids, and no pathological time.
4. A cached meta written by Node (BSON Date, `null` fields, int32 vs double) read and re-served by Rust, and the reverse.
5. A malformed `/search/%E0.json` (400), a search that throws (500), an unknown catalog (404), `skip` and `genre` extras, `lastVideosIds` over the limit.

---

## Milestone 0: Groundwork

### Task 0.1: Cargo workspace and CI

**Files:** Create `Cargo.toml` (workspace, lints and profiles copied from `catalogs/Cargo.toml`), `rust-toolchain.toml`, `rustfmt.toml` (copy from catalogs), `crates/kitsu-search/{Cargo.toml,src/lib.rs}`, `crates/kitsu-addon/{Cargo.toml,src/main.rs}`; modify `.github/workflows/deploy.yml`.

- [ ] Add the workspace with empty crates and the catalogs lint set.
- [ ] Add a `check` job to `deploy.yml`: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, alongside the existing `npm test`; the image job needs both.
- [ ] Verify: the workflow's `check` runs green on a branch push (no deploy from a branch).

### Task 0.2: Node oracle and golden files (also freezes the search, R4)

**Files:** Create `search_eval/golden/dump.js`, `search_eval/golden/README.md`; output `search_eval/golden/*.jsonl` (git-ignored, regenerated by the script, except the small `smoke.jsonl` committed).

**Interfaces — Produces** (one JSON object per line, floats printed with 12 decimals):
- `norm.jsonl`: `{title, norm}` for every title and synonym in `search_titles.jsonl` and `search_aliases.json`.
- `parse_query.jsonl`: `{query, parsed}` where `parsed` is the full `parseQuery(query)` object, for every `sample.jsonl` query and raw `example`.
- `search.jsonl`: `{query, ids: [{id, score}], confidence}` for the top 20 of `searchIds`-level ranking, same query set.
- `index.json`: entry count, variant count, distinct word count, the first and last 5 posting lists by word.

- [ ] Write the dump script against `lib/title_search.js` (export the internals it needs behind a `module.exports._internals`, a Node-only change).
- [ ] Record the commit hash in `golden/README.md` and the dump's own checksum line.
- [ ] Verify: running the dump twice yields identical files (the search is deterministic).

---

## Milestone 1: `kitsu-search`

Each task ends with its parity gate against the golden files; a gate failure is fixed in Rust, never by editing the golden.

### Task 1.1: Normalisation and records

**Files:** `crates/kitsu-search/src/{norm.rs,records.rs}`, `tests/norm_parity.rs`.

**Interfaces — Produces:** `norm::norm(&str) -> String`; `records::{TitleRecord, read_records(impl Read) -> Result<Vec<TitleRecord>>}` reading `search_titles.jsonl`.

- [ ] Port `norm` (NFKD, strip combining marks, lowercase, `\p{L}\p{N}` cleanup via `fastjs-regex` with the `u` flag, split letters and digits, fold macron vowels) and the record reader.
- [ ] Test `norm` over every line of `norm.jsonl`: 0 differences. A difference in final sigma, `İ` or an NFKD table is handled in code, with a test naming the character.

### Task 1.2: Query parsing

**Files:** `src/query.rs`, `tests/parse_query_parity.rs`.

**Interfaces — Produces:** `query::{parse_query(&str) -> ParsedQuery, season_of(&str) -> Option<Season>, split_titles(&str) -> Vec<String>, glue(..)}`; `ParsedQuery` carries every field `parseQuery` returns, as `Option` where JS yields `undefined`.

- [ ] Port `title_query.js` 57-265: the 12 `seasonOf` rules in priority order, `parseQuery`, `splitTitles`, `glue`. The two variable-length lookbehinds stay regexes (`fastjs-regex` handles them); only `String.replace` templates need care (`$1`, `$2`).
- [ ] Test: `parse_query.jsonl` has 0 differences, field by field.

### Task 1.3: Index

**Files:** `src/index.rs`, `tests/index_parity.rs`.

**Interfaces — Produces:** `Index::build(records, mapping, aliases) -> Index`; `Index::{entries, variants}`; interned words as `u32`, posting lists `Vec<u32>` in insertion order, the trigram index built lazily behind a `OnceLock`.

- [ ] Port entries, title variants (own, synonyms, the mapping's English title), movie numbering, season groups, absolute-episode layout, split seasons and word posting lists (`title_search.js` 133-160, 864-1011).
- [ ] Test: entry, variant and distinct-word counts equal `index.json` (19,160 / 127,786 / 61,773 at the recording), and the sampled posting lists are identical.

### Task 1.4: Scoring kernel

**Files:** `src/{jaro_winkler.rs,score.rs}`, `tests/kernel_parity.rs`.

**Interfaces — Produces:** `jaro_winkler::distance(&[u16], &[u16]) -> f64`; `score::{token_score, title_score}`; candidate generation (rarer words, `< 400` entries, idf-weighted, top 1,000; trigram fallback).

- [ ] Port jaro-winkler line for line (0.7 boost threshold, range formula, code-unit comparison).
- [ ] Port candidate generation and `scoreTitles` (exact tiers 1 / 0.985 / 0.95, then 0.6 token + 0.4 jaro-winkler, numbered-movie floor).
- [ ] Test: the per-query candidate lists and raw scores equal Node's (add them to `search.jsonl` as `candidates` for 200 sampled queries) to 1e-12.

### Task 1.5: Rules, confidence, search

**Files:** `src/{rank.rs,search.rs}`, `tests/search_parity.rs`.

**Interfaces — Produces:** `Searcher::search(&self, query: &str) -> Vec<Hit>` (`Hit { id: u32, score: f64 }`); `MIN_CONFIDENCE = 0.65`.

- [ ] Port `applyRules` (11 bonus rules, about 40 constants, in order), keepExactAhead, preferSeriesOverSynonymExtra, preferRequestedKind, preferFirstOfFranchise, preferNthOfFranchise, distinctive and telling words, split-title reruns, typo coverage and the typo-corrected rerun (`title_search.js` 324-862, 540-598).
- [ ] Make every `undefined` an explicit `Option` decision; `!(entry.episodeCount <= BAND.ordinalSpan)` and `??`/`||` chains are covered by a table test of the edge values.
- [ ] Test: `search.jsonl` has identical id lists and scores to 1e-12 on the whole corpus, **run on the musl target** (`cargo test --target x86_64-unknown-linux-musl` in CI) to cover `ln` against V8's `Math.log`.

### Task 1.6: Evaluation binary and refresh

**Files:** `src/bin/evaluate.rs`, `src/lib.rs` (`SharedIndex` with `ArcSwap`).

**Interfaces — Produces:** `cargo run --release --bin evaluate [-- --diff <file>]` printing the same buckets as `search_eval/evaluate.js`; `SharedIndex::{load, swap, search}`.

- [ ] Port `evaluate.js` (label fixes, buckets, `--diff`).
- [ ] Verify: the bucket table equals Node's (96.3% accepted, 92.4% preferred, 34 no-result at the recording).
- [ ] Benchmark: per-query CPU on the corpus is within 20% of the 3.7 ms prototype on the same machine; record the numbers in the doc.

### Task 1.7: Unicode gap into `fastjs-regex` (follow-up after Milestone 1)

**Files (parse-torrent-title repo):** `crates/fastjs-regex/src/regex.rs` (or a new `unicode.rs`), tests, README; **(kitsu-anime)** `crates/kitsu-search/src/{unicode.rs,norm.rs,query.rs}`, `Cargo.toml`.

**Interfaces — Produces:** `fastjs-regex` matches `\p{L}` and `\p{N}` (and their negations, in classes too) like V8 on Unicode 17: the 4,644 letters and 13 numbers `regress` 0.12 misses, patched inside the crate or by a newer `regress` if one exists; a new `fastjs-regex` version (a bump of its own `version`, released by the publish workflow).

- [ ] Check first whether a released `regress` already carries the newer tables; if so, bump the dependency and delete the workaround. Otherwise patch in `fastjs-regex`: extend the class translation for `\p{L}`/`\p{N}` with the missing ranges, with the sibling test that fails when `regress` catches up kept.
- [ ] Add parity cases (one per range family) to the crate's fixture generated with `gen.mjs` from Node; the parity test stays at 0 failures.
- [ ] In kitsu-search: depend on the new version, delete `NODE_ONLY_LETTERS` / `NODE_ONLY_NUMBERS` and the extra class members appended in six places (`norm.rs` x3, `query.rs` `LETTER` used twice, `index.rs` `DOTTED`), so the pattern sources equal the JS again. `NFKD_UNASSIGNED_IN_NODE` stays (it is a `unicode-normalization` gap).
- [ ] Gate: `norm_parity`, `parse_query_parity` and the `norm_unicode` tests still pass; rerun the code-point differential of the 1.1 report (Node vs Rust over every code point in the 6 contexts) and record 0 differences.
- Needs the owner: the `fastjs-regex` release (tag and publish) — the task stops at a ready, committed change in the parse-torrent-title repo.

### Task 1.8: Goldens in CI and platform-aware `ln` (follow-up after Milestone 1)

**Files:** `.github/workflows/deploy.yml`, `crates/kitsu-search/tests/{kernel_parity.rs,common/mod.rs}`, `search_eval/golden/README.md`.

- [ ] The `check` job generates the goldens itself before `cargo test`: Node setup, `npm ci`, `FANART_APIKEY=test NO_CACHE=true node search_eval/golden/dump.js` (about 8 minutes), cached with `actions/cache` keyed on the hash of `lib/**`, `static/data/**`, `search_eval/**` and `package-lock.json`, so a push that changes none of them skips the dump. `KITSU_GOLDEN_OPTIONAL` stays unset in CI.
- [ ] The `ln` tests become platform-aware: on x86_64 `math::ln` must equal Node's `Math.log` exactly on `log.jsonl` and the sweep digest; on aarch64 (where Node contracts fdlibm with FMA) they allow at most 1 ulp and say so in the assertion message. Remove any digest pinned to the arm64 recording.
- [ ] Also run `cargo test --workspace --release --target x86_64-unknown-linux-musl` against the generated goldens (CI only); document in the golden README that CI regenerates the goldens on x86-64 Linux, which is the production platform.
- [ ] Verify what can be verified locally: the YAML parses, the tests pass here on aarch64, the cache key expression lists existing paths. State plainly that the first real run is the owner's push.

---

## Milestone 2: runtime

### Task 2.1: Skeleton, config, errors

**Files:** `crates/kitsu-addon/src/{main.rs,lib.rs,domain/*,application/error.rs,infrastructure/config.rs}`.

**Interfaces — Produces:** `domain::{Meta, CatalogMeta, Manifest}` serde types with the field order of the Node responses (`serde_json` `preserve_order`); `application::Error::{NotFound, Failed}` (404 / 500).

- [ ] Port the manifest from `addon.js` verbatim (ids, names, extras, `idPrefixes`, version) and reject unknown catalog ids with `NotFound`.
- [ ] Test: `GET /manifest.json` equals the Node manifest byte for byte (golden).

### Task 2.2: Keyv-compatible store

**Files:** `infrastructure/store.rs`.

**Interfaces — Produces:** `Store::{get, mget, set}` over `kitsu_collection` with the key formats `stremio-kitsu|meta:<id>`, `|catalog:<id>|<genre>|<skip>`, `|images:<id>`, `|id_mapping:<type>:<id>`; documents `{key, value: {value, expires}, expiresAt}`, unique `key` and TTL `expiresAt` indexes assumed (created by hand, as today).

- [ ] Start from catalogs' `store.rs`; add the `{value, expires}` wrapper, `expires` checking on read, `$in` for `mget`.
- [ ] Dates: render BSON Date as `toISOString` (`.sssZ`); read-through `null`s as absent where the Node response omits them, tested on a recorded document.
- [ ] Test: round-trip against a document dumped from the live collection (fixture, no secrets) both ways; a document Rust wrote deserialises in `@keyv/mongo` (a Node test in `test/`).

### Task 2.2b: Memory layer (R8)

**Files:** `infrastructure/cache.rs` (the layering and in-flight deduplication, replacing the Node `cache-manager` wrap), tests.

**Interfaces — Produces:** `Cache::{wrap_meta, wrap_catalog, wrap_images, wrap_id_mapping, cached_metas(&[id])}` with the Node semantics: memory hit → Mongo hit (promoted to memory) → load, concurrent callers for one key share one load; `kitsu_cache_requests_total{kind,result}` counted as in `lib/cache.js` (a hit includes callers that waited on a load); `NO_CACHE` bypasses both.

- [ ] A moka cache of zstd-compressed JSON bytes (level as in the torrentio addon's `cache.rs`, reuse its packing helper pattern), `max_capacity` a byte budget from `MEMORY_CACHE_MB` (default 384) with the weigher key length + compressed bytes, as torrentio's stream cache (no entry-count limit), LRU eviction, the configured MB logged once at startup, with a per-entry expiry from the kind's TTL (meta and catalog 24 h by `META_TTL` / `CATALOG_TTL`, images 14 d / 4 d non-English logo / 2 d none, id mapping 14 d).
- [ ] `cached_metas` serves memory hits first and fetches only the rest with one Mongo `$in`.
- [ ] Tests: promotion from Mongo to memory, expiry per kind, two concurrent callers produce one load and count as one miss and one hit, a failing Mongo only skips the cache, a corrupt entry is treated as a miss.
- [ ] Record resident memory and the entry count once the budget fills in the shadow (T4.1) and set the stack's memory limit from `MEMORY_CACHE_MB` (about 1.1 x budget + base + search index).

### Task 2.3: Kitsu client

**Files:** `infrastructure/kitsu.rs`, `tests/fixtures/kitsu/*.json` (recorded responses).

**Interfaces — Produces:** `KitsuApi::{anime_data(id), anime_entries(options), anime_catalog_metas(ids), anime_title_page(page), anime_start_entries()}`; a small JSON:API deserialiser resolving `included` for genres, episodes and `mediaRelationships.destination`.

- [ ] Port `lib/kitsu_api.js` with a 30 s timeout, no retries; Kitsu 404 maps to `NotFound`.
- [ ] Test with `wiremock` and the recorded pages: the deserialised struct equals the Node `kitsu` library's output (golden JSON from a Node dump).

### Task 2.4: Other upstreams

**Files:** `infrastructure/{cinemeta.rs,fanart.rs,ids.rs,opensubtitles.rs}`.

**Interfaces — Produces:** `Cinemeta::get(imdb_id)` (series then movie, 3 retries, 60 s), `Fanart::{images, tvdb_id}` (fanart.tv REST, TheTVDB XML lookup, metahub HEAD probes), `Ids::to_kitsu(id)` (yuna.moe for `mal:`, `anilist:`, `anidb:`), `opensubtitles::redirect_url(meta, args)`.

- [ ] Port each module of `lib/` with the same URLs and error mapping, keeping the upstream labels (`kitsu`, `cinemeta`, `fanart`) for metrics.
- [ ] Test with `wiremock`: retry counts, the series-then-movie fallback, a Cinemeta 404.

### Task 2.5: Metadata and episode mapping

**Files:** `application/{metadata.rs,enrich.rs,mapping.rs}`; data read from `static/data/imdb_mapping.json`.

**Interfaces — Produces:** `Mapping::{imdb_mapping(id), episode_cap(..)}`; `metadata::to_catalog_meta`; `enrich::{enrich_kitsu, enrich_imdb}` returning the same `Meta` JSON as `metadata.js` / `metadataEnrich.js` (absolute-episode offsets, `imdbEpisodes` lists, run hand-out stepping over listed specials, `nonImdbEpisodes`).

- [ ] Port `metadata.js` (190 lines) and `metadataEnrich.js` (278 lines) following `mapping/README.md`.
- [ ] Golden test: for every kitsu id in `imdb_mapping.json` with a recorded Kitsu and Cinemeta fixture, the Node-built meta JSON equals the Rust-built one (keys sorted, dates as instants). The fixture set is recorded by a Node script and committed compressed.

### Task 2.6: HTTP surface

**Files:** `http/{server.rs,extras.rs,landing.rs}`.

**Interfaces — Produces:** routes `GET /manifest.json`, `/catalog/{type}/{id}.json` and `/catalog/{type}/{id}/{extra}.json` (SDK extra parsing: `search=`, `skip=`, `genre=`, `lastVideosIds=`; `?` query dropped before parsing), `/meta/{type}/{id}.json`, `/subtitles/{type}/{id}[/{extra}].json` (307 to opensubtitles-v3), `/search/{q}.json` (`{ids}`, 400 on a malformed query), `/metrics` (basic auth, constant-time compare, 404 when unset), landing page (R7).

- [ ] Port the catalog handler: search (invalid-URL query rejected; found ids → cached metas via `mget`, the rest in one Kitsu request), `lastVideosIds` (`metasDetailed`, 20 ids, last 20 videos), browse (cached by `catalog:<id>|<genre>|<skip>`).
- [ ] Port the meta handler (kitsu-like ids, `tt` ids via the imdb mapping, 404 mapping) and the cache-wrap with in-flight deduplication.
- [ ] Tests (axum `oneshot`): every route's status, headers (CORS, `cache-control`, `content-encoding` with `accept-encoding: gzip`), the 404-versus-500 table, extras parsing edge cases.

### Task 2.7: Metrics

**Files:** `infrastructure/metrics.rs`.

**Interfaces — Produces:** the registry of `lib/metrics.js` with identical names, labels and buckets; the cache counters and the store timer; `kitsu_runtime_*` (tokio worker busy ratio, task count, RSS, CPU via the `process` feature) replacing `kitsu_nodejs_*` (R6).

- [ ] Port the middleware (`routeOf` with non-slash URLs, `/search` counted, bounded catalog label).
- [ ] Test: the `/metrics` body of a request mix contains each family with the same label sets as the Node `test/metrics.test.js` asserts.

### Task 2.8: Main

**Files:** `src/main.rs`, `docker-entrypoint.sh`.

- [ ] Config from env (`PORT`, `MONGODB_URI`, `METRICS_USER`, `METRICS_PASSWORD`, `FANART_APIKEY`, `CACHE_MAX_AGE`, `NO_CACHE`), the hourly title-index refresh (`title_index.js`: up to 5 pages of the newest entries, then a rebuild swapped in), and graceful shutdown on SIGTERM / SIGINT.
- [ ] Verify: starts with only the swarm secrets exported by the entrypoint; SIGTERM exits 0 within the stop grace period (a test like `test/shutdown.test.js`).

---

## Milestone 3: packaging

### Task 3.1: Image and workflow

**Files:** `Dockerfile` (cargo-chef + zigbuild musl, as `catalogs/Dockerfile`), `Dockerfile.dockerignore`, `.github/workflows/deploy.yml`.

- [ ] Build the image with the binary, `static/data/*` baked in, and the entrypoint; the Node image is tagged `node-<sha>` and kept for rollback.
- [ ] Deploy workflow: `check` (Rust + `npm test`), image build and push, `docker service update`, then the mapping cache-clear on the runner as today.
- [ ] Verify: the image runs locally with `NO_CACHE=true` and answers the manifest and a search.

---

## Milestone 4: shadow

### Task 4.1: Shadow service and compare

**Files:** a shadow stack file and an ops section in the infrastructure repo; **(kitsu-anime repo)** `shadow/compare.mjs`.

- [ ] A shadow service on loopback, pinned to the production host, `MONGODB` collection `kitsu_collection_rs`, no nginx routing, capped CPU and memory.
- [ ] `compare.mjs` replays a sampled day of URLs against the production service and the shadow service: `/search` and catalog-search ids exactly; meta and catalog JSON with `null` normalised to absent and dates compared as instants.
- [ ] Gate: zero search differences; only cache-age differences elsewhere; shadow `kitsu_*` series in Grafana for CPU and RSS.
- [ ] Remove the shadow service the same day the comparison finishes (a one-off left running blocks deploys).

---

## Milestone 5: cutover and cleanup

### Task 5.1: Cutover

- [ ] Swap the `kitsu_addon` image to the Rust build (`docker service update`); limits to 0.5 CPU / 256M.
- [ ] Watch 5xx, `kitsu_meta_requests_total` outcomes and p95 for a week; rollback is the `node-<sha>` tag.

### Task 5.2: Dashboard and keep-list

**Files (stremio-torrentio repo):** `vmagent-scrape.yml`, `gen-kitsu-dashboard.mjs`, `docs/monitoring.md`.

- [ ] Replace `kitsu_nodejs_*` with `kitsu_runtime_*` in the keep-list; edit the event loop, heap and active handles panels (R6); regenerate and import the dashboard.

### Task 5.3: Importer and cleanup

**Files (kitsu-anime):** `mapping/mal_seasonal_import.js`, `mapping/offset_check.js`, `lib/title_search.js` and the Node runtime files (`app.js`, `index.js`, `serverless.js`, `addon.js`), `test/`.

- [ ] Point the importer's search at the Rust `/search` (a base-URL option, the deployed URL by default) and add a fixture test that `toTitleRecord` and `episodeCap` match the Rust values.
- [ ] After a week with no diffs, delete the Node runtime and `title_search.js`; keep `lib/` modules the scripts still import and `npm test` for them.
- [ ] Update `CLAUDE.md`, `README.md` and `docs/ops.md`; release the freeze (R4).

---

## Milestone 6: improvements after cutover

Made in Rust only, one task each, each checked against `evaluate` and the corpus; the goldens are re-recorded when a task changes results. Starting points: `docs/kitsu-rust-port-evaluation.md` section 7 option 1.

### Task 6.1: Search result cache

- [ ] A process-local cache of search results keyed by the normalised query string, cleared on every index swap; count hits and misses in `kitsu_cache_requests_total{kind="search"}`. Add the kind to `docs/monitoring.md`.

### Task 6.2: Skip the paged search

- [ ] The search catalog ignores `skip`: a request with `skip > 0` returns an empty `metas` without searching (browse catalogs are untouched). Verify first, from a sampled access log, that Stremio's `skip>0` search requests repeat the first page; if they do not, drop the task.

### Task 6.3: Cap the long-query tail

- [ ] Cap `split_titles` parts and typo reruns for very long queries (the 200 ms+ cases, all long bracketed names). The cap's threshold is chosen on the corpus so that accepted and preferred rates stay the same on `evaluate`; report both before and after.

### Task 6.4: Dashboard

**Files (stremio-torrentio repo):** `gen-kitsu-dashboard.mjs`.

- [ ] Split "Cache hit % (meta+catalog)" into separate meta and catalog tiles; add the search-cache hit ratio from T6.1.

## Success criteria

- Zero search differences on the goldens (musl build) and in the shadow comparison.
- `kitsu_process_cpu_seconds_total` at least 50% lower at equal traffic; RSS under 120 MB; `/search` p99 under 30 ms in production.
- No change in `meta_requests_total` outcomes or the 5xx rate over the week after cutover.

## Decisions taken by the owner (2026-10-25)

- **R4 (freeze), R3 (keyv-compatible store) and R6 (`kitsu_runtime_*`) are accepted.**
- **R8 (memory layer of zstd bytes, sized by a 384 MB byte budget like torrentio's stream cache) is added.**
- **Milestone 6 follows cutover.**
- Execution: subagent-driven, one implementer per task, a reviewer after each milestone.
