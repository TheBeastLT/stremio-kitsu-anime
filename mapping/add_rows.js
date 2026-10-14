// Adds reviewed rows to static/data/imdb_mapping.json where the review page and the importer would put them.
// Rows are { kitsu_id, imdb_id, title?, fromSeason?, fromEpisode?, nonImdbEpisodes? }; a row without a title takes its
// imdb's existing title. Nothing is written when any row is invalid or its kitsu id already has a row (the runtime
// keeps one per id).
const fs = require('fs');
const path = require('path');
const { insertMappingEntry } = require('./mapping_file');

const MAPPING_PATH = path.join(__dirname, '../static/data/imdb_mapping.json');

function addRows(base, rows) {
  const taken = new Set(base.map(row => row.kitsu_id));
  const problems = [];
  const items = rows.map(row => {
    const item = { kitsu_id: row.kitsu_id, imdb_id: row.imdb_id, title: row.title };
    item.title = item.title || base.find(existing => existing.imdb_id === row.imdb_id)?.title;
    if (row.fromSeason !== undefined || row.fromEpisode !== undefined) {
      item.fromSeason = row.fromSeason;
      item.fromEpisode = row.fromEpisode;
    }
    if (row.nonImdbEpisodes) {
      item.nonImdbEpisodes = row.nonImdbEpisodes;
    }
    const problem = rowProblem(item, taken);
    if (problem) {
      problems.push(`${row.kitsu_id}: ${problem}`);
    }
    taken.add(item.kitsu_id);
    return item;
  });
  if (problems.length) {
    throw new Error(`Nothing added:\n${problems.join('\n')}`);
  }
  items.forEach(item => insertMappingEntry(base, item));
  return items;
}

// Why the row can't be added, or undefined when it can.
function rowProblem(item, taken) {
  if (!Number.isInteger(item.kitsu_id)) return 'kitsu_id is not a number';
  if (taken.has(item.kitsu_id)) return 'kitsu id already has a row';
  if (!/^tt\d+$/.test(item.imdb_id || '')) return 'imdb_id is not a tt id';
  if (!item.title) return 'no title given and the imdb has no rows to take one from';
  if ('fromSeason' in item && !(Number.isInteger(item.fromSeason) && Number.isInteger(item.fromEpisode))) {
    return 'fromSeason and fromEpisode must both be numbers';
  }
  return undefined;
}

// usage: node mapping/add_rows.js <rows.json>
// Reads the live mapping file right before writing, as the review page may have changed it.
if (require.main === module) {
  const [rowsPath] = process.argv.slice(2);
  if (!rowsPath) {
    console.error('usage: node mapping/add_rows.js <rows.json>');
    process.exit(1);
  }
  const rows = JSON.parse(fs.readFileSync(rowsPath, 'utf8'));
  const source = fs.readFileSync(MAPPING_PATH, 'utf8');
  const base = JSON.parse(source);
  try {
    const added = addRows(base, rows);
    fs.writeFileSync(MAPPING_PATH, JSON.stringify(base, null, 2) + (source.endsWith('\n') ? '\n' : ''));
    added.forEach(item => console.log(`added ${item.kitsu_id} -> ${item.imdb_id}`
        + `${'fromSeason' in item ? ` S${item.fromSeason}E${item.fromEpisode}` : ''} (${item.title})`));
  } catch (error) {
    console.error(error.message);
    process.exit(1);
  }
}

module.exports = { addRows };
