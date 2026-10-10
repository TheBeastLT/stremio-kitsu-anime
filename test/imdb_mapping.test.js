const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const path = require('path');

const MAPPING_PATH = path.join(__dirname, '../static/data/imdb_mapping.json');
const source = fs.readFileSync(MAPPING_PATH, 'utf8');
const rows = JSON.parse(source);

test('imdb_mapping has one row per kitsu id', () => {
  // the runtime keys rows by kitsu id, so a second row silently replaces the first
  const seen = new Set();
  const duplicates = rows.filter(row => seen.has(row.kitsu_id) || !seen.add(row.kitsu_id)).map(row => row.kitsu_id);
  assert.deepStrictEqual(duplicates, []);
});

test('imdb_mapping keeps the 2-space JSON format the review page and importer write', () => {
  // the page and the importer write no trailing newline, editors add one: either is fine
  assert.ok(source.trimEnd() === JSON.stringify(rows, null, 2), 'reformat with JSON.stringify(rows, null, 2)');
});

test('imdb_mapping rows have a numeric kitsu id, a tt imdb id and both start fields or neither', () => {
  const invalid = rows.filter(row => !Number.isInteger(row.kitsu_id)
      || (row.imdb_id !== undefined && !/^tt\d+$/.test(row.imdb_id))
      || (row.fromSeason === undefined) !== (row.fromEpisode === undefined));
  assert.deepStrictEqual(invalid, []);
});
