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

test('imdb_mapping has no two rows starting at the same imdb episode', () => {
  // the imdb meta gives each episode to one of them, so the other's season links to nothing (Dog Days at S2E1 with
  // Dog Days')
  const starts = new Map();
  rows.filter(row => row.imdb_id && row.fromSeason !== undefined).forEach(row => {
    const start = `${row.imdb_id} S${row.fromSeason}E${row.fromEpisode}`;
    starts.set(start, [...(starts.get(start) || []), row.kitsu_id]);
  });
  assert.deepStrictEqual([...starts].filter(([, kitsuIds]) => kitsuIds.length > 1), []);
});

const { addRows } = require('../mapping/add_rows');

test('addRows inserts a special after its imdb siblings, taking their title', () => {
  const base = [
    { kitsu_id: 1, imdb_id: 'tt1', title: 'Show', fromSeason: 1, fromEpisode: 1 },
    { kitsu_id: 9, imdb_id: 'tt9', title: 'Other' },
  ];
  addRows(base, [{ kitsu_id: 5, imdb_id: 'tt1', fromSeason: 0, fromEpisode: 2 }]);
  assert.deepStrictEqual(base.map(row => row.kitsu_id), [1, 5, 9]);
  assert.deepStrictEqual(base[1], { kitsu_id: 5, imdb_id: 'tt1', title: 'Show', fromSeason: 0, fromEpisode: 2 });
});

test('addRows adds nothing when a kitsu id already has a row', () => {
  const base = [{ kitsu_id: 1, imdb_id: 'tt1', title: 'Show', fromSeason: 1, fromEpisode: 1 }];
  assert.throws(() => addRows(base, [
    { kitsu_id: 2, imdb_id: 'tt1', fromSeason: 2, fromEpisode: 1 },
    { kitsu_id: 1, imdb_id: 'tt1', fromSeason: 0, fromEpisode: 1 },
  ]), /1: kitsu id already has a row/);
  assert.strictEqual(base.length, 1);
});
