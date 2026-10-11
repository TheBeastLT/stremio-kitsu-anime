const test = require('node:test');
const assert = require('node:assert');

const { seasonWindow, verifyCandidate, verifySpecial, nextSeason, continuationStart } = require('../mapping/mapping_verifier');

const FALL_2026 = seasonWindow('2026/fall');

// Cinemeta-like series meta with `counts[season]` episodes per season, each season premiering on `premieres[season]`.
function series(counts, premieres) {
  const videos = Object.entries(counts).flatMap(([season, count]) => [...Array(count).keys()].map(i => ({
    season: Number(season),
    episode: i + 1,
    released: new Date(new Date(premieres[season]).getTime() + i * 7 * 86400000).toISOString(),
  })));
  return { type: 'series', name: 'Show', genres: ['Animation'], videos };
}

function verify(overrides) {
  return verifyCandidate({
    candidate: { imdb_id: 'tt1', fromSeason: 1, fromEpisode: 1 },
    linked: true,
    animeType: 'TV',
    cinemeta: series({ 1: 12 }, { 1: '2026-10-04' }),
    siblings: [],
    window: FALL_2026,
    ...overrides,
  });
}

test('seasonWindow covers the MAL season, padded a month early', () => {
  assert.deepStrictEqual(seasonWindow('2026/fall'),
      { from: new Date('2026-09-01T00:00:00Z'), to: new Date('2026-12-31T23:59:59Z') });
  assert.deepStrictEqual(seasonWindow('2025/winter'),
      { from: new Date('2024-12-01T00:00:00Z'), to: new Date('2025-03-31T23:59:59Z') });
  assert.throws(() => seasonWindow('2025/autumn'));
});

test('nextSeason is one past the distinct regular seasons, ignoring specials', () => {
  assert.strictEqual(nextSeason([]), 1);
  assert.strictEqual(nextSeason([{ fromSeason: 1 }, { fromSeason: 0 }, { fromSeason: 0 }]), 2);
  assert.strictEqual(nextSeason([{ fromSeason: 1 }, { fromSeason: 2 }, { fromSeason: 3 }]), 4);
});

test('accepts a brand new series premiering this season', () => {
  assert.deepStrictEqual(verify({}), { ok: true });
});

test('rejects when Fribb does not confirm the mal-kitsu link', () => {
  assert.strictEqual(verify({ linked: false }).ok, false);
});

test('rejects non-title imdb ids and ids unknown to cinemeta', () => {
  assert.strictEqual(verify({ candidate: { imdb_id: 'nm5918389', fromSeason: 1 } }).ok, false);
  assert.strictEqual(verify({ candidate: { imdb_id: '/scary-good/', fromSeason: 1 } }).ok, false);
  assert.strictEqual(verify({ cinemeta: { id: 'tt1', imdb_id: 'tt1' } }).ok, false); // cinemeta failure fallback
});

test('rejects a kitsu TV series pointing at an imdb movie (Pan no Akachan -> Rosemary\'s Baby)', () => {
  const result = verify({ cinemeta: { type: 'movie', name: "Rosemary's Baby", genres: ['Animation'], year: '1968' } });
  assert.match(result.reason, /type mismatch/);
});

test('rejects a kitsu movie numbered as a new season of a series (Made in Abyss movie as S3)', () => {
  const result = verify({ animeType: 'movie', candidate: { imdb_id: 'tt1', fromSeason: 3 } });
  assert.match(result.reason, /type mismatch/);
});

test('accepts a current movie, rejects one from years ago (Girls und Panzer Part 4 id for Part 5)', () => {
  const movie = year => ({ animeType: 'movie', candidate: { imdb_id: 'tt1' }, cinemeta: { type: 'movie', name: 'M', genres: ['Animation'], year } });
  assert.deepStrictEqual(verify(movie('2026')), { ok: true });
  assert.deepStrictEqual(verify(movie('2027')), { ok: true });
  assert.match(verify(movie('2023')).reason, /far from season/);
});

test('rejects a series whose season premiered outside this MAL season (Frieren id for Tomica)', () => {
  const result = verify({ cinemeta: series({ 1: 28 }, { 1: '2023-09-29' }) });
  assert.match(result.reason, /outside season/);
});

