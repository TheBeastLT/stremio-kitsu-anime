const test = require('node:test');
const assert = require('node:assert');

const { enrichKitsuMetadata, enrichImdbMetadata } = require('../lib/metadataEnrich');

// Attack on Titan (tt2560140): kitsu 7442 is S1, 41982 starts at S3E13 (Season 3 Part 2), 7838 is the S0E1 special.
// One Piece (tt0388629): kitsu 12 from S1E1, with episode 590 missing on imdb (nonImdbEpisodes).

// Cinemeta-like videos of [season, episodeCount] runs
function cinemetaVideos(imdbId, seasons) {
  return seasons.flatMap(([season, count]) => [...Array(count).keys()].map((index) => ({
    id: `${imdbId}:${season}:${index + 1}`, season, episode: index + 1, released: '2020-01-01T00:00:00.000Z',
  })));
}

function kitsuMeta(kitsuId, episodeCount, type = 'series') {
  return {
    kitsu_id: kitsuId, type, genres: ['Action'], links: [],
    videos: [...Array(episodeCount).keys()].map((index) => ({
      id: `kitsu:${kitsuId}:${index + 1}`, title: `Episode ${index + 1}`, season: 1, episode: index + 1,
      released: new Date('2020-01-01'),
    })),
  };
}

const AOT_VIDEOS = cinemetaVideos('tt2560140', [[0, 2], [1, 25], [2, 12], [3, 22]]);

test('a kitsu entry starting mid-series counts on from the imdb episodes before its start', async () => {
  const meta = await enrichKitsuMetadata(kitsuMeta(41982, 10), async () => ({ videos: AOT_VIDEOS }));
  assert.deepStrictEqual(meta.videos.slice(0, 2)
      .map((video) => [video.imdbSeason, video.imdbEpisode, video.absoluteEpisode]), [[3, 13, 50], [3, 14, 51]]);
});

test('kitsu episodes missing on imdb keep kitsu\'s own numbering', async () => {
  const videos = cinemetaVideos('tt0388629', [[1, 591]]);
  const meta = await enrichKitsuMetadata(kitsuMeta(12, 592), async () => ({ videos }));
  const video = meta.videos.find((video) => video.episode === 591);
  assert.deepStrictEqual([video.imdbEpisode, video.absoluteEpisode], [590, 591]);
});

test('without imdb episodes only an entry starting the first season is counted', async () => {
  const first = await enrichKitsuMetadata(kitsuMeta(7442, 2), async () => undefined);
  const later = await enrichKitsuMetadata(kitsuMeta(41982, 2), async () => undefined);
  assert.deepStrictEqual(first.videos.map((video) => video.absoluteEpisode), [1, 2]);
  assert.deepStrictEqual(later.videos.map((video) => video.absoluteEpisode), [undefined, undefined]);
});

test('specials and unmapped kitsu entries', async () => {
  const special = await enrichKitsuMetadata(kitsuMeta(7838, 1), async () => ({ videos: AOT_VIDEOS }));
  const unmapped = await enrichKitsuMetadata(kitsuMeta(99999999, 2), async () => assert.fail('no imdb lookup'));
  assert.strictEqual(special.videos[0].absoluteEpisode, undefined);
  assert.deepStrictEqual(unmapped.videos.map((video) => video.absoluteEpisode), [1, 2]);
});

test('an imdb meta numbers its episodes like the kitsu metas do', async () => {
  // copied: the imdb enrichment sorts the videos in place
  const meta = await enrichImdbMetadata({ id: 'tt2560140', type: 'series', videos: [...AOT_VIDEOS] },
      async () => assert.fail('no kitsu lookup'));
  const absolute = (season, episode) => meta.videos
      .find((video) => video.season === season && video.episode === episode).absoluteEpisode;
  assert.deepStrictEqual([absolute(0, 1), absolute(1, 1), absolute(2, 12), absolute(3, 13)], [undefined, 1, 37, 50]);
});

