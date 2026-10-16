const fs = require('fs');
const axios = require('axios');
const cheerio = require('cheerio');
const { queryIdMapping } = require('../lib/id_convert');
const { getImdbMapping } = require('../lib/metadataEnrich');
const { searchIds, animeData, animeEpisodeCounts } = require('../lib/kitsu_api');
const { loadCurrentTitleIndex } = require('../lib/title_index');
const { getTvdbId } = require("../lib/fanart");
const {
  getFribbEntry, getFribbEntryByKitsu, isFribbLinked, malKitsuLink, kitsuIdFromLinks, resolveImdbId,
} = require('./fribb_mapping');
const { getFreshCinemetaMetadata } = require('../lib/cinemeta_api');
const { insertMappingEntry, insertInKitsuOrder, formatJson } = require('./mapping_file');
const { searchImdbIds, firstTrusted } = require('./imdb_search');
const {
  seasonWindow, verifyCandidate, verifyFreshTitle, verifySpecial, nextSeason, continuationStart, SPECIAL_TYPES,
} = require('./mapping_verifier');

const IMPORT_CONCURRENCY = 4;
const MAL_HENTAI_GENRE = '12';

async function importMalSeason(season, { reviewPath = "./static/data/new_mappings.json" } = {}) {
  const imdbMapping = require('../static/data/imdb_mapping')
      .reduce((map, entry) => (map[entry.kitsu_id] = entry, map), {});;
  const window = seasonWindow(season);
  await loadCurrentTitleIndex().catch(error => console.log(`Title index refresh failed: ${error?.message || error}`));
  const malEntries = await getMalSeasonalEntries(season);
  const newMappingEntries = await mapWithConcurrency(malEntries, IMPORT_CONCURRENCY,
      malEntry => createImdbMappingEntry(malEntry, window));

  const resolvedEntries = newMappingEntries.filter(entry => entry && !imdbMapping[entry.kitsuId]);
  const trustedEntries = resolvedEntries.filter(entry => entry._trusted);
  const reviewEntries = resolvedEntries.filter(entry => !entry._trusted);

  if (trustedEntries.length) {
    mergeIntoImdbMapping(trustedEntries);
    console.log(`Auto-merged ${trustedEntries.length} verified entries into imdb_mapping.json`);
  }

  // Exclude review entries whose kitsuId is already mapped (same-run trusted merges or prior manual
  // UI additions) so we never re-surface something we already have.
  const mapped = new Set(require('../static/data/imdb_mapping').map(e => e.kitsu_id));
  const newMappings = reviewEntries
      .filter(entry => !mapped.has(parseInt(entry.kitsuId)))
      .reduce((map, entry) => (map[entry.kitsuId] = entry, map), {})
  writeToFile(newMappings, reviewPath)
  console.log("Ids to remove from cache:")
  console.log(newMappingEntries.map(e => [e?.kitsuId, e?.imdb_id]).flat().filter(x => x).join('|'))
  return season;
}

