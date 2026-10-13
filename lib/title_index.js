// The title search index (title_search.js) over the data file (title_records.js), built on the first search.
// startNewTitlesRefresh() adds the entries Kitsu creates in between (hourly), until the next weekly rebuild of the file
// brings their synonyms.
const { createTitleIndex } = require('./title_search');
const { readRecords, toTitleRecord } = require('./title_records');
const { getImdbMapping } = require('./metadataEnrich');

const NEW_TITLES_INTERVAL = 60 * 60 * 1000; // 1 hour
const NEW_TITLES_MAX_PAGES = 5;

// The file's records aren't kept once indexed (their synonyms alone are ~10MB): a rebuild reads the file again.
let knownIds;
let newRecords = [];
let index;

function buildIndex() {
  const records = readRecords().concat(newRecords);
  knownIds = new Set(records.map(record => record.id));
  return createTitleIndex(records, { imdbMapping: getImdbMapping });
}

function getIndex() {
  if (!index) {
    index = buildIndex();
  }
  return index;
}

// [{ id, score, confidence }] best first; empty when no title matches well enough to trust
function searchTitles(query, limit) {
  return getIndex().search(query, { limit });
}

// Kitsu's newest entries (by creation) the index doesn't have yet. Offset paging shifts when an entry is created
// meanwhile, repeating one on the next page, so they're collected by id.
async function fetchNewRecords(known) {
  // required here, not at the top: kitsu_api requires this module for its search
  const { animeTitlePage, PAGE_SIZE } = require('./kitsu_api');
  const fresh = new Map();
  for (let page = 0; page < NEW_TITLES_MAX_PAGES; page++) {
    const { entries } = await animeTitlePage({ offset: page * PAGE_SIZE, sort: '-createdAt' });
    const unknown = entries.filter(anime => !known.has(+anime.id));
    unknown.forEach(anime => fresh.set(+anime.id, toTitleRecord(anime)));
    if (unknown.length < entries.length || entries.length < PAGE_SIZE) break;
  }
  return [...fresh.values()];
}

async function addNewTitles() {
  if (!index) {
    return; // not searched yet: the first search builds it
  }
  const fresh = await fetchNewRecords(knownIds);
  if (fresh.length) {
    newRecords = newRecords.concat(fresh);
    index = buildIndex();
    console.log(`Title index: added ${fresh.length} new Kitsu entries`);
  }
}

function startNewTitlesRefresh() {
  const refresh = () => addNewTitles()
      .catch(error => console.warn(`Title index refresh failed: ${error.message || error}`));
  setInterval(refresh, NEW_TITLES_INTERVAL).unref();
}

// Builds the index with Kitsu's newest entries in it now: for one-off runs (the MAL importer) without the hourly
// refresh, which would otherwise search a file up to a week old and miss the newly announced titles.
async function loadCurrentTitleIndex() {
  getIndex();
  await addNewTitles();
}

module.exports = { searchTitles, startNewTitlesRefresh, loadCurrentTitleIndex };