test('an imdb meta without cinemeta episodes counts the first season\'s kitsu episodes', async () => {
  const meta = await enrichImdbMetadata({ id: 'tt0388629' }, async (kitsuId) => kitsuMeta(kitsuId, 2));
  const counted = meta.videos.filter((video) => video.absoluteEpisode !== undefined);
  assert.deepStrictEqual(counted.map((video) => [video.kitsu_id, video.absoluteEpisode]), [['12', 1], ['12', 2]]);
});

test('an imdb meta skips kitsu episodes missing on imdb', async () => {
  const videos = cinemetaVideos('tt0388629', [[1, 591]]);
  const meta = await enrichImdbMetadata({ id: 'tt0388629', type: 'series', videos }, async () => assert.fail());
  const video = meta.videos.find((video) => video.season === 1 && video.episode === 590);
  assert.deepStrictEqual([video.kitsuEpisode, video.absoluteEpisode], [591, 591]);
});

test('an imdb meta gives no kitsu episode past a finished entry\'s last one', async () => {
  // Mobile Suit Gundam SEED (tt0374407): kitsu 72 is the 50 episodes of S1, S2 has no row of its own
  const videos = cinemetaVideos('tt0374407', [[1, 50], [2, 50]]);
  const meta = await enrichImdbMetadata({ id: 'tt0374407', type: 'series', videos }, async () => assert.fail());
  const episode = (season, number) => meta.videos.find((video) => video.season === season && video.episode === number);
  assert.deepStrictEqual([episode(1, 50), episode(2, 1)].map((video) => [video.kitsu_id, video.kitsuEpisode,
    video.absoluteEpisode]), [['72', 50, 50], [undefined, undefined, 51]]);
});

test('movies get no franchise-wide number', async () => {
  const movie = await enrichKitsuMetadata(kitsuMeta(99999999, 1, 'movie'), async () => assert.fail('no imdb lookup'));
  const toImdbMovie = await enrichKitsuMetadata(kitsuMeta(7442, 1), async () => ({ type: 'movie' }));
  assert.deepStrictEqual([movie.videos[0].absoluteEpisode, toImdbMovie.videos[0].absoluteEpisode],
      [undefined, undefined]);
});

// One Piece (tt0388629) specials: Barto's Secret Room (45690) is S0E40, 41, 49 and 50; 48291 is S0E47
const ONE_PIECE_SPECIALS = cinemetaVideos('tt0388629', [[0, 60]]);

test('a kitsu entry listing its imdb episodes takes them, out of sequence', async () => {
  const meta = await enrichKitsuMetadata(kitsuMeta(45690, 4), async () => ({ videos: ONE_PIECE_SPECIALS }));
  assert.deepStrictEqual(meta.videos.map((video) => video.imdbEpisode), [40, 41, 49, 50]);
  // Stargazer (1092) is one IMDb special for its three Kitsu episodes
  const stargazer = await enrichKitsuMetadata(kitsuMeta(1092, 3), async () => ({
    videos: cinemetaVideos('tt0434694', [[0, 10]]),
  }));
  assert.deepStrictEqual(stargazer.videos.map((video) => video.imdbEpisode), [7, undefined, undefined]);
});

test('a kitsu entry\'s run steps over the episodes another row lists', async () => {
  const meta = await enrichKitsuMetadata(kitsuMeta(48291, 3), async () => ({ videos: ONE_PIECE_SPECIALS }));
  assert.deepStrictEqual(meta.videos.map((video) => video.imdbEpisode), [47, 48, 51]);
});

test('an imdb meta gives listed episodes to the row listing them, not to the run they fall in', async () => {
  const meta = await enrichImdbMetadata({ id: 'tt0388629', type: 'series', videos: [...ONE_PIECE_SPECIALS] },
      async () => assert.fail('no kitsu lookup'));
  const special = (episode) => meta.videos.find((video) => video.season === 0 && video.episode === episode);
  assert.deepStrictEqual([41, 42, 49, 50].map((episode) => [special(episode).kitsu_id, special(episode).kitsuEpisode]),
      [['45690', 2], ['46215', 1], ['45690', 3], ['45690', 4]]);
});
