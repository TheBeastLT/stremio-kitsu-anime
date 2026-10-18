const test = require('node:test');
const assert = require('node:assert');

const { resolveImdbId, linkFromFribb, linkFromKitsuMappings, malIdByKitsu } = require('../mapping/fribb_mapping');
const { insertInKitsuOrder, insertMappingEntry, mergeVerifiedRows } = require('../mapping/mapping_file');

test('resolveImdbId takes the first imdb id Fribb lists', () => {
  assert.strictEqual(resolveImdbId({ imdb_id: ['tt1920940', 'tt0089206'], tvdb_id: 72025 }), 'tt1920940');
  assert.strictEqual(resolveImdbId({ type: 'TV', imdb_id: [], tvdb_id: 500 }), undefined);
});

test('insertInKitsuOrder places an entry by kitsu id even in a non-monotonic list', () => {
  const base = [
    { kitsu_id: 5, imdb_id: 'ttA' },
    { kitsu_id: 1007, imdb_id: 'ttA' }, // franchise sequel breaks strict ordering
    { kitsu_id: 6, imdb_id: 'ttB' },
    { kitsu_id: 30, imdb_id: 'ttC' },
  ];
  insertInKitsuOrder(base, { kitsu_id: 20, imdb_id: 'ttD' });
  // groups are ordered by their first id: between ttB (6) and ttC (30), never inside ttA's group.
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

test('insertInKitsuOrder places a new group between the first ids around it, past a misplaced group further down', () => {
  const base = [
    { kitsu_id: 637, imdb_id: 'ttA' }, { kitsu_id: 7786, imdb_id: 'ttB' }, { kitsu_id: 455, imdb_id: 'ttC' }, // misplaced
    { kitsu_id: 12755, imdb_id: 'ttD' }, { kitsu_id: 12593, imdb_id: 'ttE' },
  ];
  insertInKitsuOrder(base, { kitsu_id: 500, imdb_id: 'ttN' });
  insertInKitsuOrder(base, { kitsu_id: 12000, imdb_id: 'ttM' });
  assert.deepStrictEqual(base.map(e => e.kitsu_id), [500, 637, 7786, 455, 12000, 12755, 12593]);
});

test('insertMappingEntry orders an imdb\'s rows by season and episode, specials last (Tomb of Fallen Gods S1 before S3)', () => {
  const base = [
    { kitsu_id: 50238, imdb_id: 'ttT', fromSeason: 3, fromEpisode: 1 },
    { kitsu_id: 51000, imdb_id: 'ttT', fromSeason: 0, fromEpisode: 1 },
    { kitsu_id: 51001, imdb_id: 'ttT', fromSeason: 0, fromEpisode: 5 },
    { kitsu_id: 60000, imdb_id: 'ttZ' },
  ];
  insertMappingEntry(base, { kitsu_id: 47172, imdb_id: 'ttT', fromSeason: 1, fromEpisode: 1 });
  insertMappingEntry(base, { kitsu_id: 49096, imdb_id: 'ttT', fromSeason: 2, fromEpisode: 1 });
  insertMappingEntry(base, { kitsu_id: 51002, imdb_id: 'ttT', fromSeason: 0, fromEpisode: 2 });
  assert.deepStrictEqual(base.map(e => e.kitsu_id), [47172, 49096, 50238, 51000, 51002, 51001, 60000]);
});

test('insertMappingEntry never puts a new imdb inside another imdb\'s group (Jewelpet / Battle Spirits case)', () => {
  const base = [
    { kitsu_id: 4349, imdb_id: 'ttA' },
    { kitsu_id: 4383, imdb_id: 'ttJ', fromSeason: 1, fromEpisode: 1 },
    { kitsu_id: 5249, imdb_id: 'ttJ', fromSeason: 2, fromEpisode: 1 },
    { kitsu_id: 10597, imdb_id: 'ttJ', fromSeason: 7, fromEpisode: 1 },
    { kitsu_id: 4817, imdb_id: 'ttC' },
  ];
  insertMappingEntry(base, { kitsu_id: 4779, imdb_id: 'ttB', fromSeason: 1, fromEpisode: 1 });
  assert.deepStrictEqual(base.map(e => e.kitsu_id), [4349, 4383, 5249, 10597, 4779, 4817]);
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

test('malIdByKitsu takes Fribb\'s MAL id, else the one Kitsu links', async () => {
  const kitsuLinks = async () => [{ externalSite: 'anilist/anime', externalId: '7' },
    { externalSite: 'myanimelist/anime', externalId: '65095' }];
  assert.strictEqual(await malIdByKitsu('1', { fribbEntry: async () => ({ mal_id: 290 }), kitsuLinks }), '290');
  assert.strictEqual(await malIdByKitsu('1', { fribbEntry: async () => ({ kitsu_id: 1 }), kitsuLinks }), '65095');
  assert.strictEqual(await malIdByKitsu('1', { fribbEntry: async () => undefined, kitsuLinks: async () => [] }),
      undefined);
});

test('mergeVerifiedRows sends an entry starting where another row starts to review', () => {
  const base = [{ kitsu_id: 5, imdb_id: 'ttA', title: 'A', fromSeason: 1, fromEpisode: 1 }];
  const { merged, clashing } = mergeVerifiedRows(base, [
    { _trusted: true, kitsuId: '9', imdb_id: 'ttA', title: 'A', fromSeason: 1, fromEpisode: 1 }, // same start as 5
    { _trusted: true, kitsuId: '10', imdb_id: 'ttB', title: 'B', fromSeason: 0, fromEpisode: 3 },
    { _trusted: true, kitsuId: '11', imdb_id: 'ttB', title: 'B', fromSeason: 0, fromEpisode: 3 }, // same as 10's
    { _trusted: true, kitsuId: '12', imdb_id: 'ttM', title: 'M' }, // movies have no start to clash on
    { _trusted: true, kitsuId: '13', imdb_id: 'ttM', title: 'M' },
    { _trusted: true, kitsuId: '5', imdb_id: 'ttA', title: 'A', fromSeason: 2, fromEpisode: 1 }, // has a row
  ]);
  assert.deepStrictEqual(merged.map(entry => entry.kitsuId), ['10', '12', '13']);
  assert.deepStrictEqual(clashing.map(entry => [entry.kitsuId, entry.reviewReason, entry._trusted]), [
    ['9', 'ttA S1E1 is already where kitsu 5 starts', undefined],
    ['11', 'ttB S0E3 is already where kitsu 10 starts', undefined],
  ]);
  assert.deepStrictEqual(base.map(row => row.kitsu_id).sort((a, b) => a - b), [5, 10, 12, 13]);
});