async function createImdbMappingEntry(malEntry, window) {
  const kitsuIdFromMal = await queryIdMapping(`mal`, malEntry.malId).catch((err) => undefined);
  const kitsuIdFromSearch = !kitsuIdFromMal && searchIds(malEntry.title)[0];
  const kitsuId = await confirmedKitsuId(malEntry.malId, kitsuIdFromMal || kitsuIdFromSearch);
  if (!kitsuId) {
    console.log(`No kitsuId found for: ${JSON.stringify(malEntry)}`);
    return kitsuId;
  }
  const existing = getImdbMapping(kitsuId);
  if (existing) {
    // Already mapped: nothing to resolve, and importMalSeason filters it out of merges and review.
    return { _existing: true, kitsuId: String(kitsuId), imdb_id: existing.imdb_id };
  }

  const kitsuMetadata = await animeData(kitsuId, { images: false, relations: true }).catch(() => undefined);

  const trustedEntry = await trustedFribbMapping(malEntry, kitsuId, kitsuMetadata, window);
  if (trustedEntry) {
    return trustedEntry;
  }

  const reviewBase = {
    malId: malEntry.malId,
    malTitle: malEntry.title,
    kitsuId: kitsuId,
    kitsuTitle: kitsuMetadata?.name,
    kitsuAliases: kitsuMetadata?.aliases,
    animeType: kitsuMetadata?.animeType,
  };
  // OVAs/ONAs/specials of an already-mapped franchise often are the franchise's IMDb specials (season 0).
  const special = await franchiseSpecial(kitsuId, kitsuMetadata, window);
  if (special?._trusted) {
    return special;
  }

  // Only TV/ONA sequels continue their prequel's imdb as the next season; movies, OVAs and specials get their own
  // imdb lookup instead of being numbered as a new season.
  const prequelKitsuId = isSeasonableType(kitsuMetadata?.animeType) && kitsuMetadata.links
      .filter(link => link.name.startsWith("Prequel"))
      .map(link => link.url.match(/kitsu:(\d+)$/)[1])[0];
  const prequelImdbMapping = prequelKitsuId && getImdbMapping(prequelKitsuId);
  if (prequelImdbMapping) {
    return withSpecialSuggestion(await verifiedOrReview(malEntry, kitsuId, kitsuMetadata, window, {
      ...reviewBase,
      imdb_id: prequelImdbMapping.imdb_id,
      title: prequelImdbMapping.title,
      fromSeason: prequelImdbMapping.fromSeason + 1,
      fromEpisode: 1,
    }), special);
  }

  const foundImdbIds = await searchImdbIds(malEntry.title).catch((err) => []);
  if (!foundImdbIds.length) {
    console.log(`No imdbId found for: ${JSON.stringify(malEntry)}`);
    return withSpecialSuggestion(reviewBase, special);
  }
  const hasVideos = kitsuMetadata?.videos?.length > 1 || kitsuMetadata?.animeType === 'TV';
  const entry = await firstTrusted(foundImdbIds, imdbId => verifiedOrReview(malEntry, kitsuId, kitsuMetadata, window, {
    ...reviewBase,
    imdb_id: imdbId,
    fromSeason: hasVideos ? 1 : undefined,
    fromEpisode: hasVideos ? 1 : undefined,
  }));
  const [imdbMeta, tvdbId] = await Promise.all([
    getImdbMeta(entry.imdb_id).catch((err) => undefined),
    getTvdbId({ imdb_id: entry.imdb_id }).catch(() => false),
  ]);
  return withSpecialSuggestion({ ...entry, tvdb_id: tvdbId, title: imdbMeta?.title || entry.title }, special);
}

// The id mapping service / title search can land on another entry of the franchise (often the previous season). When
// the MAL <-> Kitsu link isn't confirmed, take the kitsu entry Kitsu itself links to this MAL entry's ids, if any.
async function confirmedKitsuId(malId, kitsuId) {
  if (kitsuId && (await malKitsuLink(malId, kitsuId).catch(() => ({ linked: false }))).linked) {
    return kitsuId;
  }
  const linkedKitsuId = await kitsuIdFromLinks(malId).catch(() => undefined);
  if (linkedKitsuId && String(linkedKitsuId) !== String(kitsuId)) {
    console.log(`MAL ${malId}: kitsu ${kitsuId} -> ${linkedKitsuId} (linked by Kitsu ids)`);
    return linkedKitsuId;
  }
  return kitsuId;
}

// Kitsu relations that tie a special to its franchise (not alternative versions, spin-offs or adaptations).
const FRANCHISE_ROLES = ['parent_story', 'prequel', 'sequel', 'side_story', 'full_story', 'summary', 'other'];

