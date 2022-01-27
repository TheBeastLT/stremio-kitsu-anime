const FanartTvApi = require("fanart.tv-api");
const needle = require('needle');
const urlExists = require("url-exists");
const { cacheWrapImages } = require('./cache');
const fanart = new FanartTvApi({ apiKey: process.env.FANART_APIKEY });

async function getImages(imdbId, type) {
  if (!imdbId) {
    return {};
  }
  return cacheWrapImages(imdbId, () => retrieveImages(imdbId, type));
}

function retrieveImages(imdbId, type) {
  return retrieveFanartImages(imdbId, type)
      .catch(() => ({}))
      .then(images => retrieveMetahubImages(imdbId, images))
}

function retrieveFanartImages(imdbId, type) {
  if (type === 'movie') {
    return retrieveMovieImages(imdbId);
  }
  return retrieveSeriesImages(imdbId);
}

function retrieveSeriesImages(imdbId) {
  return getTvdbId(imdbId)
      .then(tvdbId => fanart.getShowImages(tvdbId))
      .then(response => response.thetvdb_id && response.thetvdb_id !== '0' ? response : {})
      .then(response => ({
          poster: firstUrl(response.tvposter),
          logo: firstUrl(response.hdtvlogo),
          background: firstUrl(response.showbackground)
        }));
}

function retrieveMovieImages(imdbId) {
  return fanart.getMovieImages(imdbId)
      .then(response => ({
        poster: firstUrl(response.movieposter),
        logo: firstUrl(response.hdmovielogo),
        background: firstUrl(response.moviebackground)
      }));
}

async function retrieveMetahubImages(imdbId, images) {
  if (!images.logo) {
    images.logo = await checkIfExists(`https://images.metahub.space/logo/medium/${imdbId}/img`)
  }
  if (!images.background) {
    images.background = await checkIfExists(`https://images.metahub.space/background/medium/${imdbId}/img`)
  }
  return images
}

function getTvdbId(imdbId) {
  return needle('get', `https://thetvdb.com/api/GetSeriesByRemoteID.php?imdbid=${imdbId}`)
      .then(response => response.body && response.body.children[0])
      .then(series => series && series.children[0])
      .then(tvdbId => tvdbId && tvdbId.value || Promise.reject("no id found"));
}

async function checkIfExists(imdbImage) {
  return new Promise((resolve) => {
    urlExists(imdbImage, (err, exists) => {
      if (exists) {
        resolve(imdbImage)
      } else {
        resolve(undefined);
      }
    })
  });
}

function firstUrl(array) {
  return (array || [])
      .filter(entry => !entry.lang || entry.lang === 'en')
      .map(entry => entry.url)[0];
}

module.exports = { getImages };