test('accepts a sequel after cleanly mapped seasons, ignoring specials (Black Clover S2)', () => {
  const result = verify({
    candidate: { imdb_id: 'tt1', fromSeason: 2, fromEpisode: 1 },
    cinemeta: series({ 0: 19, 1: 170, 2: 13 }, { 0: '2016-11-27', 1: '2017-10-03', 2: '2026-10-03' }),
    siblings: [
      { kitsu_id: 13209, fromSeason: 1, fromEpisode: 1, episodeCount: 170 },
      { kitsu_id: 12625, fromSeason: 0, fromEpisode: 1 },
    ],
  });
  assert.deepStrictEqual(result, { ok: true });
});

test('rejects a "Part 2" cour that imdb keeps in the previous season (Yozakura S2 Part 2 as S3)', () => {
  const result = verify({
    candidate: { imdb_id: 'tt1', fromSeason: 3, fromEpisode: 1 },
    cinemeta: series({ 1: 27, 2: 15 }, { 1: '2024-04-07', 2: '2026-04-12' }),
    siblings: [
      { kitsu_id: 1, fromSeason: 1, fromEpisode: 1, episodeCount: 27 },
      { kitsu_id: 2, fromSeason: 2, fromEpisode: 1, episodeCount: 12 },
    ],
  });
  assert.match(result.reason, /no S3E1/);
});

test('rejects when an earlier season is split across kitsu entries or counts differ', () => {
  const cinemeta = series({ 1: 24, 2: 12 }, { 1: '2025-04-01', 2: '2026-10-01' });
  const candidate = { imdb_id: 'tt1', fromSeason: 2, fromEpisode: 1 };
  const split = verify({ candidate, cinemeta, siblings: [
    { kitsu_id: 1, fromSeason: 1, fromEpisode: 1, episodeCount: 12 },
    { kitsu_id: 2, fromSeason: 1, fromEpisode: 13, episodeCount: 12 },
  ] });
  assert.match(split.reason, /split/);
  const mismatch = verify({ candidate, cinemeta, siblings: [{ kitsu_id: 1, fromSeason: 1, fromEpisode: 1, episodeCount: 12 }] });
  assert.match(mismatch.reason, /kitsu eps 12 != imdb eps 24/);
  const unknown = verify({ candidate, cinemeta, siblings: [{ kitsu_id: 1, fromSeason: 1, fromEpisode: 1 }] });
  assert.strictEqual(unknown.ok, false);
});

test('accepts the next season after a used-up season split across cours, airing with kitsu episode 1 (Re:Zero S4)', () => {
  const candidate = { imdb_id: 'tt1', fromSeason: 4, fromEpisode: 1 };
  const cinemeta = series({ 1: 25, 2: 25, 3: 16, 4: 12 },
      { 1: '2016-04-04', 2: '2020-07-08', 3: '2024-10-02', 4: '2026-10-07' });
  const siblings = [
    { kitsu_id: 1, fromSeason: 1, fromEpisode: 1, episodeCount: 25 },
    { kitsu_id: 2, fromSeason: 2, fromEpisode: 1, episodeCount: 13 },
    { kitsu_id: 3, fromSeason: 2, fromEpisode: 14, episodeCount: 12 },
    { kitsu_id: 4, fromSeason: 3, fromEpisode: 1, episodeCount: 8 },
    { kitsu_id: 5, fromSeason: 3, fromEpisode: 9, episodeCount: 8 },
  ];
  assert.deepStrictEqual(verify({ candidate, cinemeta, siblings, kitsuFirstAired: '2026-10-08' }), { ok: true });
  // Also when IMDb's S4E1 premiered before the MAL season's window, as long as it aired with kitsu episode 1.
  const early = series({ 1: 25, 2: 25, 3: 16, 4: 12 }, { 1: '2016-04-04', 2: '2020-07-08', 3: '2024-10-02', 4: '2026-06-01' });
  assert.deepStrictEqual(verify({ candidate, cinemeta: early, siblings, kitsuFirstAired: '2026-06-01' }), { ok: true });
});