// For an OVA/ONA/special whose related Kitsu entries all map to one series imdb: the verified season-0 row, or
// { imdb_id, title, fromEpisode?, reason } describing why it could not be verified. Undefined when there is no
// such franchise.
async function franchiseSpecial(kitsuId, kitsuMetadata, window) {
  if (!SPECIAL_TYPES.includes(kitsuMetadata?.animeType)) {
    return undefined;
  }
  const franchiseRows = (kitsuMetadata.kitsuRelations || [])
      .filter(relation => FRANCHISE_ROLES.includes(relation.role))
      .map(relation => getImdbMapping(relation.kitsuId))
      .filter(row => row?.imdb_id && row.fromSeason !== undefined);
  const imdbIds = [...new Set(franchiseRows.map(row => row.imdb_id))];
  if (imdbIds.length !== 1) {
    return imdbIds.length ? { reason: `related kitsu entries map to ${imdbIds.length} imdb ids` } : undefined;
  }
  const imdbId = imdbIds[0];
  // Fribb giving the entry its own imdb (Pikachu shorts, One Piece films) means IMDb lists it as a title of its own.
  const ownImdbIds = [].concat((await getFribbEntryByKitsu(kitsuId).catch(() => undefined))?.imdb_id || []);
  if (ownImdbIds.length && !ownImdbIds.includes(imdbId)) {
    return { imdb_id: imdbId, reason: `Fribb lists its own imdb ${ownImdbIds.join(', ')}` };
  }
  const [cinemeta, siblings] = await Promise.all([
    cachedCinemeta(imdbId),
    siblingsOf(imdbId, true, { specials: true }),
  ]);
  const title = franchiseRows[0].title || cinemeta?.name;
  const result = verifySpecial({
    animeType: kitsuMetadata.animeType,
    kitsuAirDates: (kitsuMetadata.videos || []).map(video => video.released),
    kitsuEpisodeLength: parseInt(kitsuMetadata.runtime) || undefined,
    kitsuTitles: [kitsuMetadata.name, ...(kitsuMetadata.aliases || [])],
    cinemeta,
    siblings,
    window,
  });
  if (!result.ok) {
    return { imdb_id: imdbId, title, fromEpisode: result.fromEpisode, reason: result.reason };
  }
  return { _trusted: true, kitsuId: String(kitsuId), imdb_id: imdbId, title, fromSeason: 0, fromEpisode: result.fromEpisode };
}

// Adds the franchise-special alternative to a review entry: as its candidate when nothing else was found, otherwise
// appended to the review reason. The review page offers an "as special ttX S0Ey" part as a one-click fill.
function withSpecialSuggestion(entry, special) {
  if (!special || entry._trusted) {
    return entry;
  }
  const position = special.fromEpisode ? ` S0E${special.fromEpisode}` : '';
  const note = `as special${special.imdb_id ? ` ${special.imdb_id}` : ''}${position}: ${special.reason}`;
  if (!entry.imdb_id && special.imdb_id) {
    return {
      ...entry,
      imdb_id: special.imdb_id,
      title: special.title,
      fromSeason: 0,
      fromEpisode: special.fromEpisode || 1,
      reviewReason: note,
    };
  }
  return { ...entry, reviewReason: [entry.reviewReason, note].filter(Boolean).join('; ') };
}

