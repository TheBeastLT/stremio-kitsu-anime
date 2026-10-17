// Imports the anime Kitsu created recently, which the MAL season import never sees when they aired before its
// seasons (older shows and donghua seasons Kitsu adds late). Each entry goes through the season importer's pipeline,
// verified against the MAL season it aired in; entries with no MAL id (often donghua) are matched from the Kitsu side
// only, by their franchise's rows or an IMDb title search. Verified rows are merged, the rest that found an imdb
// candidate are written for review; entries with none (mostly CMs and PVs) are dropped.
// Each entry is seen by one or two weekly runs only, so the run itself retries what is left (see retryLeft).
const { execFileSync } = require('child_process');
const { recentAnimeEntries, animeStartEntries } = require('../lib/kitsu_api');
const { getImdbMapping } = require('../lib/metadataEnrich');
const { malIdByKitsu } = require('./fribb_mapping');
const { seasonWindow } = require('./mapping_verifier');
const { malSeason, recentMalSeasons } = require('./mal_seasons');
const {
  createKitsuMappingEntry, saveImportResults, mapWithConcurrency, IMPORT_CONCURRENCY,
} = require('./mal_seasonal_import');

// A week between runs plus slack, so a late or failed run misses nothing.
const DEFAULT_DAYS = 9;
const MAX_PASSES = 3;
const UNAIRED_STATUSES = ['tba', 'unreleased', 'upcoming'];

async function importRecentKitsu({ days = DEFAULT_DAYS, ids, dryRun = false, reviewPath, pass = 1 } = {}) {
  const today = new Date();
  const byId = ids ? await animeStartEntries({ id: ids.join(',') }) : await recentAnimeEntries(daysAgo(days, today));
  const candidates = Object.entries(byId).map(([id, entry]) => ({ id, ...entry }));
  const entries = importableEntries(candidates, { today, isMapped: id => !!getImdbMapping(id) });
  console.log(`Pass ${pass}: ${entries.length} of ${candidates.length} Kitsu entries to import`);

  const imported = [];
  const results = await mapWithConcurrency(entries, IMPORT_CONCURRENCY, async entry => {
    const malId = await malIdByKitsu(entry.id).catch(() => undefined);
    if (!ids && coveredByMalImport(entry, malId, today)) {
      return undefined;
    }
    imported.push(entry.id);
    const season = malSeason(new Date(entry.startDate));
    const mapping = await createKitsuMappingEntry(entry.id, { malId, searchTitle: entry.title }, seasonWindow(season));
    return mapping?.imdb_id && { ...mapping, kitsuTitle: mapping.kitsuTitle || entry.title, season };
  });

  const { merged, review } = saveImportResults(results, { reviewPath, dryRun });
  merged.forEach(entry => logEntry('verified', entry, entry.title));
  review.forEach(entry => logEntry('review', entry, entry.reviewReason));
  console.log(`Pass ${pass}: ${merged.length} verified, ${review.length} for review${dryRun ? ', nothing written' : ''}`);
  if (!dryRun && merged.length && pass < MAX_PASSES) {
    const mergedIds = new Set(merged.map(entry => entry.kitsuId));
    retryLeft(imported.filter(id => !mergedIds.has(id)), { reviewPath, pass });
  }
}

// A sequel only continues a prequel that had a row when the process loaded the mapping (getImdbMapping is indexed
// once), so while a pass merges rows the entries left are retried in a new process: a batch of donghua seasons is
// mapped a season per pass. The retry rewrites the review file with what is still left.
function retryLeft(ids, { reviewPath, pass }) {
  if (!ids.length) {
    return;
  }
  execFileSync(process.execPath, [__filename, '--ids', ids.join(','), '--pass', String(pass + 1),
    ...(reviewPath ? ['--review', reviewPath] : [])], { stdio: 'inherit' });
}

// Entries that aired already and have no row. Unaired ones are left to the MAL season import, which maps them as
// their season comes up.
function importableEntries(entries, { today, isMapped }) {
  return entries
      .filter(entry => entry.startDate && new Date(entry.startDate) <= today)
      .filter(entry => !UNAIRED_STATUSES.includes(entry.status)) // a placeholder startDate can be in the past
      .filter(entry => !isMapped(entry.id));
}

// The MAL season import already runs an entry through the same pipeline when it is on MAL and aired in one of the
// seasons the import covers (the workflow's `mal_seasons.js 2 --upcoming`). Kitsu's start date stands in for MAL's
// season, which can differ by a few days at a season's edge.
function coveredByMalImport(entry, malId, today) {
  return !!malId && recentMalSeasons(2, today, { upcoming: true }).includes(malSeason(new Date(entry.startDate)));
}

const daysAgo = (days, date) => new Date(date.getTime() - days * 24 * 60 * 60 * 1000);
const position = entry => entry.fromSeason === undefined ? '' : ` S${entry.fromSeason}E${entry.fromEpisode}`;
const logEntry = (label, entry, note) => console.log(`${label}: kitsu ${entry.kitsuId} ${entry.kitsuTitle} `
    + `[${entry.season}] -> ${entry.imdb_id}${position(entry)} ${note || ''}`);

const argValue = name => {
  const index = process.argv.indexOf(name);
  return index > 0 ? process.argv[index + 1] : undefined;
};

// usage: node mapping/kitsu_recent_import.js [--days 9 | --ids 1,2,3] [--dry-run] [--review <output path>]
// (review entries are only written with --review)
if (require.main === module) {
  importRecentKitsu({
    days: argValue('--days') && parseInt(argValue('--days')),
    ids: argValue('--ids')?.split(','),
    dryRun: process.argv.includes('--dry-run'),
    reviewPath: argValue('--review'),
    pass: parseInt(argValue('--pass')) || 1,
  })
      .then(() => console.log('Finished importing recent Kitsu entries'))
      .catch(error => {
        console.error(`Recent Kitsu import failed: ${error?.message || JSON.stringify(error)}`);
        process.exit(1);
      });
}

module.exports = { importRecentKitsu, importableEntries, coveredByMalImport };