test('rejects the next season when the last season is not used up or the dates disagree', () => {
  const candidate = { imdb_id: 'tt1', fromSeason: 2, fromEpisode: 1 };
  const siblings = [{ kitsu_id: 1, fromSeason: 1, fromEpisode: 1, episodeCount: 12 }, { kitsu_id: 2, fromSeason: 1, fromEpisode: 13, episodeCount: 6 }];
  const cinemeta = series({ 1: 18, 2: 12 }, { 1: '2025-04-01', 2: '2026-10-01' });
  assert.deepStrictEqual(verify({ candidate, cinemeta, siblings, kitsuFirstAired: '2026-10-02' }), { ok: true });
  // IMDb's S1 runs past the Kitsu entries (a cour Kitsu has no entry for yet, or a short count).
  const longer = series({ 1: 24, 2: 12 }, { 1: '2025-04-01', 2: '2026-10-01' });
  assert.match(verify({ candidate, cinemeta: longer, siblings, kitsuFirstAired: '2026-10-02' }).reason, /split/);
  assert.match(verify({ candidate, cinemeta, siblings, kitsuFirstAired: '2026-10-20' }).reason, /split/);
  assert.match(verify({ candidate, cinemeta, siblings }).reason, /split/);
  // An unknown episode count can't prove the season is used up.
  const unknown = [{ kitsu_id: 1, fromSeason: 1, fromEpisode: 1, episodeCount: 12 }, { kitsu_id: 2, fromSeason: 1, fromEpisode: 13 }];
  assert.strictEqual(verify({ candidate, cinemeta, siblings: unknown, kitsuFirstAired: '2026-10-02' }).ok, false);
});

test('rejects a sequel with a gap in the mapped seasons (Tomica Season 2 suggested as S1 while S1,S2 exist)', () => {
  const result = verify({
    candidate: { imdb_id: 'tt1', fromSeason: 3, fromEpisode: 1 },
    cinemeta: series({ 1: 12, 2: 12, 3: 12 }, { 1: '2024-01-01', 2: '2025-01-01', 3: '2026-10-01' }),
    siblings: [{ kitsu_id: 1, fromSeason: 1, fromEpisode: 1, episodeCount: 12 }],
  });
  assert.match(result.reason, /not contiguous/);
});

test('rejects a same-named live-action title (LOCA! -> "Vieja Loca")', () => {
  const result = verify({
    animeType: 'movie',
    candidate: { imdb_id: 'tt31613162' },
    cinemeta: { type: 'movie', name: 'Vieja Loca', genres: ['Horror', 'Thriller'], year: '2025' },
  });
  assert.match(result.reason, /not tagged Animation/);
});

test('rejects a movie whose kitsu entry is an older namesake (1983 Doraemon film -> 2025 one)', () => {
  const doraemon = kitsuYear => verify({
    animeType: 'movie',
    kitsuYear,
    candidate: { imdb_id: 'tt33512868' },
    cinemeta: { type: 'movie', name: "Doraemon the Movie: Nobita's Art World Tales", genres: ['Animation'], year: '2025' },
    window: seasonWindow('2025/spring'),
  });
  assert.match(doraemon('1983').reason, /far from kitsu year 1983/);
  assert.deepStrictEqual(doraemon('2025'), { ok: true });
  assert.deepStrictEqual(doraemon(undefined), { ok: true });
});

test('treats a single-episode ONA/OVA/special as a movie when imdb lists it as one (Soul Land: Sword Master)', () => {
  const extra = (animeType, kitsuEpisodes, candidate = { imdb_id: 'tt42220747' }) => verify({
    animeType,
    kitsuEpisodes,
    kitsuYear: '2025',
    candidate,
    cinemeta: { type: 'movie', name: 'Soul Land: Sword Master', genres: ['Animation'], releaseInfo: '2025' },
    window: seasonWindow('2025/summer'),
  });
  assert.deepStrictEqual(extra('ONA', 1), { ok: true });
  assert.deepStrictEqual(extra('OVA', 1), { ok: true });
  assert.deepStrictEqual(extra('special', 1), { ok: true });
  assert.match(extra('ONA', 26).reason, /type mismatch/);       // multi-episode ONA -> movie (Xuan Jie Zhi Men -> "Bad Sister")
  assert.match(extra('ONA', undefined).reason, /type mismatch/); // unknown episode count stays in review
  assert.match(extra('TV', 1).reason, /type mismatch/);
  assert.match(extra('ONA', 1, { imdb_id: 'tt42220747', fromSeason: 1 }).reason, /movie mapped onto a season/);
});

