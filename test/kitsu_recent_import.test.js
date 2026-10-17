const test = require('node:test');
const assert = require('node:assert');

const { importableEntries, coveredByMalImport } = require('../mapping/kitsu_recent_import');

test('importableEntries keeps aired, unmapped entries', () => {
  const entries = [
    { id: '1', startDate: '2024-05-01' },
    { id: '2', startDate: '2027-01-10' }, // not aired yet: left to the MAL season import
    { id: '3', startDate: null }, // no date to verify against
    { id: '4', startDate: '2021-02-05' },
    { id: '5', startDate: '2020-01-01' }, // already mapped
    { id: '6', startDate: '2026-01-01', status: 'tba' }, // a placeholder date, not aired
  ];
  const result = importableEntries(entries, { today: new Date('2026-10-16'), isMapped: id => id === '5' });
  assert.deepStrictEqual(result.map(entry => entry.id), ['1', '4']);
});

test('coveredByMalImport leaves out MAL entries of the seasons the MAL import runs', () => {
  const today = new Date('2026-10-16'); // the import runs 2026/summer and 2026/fall
  assert.strictEqual(coveredByMalImport({ startDate: '2026-07-01' }, '123', today), true);
  assert.strictEqual(coveredByMalImport({ startDate: '2026-06-30' }, '123', today), false); // 2026/spring
  assert.strictEqual(coveredByMalImport({ startDate: '2026-10-01' }, undefined, today), false); // not on MAL
  // in a season's last month the next season is imported too
  assert.strictEqual(coveredByMalImport({ startDate: '2027-01-05' }, '123', new Date('2026-12-10')), true);
});