// Treat Fribb's list as ground truth: if it maps this MAL id to the same kitsu id we resolved and
// lists an imdb id, skip review and auto-merge.
// Standalone/first-season entries map to season 1; later seasons of an already-mapped imdb are
// auto-seasoned only when Cinemeta confirms the next season starts fresh this MAL season.
async function trustedFribbMapping(malEntry, kitsuId, kitsuMetadata, window) {
  const fribbEntry = await getFribbEntry(malEntry.malId).catch(() => undefined);
  // The mal <-> kitsu link must be confirmed (by Fribb, or by Kitsu's own MAL/AniList/AniDB ids): a Fribb entry
  // without a kitsu id would otherwise trust a title-search kitsu hit, which for new movies often lands on an older
  // namesake (1983 vs 2025 Doraemon film).
  const linked = await isFribbLinked(malEntry.malId, kitsuId).catch(() => false);
  if (!fribbEntry || !linked) {
    return undefined;
  }
  const imdbId = resolveImdbId(fribbEntry);
  if (!imdbId) {
    return undefined;
  }

  const siblings = require('../static/data/imdb_mapping').filter(e => e.imdb_id === imdbId);
  let fromSeason;
  let fromEpisode;
  if (siblings.length === 0) {
    // Fresh imdb -> standalone / first season. MAL->Kitsu for OVAs/specials is often wrong, so only
    // trust those when Fribb itself carries the exact mal->kitsu match (not a fuzzy title-search id).
    if (isExtraType(fribbEntry.type) && !hasExactFribbKitsu(fribbEntry, kitsuId)) {
      return undefined;
    }
    const hasVideos = kitsuMetadata?.videos?.length > 1 || kitsuMetadata?.animeType === 'TV';
    fromSeason = hasVideos ? 1 : undefined;
    fromEpisode = hasVideos ? 1 : undefined;
    const fresh = verifyFreshTitle({
      candidate: { imdb_id: imdbId, fromSeason, fromEpisode },
      animeType: kitsuMetadata?.animeType,
      kitsuYear: kitsuMetadata?.year,
      kitsuEpisodes: kitsuMetadata?.videos?.length,
      cinemeta: await cachedCinemeta(imdbId),
    });
    if (!fresh.ok) {
      return undefined;
    }
  } else {
    // Franchise: auto-season only TV/ONA sequels that Cinemeta confirms, either as the next season premiering
    // this MAL season or as a continuation cour inside the running season; anything else goes to review.
    if (!isSeasonableType(kitsuMetadata?.animeType)) {
      return undefined;
    }
    const sequel = { imdb_id: imdbId, fromSeason: nextSeason(siblings), fromEpisode: 1 };
    const { result, candidate } = await verifySequel(malEntry, kitsuId, kitsuMetadata, window, sequel);
    if (!result.ok) {
      return undefined;
    }
    fromSeason = candidate.fromSeason;
    fromEpisode = candidate.fromEpisode;
  }

  // Every row of an imdb carries the same title, so a sequel reuses its prequel's.
  const imdbMeta = siblings.length ? undefined : await getImdbMeta(imdbId).catch(() => undefined);
  return {
    _trusted: true,
    kitsuId: String(kitsuId),
    imdb_id: imdbId,
    title: siblings[0]?.title || imdbMeta?.title || kitsuMetadata?.name,
    fromSeason,
    fromEpisode,
  };
}

// Auto-merge a review candidate when it passes the mapping verifier; otherwise keep it for review with the reason.
async function verifiedOrReview(malEntry, kitsuId, kitsuMetadata, window, entry) {
  const isSequel = entry.fromSeason > 0 && isSeasonableType(kitsuMetadata?.animeType);
  const { result, cinemeta, candidate } = isSequel
      ? await verifySequel(malEntry, kitsuId, kitsuMetadata, window, entry)
      : { ...await verifyMapping(malEntry, kitsuId, kitsuMetadata, window, entry), candidate: entry };
  if (result.ok) {
    return { ...entry, ...candidate, _trusted: true, title: entry.title || cinemeta.name };
  }
  return { ...entry, reviewReason: result.reason };
}

// A TV/ONA sequel is either the next IMDb season starting at episode 1, or a continuation cour that IMDb keeps in
// the running season (needs an episode offset). Returns the verified candidate, or the fresh-season failure.
async function verifySequel(malEntry, kitsuId, kitsuMetadata, window, candidate) {
  const fresh = await verifyMapping(malEntry, kitsuId, kitsuMetadata, window, candidate);
  if (fresh.result.ok) {
    return { ...fresh, candidate };
  }
  const start = continuationStart(fresh.siblings);
  if (!start) {
    return { ...fresh, candidate };
  }
  const continuation = { ...candidate, ...start };
  const verified = await verifyMapping(malEntry, kitsuId, kitsuMetadata, window, continuation);
  if (verified.result.ok) {
    return { ...verified, candidate: continuation };
  }
  const reason = `${fresh.result.reason}; as continuation S${start.fromSeason}E${start.fromEpisode}: ${verified.result.reason}`;
  return { ...fresh, candidate, result: { ok: false, reason } };
}

