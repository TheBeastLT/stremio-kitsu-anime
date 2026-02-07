const axios = require('axios');

const CINEMETA_URL = 'https://v3-cinemeta.strem.io';
const TMDB_URL = 'https://94c8cb9f702d-tmdb-addon.baby-beamup.club';
const IMDB_IDS_FOR_TMDB = {}
const DEFAULT_TIMEOUT = 60000; // 60s

function getCinemetaMetadata(imdbId) {
  return _getMetadata(imdbId, 'series')
      .catch((err) => _getMetadata(imdbId, 'movie'))
      .catch(error => {
        console.warn(`failed cinemeta query ${imdbId} due: ${error.message}`);
        return { id: imdbId, imdb_id: imdbId };
      });
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

function _getAddonMetadata(baseUrl, metaId, type) {
  return axios.get(`${baseUrl}/meta/${type}/${metaId}.json`, { timeout: DEFAULT_TIMEOUT })
      .then(response => {
        if (response?.data?.meta) {
          return response.data.meta;
        } else {
          throw new Error('No search results');
        }
      })
}

module.exports = { getCinemetaMetadata };