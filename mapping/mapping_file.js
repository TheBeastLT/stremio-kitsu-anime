// Rows are kept in groups, an imdb's rows together, ordered by their first row's kitsu id. A row of a mapped imdb goes
// among its rows by season and episode, specials (season 0) after every season; a new imdb starts a group of its own.
function insertMappingEntry(base, item) {
  const siblings = item.imdb_id ? base.filter(row => row.imdb_id === item.imdb_id) : [];
  if (!siblings.length) {
    insertInKitsuOrder(base, item);
    return;
  }
  const next = siblings.find(row => compareRuns(row, item) > 0);
  base.splice(next ? base.indexOf(next) : base.indexOf(siblings[siblings.length - 1]) + 1, 0, item);
}

// A new group goes where its kitsu id falls between two groups' first ids; as the file isn't fully sorted there can be
// several such places, and it takes the one after the group starting closest below it.
function insertInKitsuOrder(base, item) {
  const starts = base.map((row, i) => i).filter(i => i === 0 || !sameGroup(base[i - 1], base[i]));
  const firstIds = starts.map(i => base[i].kitsu_id);
  // before group g (or at the end, g = firstIds.length) when it falls between their first ids: of those places the
  // one after the group starting closest below it
  let group;
  for (let g = 0; g <= firstIds.length; g++) {
    const below = g ? firstIds[g - 1] : -Infinity;
    const above = g < firstIds.length ? firstIds[g] : Infinity;
    if (below < item.kitsu_id && item.kitsu_id < above && (group === undefined || below > firstIds[group - 1])) {
      group = g;
    }
  }
  base.splice(group === undefined || group === firstIds.length ? base.length : starts[group], 0, item);
}

const sameGroup = (a, b) => !!a.imdb_id && a.imdb_id === b.imdb_id;

// season, then episode, with specials (season 0) after every season
function compareRuns(a, b) {
  const season = row => row.fromSeason === 0 ? Infinity : row.fromSeason ?? 1;
  return (season(a) - season(b)) || ((a.fromEpisode ?? 1) - (b.fromEpisode ?? 1));
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