async function verifyMapping(malEntry, kitsuId, kitsuMetadata, window, candidate) {
  const validImdbId = /^tt\d+$/.test(candidate.imdb_id || '');
  const [{ linked, reason: linkReason }, cinemeta] = await Promise.all([
    malKitsuLink(malEntry.malId, kitsuId).catch(() => ({ linked: false })),
    validImdbId ? cachedCinemeta(candidate.imdb_id) : undefined,
  ]);
  // Episode counts are only needed to place a series season, so skip the Kitsu calls when it can't get that far.
  const needsCounts = linked && cinemeta?.type === 'series';
  const siblings = validImdbId ? await siblingsOf(candidate.imdb_id, needsCounts) : [];
  const result = verifyCandidate({
    candidate,
    linked,
    linkReason,
    animeType: kitsuMetadata?.animeType,
    kitsuYear: kitsuMetadata?.year,
    kitsuEpisodes: kitsuMetadata?.videos?.length,
    kitsuFirstAired: kitsuMetadata?.videos?.[0]?.released,
    cinemeta,
    siblings,
    window,
  });
  return { result, cinemeta, siblings };
}

const cinemetaCache = new Map();

function cachedCinemeta(imdbId) {
  if (!cinemetaCache.has(imdbId)) {
    cinemetaCache.set(imdbId, getFreshCinemetaMetadata(imdbId));
  }
  return cinemetaCache.get(imdbId);
}

const episodeCounts = new Map();

// Existing imdb_mapping rows for an imdb, with Kitsu episode counts fetched in one batch: of the regular seasons,
// and with `specials` of the season-0 rows as well (to know which S0 and regular episodes are already mapped).
async function siblingsOf(imdbId, withEpisodeCounts, { specials = false } = {}) {
  const siblings = require('../static/data/imdb_mapping').filter(e => e.imdb_id === imdbId);
  if (!withEpisodeCounts) {
    return siblings;
  }
  const counted = sibling => specials ? sibling.fromSeason >= 0 : sibling.fromSeason > 0;
  const missing = siblings.filter(e => counted(e) && !episodeCounts.has(e.kitsu_id)).map(e => e.kitsu_id);
  if (missing.length) {
    const batch = animeEpisodeCounts(missing).catch(() => ({}));
    // Specials often have no Kitsu episodeCount; their episode list still tells how many S0 episodes they use.
    // Regular seasons keep an unknown count as unknown, so their offsets are never guessed.
    const specialRows = new Set(siblings.filter(e => e.fromSeason === 0).map(e => e.kitsu_id));
    missing.forEach(id => episodeCounts.set(id, batch.then(counts => counts[id] ?? (specialRows.has(id)
        ? animeData(id, { images: false }).then(meta => meta.videos?.length).catch(() => undefined)
        : undefined))));
  }
  return Promise.all(siblings.map(async sibling => counted(sibling)
      ? { ...sibling, episodeCount: await episodeCounts.get(sibling.kitsu_id) }
      : sibling));
}

function isSeasonableType(animeType) {
  return animeType === 'TV' || animeType === 'ONA';
}

function isExtraType(type) {
  return type === 'OVA' || type === 'SPECIAL' || type === 'MUSIC';
}

function hasExactFribbKitsu(fribbEntry, kitsuId) {
  return fribbEntry.kitsu_id != null && String(fribbEntry.kitsu_id) === String(kitsuId);
}

