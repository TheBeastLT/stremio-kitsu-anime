const { addonBuilder } = require('stremio-addon-sdk');
const genres = require('./static/data/genres');
const { enrichKitsuMetadata, enrichImdbMetadata, hasImdbMapping } = require('./lib/metadataEnrich');
const { cacheWrapMeta, cacheWrapCatalog } = require('./lib/cache');
const kitsu = require('./lib/kitsu_api');
const cinemeta = require('./lib/cinemeta_api');

const CACHE_MAX_AGE = process.env.CACHE_MAX_AGE || 12 * 60 * 60; // 12 hours

const manifest = {
  id: 'community.anime.kitsu',
  version: '0.0.3',
  name: 'Anime Kitsu',
  description: 'Unofficial Kitsu.io anime catalog addon',
  logo: 'https://i.imgur.com/ANMG9VF.png',
  background: 'https://i.imgur.com/ym4n96o.png',
  resources: ['catalog', 'meta'],
  types: ['movie', 'series'],
  catalogs: [
    {
      id: 'kitsu-anime-trending',
      name: 'Kitsu Trending',
      type: 'series'
    },
    {
      id: 'kitsu-anime-popular',
      name: 'Kitsu Most Popular',
      type: 'series',
      extra: [{ name: 'genre', options: genres }, { name: 'skip' }],
      genres: Object.values(genres)
    },
    {
      id: 'kitsu-anime-rating',
      name: 'Kitsu Highest Rated',
      type: 'series',
      extra: [{ name: 'genre', options: genres }, { name: 'skip' }],
      genres: Object.values(genres)
    },
    {
      id: 'kitsu-anime-newest',
      name: 'Kitsu Newest',
      type: 'series',
      extra: [{ name: 'genre', options: genres }, { name: 'skip' }],
      genres: Object.values(genres)
    },
    {
      id: 'kitsu-anime-list',
      name: 'Kitsu',
      type: 'series',
      extra: [{ name: 'genre', options: genres }, { name: 'skip' }, { name: 'search' }],
      genres: Object.values(genres)
    },
  ],
  idPrefixes: ['kitsu']
};
const builder = new addonBuilder(manifest);
const sortValue = {
  'kitsu-anime-list': 'createdAt',
  'kitsu-anime-newest': '-createdAt',
  'kitsu-anime-rating': '-average_rating',
  'kitsu-anime-popular': '-user_count'
};

builder.defineCatalogHandler((args) => {
  const skip = args.extra && args.extra.skip || 0;
  const id = `${args.id}|${args.extra && args.extra.genre || 'All'}|${skip}`;

  if (args.extra && args.extra.search) {
    // no need to cache search results
    return kitsu.search(args.extra.search)
        .then((metas) => ({ metas: metas, cacheMaxAge: CACHE_MAX_AGE }));
  }

  const options = {
    offset: skip,
    genre: args.extra && args.extra.genre,
    sort: sortValue[args.id],
    trending: args.id === 'kitsu-anime-trending'
  };

  return cacheWrapCatalog(id, () => kitsu.animeEntries(options)
      .then((metas) => ({ metas: metas, cacheMaxAge: CACHE_MAX_AGE })));
});

builder.defineMetaHandler((args) => {
  if (args.id.match(/^kitsu:\d+$/)) {
    const id = parseInt(args.id.replace('kitsu:', ''));

    return cacheWrapMeta(id, () => kitsu.animeData(id)
        .then((metadata) => enrichKitsuMetadata(metadata, cinemeta.getCinemetaMetadata))
        .then((meta) => ({ meta: meta, cacheMaxAge: CACHE_MAX_AGE })));
  }
  if (args.id.match(/^tt\d+$/)) {
    const id = args.id;

    if (!hasImdbMapping(id)) {
      return Promise.reject(`No imdb mapping for: ${id}`);
    }

    return cacheWrapMeta(id, () => cinemeta.getCinemetaMetadata(id, args.type)
        .then((metadata) => enrichImdbMetadata(metadata, kitsu.animeData))
        .then((meta) => ({ meta: meta, cacheMaxAge: CACHE_MAX_AGE })));
  }

  return Promise.reject(new Error('invalid id'));
});

module.exports = builder.getInterface();