test('a single-episode ONA still needs the movie year checks (Hizukuri -> 2021 "Fire Craft" in 2025)', () => {
  const result = verify({
    animeType: 'ONA',
    kitsuEpisodes: 1,
    kitsuYear: '2023',
    candidate: { imdb_id: 'tt13421562' },
    cinemeta: { type: 'movie', name: 'Fire Craft', genres: ['Animation'], releaseInfo: '2021' },
    window: seasonWindow('2025/fall'),
  });
  assert.match(result.reason, /far from season/);
});

// Weekly-aired season of `count` episodes from `premiere`, optionally with a break before episode `breakAt`.
function weekly(season, count, premiere, { breakAt, breakDays = 0 } = {}) {
  return [...Array(count).keys()].map(i => ({
    season,
    episode: i + 1,
    released: new Date(new Date(premiere).getTime() + (i * 7 + (breakAt && i + 1 >= breakAt ? breakDays : 0)) * 86400000).toISOString(),
  }));
}

function continuation(overrides) {
  return verify({
    candidate: { imdb_id: 'tt1', fromSeason: 1, fromEpisode: 13 },
    // Puniru-style: IMDb keeps a 24 episode S1, second cour starts after a 2.5 month break.
    cinemeta: { type: 'series', name: 'Show', genres: ['Animation'], videos: weekly(1, 24, '2026-06-01', { breakAt: 13, breakDays: 75 }) },
    siblings: [{ kitsu_id: 1, fromSeason: 1, fromEpisode: 1, episodeCount: 12 }],
    kitsuFirstAired: '2026-11-07',
    ...overrides,
  });
}

test('continuationStart is one past the episodes covered in the last mapped season', () => {
  assert.deepStrictEqual(continuationStart([{ fromSeason: 1, fromEpisode: 1, episodeCount: 12 }]), { fromSeason: 1, fromEpisode: 13 });
  // Hyakushou Kizoku: S1E1 (12) + S1E13 (12) -> next cour starts at E25, not E24.
  assert.deepStrictEqual(continuationStart([
    { fromSeason: 1, fromEpisode: 1, episodeCount: 12 },
    { fromSeason: 1, fromEpisode: 13, episodeCount: 12 },
    { fromSeason: 0, fromEpisode: 1, episodeCount: 2 },
  ]), { fromSeason: 1, fromEpisode: 25 });
  assert.strictEqual(continuationStart([{ fromSeason: 1, fromEpisode: 1, episodeCount: null }]), undefined);
  assert.strictEqual(continuationStart([]), undefined);
});

test('accepts a continuation cour whose first imdb episode airs with kitsu episode 1', () => {
  assert.deepStrictEqual(continuation({}), { ok: true });
  // Broadcast vs streaming / timezone differences up to 3 days (Let This Grieving Soul Retire! was 2.6 days).
  assert.deepStrictEqual(continuation({ kitsuFirstAired: '2026-11-04' }), { ok: true });
});

test('rejects a continuation when the dates disagree beyond the leeway (Yozakura: imdb 10-11, kitsu 10-31)', () => {
  assert.match(continuation({ kitsuFirstAired: '2026-11-11' }).reason, /kitsu ep 1 2026-11-11/);
  assert.match(continuation({ kitsuFirstAired: undefined }).reason, /kitsu ep 1 unknown/);
});

test('rejects a continuation that is not the computed offset or has no dated imdb episode', () => {
  assert.match(continuation({ candidate: { imdb_id: 'tt1', fromSeason: 1, fromEpisode: 12 } }).reason, /should start at S1E13/);
  const short = { type: 'series', name: 'Show', genres: ['Animation'], videos: weekly(1, 12, '2026-06-01') };
  assert.match(continuation({ cinemeta: short }).reason, /no dated S1E13/);
});

