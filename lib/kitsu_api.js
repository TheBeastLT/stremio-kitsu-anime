const Kitsu = require('kitsu')
const distance = require('jaro-winkler');
const { Parser, addDefaults } = require('parse-torrent-title');
const { value, integer, boolean } = require('parse-torrent-title/src/transformers');
const { toStremioEntryMeta, toStremioCatalogMeta } = require('./metadata');

const PAGE_SIZE = 20;
const IDS_MAX_SIZE = 40;
const kitsuApi = new Kitsu({ pluralize: false })
const parser = initSeasonParser();

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

async function search(searchTerm) {
  const query = { filter: { text: searchTerm } };
  const parsedQuery = parser.parse(searchTerm);
  const parsedTitle = escapeTitle(parsedQuery.title);
  const parsedSeason = parsedQuery.season;
  const isOvaOrSpecial = parsedTitle.match(/special|ova|(?:\b|\d)sp\b/i);
  const isEntryOvaOrSpecial = entry => ['special', "OVA"].includes(entry.animeType);
  const isTvShow = (a, b) => a.sort.title.toLowerCase().includes(b.sort.title.toLowerCase()) && a.sort.tv;
  const compareSeason = (entry) => {
    if (parsedSeason === undefined) return parsedTitle.endsWith(`${entry.sort.season}`) ? -1 : entry.sort.season || 0;
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
    return isTvShow(aEntry, bEntry) ? -1 : 0;
  }

  return _getCatalogEntries('anime', query)
      .then((entries) => entries.map(entry => {
        const parsedTitles = [entry.name].concat(entry.aliases).map(title => parser.parse(title));
        const distances = parsedTitles.map(parsed => distance(parsedTitle, escapeTitle(parsed.title)));
        const maxDistance = Math.max(...distances);
        const maxTitle = parsedTitles[distances.indexOf(maxDistance)].title;
        const maxTv = parsedTitles[distances.indexOf(maxDistance)].tv;
        const maxSeason = parsedTitles.map(v => v.season).find(v => v) ||
            (entry.description && parser.parse(entry.description).season);
        return { ...entry, sort: { distance: maxDistance, title: maxTitle, tv: maxTv, season: maxSeason } };
      }))
      .then((entries) => entries.sort((a, b) => {
        const aSeasonSort = compareSeason(a);
        const bSeasonSort = compareSeason(b);
        const aTypeSort = compareType(a, b);
        const bTypeSort = compareType(b, a);

        const distanceSort = b.sort.distance - a.sort.distance;
        const seasonSort = aSeasonSort - bSeasonSort; // give higher order if seasons match
        const typeSort = aTypeSort - bTypeSort; // give higher order for TV type if ova or specials are not in the query
        const hasExactMatch = (a.sort.distance === 1 || b.sort.distance === 1) && distanceSort !== 0;

        if (parsedSeason === undefined) {
          return distanceSort < 0.2 && !hasExactMatch
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

async function list(kitsuIdsRaw) {
  const kitsuIds = kitsuIdsRaw.split(',').slice(0, IDS_MAX_SIZE);
  const params = {
    include: 'genres',
    filter: {
      id: kitsuIds.join(',')
    },
    page: {
      limit: PAGE_SIZE
    }
  }
  return _list(params)
      .then(responses => responses.reduce((a, b) => a.concat(b.data),[]))
      .then(metas => Promise.all(metas
          .sort((a, b) => kitsuIds.indexOf(a.id) - kitsuIds.indexOf(b.id))
          .map(result => toStremioCatalogMeta(result))));
}

async function _list(params, offset = 0) {
  return kitsuApi.get('anime', { params })
      .then(response => {
        if (response.links.next) {
          const nextOffset = offset + response.data.length;
          return _list({ ...params, page: { ...params.page, offset: nextOffset } }, nextOffset)
              .then(response2 => [response].concat(response2))
        }
        return [response];
      })
}

async function animeData(kitsuId) {
  const params = { include: 'genres,episodes,mediaRelationships.destination' };
  return kitsuApi.get(`anime/${kitsuId}`, { params })
      .then((response) => toStremioEntryMeta(response.data))
      .catch((error) => Promise.reject(error.response?.data || error.message || error))
}

async function _getCatalogEntries(endpoint, params = {}) {
  params.include = 'genres';
  params.filter = params.filter || {}
  params.filter.subtype = 'TV,OVA,ONA,movie,special';
  params.filter.status = params.filter.status || 'finished,current,upcoming';
  params.page = params.page || {}
  params.page.limit = PAGE_SIZE;
  return kitsuApi.get(endpoint, { params })
      .then((response) => Promise.all(response.data.map((result) => toStremioCatalogMeta(result))));
}

function initSeasonParser() {
  const parser = new Parser();
  addDefaults(parser);
  parser.handlers = parser.handlers.filter(handler => ['seasons', 'season'].includes(handler.handlerName));
  parser.addHandler('season', /\W(\d{1,2})[. ]?(?:st|nd|rd|th)$/i, integer);
  parser.addHandler('season', /second[. ]?(?:season|$)/i, value(2));
  parser.addHandler('season', /third[. ]?(?:season|$)/i, value(3));
  parser.addHandler('season', /fourth[. ]?(?:season|$)/i, value(4));
  parser.addHandler('season', /fifth[. ]?(?:season|$)/i, value(5));
  parser.addHandler('season', /sixth[. ]?(?:season|$)/i, value(6));
  parser.addHandler('season', /seventh[. ]?(?:season|$)/i, value(7));
  parser.addHandler('season', /eigth[. ]?(?:season|$)/i, value(8));
  parser.addHandler('season', /ninth[. ]?(?:season|$)/i, value(9));
  parser.addHandler('season', /tenth[. ]?(?:season|$)/i, value(10));
  parser.addHandler('part', /part[. ]?(\d)$/i, integer);
  parser.addHandler('tv', /\(?TV\)?$/i, boolean);
  return parser;
}

function escapeTitle(value) {
  return value.toLowerCase()
      .replace(/[;, ~\-]+/g, ' ') // replace dots, commas or underscores with spaces
      .replace(/[^\w ()+#@%!']+/g, '') // remove all non-alphanumeric chars
      .trim();
}

module.exports = { PAGE_SIZE, search, list, animeData, animeEntries };