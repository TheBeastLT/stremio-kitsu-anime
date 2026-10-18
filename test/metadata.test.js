const test = require('node:test');
const assert = require('node:assert');

const { toStremioEntryMeta } = require('../lib/metadata');

// a Kitsu anime resource as kitsu_api passes it (episodes included)
function animeData(episodeCount, listedEpisodes) {
  return {
    id: '14095', subtype: 'TV', canonicalTitle: 'Gintama.: Shirogane no Tamashii-hen', titles: {},
    startDate: '2018-01-08', episodeCount, genres: { data: [] }, mediaRelationships: { data: [] },
    episodes: {
      data: listedEpisodes.map((number) => ({
        number, titles: { en_us: `Title ${number}` }, airdate: `2018-01-${String(number + 7).padStart(2, '0')}`,
      })),
    },
  };
}

test('an episode list stopping short of the episode count gets the missing episodes too', async () => {
  const meta = await toStremioEntryMeta(animeData(12, [1, 2, 3, 4, 5, 6, 7]), { images: false });
  assert.deepStrictEqual(meta.videos.map((video) => video.episode), [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
  assert.strictEqual(meta.videos[7].title, 'Episode 8'); // a placeholder the imdb enrich renames
  assert.strictEqual(meta.videos[7].id, 'kitsu:14095:8');
  assert.ok(meta.videos[7].released.getTime() >= meta.videos[6].released.getTime());
});

test('a complete episode list stays as kitsu lists it', async () => {
  const meta = await toStremioEntryMeta(animeData(3, [1, 2, 3]), { images: false });
  assert.deepStrictEqual(meta.videos.map((video) => video.title), ['Title 1', 'Title 2', 'Title 3']);
});