test('rejects a continuation without a clear cour boundary (whole part dropped on one day)', () => {
  const drop = [...Array(24).keys()].map(i => ({ season: 1, episode: i + 1, released: i < 12 ? '2026-06-01T00:00:00Z' : '2026-11-07T00:00:00Z' }));
  // Off-by-one counts would land inside the same-day drop: E14's predecessor E13 aired the same day.
  const offByOne = continuation({
    candidate: { imdb_id: 'tt1', fromSeason: 1, fromEpisode: 14 },
    siblings: [{ kitsu_id: 1, fromSeason: 1, fromEpisode: 1, episodeCount: 13 }],
    cinemeta: { type: 'series', name: 'Show', genres: ['Animation'], videos: drop },
  });
  assert.match(offByOne.reason, /no clear cour boundary before S1E14/);
  // The real boundary in the same drop is still provable.
  assert.deepStrictEqual(continuation({ cinemeta: { type: 'series', name: 'Show', genres: ['Animation'], videos: drop } }), { ok: true });
});

test('rejects a continuation airing outside the imported MAL season', () => {
  assert.match(continuation({ window: seasonWindow('2026/summer') }).reason, /outside season/);
});

// Franchise with IMDb specials dated like Hyakushô Kizoku: S0E1-2 2023-12-20, S0E3-4 2025-04-23, S0E5-6 2026-04-24.
const SPRING_2026 = seasonWindow('2026/spring');
const HYAKUSHO_SPECIALS = {
  type: 'series', name: 'Hyakushô Kizoku', genres: ['Animation'],
  videos: [['2023-12-20', 1], ['2023-12-20', 2], ['2025-04-23', 3], ['2025-04-23', 4], ['2026-04-24', 5], ['2026-04-24', 6]]
      .map(([day, episode]) => ({ season: 0, episode, released: `${day}T00:00:00.000Z` }))
      .concat([{ season: 1, episode: 1, released: '2023-07-07T00:00:00.000Z' }]),
};

function special(overrides) {
  return verifySpecial({
    animeType: 'special',
    kitsuAirDates: ['2026-04-24T00:00:00.000Z', '2026-04-24T00:00:01.000Z'],
    kitsuEpisodeLength: 24,
    cinemeta: HYAKUSHO_SPECIALS,
    siblings: [
      { kitsu_id: 48933, fromSeason: 0, fromEpisode: 1, episodeCount: 2 },
      { kitsu_id: 49440, fromSeason: 0, fromEpisode: 3, episodeCount: 2 },
      { kitsu_id: 49439, fromSeason: 1, fromEpisode: 25 },
    ],
    window: SPRING_2026,
    ...overrides,
  });
}

test('maps a franchise special onto the S0 episodes aired with it (Hyakushô Kizoku specials -> S0E5)', () => {
  assert.deepStrictEqual(special({}), { ok: true, fromEpisode: 5 });
  // within the leeway: streaming a couple of days after the broadcast date IMDb lists
  assert.deepStrictEqual(special({ kitsuAirDates: ['2026-04-26T00:00:00.000Z', '2026-04-26T00:00:00.000Z'] }),
      { ok: true, fromEpisode: 5 });
});

test('does not reuse S0 episodes other kitsu entries already cover', () => {
  const result = special({ kitsuAirDates: ['2025-04-23T00:00:00.000Z', '2025-04-23T00:00:00.000Z'] });
  assert.strictEqual(result.ok, false);
  assert.match(result.reason, /no unused S0 episodes/);
});

test('rejects when the S0 span of an existing special is unknown', () => {
  const result = special({ siblings: [{ kitsu_id: 48933, fromSeason: 0, fromEpisode: 1, episodeCount: undefined }] });
  assert.match(result.reason, /S0 span of kitsu 48933 unknown/);
});

test('rejects when several S0 runs fit the dates', () => {
  const result = special({ kitsuAirDates: ['2026-04-24T00:00:00.000Z'], siblings: [] });
  assert.strictEqual(result.ok, false);
  assert.match(result.reason, /2 S0 runs match .*S0E5, S0E6/);
});

test('leaves a single feature-length special for review (Violet Evergarden: Recollections)', () => {
  const result = special({ kitsuAirDates: ['2026-04-24T00:00:00.000Z'], kitsuEpisodeLength: 95,
    siblings: [{ kitsu_id: 1, fromSeason: 0, fromEpisode: 6, episodeCount: 1 }] });
  assert.match(result.reason, /single 95 min special may be a standalone movie/);
});

