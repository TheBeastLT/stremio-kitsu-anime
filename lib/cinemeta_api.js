const axios = require('axios');

const CINEMETA_URL = 'https://v3-cinemeta.strem.io';
const TMDB_URL = 'https://94c8cb9f702d-tmdb-addon.baby-beamup.club';
const IMDB_IDS_FOR_TMDB = {}
const DEFAULT_TIMEOUT = 60000; // 60s

function getCinemetaMetadata(imdbId) {
  return _getMetadata(imdbId, 'series')
      .catch((err) => _getMetadata(imdbId, 'movie'))
      .catch(error => {
        console.warn(`failed cinemeta query ${imdbId} due: ${error}`);
        return { id: imdbId, imdb_id: imdbId };
      });
}

// Uncached Cinemeta read for the mapping importer, which must see seasons/episodes IMDb added in the last weeks.
// Not for addon requests: it bypasses the CDN cache on every call.
function getFreshCinemetaMetadata(imdbId) {
  return _getFreshCinemetaMetadata(imdbId, 'series')
      .catch(() => _getFreshCinemetaMetadata(imdbId, 'movie'))
      .catch(error => {
        console.warn(`failed fresh cinemeta query ${imdbId} due: ${error}`);
        return { id: imdbId, imdb_id: imdbId };
      });
}

// v3-cinemeta stays the source of truth. Only for ids it doesn't serve itself it redirects (307) to its fallback,
// cinemeta-live, dropping the query string; that fallback caches for up to two weeks, so the cache-breaking query is
// carried over to the redirect target by hand.
async function _getFreshCinemetaMetadata(imdbId, type, retry = 3) {
  const cacheBreak = `?${Date.now()}`;
  try {
    const response = await axios.get(`${CINEMETA_URL}/meta/${type}/${imdbId}.json${cacheBreak}`, {
      timeout: DEFAULT_TIMEOUT,
      maxRedirects: 0,
      validateStatus: status => status === 200 || status === 307,
    });
    const meta = response.status === 307
        ? (await axios.get(`${response.headers.location}${cacheBreak}`, { timeout: DEFAULT_TIMEOUT })).data?.meta
        : response.data?.meta;
    if (!meta?.imdb_id) {
      throw new Error('No search results');
    }
    return meta;
  } catch (error) {
    if (retry > 0 && error.message !== 'No search results') {
      return _getFreshCinemetaMetadata(imdbId, type, retry - 1);
    }
    throw error;
  }
}

function _getMetadata(imdbId, type) {
  if (IMDB_IDS_FOR_TMDB[imdbId]) {
    return _getTmdbMetadata(imdbId, type).catch(error => {
      console.warn(`failed tmdb query ${imdbId} due: ${error.message}`);
      return _getCinemetaMetadata(imdbId, type);
    });
  }
  return _getCinemetaMetadata(imdbId, type);
}

function _getCinemetaMetadata(imdbId, type) {
  return _getAddonMetadata(CINEMETA_URL, imdbId, type)
      .then(meta => {
          if (meta.type === 'movie' && !meta.imdb_id) {
              return Promise.reject("Incorrect metadata");
          }
          return Promise.resolve(meta)
      });
}

function _getTmdbMetadata(imdbId, type) {
  const tmdbId = `tmdb:${IMDB_IDS_FOR_TMDB[imdbId]}`;
  return _getAddonMetadata(TMDB_URL, tmdbId, type)
      .then(meta => {
        meta.id = imdbId;
        return meta;
      })
}

function _getAddonMetadata(baseUrl, metaId, type, retry = 3, query = '') {
  return axios.get(`${baseUrl}/meta/${type}/${metaId}.json${query}`, { timeout: DEFAULT_TIMEOUT })
      .then(response => {
        if (response?.data?.meta?.imdb_id) {
          return response.data.meta;
        } else {
          throw new Error('No search results');
        }
      })
      .catch(error => {
          if (retry > 0) {
              return _getAddonMetadata(baseUrl, metaId, type, retry - 1, query);
          }
          throw error;
      })
}

module.exports = { getCinemetaMetadata, getFreshCinemetaMetadata };