# Search golden files

The Node title search frozen as the reference for the Rust port. A rule change in Node means re-recording these.

## Oracle

- Base commit (HEAD before the dump existed): `5cb9df9e55843c50844a0ef76cb03ba47effe152`. The last commit that changed
  search code is `703ed24`; the only search-code changes since are the `_internals` exports in `lib/title_search.js`
  (`trigramIndex` forces the lazy trigram build), which do not change behaviour.
- Node `v26.4.0`, `jaro-winkler` 0.2.8.
- Inputs: `static/data/search_titles.jsonl` sha256 `1efeedd2faffc38ca62961d3ba760fa0db1b2ad5026a2029656f665a0710ab3f`,
  `static/data/search_aliases.json` sha256 `daa2cc0beb670ec51eb55be375da69c0ac304433143f9e81a21702face1b5ead`,
  `static/data/imdb_mapping.json` sha256 `deb1e45ef0a4cc92761c1d09c1652811e19002a3323e1cfdfcfda6f7ce133f2a`,
  `search_eval/sample.jsonl` sha256 `45acc1e99cbf75aa1ea9990cbc1ed941a7082f57a4f47c572ab9d89f1f87c762`.

## Regenerate

    FANART_APIKEY=test NO_CACHE=true node search_eval/golden/dump.js

Takes about 8 minutes. Deterministic: two runs give identical files. It prints one `sha256  lines  file` line per file
(also written to `SHA256SUMS`, git-ignored, every file of the table); the table below must match. Only `dump.js`, `kernel_sweep.js`, this README and `smoke.jsonl`
are committed.

CI regenerates the goldens and `kernel_sweep.json` on x86-64 Linux, the production platform, in the `check` job (the
dump is cached on the hash of `lib/`, `static/data/`, `search_eval/` and `package-lock.json`) and runs the tests there,
including the `x86_64-unknown-linux-musl` target in release. The goldens recorded on an arm64 Mac therefore differ from
CI's only in `log.jsonl`, and the tests accept that difference only on aarch64.

## Files

Floats are printed with 12 decimals, except `log.jsonl` (17 significant digits). `undefined` fields are omitted; the
dump throws on a non-finite number, Map or Set, so nothing is lost silently. No lone surrogates occur in `norm`,
`parse_query`, `search`, `candidates` or `jw` (0 `\ud800`-`\udfff` escapes in each), so every row loads into a Rust
`String`.
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
- `jw.jsonl`: 5,000 `{a, b, distance}` pairs from the npm `jaro-winkler` (the Rust port's gate): word pairs from
  the corpus titles and queries (normalised and raw, shared prefixes, one-edit mutations, non-ASCII, astral
  characters), phrase pairs, and edge cases (empty strings, identical, single characters).
- `log.jsonl`: 200 `{x, ln}`: `Math.log(x)` for the scorer's idf arguments `entries / (1 + documentFrequency)`, spread
  over the index's distinct document frequencies (including 0), both printed with 17 significant digits. Recorded on
  darwin-arm64 Node, whose `Math.log` (fdlibm's `__ieee754_log` built by clang with FP contraction) uses fused
  multiply-adds: 3 of the 200 values are one ulp off the uncontracted fdlibm V8 runs on x86-64, which the Rust `ln`
  ports. The Rust tests are platform-aware: on x86_64 `ln` must equal Node's `Math.log` bit for bit on every `log.jsonl`
  row and on the sweep digest; on aarch64 `ln` may be one ulp off the rows, and a test-only model of the contracted
  fdlibm must equal the rows and the sweep digest exactly.
- `index.json`: entry, variant and distinct word counts, a sha256 over every posting list (`word\tindex,index...\n`
  in sorted word order: the JS default sort, by UTF-16 code units, which Rust must reproduce by sorting on UTF-16), and the first and last 5 posting lists by word (entry indexes and ids, insertion order). The
  first word is the empty string (entry 134, "X": its imdb mapping title "X" normalises to "", and mapping titles,
  unlike Kitsu's own, are not filtered for empty ones).
- `entries.jsonl`: every index entry in index order, the entry object as built (title variants included, after the
  movie numbering, season grouping, episode layout and split seasons); `absTo` `Infinity` is written as `"Infinity"`.
- `trigrams.json`: the lazy trigram index: gram count, a sha256 over every posting list (`gram\tindex,index...\n`, the
  gram as its three UTF-16 code units in 4-digit hex, grams in the JS default sort), the first and last 5 posting lists.
- `kernel_sweep.js` (committed, not part of the dump): `node search_eval/golden/kernel_sweep.js` prints the count and
  a JSON line with the count and sha256 of `Math.log` over 1,749,545 deterministic inputs (random doubles, around 1,
  integers and their inverses, idf ratios) and of `jaro-winkler` over 300,000 random UTF-16 pairs (lone surrogates
  included). No digest is pinned in the Rust tests: they read the output of the same machine's Node from
  `kernel_sweep.json` (git-ignored; `node search_eval/golden/kernel_sweep.js > search_eval/golden/kernel_sweep.json`,
  under a second). The jw digest is the same on every platform; the log digest depends on the platform (below).
- `smoke.jsonl`: at most 20 rows per kind, `{kind, row}` (committed); the `jw` rows include 5 non-ASCII and 2 astral
  pairs.

## Checksums

| file | lines | sha256 |
|---|---|---|
| `norm.jsonl` | 159671 | `b2c0a488499bc8fc6c60abf6f56bb03a8be57a00a2079b5ca12f5247da5b2e7e` |
| `parse_query.jsonl` | 8496 | `8a1f675db8af43edf3f37033dada00867bed50c8dc3339a1ff8bd3eda4a19f71` |
| `search.jsonl` | 8496 | `f9c32945ee5d6422a94ce1fcd962fa7bccaccc4656edea87de762d6e4b4855cb` |
| `candidates.jsonl` | 200 | `33ea65ccad3d4aff1a1bc2921ca29a4222125d74ca422e11d430589a159f5894` |
| `jw.jsonl` | 5000 | `d97178966ba5c2966f7ba2f06f1bd1a5bc2ac28eef81d60e41e223dc3c294ebe` |
| `log.jsonl` | 200 | `5b8948c166718c3ebbeee36d421c679dde69e138014fd14efb690b4c9e5fd3b5` |
| `index.json` | 582 | `3d78624c544c7fa1bf6ddff332ab0f2d6ebff8a9d2e14ceb0160c4f2624fe685` |
| `entries.jsonl` | 19160 | `a07147964f413033c0a6b3549eb1305bf3a05f10ccda61f23041fe6223a87884` |
| `trigrams.json` | 143 | `64fadf9806ac77a72472577bfdd0fda35e9d461aecdcca270eaf7a92382de349` |
| `smoke.jsonl` | 121 | `d80cc5f04e03f9e50ef4ab80a227ec86f3cb96ff4b7bf5c666ac85a95a135c7b` |
| `dump.js` | 225 | `2a73220f788a173ae4cca1b36fb16015f70da3451b88de53bcbc29f839282ce5` |
