const Kitsu = require('kitsu')
const { toStremioEntryMeta, toStremioCatalogMeta } = require('./metadata');
const { searchTitles } = require('./title_index');

const PAGE_SIZE = 20;
// The subtypes the addon serves (not music videos).
const ANIME_SUBTYPES = 'TV,OVA,ONA,movie,special';
const TITLE_FIELDS = 'canonicalTitle,titles,abbreviatedTitles,subtype,status,startDate,episodeCount';
const kitsuApi = new Kitsu({ pluralize: false })

async function animeEntries(options = {}) {
  if (options.trending) {
    return trendingEntries(options.limit);
  }

  const params = { filter: {}, page: {} };
  if (options.offset) {
    params.page.offset = options.offset;
  }
  if (options.genre) {
    params.filter.genres = options.genre;
  }
  if (options.status) {
    params.filter.status = options.status;
  }
  if (options.sort) {
    params.sort = options.sort;
  }
  return _getCatalogEntries('anime', params);
}

async function trendingEntries(limit = 50) {
  return _getCatalogEntries('trending/anime', { limit });
}

function searchIds(searchTerm) {
  return searchTitles(searchTerm, PAGE_SIZE).map(result => `${result.id}`);
}

async function search(searchTerm) {
  return animeCatalogMetas(searchIds(searchTerm)).then((metas) => metas.filter(meta => meta));
}

async function animeCatalogMetas(kitsuIds) {
  if (!kitsuIds.length) {
    return [];
  }
  const params = { filter: { id: kitsuIds.join(',') }, include: 'genres', page: { limit: PAGE_SIZE } };
  return kitsuApi.get('anime', { params })
      .then((response) => Promise.all(response.data.map((result) => toStremioCatalogMeta(result))))
      .then((metas) => {
        const byId = new Map(metas.map(meta => [`${meta.kitsu_id}`, meta]));
        return kitsuIds.map(id => byId.get(`${id}`));
      })
      .catch((error) => Promise.reject(error.response?.data || error.message || error))
}

async function animeTitlePage({ offset = 0, sort = 'id' } = {}) {
  const params = { fields: { anime: TITLE_FIELDS }, sort, page: { limit: PAGE_SIZE, offset } };
  return kitsuApi.get('anime', { params })
      .then((response) => ({ entries: response.data, count: response.meta?.count }))
      .catch((error) => Promise.reject(error.response?.data || error.message || error))
}

// options.images: false skips the fanart/metahub image lookups (not needed by the mapping importer).
async function animeData(kitsuId, options = {}) {
  const params = { include: 'genres,episodes,mediaRelationships.destination' };
  return kitsuApi.get(`anime/${kitsuId}`, { params })
      .then((response) => toStremioEntryMeta(response.data, options))
      .catch((error) => Promise.reject(error.response?.data || error.message || error))
}

// Kitsu's own links to other sites ([{ externalSite: 'anilist/anime', externalId: '199068' }, ...]).
async function animeMappings(kitsuId) {
  return kitsuApi.get(`anime/${kitsuId}/mappings`)
      .then(response => response.data || [])
      .catch((error) => Promise.reject(error.response?.data || error.message || error))
}

// The kitsu anime id Kitsu itself links to an external id (site like 'myanimelist/anime', 'anilist/anime', 'anidb').
async function kitsuIdByMapping(externalSite, externalId) {
  const params = { filter: { externalSite, externalId: String(externalId) }, include: 'item' };
  return kitsuApi.get('mappings', { params })
      .then(response => (response.data || []).map(mapping => mapping.item?.data || mapping.item)
          .find(item => item?.type === 'anime')?.id)
      .catch((error) => Promise.reject(error.response?.data || error.message || error))
}

// Kitsu's own episode counts keyed by kitsu id, without fetching the (paginated) episode lists.
async function animeEpisodeCounts(kitsuIds) {
  const chunks = [];
  for (let i = 0; i < kitsuIds.length; i += PAGE_SIZE) {
    chunks.push(kitsuIds.slice(i, i + PAGE_SIZE));
  }
  const params = ids => ({ filter: { id: ids.join(',') }, fields: { anime: 'episodeCount' }, page: { limit: PAGE_SIZE } });
  return Promise.all(chunks.map(ids => kitsuApi.get('anime', { params: params(ids) })))
      .then(responses => responses.flatMap(response => response.data))
      .then(entries => Object.fromEntries(entries.map(entry => [entry.id, entry.episodeCount])))
      .catch((error) => Promise.reject(error.response?.data || error.message || error))
}

// { startDate, title } keyed by kitsu id of every anime matching a Kitsu filter, page by page, like
// { season: 'fall', seasonYear: 2026 } (Kitsu's fall is Sep-Nov) or { id: '1,2,3' }.
async function animeStartEntries(filter) {
  const entries = await pageAnime({ filter, fields: { anime: 'startDate,canonicalTitle' } },
      (response, entries) => Object.keys(entries).length >= response.meta?.count);
  return startEntries(Object.values(entries));
}

// animeStartEntries (with each `status`) of the anime Kitsu created since `since` (a Date), paging by creation date
// until a page reaches older ones.
async function recentAnimeEntries(since) {
  const isRecent = entry => new Date(entry.createdAt) >= since;
  const entries = await pageAnime({
    filter: { subtype: ANIME_SUBTYPES }, fields: { anime: 'createdAt,startDate,canonicalTitle,status' }, sort: '-createdAt',
  }, response => !response.data.every(isRecent));
  return startEntries(Object.values(entries).filter(isRecent));
}

// Every page of an anime listing, keyed by kitsu id, until `done(response, entries)` or a page runs short. Kitsu's
// offset paging is unreliable (the first page repeats, `next` links go missing a page early), hence the keying and
// the own stop condition.
async function pageAnime(params, done) {
  const entries = {};
  for (let offset = 0; ; offset += PAGE_SIZE) {
    const response = await kitsuApi.get('anime', { params: { ...params, page: { limit: PAGE_SIZE, offset } } })
        .catch((error) => Promise.reject(error.response?.data || error.message || error));
    response.data.forEach(entry => entries[entry.id] = entry);
    if (response.data.length < PAGE_SIZE || done(response, entries)) {
      return entries;
    }
  }
}

const startEntries = entries => Object.fromEntries(entries
    .map(entry => [entry.id, { startDate: entry.startDate, title: entry.canonicalTitle, status: entry.status }]));

async function _getCatalogEntries(endpoint, params = {}) {
  params.include = 'genres';
  params.filter = params.filter || {}
  params.filter.subtype = ANIME_SUBTYPES;
  params.filter.status = params.filter.status || 'finished,current,upcoming';
  params.page = params.page || {}
  params.page.limit = PAGE_SIZE;
  return kitsuApi.get(endpoint, { params })
      .then((response) => Promise.all(response.data.map((result) => toStremioCatalogMeta(result))));
}

module.exports = {
  PAGE_SIZE, search, searchIds, animeCatalogMetas, animeTitlePage, animeData, animeEpisodeCounts, animeStartEntries,
  animeMappings, kitsuIdByMapping, animeEntries, recentAnimeEntries,
};
