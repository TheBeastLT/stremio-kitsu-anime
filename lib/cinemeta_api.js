const needle = require('needle');

const CINEMETA_URL = 'https://v3-cinemeta.strem.io';
const DEFAULT_TIMEOUT = 60000; // 60s

function getCinemetaMetadata(imdbId, type) {
  return _getCinemetaMetadata(imdbId, type)
      .catch(() => _getCinemetaMetadata(imdbId, type === 'movie' ? 'series' : 'movie'))
      .catch(error => {
        console.warn(`failed cinemeta query ${imdbId} due: ${error.message}`);
        return undefined;
      });
}

function _getCinemetaMetadata(imdbId, type) {
  return needle('get', `${CINEMETA_URL}/meta/${type}/${imdbId}.json`, { open_timeout: DEFAULT_TIMEOUT })
      .then(response => {
        if (response.body && response.body.meta) {
          return response.body.meta;
        } else {
          throw new Error('No search results');
        }
      })
}

module.exports = { getCinemetaMetadata };