// Keep an imdb's entries together, with season-0 specials trailing at the end of the group. A regular
// season goes after the last non-special sibling; a special goes after the very last sibling. A
// brand-new imdb slots into kitsu-id order.
function insertMappingEntry(base, item) {
  const siblings = base.filter(e => e.imdb_id === item.imdb_id);
  if (!siblings.length) {
    insertInKitsuOrder(base, item);
    return;
  }
  const anchors = item.fromSeason === 0 ? siblings : siblings.filter(e => e.fromSeason !== 0);
  if (anchors.length) {
    base.splice(base.indexOf(anchors[anchors.length - 1]) + 1, 0, item);
  } else {
    base.splice(base.indexOf(siblings[0]), 0, item); // only specials exist -> put this before them
  }
}

// never between two rows of one imdb (a franchise's group starts at its first season's kitsu id)
function insertInKitsuOrder(base, item) {
  const sameGroup = (a, b) => !!a.imdb_id && a.imdb_id === b.imdb_id;
  const idx = base.findIndex((e, i) => e.kitsu_id > item.kitsu_id
      && (i === 0 || !sameGroup(base[i - 1], e))
      && base.slice(i).every(x => x.kitsu_id > item.kitsu_id || sameGroup(x, e)));
  base.splice(idx === -1 ? base.length : idx, 0, item);
}

// Inserts the rows of verified import entries ({ kitsuId, imdb_id, title, fromSeason?, fromEpisode? }) into base,
// skipping kitsu ids that have a row. Returns the merged entries, and as review entries the ones starting at an imdb
// episode another row starts at (the imdb meta gives that episode to one of them, npm test rejects the pair).
function mergeVerifiedRows(base, entries) {
  const startOf = row => row.fromSeason !== undefined ? `${row.imdb_id} S${row.fromSeason}E${row.fromEpisode}` : undefined;
  const mapped = new Set(base.map(row => row.kitsu_id));
  const starts = new Map(base.filter(startOf).map(row => [startOf(row), row.kitsu_id]));
  const merged = [];
  const clashing = [];
  for (const { _trusted, ...entry } of entries) {
    const item = { kitsu_id: parseInt(entry.kitsuId), imdb_id: entry.imdb_id, title: entry.title };
    if (entry.fromSeason !== undefined) {
      item.fromSeason = entry.fromSeason;
      item.fromEpisode = entry.fromEpisode;
    }
    if (mapped.has(item.kitsu_id)) {
      continue;
    }
    const start = startOf(item);
    if (starts.has(start)) {
      clashing.push({ ...entry, reviewReason: `${start} is already where kitsu ${starts.get(start)} starts` });
      continue;
    }
    insertMappingEntry(base, item);
    mapped.add(item.kitsu_id);
    if (start) {
      starts.set(start, item.kitsu_id);
    }
    merged.push(entry);
  }
  return { merged, clashing };
}

// The mapping files' format: 2-space JSON, a list of numbers on one line ("imdbEpisodes": [40, 41, 49, 50]).
// mapping_index.html keeps a copy.
function formatJson(object) {
  return JSON.stringify(object, null, 2).replace(/\[\s+((?:-?\d+|null)(?:,\s+(?:-?\d+|null))*)\s+\]/g,
      (_, items) => `[${items.split(/,\s+/).join(', ')}]`);
}

module.exports = { insertMappingEntry, insertInKitsuOrder, mergeVerifiedRows, formatJson };
