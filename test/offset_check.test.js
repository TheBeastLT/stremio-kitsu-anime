const test = require('node:test');
const assert = require('node:assert');

const { malSeason, recentMalSeasons, kitsuSeasons } = require('../mapping/mal_seasons');
const { verifyOffset } = require('../mapping/mapping_verifier');

// Cinemeta-like series meta from [season, episode, released] tuples.
function cinemeta(videos) {
  return { name: 'Show', type: 'series', videos: videos.map(([season, episode, released]) => ({ season, episode, released })) };
}

test('malSeason and recentMalSeasons follow MAL quarters across the year boundary', () => {
  assert.strictEqual(malSeason(new Date('2026-10-10')), '2026/fall');
  assert.strictEqual(malSeason(new Date('2026-03-31')), '2026/winter');
  assert.deepStrictEqual(recentMalSeasons(2, new Date('2026-10-10')), ['2026/summer', '2026/fall']);
  assert.deepStrictEqual(recentMalSeasons(2, new Date('2026-01-05')), ['2025/fall', '2026/winter']);
});

test('kitsuSeasons maps months onto Kitsu seasons, December being next year\'s winter', () => {
  assert.deepStrictEqual(kitsuSeasons(new Date('2026-06-01'), new Date('2026-10-10')),
      [{ season: 'summer', year: 2026 }, { season: 'fall', year: 2026 }]);
  assert.deepStrictEqual(kitsuSeasons(new Date('2025-11-01'), new Date('2025-12-20')),
      [{ season: 'fall', year: 2025 }, { season: 'winter', year: 2026 }]);
});

test('verifyOffset accepts a start episode aired within the leeway of the Kitsu start', () => {
  const meta = cinemeta([[1, 12, '2026-06-20T15:00:00.000Z'], [1, 13, '2026-07-04T15:00:00.000Z']]);
  const result = verifyOffset({ row: { fromSeason: 1, fromEpisode: 13 }, kitsuStart: '2026-07-05', cinemeta: meta });
  assert.deepStrictEqual(result, { ok: true });
});

test('verifyOffset flags a season IMDb folded into the running one and suggests where it starts now', () => {
  const meta = cinemeta([
    [1, 26, '2026-03-28T00:00:00.000Z'], [1, 27, '2026-07-04T00:00:00.000Z'], [1, 28, '2026-07-11T00:00:00.000Z'],
  ]);
  const result = verifyOffset({ row: { fromSeason: 2, fromEpisode: 1 }, kitsuStart: '2026-07-04', cinemeta: meta });
  assert.strictEqual(result.ok, false);
  assert.strictEqual(result.reason, 'imdb has no S2E1');
  assert.deepStrictEqual(result.suggestions, [{ fromSeason: 1, fromEpisode: 27 }]);
});

test('verifyOffset suggests only the first episode of a streaming drop, and specials only for season-0 rows', () => {
  const meta = cinemeta([
    [0, 3, '2026-07-01T00:00:00.000Z'],
    [2, 1, '2026-07-01T00:00:00.000Z'], [2, 2, '2026-07-01T00:00:00.000Z'], [2, 3, '2026-07-01T00:00:00.000Z'],
  ]);
  const series = verifyOffset({ row: { fromSeason: 1, fromEpisode: 13 }, kitsuStart: '2026-07-01', cinemeta: meta });
  assert.deepStrictEqual(series.suggestions, [{ fromSeason: 2, fromEpisode: 1 }]);
  const special = verifyOffset({ row: { fromSeason: 0, fromEpisode: 2 }, kitsuStart: '2026-07-01', cinemeta: meta });
  assert.deepStrictEqual(special.suggestions, [{ fromSeason: 0, fromEpisode: 3 }]);
});

test('verifyOffset reports a moved start date and leaves undated or unknown episodes unchecked', () => {
  const moved = verifyOffset({
    row: { fromSeason: 2, fromEpisode: 1 }, kitsuStart: '2026-07-04',
    cinemeta: cinemeta([[2, 1, '2026-08-01T00:00:00.000Z']]),
  });
  assert.strictEqual(moved.ok, false);
  assert.strictEqual(moved.unknown, undefined);
  assert.strictEqual(moved.reason, 'S2E1 aired 2026-08-01, kitsu started 2026-07-04');
  assert.deepStrictEqual(moved.suggestions, []);
  const undated = verifyOffset({ row: { fromSeason: 2, fromEpisode: 1 }, kitsuStart: '2026-07-04', cinemeta: cinemeta([[2, 1]]) });
  assert.strictEqual(undated.unknown, true);
  assert.strictEqual(verifyOffset({ row: { fromSeason: 2, fromEpisode: 1 }, kitsuStart: '2026-07-04' }).unknown, true);
});
