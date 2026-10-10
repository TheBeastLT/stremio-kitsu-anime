const test = require('node:test');
const assert = require('node:assert');

const { resolveImdbId, linkFromFribb, linkFromKitsuMappings } = require('../mapping/fribb_mapping');
const {
  insertInKitsuOrder,
  insertMappingEntry,
} = require('../mapping/mal_seasonal_import');

test('resolveImdbId prefers the direct imdb id without calling Trakt', async () => {
  let called = false;
  const imdb = await resolveImdbId(
    { imdb_id: ['tt1920940', 'tt0089206'], tvdb_id: 72025 },
    async () => { called = true; return 'ttWRONG'; }
  );
  assert.strictEqual(imdb, 'tt1920940');
  assert.strictEqual(called, false);
});

test('resolveImdbId derives from tvdb, then tmdb, normalizing both shapes', async () => {
  const calls = [];
  const lookup = async (idType, id, type) => {
    calls.push([idType, id, type]);
    return idType === 'tmdb' ? 'tt0119698' : undefined; // tvdb misses, tmdb hits
  };
  const imdb = await resolveImdbId(
    { type: 'MOVIE', imdb_id: [], tvdb_id: 500, themoviedb_id: { movie: [128] } },
    lookup
  );
  assert.strictEqual(imdb, 'tt0119698');
  assert.deepStrictEqual(calls, [['tvdb', 500, 'movie'], ['tmdb', 128, 'movie']]);
});

test('resolveImdbId returns undefined when nothing resolves', async () => {
  const imdb = await resolveImdbId({ type: 'TV', imdb_id: [], tvdb_id: 500 }, async () => undefined);
  assert.strictEqual(imdb, undefined);
});

test('insertInKitsuOrder places an entry by kitsu id even in a non-monotonic list', () => {
  const base = [
    { kitsu_id: 5 },
    { kitsu_id: 1007 }, // franchise sequel breaks strict ordering
    { kitsu_id: 6 },
    { kitsu_id: 30 },
  ];
  insertInKitsuOrder(base, { kitsu_id: 20 });
  // Must land before 30 (the first id greater-than with all following also greater), not before 1007.
  assert.deepStrictEqual(base.map(e => e.kitsu_id), [5, 1007, 6, 20, 30]);
});

test('insertMappingEntry places a later season right after the last existing sibling', () => {
  const base = [
    { kitsu_id: 5, imdb_id: 'ttShared', fromSeason: 1, fromEpisode: 1 },
    { kitsu_id: 30, imdb_id: 'ttOther' },
  ];
  insertMappingEntry(base, { kitsu_id: 1007, imdb_id: 'ttShared', fromSeason: 2, fromEpisode: 1 });
  assert.deepStrictEqual(base.map(e => e.kitsu_id), [5, 1007, 30]);
});

test('insertMappingEntry puts a continuing cour after the last regular season, before trailing specials (Bookworm case)', () => {
  const base = [
    { kitsu_id: 100, imdb_id: 'ttB', fromSeason: 1, fromEpisode: 1 },
    { kitsu_id: 101, imdb_id: 'ttB', fromSeason: 1, fromEpisode: 15 },
    { kitsu_id: 102, imdb_id: 'ttB', fromSeason: 1, fromEpisode: 27 },
    { kitsu_id: 103, imdb_id: 'ttB', fromSeason: 0, fromEpisode: 1 }, // specials trail
    { kitsu_id: 104, imdb_id: 'ttB', fromSeason: 0, fromEpisode: 3 },
    { kitsu_id: 200, imdb_id: 'ttZ' },
  ];
  insertMappingEntry(base, { kitsu_id: 48293, imdb_id: 'ttB', fromSeason: 1, fromEpisode: 37 });
  // After the last regular season (k102), before the S0 specials.
  assert.deepStrictEqual(base.map(e => e.kitsu_id), [100, 101, 102, 48293, 103, 104, 200]);
});

test('insertMappingEntry appends a special (season 0) after the very last sibling', () => {
  const base = [
    { kitsu_id: 100, imdb_id: 'ttB', fromSeason: 1, fromEpisode: 1 },
    { kitsu_id: 103, imdb_id: 'ttB', fromSeason: 0, fromEpisode: 1 },
    { kitsu_id: 200, imdb_id: 'ttZ' },
  ];
  insertMappingEntry(base, { kitsu_id: 48294, imdb_id: 'ttB', fromSeason: 0, fromEpisode: 5 });
  assert.deepStrictEqual(base.map(e => e.kitsu_id), [100, 103, 48294, 200]);
});

test('insertMappingEntry keeps first-season/standalone entries in kitsu-id order', () => {
  const base = [
    { kitsu_id: 5, imdb_id: 'ttA', fromSeason: 1, fromEpisode: 1 },
    { kitsu_id: 30, imdb_id: 'ttC' },
  ];
  insertMappingEntry(base, { kitsu_id: 20, imdb_id: 'ttB', fromSeason: 1, fromEpisode: 1 });
  assert.deepStrictEqual(base.map(e => e.kitsu_id), [5, 20, 30]);
});

test('linkFromFribb confirms a direct Fribb link from either side, and reports one pointing elsewhere', () => {
  assert.deepStrictEqual(linkFromFribb('5114', 3936, { kitsu_id: 3936 }, undefined), { linked: true });
  assert.deepStrictEqual(linkFromFribb('5114', 3936, undefined, { mal_id: 5114 }), { linked: true });
  assert.deepStrictEqual(linkFromFribb('62914', 48486, undefined, { mal_id: 51898 }),
      { linked: false, reason: 'Fribb links kitsu 48486 to MAL 51898' });
  assert.deepStrictEqual(linkFromFribb('1', 2, { kitsu_id: 9 }, undefined),
      { linked: false, reason: 'Fribb links MAL 1 to kitsu 9' });
  // Fribb has the show as two unjoined records (Prince of Tennis U-17 Ketteisen): no decision here
  assert.strictEqual(linkFromFribb('62534', 51007, { mal_id: 62534, anilist_id: 199068 }, { kitsu_id: 51007 }), undefined);
});

test('linkFromKitsuMappings confirms through Kitsu\'s own MAL/AniList/AniDB ids, ignoring series-wide ids', () => {
  const fribbMal = { mal_id: 62534, anilist_id: 199068, anidb_id: 20348 };
  assert.deepStrictEqual(linkFromKitsuMappings('62534', fribbMal, [{ externalSite: 'anilist/anime', externalId: '199068' }]),
      { linked: true });
  assert.deepStrictEqual(linkFromKitsuMappings('5114', undefined, [{ externalSite: 'myanimelist/anime', externalId: '5114' }]),
      { linked: true });
  // a TVDB/Trakt id is shared by every season, so it neither confirms nor refutes
  assert.deepStrictEqual(linkFromKitsuMappings('62534', fribbMal, [{ externalSite: 'thetvdb/series', externalId: '1' }]),
      { linked: false });
  assert.deepStrictEqual(linkFromKitsuMappings('62534', fribbMal, []), { linked: false });
});

test('linkFromKitsuMappings rejects when any per-entry id disagrees (previous season picked by search)', () => {
  const fribbMal = { mal_id: 65095, anilist_id: 210000 };
  const season2 = [{ externalSite: 'myanimelist/anime', externalId: '58000' }, { externalSite: 'anilist/anime', externalId: '210000' }];
  assert.deepStrictEqual(linkFromKitsuMappings('65095', fribbMal, season2),
      { linked: false, reason: 'kitsu entry is a different show (MAL 58000 != 65095)' });
});