test('rejects movies, undated episodes and franchises that are not series', () => {
  assert.match(special({ animeType: 'movie' }).reason, /not a special/);
  assert.match(special({ animeType: 'TV' }).reason, /not a special/);
  assert.match(special({ kitsuAirDates: [undefined, '2026-04-24T00:00:00.000Z'] }).reason, /undated/);
  assert.match(special({ cinemeta: { ...HYAKUSHO_SPECIALS, type: 'movie' } }).reason, /franchise imdb is a movie/);
  assert.match(special({ cinemeta: undefined }).reason, /unknown to cinemeta/);
});

test('suggests but does not accept a matching S0 run that aired outside the season', () => {
  const result = special({ window: seasonWindow('2025/fall') });
  assert.deepStrictEqual({ ok: result.ok, fromEpisode: result.fromEpisode }, { ok: false, fromEpisode: 5 });
  assert.match(result.reason, /S0E5 aired 2026-04-24 outside season/);
});

test('rejects a run when another free S0 episode aired on the same dates (Elf-san: E14 and E15 both on 10-19)', () => {
  const elfSan = {
    type: 'series', name: 'Elf-san wa Yaserarenai.', genres: ['Animation'],
    videos: [['2024-09-22', 12], ['2024-10-05', 13], ['2024-10-19', 14], ['2024-10-19', 15], ['2024-11-21', 16]]
        .map(([day, episode]) => ({ season: 0, episode, released: `${day}T00:00:00.000Z` })),
  };
  const result = special({
    cinemeta: elfSan, siblings: [], window: seasonWindow('2024/fall'),
    kitsuAirDates: ['2024-10-05T00:00:00.000Z', '2024-10-19T00:00:00.000Z'],
  });
  assert.strictEqual(result.ok, false);
  assert.match(result.reason, /S0E13 fits, but S0E15 aired on the same dates/);
});

test('rejects a date match when another free S0 episode is titled after the special (Guilty Crown: Lost Christmas)', () => {
  const guiltyCrown = {
    type: 'series', name: 'Guilty Crown', genres: ['Animation'],
    videos: [
      { season: 0, episode: 2, released: '2012-05-31T00:00:00.000Z', name: 'Lost Christmas' },
      { season: 0, episode: 9, released: '2012-07-25T00:00:00.000Z', name: 'Guilty Crown 4-Panel Theater - Part 7' },
    ],
  };
  const result = special({
    animeType: 'OVA', cinemeta: guiltyCrown, siblings: [], window: seasonWindow('2012/summer'),
    kitsuAirDates: ['2012-07-26T00:00:00.000Z'], kitsuTitles: ['Guilty Crown: Lost Christmas'],
  });
  assert.match(result.reason, /S0E9 fits the dates, but S0E2 is titled "Lost Christmas"/);
  // the matched episode carrying the title is fine
  assert.deepStrictEqual(special({ kitsuTitles: ['Hyakushou Kizoku: Specials'], cinemeta: {
    ...HYAKUSHO_SPECIALS, videos: HYAKUSHO_SPECIALS.videos.map(v => ({ ...v, name: `Specials ${v.episode}` })),
  } }), { ok: true, fromEpisode: 5 });
});

test('rejects when an unmapped regular episode aired on the same dates (Kamichu! OVA is IMDb S1E13)', () => {
  const kamichu = {
    type: 'series', name: 'Kamichu!', genres: ['Animation'],
    videos: [...Array(13).keys()].map(i => ({
      season: 1, episode: i + 1,
      released: i < 12 ? new Date(Date.UTC(2005, 5, 21 + 7 * i)).toISOString() : '2006-03-24T00:00:00.000Z',
    })).concat([{ season: 0, episode: 1, released: '2006-03-24T00:00:00.000Z' }]),
  };
  const tvRow = { kitsu_id: 1, fromSeason: 1, fromEpisode: 1, episodeCount: 12 };
  const result = special({
    cinemeta: kamichu, siblings: [tvRow], window: seasonWindow('2006/winter'),
    kitsuAirDates: ['2006-03-24T00:00:00.000Z'],
  });
  assert.match(result.reason, /unmapped S1E13 aired on the same dates/);
  // once a kitsu entry covers S1E13, the special is free to take S0E1
  assert.deepStrictEqual(special({
    cinemeta: kamichu, siblings: [{ ...tvRow, episodeCount: 13 }], window: seasonWindow('2006/winter'),
    kitsuAirDates: ['2006-03-24T00:00:00.000Z'],
  }), { ok: true, fromEpisode: 1 });
});
