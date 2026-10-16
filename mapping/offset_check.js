// Re-checks the IMDb offsets of recently started sequels, continuation cours and specials against fresh Cinemeta data
// (the rule is verifyOffset in mapping_verifier.js), and that no season-0 row of those imdbs covers more IMDb specials
// than its Kitsu entry has episodes (verifySpecialRun), unless the runtime caps it there (a finished entry).
// Cheap selection: one paged Kitsu season listing gives the start date of every recent entry, so Cinemeta is only
// fetched for the imdb ids of recent rows that start past S1E1.
const fs = require('fs');
const { PAGE_SIZE, animeStartEntries } = require('../lib/kitsu_api');
const { getFreshCinemetaMetadata } = require('../lib/cinemeta_api');
const { seasonWindow, verifyOffset, verifySpecialRun } = require('./mapping_verifier');
const { recentMalSeasons, kitsuSeasons } = require('./mal_seasons');
const { mapWithConcurrency, siblingsOf } = require('./mal_seasonal_import');
const { episodeCap } = require('../lib/metadataEnrich');

const CINEMETA_CONCURRENCY = 4;

// Rows placed past S1E1 of their imdb, of entries that started in the last `seasons` MAL seasons (up to `now`), or of
// every entry when `seasons` is 'all' (start dates then come from id lookups instead of the season listings).
async function checkRecentOffsets({ seasons = 2, now = new Date() } = {}) {
  const rows = require('../static/data/imdb_mapping')
      .filter(row => row.imdb_id && row.fromSeason !== undefined && !row.nonImdbEpisodes)
      .filter(row => !(row.fromSeason === 1 && row.fromEpisode === 1));
  const from = seasons === 'all' ? new Date(0) : seasonWindow(recentMalSeasons(seasons, now)[0]).from;
  const filters = seasons === 'all'
      ? chunks([...new Set(rows.map(row => row.kitsu_id))], PAGE_SIZE).map(ids => ({ id: ids.join(',') }))
      : kitsuSeasons(from, now).map(({ season, year }) => ({ season, seasonYear: year }));
  const kitsuEntries = {};
  for (const filter of filters) {
    Object.assign(kitsuEntries, await animeStartEntries(filter));
  }
  const started = row => {
    const startDate = kitsuEntries[row.kitsu_id]?.startDate;
    return startDate && new Date(startDate) >= from && new Date(startDate) <= now;
  };
  return checkRows(rows.filter(started), kitsuEntries, from, now);
}

async function checkRows(rows, kitsuEntries, from, now) {
  const imdbIds = [...new Set(rows.map(row => row.imdb_id))];
  const metas = await mapWithConcurrency(imdbIds, CINEMETA_CONCURRENCY, imdbId => getFreshCinemetaMetadata(imdbId));
  const cinemetas = Object.fromEntries(imdbIds.map((imdbId, index) => [imdbId, metas[index]]));
  const results = rows.map(row => {
    const { title: kitsuTitle, startDate: kitsuStart } = kitsuEntries[row.kitsu_id];
    const { ok, unknown, reason, suggestions } = verifyOffset({ row, kitsuStart, cinemeta: cinemetas[row.imdb_id] });
    return { row, kitsuTitle, status: ok ? 'ok' : unknown ? 'unknown' : 'mismatch', reason, suggestions };
  });
  return { from, now, results: results.concat(await checkSpecialRuns(imdbIds, kitsuEntries, cinemetas)) };
}

// The season-0 rows of the checked imdbs, whose runs the Cinemeta metas fetched anyway can check. Rows the runtime caps
// at their episode count, or listing their imdb episodes, can't spill, and rows that can't be checked (no Kitsu
// episode count) are left out, as nothing about them changed.
async function checkSpecialRuns(imdbIds, kitsuEntries, cinemetas) {
  const siblingGroups = await mapWithConcurrency(imdbIds, CINEMETA_CONCURRENCY,
      imdbId => siblingsOf(imdbId, true, { specials: true }));
  return siblingGroups.flatMap((siblings, index) => siblings
      .filter(row => row.fromSeason === 0 && !row.imdbEpisodes && !episodeCap(row.kitsu_id))
      .map(row => ({ row, ...verifySpecialRun({ row, siblings, cinemeta: cinemetas[imdbIds[index]] }) }))
      .filter(({ unknown }) => !unknown)
      .map(({ row, ok, reason }) => ({
        row, kitsuTitle: kitsuEntries[row.kitsu_id]?.title, status: ok ? 'ok' : 'mismatch', reason,
      })));
}

