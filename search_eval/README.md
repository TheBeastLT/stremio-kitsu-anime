# Search evaluation

Offline evaluation of the catalog search (`lib/title_search.js`, the in-memory title index): no requests.

    node search_eval/evaluate.js           # top-1 per bucket, next to the Kitsu text search it replaced
    node search_eval/evaluate.js --diff    # also every query whose top result differs from the Kitsu search
    node search_eval/evaluate.js --raw     # against the original labels, without label_fixes.jsonl

- `sample.jsonl`: 3,147 torrent-derived queries (as the torrentio scraper builds them) in buckets: real torrent titles
  labelled by their stored mappings, generated alias spellings of every Kitsu title, curated misses of the old search,
  numbered franchise movies, OVAs/specials, packs of a series with its extras, recaps.
- `label_fixes.jsonl`: reviewed corrections of the sample's labels (many came from the old search's own mistakes):
  `correct` replaces the accepted ids; `garbage`, `absent` (not on Kitsu) and `ambiguous` rows are left out.
- `kitsu_search_tops.json`: the old Kitsu `filter[text]` search's top result per query, for the comparison.

The ranking rules were tuned on this set; keep every bucket at or above the Kitsu search, and review every changed top
result (`--diff`) before changing them.
