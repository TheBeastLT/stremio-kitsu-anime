const { addonBuilder } = require('stremio-addon-sdk');
const genres = require('./static/data/genres');
const { enrichKitsuMetadata, enrichImdbMetadata, hasImdbMapping } = require('./lib/metadataEnrich');
const { cacheWrapMeta, cacheWrapCatalog, getCachedMetas } = require('./lib/cache');
const { mapToKitsuId } = require('./lib/id_convert');
const kitsu = require('./lib/kitsu_api');
const cinemeta = require('./lib/cinemeta_api');
const opensubtitles = require('./lib/opensubtitles_api')

const CACHE_MAX_AGE = parseInt(process.env.CACHE_MAX_AGE) || 12 * 60 * 60; // 12 hours

const manifest = {
  id: 'community.anime.kitsu',
  version: '0.0.10',
  name: 'Anime Kitsu',
  description: 'Unofficial Kitsu.io anime catalog addon',
  logo: 'https://i.imgur.com/7N6XGoO.png',
  background: 'https://i.imgur.com/ym4n96o.png',
  resources: ['catalog', 'meta', 'subtitles'],
  types: ['anime', 'movie', 'series'],
  catalogs: [
    {
      id: 'kitsu-anime-trending',
      name: 'Kitsu Trending',
      type: 'anime'
    },
    {
      id: 'kitsu-anime-airing',
      name: 'Kitsu Top Airing',
      type: 'anime',
      extra: [{ name: 'genre', options: genres }, { name: 'skip' }],
      genres: genres
    },
    {
      id: 'kitsu-anime-popular',
      name: 'Kitsu Most Popular',
      type: 'anime',
      extra: [{ name: 'genre', options: genres }, { name: 'skip' }],
      genres: genres
    },
    {
      id: 'kitsu-anime-rating',
      name: 'Kitsu Highest Rated',
      type: 'anime',
      extra: [{ name: 'genre', options: genres }, { name: 'skip' }],
      genres: genres
    },
    {
      id: 'kitsu-anime-list',
      name: 'Kitsu',
      type: 'anime',
      extra: [
          { name: 'search', isRequired: true },
          { name: 'lastVideosIds', isRequired: false, optionsLimit: 20 },
          { name: 'skip' }
      ],
    },
    {
      // Stremio's new-episode notifications only use a catalog of the library item's type (series) that requires
      // nothing besides lastVideosIds.
      id: 'kitsu-anime-last-videos',
      name: 'Last videos',
      type: 'series',
      extra: [{ name: 'lastVideosIds', isRequired: true, optionsLimit: 20 }],
    },
  ],
  idPrefixes: ['kitsu', 'mal', 'anilist', 'anidb']
};
const builder = new addonBuilder(manifest);
const sortValue = {
  'kitsu-anime-list': 'createdAt',
  'kitsu-anime-rating': '-average_rating',
  'kitsu-anime-popular': '-user_count',
  'kitsu-anime-airing': '-average_rating',
};
const statusValue = {
  'kitsu-anime-airing': 'current',
}

builder.defineCatalogHandler((args) => {
  const skip = args.extra && args.extra.skip || 0;
  const id = `${args.id}|${args.extra && args.extra.genre || 'All'}|${skip}`;

  if (args.extra?.search) {
    if (args.extra.search.match(/(?:https?|stremio):\/\//)) {
      return Promise.reject(`Invalid search term: ${args.extra.search}`)
    }
    // no need to cache search results
    return kitsu.search(args.extra.search)
        .then((metas) => ({ metas: metas, cacheMaxAge: CACHE_MAX_AGE }));
  }
  if (args.extra?.lastVideosIds) {
    // Like Cinemeta's last-videos: the last videos of each library id, from the cached metas the meta handler serves
    // (read in one query), so only cache misses are built from Kitsu and Cinemeta.
    const ids = args.extra.lastVideosIds.split(',');
    return Promise.all(ids.map((id) => mapToKitsuId(id).catch(() => undefined)))
        .then((kitsuIds) => getCachedMetas(kitsuIds))
        .then((cached) => Promise.all(ids.map((id, index) => (cached[index] ? Promise.resolve(cached[index]) : getKitsuIdMetadata(id))
            .then(({ meta }) => ({
              id,
              type: meta.type,
              name: meta.name,
              videos: meta.videos?.slice(-20)
                  .map(({ id, title, released, season, episode }) => ({ id, title, released, season, episode })),
            }))
            .catch(() => undefined))))
        .then((metas) => ({ metasDetailed: metas.filter((meta) => meta), cacheMaxAge: CACHE_MAX_AGE }));
  }

  const options = {
    offset: skip,
    genre: args.extra?.genre,
    sort: sortValue[args.id],
    status: statusValue[args.id],
    trending: args.id === 'kitsu-anime-trending'
  };

  return cacheWrapCatalog(id, () => kitsu.animeEntries(options)
      .then((metas) => ({ metas: metas, cacheMaxAge: CACHE_MAX_AGE })));
});

builder.defineMetaHandler((args) => {
  if (args.id.match(/^(?:kitsu|mal|anilist|anidb):\d+$/)) {
    return getKitsuIdMetadata(args.id);
  }
  if (args.id.match(/^tt\d+$/)) {
    const id = args.id;

    if (!hasImdbMapping(id)) {
      return Promise.reject(`No imdb mapping for: ${id}`);
    }

    return getImdbIdMetadata(id);
  }
  return Promise.reject(`Invalid id: ${args.id}`);
});

builder.defineSubtitlesHandler((args) => {
  if (!args.id.match(/^(?:kitsu|mal|anilist|anidb):\d+(?::\d+)?$/)) {
    return Promise.reject(`Invalid id: ${args.id}`);
  }

  return getKitsuIdMetadata(args.id)
      .then((metaResponse) => metaResponse.meta)
      .then((metadata) => opensubtitles.getRedirectUrl(metadata, args))
      .then((url) => ({ redirect: url }))
      .catch(() => ({ subtitles: [] }));
});

async function getKitsuIdMetadata(id) {
  return mapToKitsuId(id)
      .then((kitsuId) => cacheWrapMeta(kitsuId, () => kitsu.animeData(kitsuId)
        .then((metadata) => enrichKitsuMetadata(metadata, cinemeta.getCinemetaMetadata))
        .then((meta) => ({ meta: meta, cacheMaxAge: CACHE_MAX_AGE }))));
}

async function getImdbIdMetadata(id) {
  return cacheWrapMeta(id, () => cinemeta.getCinemetaMetadata(id)
      .then((metadata) => enrichImdbMetadata(metadata, kitsu.animeData))
      .then((meta) => ({ meta: meta, cacheMaxAge: CACHE_MAX_AGE })));
}

module.exports = builder.getInterface();