function chunks(items, size) {
  return Array.from({ length: Math.ceil(items.length / size) }, (_, i) => items.slice(i * size, (i + 1) * size));
}

// The report, its mismatches a checklist: a row ticked in `previous` (the open issue's body) stays ticked,
// acknowledged, while it keeps failing at the same mapped position.
function toMarkdown({ from, now, results }, previous = '') {
  const mismatches = results.filter(result => result.status === 'mismatch');
  const unknown = results.filter(result => result.status === 'unknown');
  const acked = new Set([...previous.matchAll(/^- \[[xX]\] <!-- (\S+) -->/gm)].map(match => match[1]));
  const lines = [
    `Checked ${results.length} sequel/cour/special rows of entries that started `
    + `${from.getTime() ? `${day(from)} – ${day(now)}` : `by ${day(now)}`}: `
    + `${mismatches.length} no longer match Cinemeta, ${unknown.length} could not be checked.`,
  ];
  if (mismatches.length) {
    lines.push('', 'Tick a row to acknowledge it.', '');
    mismatches.forEach(result => {
      const key = `${result.row.kitsu_id}:S${result.row.fromSeason}E${result.row.fromEpisode}`;
      const fix = suggestionText(result.suggestions);
      lines.push(`- [${acked.has(key) ? 'x' : ' '}] <!-- ${key} --> ${cells(result).join(' · ')}`
          + (fix === 'nothing' ? '' : ` · starts on that day: ${fix}`));
    });
  }
  if (unknown.length) {
    lines.push('', '<details><summary>Could not check</summary>', '',
        '| Kitsu | IMDb | Mapped | Problem |', '| --- | --- | --- | --- |',
        ...unknown.map(result => `| ${cells(result).join(' | ')} |`), '', '</details>');
  }
  return lines.join('\n') + '\n';
}

function cells({ row, kitsuTitle, reason }) {
  return [
    `[${row.kitsu_id}](https://kitsu.app/anime/${row.kitsu_id}) ${escapeCell(kitsuTitle)}`,
    `[${row.imdb_id}](https://www.imdb.com/title/${row.imdb_id}/) ${escapeCell(row.title)}`,
    `S${row.fromSeason}E${row.fromEpisode}`,
    escapeCell(reason),
  ];
}

function suggestionText(suggestions = []) {
  return suggestions.map(s => `S${s.fromSeason}E${s.fromEpisode}`).join(', ') || 'nothing';
}

function escapeCell(text = '') {
  return String(text).replace(/\|/g, '\\|');
}

function day(date) {
  return date.toISOString().slice(0, 10);
}

// usage: node mapping/offset_check.js [seasons=2|all] [markdown report path] [previous report path, for its ticks]
// Under GitHub Actions also writes `mismatches=<count>` to $GITHUB_OUTPUT.
if (require.main === module) {
  const [seasons = '2', reportPath, previousPath] = process.argv.slice(2);
  checkRecentOffsets({ seasons: seasons === 'all' ? 'all' : parseInt(seasons) })
      .then(check => {
        const report = toMarkdown(check, previousPath && fs.existsSync(previousPath)
            ? fs.readFileSync(previousPath, 'utf8') : '');
        console.log(report);
        if (reportPath) {
          fs.writeFileSync(reportPath, report);
        }
        if (process.env.GITHUB_OUTPUT) {
          const mismatches = check.results.filter(result => result.status === 'mismatch').length;
          fs.appendFileSync(process.env.GITHUB_OUTPUT, `mismatches=${mismatches}\n`);
        }
      })
      .catch(error => {
        console.error(error);
        process.exit(1);
      });
}

module.exports = { checkRecentOffsets, toMarkdown };
