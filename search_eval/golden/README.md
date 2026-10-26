# Search golden files

The Node title search as the reference for the Rust port. The Rust tests gate on the same ranked ids, order and
confidence (confidence and candidate title scores to a 1e-9 tolerance); a rule change means re-recording these.

## Oracle

- Base commit (HEAD before the dump existed): `5cb9df9e55843c50844a0ef76cb03ba47effe152`. The last commit that changed
  search code is `020b1bd` (a name without letters only matches an entry titled exactly so), re-dumped at merge
  `a0f2c44`.
- Node `v26.4.0`.
- Inputs: `static/data/search_titles.jsonl` sha256 `1efeedd2faffc38ca62961d3ba760fa0db1b2ad5026a2029656f665a0710ab3f`,
  `static/data/search_aliases.json` sha256 `81b98ecb4e57d2f19807bdb2580ebb28ddf23ad0cc82e03bc8064c025622f705`,
  `static/data/imdb_mapping.json` sha256 `7209cc4ac78f660ad4d96c839cedeed55a2fcb039eb1e87dc70e3bbd9ff07dc3`,
  `search_eval/sample.jsonl` sha256 `45acc1e99cbf75aa1ea9990cbc1ed941a7082f57a4f47c572ab9d89f1f87c762`.

## Regenerate

    FANART_APIKEY=test NO_CACHE=true node search_eval/golden/dump.js

Takes about 8 minutes. Deterministic: two runs give identical files. It prints one `sha256  lines  file` line per file
(also written to `SHA256SUMS`, git-ignored, every file of the table); the table below must match. Only `dump.js`, this
README and `smoke.jsonl` are committed.

CI regenerates the goldens in the `check` job (the dump is cached on the hash of `lib/`, `static/data/`, `search_eval/`
and `package-lock.json`) and runs the tests there in release on the `x86_64-unknown-linux-musl` target, production's
libm, with overflow checks on.

## Files

Floats are printed with 12 decimals. `undefined` fields are omitted; the dump throws on a non-finite number, Map or Set,
so nothing is lost silently. No lone surrogates occur in `norm`, `parse_query`, `search` or `candidates`, so every row
loads into a Rust `String`.
Queries are every `sample.jsonl` query then every raw `examples` name, de-duplicated in first-seen order (8,496).

- `norm.jsonl`: `{title, norm}` for every distinct title, abbreviated title and synonym in `search_titles.jsonl` and
  `search_aliases.json`.
- `parse_query.jsonl`: `{query, parsed}`, `parsed` the full `parseQuery(query)` object.
- `search.jsonl`: `{query, ids: [{id, score}], confidence}`, the top 20 of the `searchIds` ranking (`search` with
  limit 20): only the final top 20 and the top result's confidence, no intermediate reruns. `ids` is empty when the confidence is below 0.65 (`MIN_CONFIDENCE`), `confidence` is still the top
  result's value, or `null` when nothing was ranked.
- `candidates.jsonl`: `{query, candidates: [{id, score}]}` for 200 queries (every 15th of the distinct `sample.jsonl`
  queries, i.e. every 15th of the first 3,000): only the whole-query rank, no split-part or typo reruns. The candidates `candidatesFor` hands the scorer for the whole query, in that order, with the raw title
  score from `scoreTitles` before any ranking rule or bonus. Empty when the query has no name left to match.
- `index.json`: entry, variant and distinct word counts, a sha256 over every posting list (`word\tindex,index...\n`
  in the JS default sort, by UTF-16 code units, which the test reproduces), and the first and last 5 posting lists by
  word (entry indexes and ids, insertion order). The
  first word is the empty string (entry 134, "X": its imdb mapping title "X" normalises to "", and mapping titles,
  unlike Kitsu's own, are not filtered for empty ones).
- `entries.jsonl`: every index entry in index order, the entry object as built (title variants included, after the
  movie numbering, season grouping, episode layout and split seasons); `absTo` `Infinity` is written as `"Infinity"`.
- `trigrams.json`: the lazy trigram index: gram count, a sha256 over every posting list (`gram\tindex,index...\n`, the
  gram as its three UTF-16 code units in 4-digit hex, grams in the JS default sort), the first and last 5 posting lists.
  Rust builds trigrams over chars, so this golden holds only while no indexed base has a character outside the BMP.
- `smoke.jsonl`: at most 20 rows per kind, `{kind, row}` (committed).

## Checksums

| file | lines | sha256 |
|---|---|---|
| `norm.jsonl` | 159672 | `f1b53758e6f386f90588270c8d9ec5248aa54e6de59ef7f592b8bba057982241` |
| `parse_query.jsonl` | 8496 | `8a1f675db8af43edf3f37033dada00867bed50c8dc3339a1ff8bd3eda4a19f71` |
| `search.jsonl` | 8496 | `6737f5577ff72e33c1c3d662159dec02f1c017f98942c296b81208c4a2342387` |
| `candidates.jsonl` | 200 | `33ea65ccad3d4aff1a1bc2921ca29a4222125d74ca422e11d430589a159f5894` |
| `index.json` | 582 | `fa7a068e38ade4ae661c361b7ab72210e5c14a19395b828b5cdf69599f8c3375` |
| `entries.jsonl` | 19160 | `029bb090bad5d51f68274c9cb2a60f8c5530f400c44cd2471f54ab9328a1f10e` |
| `trigrams.json` | 143 | `a2be2e8015332e2ac88fe0abcc8004cf6cf8eb1b152190e4a56df0c9b24c9ebc` |
| `smoke.jsonl` | 81 | `4b4af032d48b9e28fd1eb7187826c4aec42a928d1d8e17368d90da22914cded3` |
| `dump.js` | 140 | `b9a7c17e0db669bed7d602527745ce6638d1ce47d3859417961ba4d50b6cd922` |
