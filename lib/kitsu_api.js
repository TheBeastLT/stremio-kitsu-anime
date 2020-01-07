const needle = require('needle');
const { toStremioEntryMeta, toStremioCatalogMeta } = require('./metadata');

const KITSU_URL = 'https://kitsu.io';
const MAX_SIZE = 20;
const DEFAULT_PAGE_SIZE = 100;

async function animeEntries(options = {}) {
  if (options.trending) {
    return trendingEntries(options.limit);
  }

  const pageSize = options.pageSize || DEFAULT_PAGE_SIZE;
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
  return _getCatalogEntries('/api/edge/anime', query, pageSize);
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
      .then((response) => toStremioEntryMeta(response.data, response.included))
}

async function _getCatalogEntries(endpoint, queryParams = {}, pageSize = MAX_SIZE) {
  const nextOffset = (queryParams['page[offset]'] || 0) + MAX_SIZE;
  const nextQueryParams = { ...queryParams, 'page[offset]': nextOffset};
  queryParams['include'] = 'genres';
  queryParams['page[limit]'] = MAX_SIZE;
  queryParams['filter[subtype]'] = 'TV,OVA,ONA,movie,special';
  return _getContent(endpoint, queryParams)
      .then((response) => response.data.map((result) => toStremioCatalogMeta(result, response.included)))
      .then((metas) => nextOffset < pageSize || metas.length < MAX_SIZE
          ? _getCatalogEntries(endpoint, nextQueryParams, pageSize).then((nextMetas) => metas.concat(nextMetas))
          : metas);
}

async function _getContent(endpoint, queryParams) {
  return needle('get', `${KITSU_URL}${endpoint}`, queryParams, { headers: { accept: 'application/vnd.api+json'} })
      .then((response) => {
        if (response.statusCode === 200 && response.body) {
          return JSON.parse(response.body);
        }
        throw new Error(`No response from kitsu: ${response.body}`);
      });
}

module.exports = { MAX_SIZE, search, animeData, animeEntries, trendingEntries };