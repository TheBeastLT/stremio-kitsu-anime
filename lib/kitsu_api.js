const needle = require('needle');
const distance = require('jaro-winkler');
const { Parser, addDefaults } = require('parse-torrent-title');
const { value, integer } = require('parse-torrent-title/src/transformers');
const { toStremioEntryMeta, toStremioCatalogMeta } = require('./metadata');

const KITSU_URL = 'https://kitsu.io';
const MAX_SIZE = 20;
const DEFAULT_PAGE_SIZE = 100;
const parser = initSeasonParser();

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
  const parsedQuery = parser.parse(searchTerm);
  const parsedTitle = escapeTitle(parsedQuery.title);
  const parsedSeason = parsedQuery.season;
  const isOvaOrSpecial = parsedTitle.match(/special|ova/i);
  const isEntryOvaOrSpecial = entry => ['special', "OVA"].includes(entry.animeType);
  const isTvShow = (a, b) => a.toLowerCase().includes(b.toLowerCase()) && a.match(/\(?TV\)?$/i);
  const compareSeason = (entry) => {
    if (parsedSeason === undefined) return entry.sort.season || 0;
    if (entry.type !== 'series') return 0;
    const entrySeason = entry.sort.season === undefined ? 1 : entry.sort.season;
    return entrySeason === parsedSeason || entry.sort.title.endsWith(`${parsedSeason}`) ? -1 : 0
  }
  const compareType = (aEntry, bEntry) => {
    if (isOvaOrSpecial) return 0;
    if (!isOvaOrSpecial && aEntry.animeType === 'special' && aEntry.animeType !== bEntry.animeType) return 1
    if (!isOvaOrSpecial && bEntry.animeType === 'special' && aEntry.animeType !== bEntry.animeType) return -1
    if (aEntry.sort.distance === bEntry.sort.distance && isEntryOvaOrSpecial(aEntry)) return 1
    if (aEntry.sort.distance === bEntry.sort.distance && isEntryOvaOrSpecial(bEntry)) return -1
    return isTvShow(aEntry.sort.title, bEntry.sort.title) ? -1 : 0;
  }

  return _getCatalogEntries('/api/edge/anime', query)
      .then((entries) => entries.map(entry => {
        const parsedTitles = [entry.name].concat(entry.aliases).map(title => parser.parse(title));
        const distances = parsedTitles.map(parsed => distance(parsedTitle, escapeTitle(parsed.title)));
        const maxDistance = Math.max(...distances);
        const maxTitle = parsedTitles[distances.indexOf(maxDistance)].title;
        const maxSeason = parsedTitles.map(v => v.season).find(v => v) ||
            (entry.description && parser.parse(entry.description).season);
        return { ...entry, sort: { distance: maxDistance, title: maxTitle, season: maxSeason } };
      }))
      .then((entries) => entries.sort((a, b) => {
        const aSeasonSort = compareSeason(a);
        const bSeasonSort = compareSeason(b);
        const aTypeSort = compareType(a, b);
        const bTypeSort = compareType(b, a);

        const distanceSort = b.sort.distance - a.sort.distance;
        const seasonSort = aSeasonSort - bSeasonSort; // give higher order if seasons match
        const typeSort = aTypeSort - bTypeSort; // give higher order for TV type if ova or specials are not in the query

        if (parsedSeason === undefined) {
          return distanceSort < 0.2
              ? typeSort || distanceSort || seasonSort
              : distanceSort || typeSort || seasonSort;
        } else if (distanceSort < 0.2) {
          return seasonSort || typeSort || distanceSort;
        }
        return distanceSort || seasonSort || typeSort;
      }))
      .then((entries) => entries.map((entry) => {
        delete entry.sort;
        return entry;
      }));
}

async function animeData(kitsuId) {
  const query = { 'include': 'genres,episodes' };
  return _getContent(`/api/edge/anime/${kitsuId}`, query)
      .then((response) => toStremioEntryMeta(response.data, response.included))
}

async function _getCatalogEntries(endpoint, queryParams = {}, pageSize = MAX_SIZE) {
  const nextOffset = (queryParams['page[offset]'] || 0) + MAX_SIZE;
  const nextQueryParams = { ...queryParams, 'page[offset]': nextOffset };
  queryParams['include'] = 'genres';
  queryParams['page[limit]'] = MAX_SIZE;
  queryParams['filter[subtype]'] = 'TV,OVA,ONA,movie,special';
  queryParams['filter[status]'] = 'finished,current,upcoming'; // dont show unreleased and tba
  return _getContent(endpoint, queryParams)
      .then((response) => response.data.map((result) => toStremioCatalogMeta(result, response.included)))
      .then((metas) => nextOffset < pageSize && metas.length >= MAX_SIZE
          ? _getCatalogEntries(endpoint, nextQueryParams, pageSize).then((nextMetas) => metas.concat(nextMetas))
          : metas);
}

async function _getContent(endpoint, queryParams) {
  return needle('get', `${KITSU_URL}${endpoint}`, queryParams, { headers: { accept: 'application/vnd.api+json' } })
      .then((response) => {
        if (response.statusCode === 200 && response.body) {
          return JSON.parse(response.body);
        }
        throw new Error(`No response from kitsu: ${response.body}`);
      });
}

function initSeasonParser() {
  const parser = new Parser();
  addDefaults(parser);
  parser.handlers = parser.handlers.filter(handler => ['seasons', 'season'].includes(handler.handlerName));
  parser.addHandler('season', /(?:\W)(\d{1,2})[. ]?(?:st|nd|rd|th)$/i, integer);
  parser.addHandler('season', /second[. ]?(?:season|$)/i, value(2));
  parser.addHandler('season', /third[. ]?(?:season|$)/i, value(3));
  parser.addHandler('season', /fourth[. ]?(?:season|$)/i, value(4));
  parser.addHandler('season', /fifth[. ]?(?:season|$)/i, value(5));
  parser.addHandler('season', /sixth[. ]?(?:season|$)/i, value(6));
  parser.addHandler('season', /seventh[. ]?(?:season|$)/i, value(7));
  parser.addHandler('season', /eigth[. ]?(?:season|$)/i, value(8));
  parser.addHandler('season', /ninth[. ]?(?:season|$)/i, value(9));
  parser.addHandler('season', /tenth[. ]?(?:season|$)/i, value(10));
  return parser;
}

function escapeTitle(value) {
  return value.toLowerCase()
      .replace(/[;, ~\-]+/g, ' ') // replace dots, commas or underscores with spaces
      .replace(/[^\w ()+#@%!']+/g, '') // remove all non-alphanumeric chars
      .trim();
}

module.exports = { MAX_SIZE, search, animeData, animeEntries, trendingEntries };