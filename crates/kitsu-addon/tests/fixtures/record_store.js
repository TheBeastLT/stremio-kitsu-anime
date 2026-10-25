process.env.FANART_APIKEY ??= 'test';
delete process.env.MONGODB_URI;

const fs = require('fs');
const path = require('path');
const { BSON } = require('mongodb');
const { KeyvMongo } = require('@keyv/mongo');
const { createCache } = require('cache-manager');
const { persistentStore } = require('../../../../lib/cache');

const NODE_WRITTEN = path.join(__dirname, 'store_node_written.json');
const RUST_WRITTEN = path.join(__dirname, 'store_rust_written.json');
const DAY = 24 * 60 * 60 * 1000;

// The real KeyvMongo methods over an in-memory collection that keeps each document as the BSON bytes the driver sends:
// an upsert on key creates { _id, key } and applies the $set, as the server does.
function fakeKeyvMongo(documents = new Map()) {
  let nextId = 1;
  const read = (bytes) => BSON.deserialize(bytes);
  const matches = (filter) => {
    const keys = filter.key.$in || [filter.key.$eq];
    return [...documents.keys()].filter((key) => keys.includes(key));
  };
  const collection = {
    async findOne(filter) {
      const [key] = matches(filter);
      return key === undefined ? null : read(documents.get(key));
    },
    find(filter) {
      return {
        project: () => ({ toArray: async () => matches(filter).map((key) => read(documents.get(key))) }),
      };
    },
    async updateOne(filter, update, options) {
      const key = filter.key.$eq;
      const existing = documents.has(key) ? read(documents.get(key)) : options.upsert && {
        _id: new BSON.ObjectId((nextId++).toString(16).padStart(24, '0')),
        key,
      };
      documents.set(key, BSON.serialize({ ...existing, ...update.$set }, { ignoreUndefined: false }));
    },
    async deleteOne(filter) {
      return { deletedCount: matches(filter).filter((key) => documents.delete(key)).length };
    },
    async deleteMany(filter) {
      return { deletedCount: matches(filter).filter((key) => documents.delete(key)).length };
    },
  };
  collection.s = { db: { collection: () => collection } };
  const adapter = Object.create(KeyvMongo.prototype);
  adapter.opts = { url: 'mongodb://fake', collection: 'kitsu_collection' };
  adapter.connect = Promise.resolve({ store: collection, mongoClient: null });
  return { adapter, documents };
}

function nodeCache(documents) {
  return createCache({ stores: [persistentStore(fakeKeyvMongo(documents).adapter)], ttl: DAY });
}

function kitsuMeta() {
  return {
    meta: {
      id: 'kitsu:1',
      kitsu_id: '1',
      type: 'series',
      animeType: 'TV',
      name: 'Cowboy Bebop',
      aliases: ['Cowboy Bebop', 'カウボーイビバップ'],
      genres: ['Action', 'Space'],
      poster: undefined,
      description: 'Bounty hunters in space.',
      releaseInfo: '1998-1999',
      imdbRating: '8.7',
      userCount: 150000,
      averageRating: 82.25,
      popularity: 3.0,
      favorites: 5000000000,
      fraction: 0.30000000000000004,
      trailers: [{ source: 'qig4KOK2R2g', type: 'Trailer' }],
      grid: [[1, 2], [3.5, [null, 'x']], []],
      videos: [
        {
          id: 'kitsu:1:1',
          title: 'Asteroid Blues',
          released: new Date(Date.UTC(1998, 9, 24, 0, 0, 0, 0)),
          season: 1,
          episode: 1,
          thumbnail: undefined,
          overview: null,
          imdb_id: 'tt0213338',
          imdbSeason: 1,
          imdbEpisode: 1,
        },
        {
          id: 'kitsu:1:2',
          title: 'Stray Dog Strut',
          released: new Date(Date.UTC(1998, 9, 31, 13, 5, 9, 123)),
          season: 1,
          episode: 2,
        },
        {
          id: 'kitsu:1:3',
          title: 'Before the epoch',
          released: new Date(Date.UTC(1963, 0, 1, 23, 59, 59, 999)),
          season: 1,
          episode: 3,
        },
      ],
      links: [{ name: '8.7', category: 'imdb', url: 'https://imdb.com/title/tt0213338' }],
      imdb_id: 'tt0213338',
    },
    cacheMaxAge: 43200,
  };
}

function imdbMeta() {
  return {
    meta: {
      id: 'tt0213338',
      type: 'series',
      name: 'Cowboy Bebop',
      released: '1998-04-03T00:00:00.000Z',
      imdb_id: 'tt0213338',
      kitsu_id: [1, 2],
      videos: [
        { id: 'tt0213338:1:1', season: 1, episode: 1, released: '1998-10-24T00:00:00.000Z', kitsu_id: 1, kitsuEpisode: 1 },
        { id: 'tt0213338:1:2', season: 1, episode: 2, released: '1998-10-31T00:00:00.000Z', absoluteEpisode: undefined },
      ],
    },
    cacheMaxAge: 43200,
  };
}

function catalog() {
  return {
    metas: [
      { id: 'kitsu:1', type: 'series', name: 'Cowboy Bebop', poster: 'https://media.kitsu.app/1.jpg', genres: [] },
      { id: 'kitsu:2', type: 'movie', name: 'Tengoku no Tobira', imdbRating: undefined, trailers: [] },
    ],
    cacheMaxAge: 43200,
  };
}

const CASES = [
  { key: 'stremio-kitsu|meta:1', ttl: DAY, value: kitsuMeta },
  { key: 'stremio-kitsu|meta:tt0213338', ttl: DAY, value: imdbMeta },
  { key: 'stremio-kitsu|catalog:kitsu-anime-list|All|0', ttl: DAY, value: catalog },
  { key: 'stremio-kitsu|images:1', ttl: 14 * DAY, value: () => ({ logo: 'https://x/logo.png', logoLang: 'en', background: undefined }) },
  { key: 'stremio-kitsu|id_mapping:mal:1', ttl: 14 * DAY, value: () => 1 },
  { key: 'stremio-kitsu|meta:2', ttl: 1, value: () => ({ meta: { id: 'kitsu:2' }, cacheMaxAge: 43200 }), expired: true },
];

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function record() {
  const documents = new Map();
  const cache = nodeCache(documents);
  for (const { key, ttl, value } of CASES) {
    await cache.wrap(key, async () => value(), ttl);
  }
  await sleep(5);
  const recordedAt = Date.now();
  const reader = nodeCache(new Map(documents));
  const cases = [];
  for (const { key, ttl, expired } of CASES) {
    const [served] = await reader.mget([key]);
    cases.push({
      key,
      ttl,
      expired: !!expired,
      bson: Buffer.from(documents.get(key)).toString('base64'),
      served: served === undefined ? null : JSON.stringify(served),
    });
  }
  return { recordedAt, cases };
}

if (require.main === module) {
  record()
      .then((fixture) => fs.writeFileSync(NODE_WRITTEN, `${JSON.stringify(fixture, null, 2)}\n`))
      .catch((error) => {
        console.error(error?.message || error);
        process.exit(1);
      });
}

module.exports = { record, fakeKeyvMongo, nodeCache, NODE_WRITTEN, RUST_WRITTEN };
