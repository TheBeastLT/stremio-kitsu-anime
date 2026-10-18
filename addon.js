const { addonBuilder } = require('stremio-addon-sdk');
const genres = require('./static/data/genres');
const { enrichKitsuMetadata, enrichImdbMetadata, hasImdbMapping } = require('./lib/metadataEnrich');
const { cacheWrapMeta, cacheWrapCatalog, getCachedMetas } = require('./lib/cache');
const { mapToKitsuId } = require('./lib/id_convert');
const { toCatalogMeta } = require('./lib/metadata');
const kitsu = require('./lib/kitsu_api');
const cinemeta = require('./lib/cinemeta_api');
const opensubtitles = require('./lib/opensubtitles_api')

const CACHE_MAX_AGE = parseInt(process.env.CACHE_MAX_AGE) || 12 * 60 * 60; // 12 hours
const LAST_VIDEOS_IDS_LIMIT = 20;

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
          { name: 'lastVideosIds', isRequired: false, optionsLimit: LAST_VIDEOS_IDS_LIMIT },
          { name: 'skip' }
      ],
    },
    {
      // Stremio's new-episode notifications only use a catalog of the library item's type (series) that requires
      // nothing besides lastVideosIds.
      id: 'kitsu-anime-last-videos',
      name: 'Last videos',
      type: 'series',
      extra: [{ name: 'lastVideosIds', isRequired: true, optionsLimit: LAST_VIDEOS_IDS_LIMIT }],
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
    // no need to cache search results; the found anime's cached metas are used, the rest come from Kitsu in one request
    const ids = kitsu.searchIds(args.extra.search).map((kitsuId) => `kitsu:${kitsuId}`);
    return getMetas(ids, (missing) => kitsu.animeCatalogMetas(missing.map((id) => id.replace('kitsu:', ''))))
        .then((metas) => ({ metas: metas.filter((meta) => meta).map(toCatalogMeta), cacheMaxAge: CACHE_MAX_AGE }));
  }
  if (args.extra?.lastVideosIds) {
    // Like Cinemeta's last-videos: the last videos of each library id
    const ids = args.extra.lastVideosIds.split(',').slice(0, LAST_VIDEOS_IDS_LIMIT);
    return getMetas(ids).then((metas) => ({
      metasDetailed: metas.map((meta, index) => meta && {
        ...toCatalogMeta(meta),
        id: ids[index],
        videos: meta.videos?.slice(-20),
      }).filter((meta) => meta),
      cacheMaxAge: CACHE_MAX_AGE,
    }));
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

// An id with no anime behind it (no imdb mapping, unknown to Kitsu): the router's 404, so a caller can tell it from a
// failure (500), the SDK's handler error for every other rejection.
const notFound = (reason) => Promise.reject({ noHandler: true, reason });
const isNotFound = (error) => error?.errors?.some((kitsuError) => `${kitsuError.status}` === '404')
    || /^No kitsu id found for:/.test(error);

builder.defineMetaHandler((args) => {
  if (args.id.match(/^(?:kitsu|mal|anilist|anidb):\d+$/)) {
    return getKitsuIdMetadata(args.id).catch((error) => isNotFound(error) ? notFound(error) : Promise.reject(error));
  }
  if (args.id.match(/^tt\d+$/)) {
    const id = args.id;

    if (!hasImdbMapping(id)) {
      return notFound(`No imdb mapping for: ${id}`);
    }

    return getImdbIdMetadata(id);
  }
  return notFound(`Invalid id: ${args.id}`);
});

builder.defineSubtitlesHandler((args) => {
  if (!args.id.match(/^(?:kitsu|mal|anilist|anidb):\d+(?::\d+)?$/)) {
    return notFound(`Invalid id: ${args.id}`);
  }

  return getKitsuIdMetadata(args.id)
      .then((metaResponse) => metaResponse.meta)
      .then((metadata) => opensubtitles.getRedirectUrl(metadata, args))
      .then((url) => ({ redirect: url }))
      .catch(() => ({ subtitles: [] }));
});

// Metas of library ids (kitsu:, mal:, ...) in their order: the meta handler's cached ones in one store query, the rest
// from fetchMissing(ids), by default each built like the meta handler does (undefined when that fails).
async function getMetas(ids, fetchMissing = (missing) => Promise.all(missing
    .map((id) => getKitsuIdMetadata(id).then(({ meta }) => meta).catch(() => undefined)))) {
  const kitsuIds = await Promise.all(ids.map((id) => mapToKitsuId(id).catch(() => undefined)));
  const cached = await getCachedMetas(kitsuIds);
  const missing = ids.filter((_, index) => !cached[index]);
  const fetched = missing.length ? await fetchMissing(missing) : [];
  let position = 0;
  return ids.map((_, index) => cached[index] ? cached[index].meta : fetched[position++]);
}

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
