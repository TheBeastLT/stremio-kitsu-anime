const fs = require('fs');
const path = require('path');
const axios = require('axios');
const { animeMappings, kitsuIdByMapping } = require('../lib/kitsu_api');

const FRIBB_URL = 'https://raw.githubusercontent.com/Fribb/anime-lists/master/anime-list-full.json';
const CACHE_PATH = path.join(__dirname, '../static/data/fribb_anime_list.json');
const CACHE_TTL_MS = 24 * 60 * 60 * 1000;

let indexes;

async function getFribbEntry(malId) {
  return (await getIndexes()).byMal.get(String(malId));
}

async function getFribbEntryByKitsu(kitsuId) {
  return (await getIndexes()).byKitsu.get(String(kitsuId));
}

// Whether the MAL entry and the Kitsu entry are the same anime: { linked, reason }.
// Fribb confirms it from either side. Fribb often has a brand-new show as two records it hasn't joined yet (one
// with the MAL id, one with the Kitsu id); then Kitsu's own links to per-entry ids decide: its MAL id against the
// imported MAL id, its AniList/AniDB ids against those on Fribb's MAL record. One agreeing id and none disagreeing
// confirms the link; a disagreeing id proves the Kitsu match is another show. (TVDB/Trakt ids are shared by every
// season of a series, so they can't tell entries apart.)
// lookupKitsuMappings is injectable for tests.
async function malKitsuLink(malId, kitsuId, lookupKitsuMappings = animeMappings) {
  const [byMal, byKitsu] = await Promise.all([getFribbEntry(malId), getFribbEntryByKitsu(kitsuId)]);
  const direct = linkFromFribb(malId, kitsuId, byMal, byKitsu);
  if (direct) {
    return direct;
  }
  const kitsuMappings = await lookupKitsuMappings(kitsuId).catch(() => []);
  return linkFromKitsuMappings(malId, byMal, kitsuMappings);
}

// A direct Fribb record linking the two ids, or one linking either id elsewhere; undefined when Fribb is silent.
function linkFromFribb(malId, kitsuId, byMal, byKitsu) {
  if (String(byMal?.kitsu_id) === String(kitsuId) || String(byKitsu?.mal_id) === String(malId)) {
    return { linked: true };
  }
  if (byMal?.kitsu_id != null) {
    return { linked: false, reason: `Fribb links MAL ${malId} to kitsu ${byMal.kitsu_id}` };
  }
  if (byKitsu?.mal_id != null) {
    return { linked: false, reason: `Fribb links kitsu ${kitsuId} to MAL ${byKitsu.mal_id}` };
  }
  return undefined;
}

const ENTRY_ID_SITES = [
  { site: 'myanimelist/anime', name: 'MAL', expected: (malId) => malId },
  { site: 'anilist/anime', name: 'AniList', expected: (malId, byMal) => byMal?.anilist_id },
  { site: 'anidb', name: 'AniDB', expected: (malId, byMal) => byMal?.anidb_id },
];

// kitsuMappings: Kitsu's own links, [{ externalSite, externalId }]
function linkFromKitsuMappings(malId, byMal, kitsuMappings = []) {
  const agree = [];
  const disagree = [];
  for (const { site, name, expected } of ENTRY_ID_SITES) {
    const want = expected(malId, byMal);
    const have = kitsuMappings.find(mapping => mapping.externalSite === site)?.externalId;
    if (want == null || have == null) {
      continue;
    }
    (String(want) === String(have) ? agree : disagree).push(`${name} ${have}${String(want) === String(have) ? '' : ` != ${want}`}`);
  }
  if (disagree.length) {
    return { linked: false, reason: `kitsu entry is a different show (${disagree.join(', ')})` };
  }
  return { linked: agree.length > 0 };
}

// The kitsu entry Kitsu itself links to this MAL entry's per-entry ids (its MAL id, or the AniList/AniDB ids on Fribb's
// MAL record), when they all agree on one entry. Recovers the right kitsu id when the id mapping service or title
// search picked another (e.g. Precure Fairies Season 4 -> Season 2). lookup is injectable for tests.
async function kitsuIdFromLinks(malId, lookup = kitsuIdByMapping) {
  const byMal = await getFribbEntry(malId);
  const ids = await Promise.all(ENTRY_ID_SITES.map(({ site, expected }) => {
    const id = expected(malId, byMal);
    return id == null ? undefined : lookup(site, id).catch(() => undefined);
  }));
  const found = [...new Set(ids.filter(id => id != null).map(String))];
  return found.length === 1 ? found[0] : undefined;
}

async function isFribbLinked(malId, kitsuId) {
  return (await malKitsuLink(malId, kitsuId)).linked;
}

async function getIndexes() {
  if (!indexes) {
    const list = await loadList();
    indexes = {
      byMal: new Map(list.filter(e => e?.mal_id != null).map(e => [String(e.mal_id), e])),
      byKitsu: new Map(list.filter(e => e?.kitsu_id != null).map(e => [String(e.kitsu_id), e])),
    };
  }
  return indexes;
}

async function loadList() {
  try {
    if (Date.now() - fs.statSync(CACHE_PATH).mtimeMs < CACHE_TTL_MS) {
      return JSON.parse(fs.readFileSync(CACHE_PATH, 'utf8'));
    }
  } catch (err) { /* missing/stale cache, download below */ }
  const { data } = await axios.get(FRIBB_URL, { timeout: 60000 });
  fs.writeFileSync(CACHE_PATH, JSON.stringify(data), 'utf8');
  return data;
}

// Fribb's own imdb id for the entry, if it lists one.
function resolveImdbId(entry) {
  return (entry.imdb_id || []).find(id => /^tt\d+/.test(id));
}

module.exports = {
  getFribbEntry, getFribbEntryByKitsu, isFribbLinked, malKitsuLink, linkFromFribb, linkFromKitsuMappings, kitsuIdFromLinks,
  resolveImdbId,
};
