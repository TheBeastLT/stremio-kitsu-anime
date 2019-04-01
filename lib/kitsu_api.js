const needle = require('needle');
const { toStremioMeta } = require('./metadata');

const KITSU_URL = 'https://kitsu.io';
const MAX_SIZE = 20;

async function animeEntries(options = {}) {
  if (options.trending) {
    return trendingEntries(options.limit);
  }

  const query = {};
  if (options.offset) {
    query['page[offset]'] = options.offset;
  }
  if (options.genre) {
    query['filter[genres]'] = options.genre;
  }
  if (options.sort) {
    query['sort'] = options.sort;
  }
  return _getCatalogEntries('/api/edge/trending/anime', query);
}

async function trendingEntries(limit = 50) {
  const query = { 'limit': limit };
  return _getCatalogEntries('/api/edge/trending/anime', query);
}

async function search(searchTerm) {
  const query = { 'filter[text]': searchTerm };
  return _getCatalogEntries('/api/edge/anime', query);
}

async function animeData(kitsuId) {
  const query = { 'include': 'genres,episodes' };
  return _getContent(`/api/edge/anime/${kitsuId}`, query)
      .then((response) => toStremioMeta(response.data, response.included))
}

async function _getCatalogEntries(endpoint, queryParams = {}) {
  queryParams['include'] = 'genres';
  queryParams['page[limit]'] = MAX_SIZE;
  queryParams['filter[subtype]'] = 'TV,OVA,ONA,movie,special';

  return _getContent(url, queryParams)
      .then((response) => response.data.map((result) => toStremioMeta(result, response.included)))
      .then((metas) => metas
          .map((meta) => ({
            id: meta.id,
            type: meta.type,
            name: meta.name,
            description: meta.description,
            releaseInfo: meta.releaseInfo,
            imdbRating: meta.imdbRating,
            genres: meta.genres,
            poster: meta.poster,
          })));
}

async function _getContent(endpoint, queryParams) {
  return needle('get', `${KITSU_URL}${endpoint}`, queryParams, { headers: { accept: 'application/vnd.api+json'} })
  .then((response) => {
    if (response.statusCode === 200 && response.body) {
      return JSON.parse(response.body);
    }
    throw new Error('No response from kitsu');
  });
}

module.exports = { MAX_SIZE, search, animeData, animeEntries, trendingEntries };