function mergeIntoImdbMapping(trustedEntries) {
  const base = require('../static/data/imdb_mapping');
  const existing = new Set(base.map(e => e.kitsu_id));
  for (const entry of trustedEntries) {
    const kitsu_id = parseInt(entry.kitsuId);
    if (existing.has(kitsu_id)) {
      continue;
    }
    const item = { kitsu_id, imdb_id: entry.imdb_id, title: entry.title };
    if (entry.fromSeason !== undefined) {
      item.fromSeason = entry.fromSeason;
      item.fromEpisode = entry.fromEpisode;
    }
    insertMappingEntry(base, item);
    existing.add(kitsu_id);
  }
  writeToFile(base, "./static/data/imdb_mapping.json");
}

async function getMalSeasonalEntries(season) {
  const requestUrl = `https://myanimelist.net/anime/season/${season}`
  return axios.get(requestUrl)
      .then(response => response.data)
      .then(body => {
        const $ = cheerio.load(body);
        return $('.js-seasonal-anime')
            // Kitsu doesn't serve hentai, so these can never be mapped.
            .filter((i, element) => !String($(element).attr('data-genre') || '').split(',').includes(MAL_HENTAI_GENRE))
            .map((i, element) => {
              const row = $(element).find('a.link-title');
              return {
                title: row.text(),
                malId: row.attr('href').match(/anime\/(\d+)/)[1],
              };
            }).get();
      });
}

async function getImdbMeta(imdbId) {
  const url = `https://www.imdb.com/title/${imdbId}`;
  const config = {
    headers: {
      'user-agent': 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/108.0.0.0 Safari/537.36',
      'accept-language': 'en-GB'
    }
  };
  return axios.get(url, config)
      .then(response => response.data)
      .then(body => {
        const $ = cheerio.load(body);
        const title = $('h1').text().trim();
        return {
          title: title || undefined,
        };
      })
}

function findMappingInsertIndex(imdbMappingEntries, mapping) {
  const kitsuId = parseInt(mapping.kitsuId);
  const imdbId = mapping.imdb_id;
  let insertIndex = 0;
  let franchiseIndex = 0;
  for (let i = imdbMappingEntries.length - 1; i >= 0; i--) {
    const imdbMappingEntry = imdbMappingEntries[i];
    const imdbMapping = imdbMappingEntry[1];
    if (imdbId === imdbMapping && imdbMapping.fromSeason !== 0) {
      franchiseIndex = i + 1;
      break;
    }
    if (!insertIndex && parseInt(imdbMappingEntry[0]) < kitsuId) {
      insertIndex = i + 1;
    }
  }
  return franchiseIndex || insertIndex;
}

function writeImdbMappingToFile(imdbMappingEntries) {
  const imdbMappings = imdbMappingEntries.reduce((map, entry) => (map[entry[0]] = entry[1], map), {})
  writeToFile(imdbMappings, "./static/data/imdb_mapping.json")
}

function writeToFile(object, path) {
  fs.writeFile(path, formatJson(object), 'utf8', function (err) {
    if (err) {
      console.log("An error occurred while writing JSON Object to File:", err);
    }
  });
}

// Like Promise.all(items.map(fn)) with at most `limit` in flight; a failing item yields undefined instead of
// aborting the whole season.
async function mapWithConcurrency(items, limit, fn) {
  const results = new Array(items.length);
  let next = 0;
  const worker = async () => {
    while (next < items.length) {
      const index = next++;
      results[index] = await fn(items[index]).catch(error => {
        console.log(`Failed ${JSON.stringify(items[index])}: ${error?.message || error}`);
        return undefined;
      });
    }
  };
  await Promise.all(Array.from({ length: Math.min(limit, items.length) }, worker));
  return results;
}

// usage: node mapping/mal_seasonal_import.js [2026/fall] [review output path]
if (require.main === module) {
  const [season = '2026/fall', reviewPath] = process.argv.slice(2);
  importMalSeason(season, { reviewPath }).then(season => console.log(`Finished importing MAL ${season}`));
}

module.exports = {
  importMalSeason,
  createImdbMappingEntry,
  franchiseSpecial,
  getMalSeasonalEntries,
  insertInKitsuOrder,
  insertMappingEntry,
  mapWithConcurrency,
  siblingsOf,
};
