const FanartTvApi = require("fanart.tv-api");
const needle = require('needle');
const urlExists = require("url-exists");
const { cacheWrapImages } = require('./cache');
const fanart = new FanartTvApi({ apiKey: process.env.FANART_APIKEY });

const OVERRIDES = {
  "13882": "tt8565186",
  "43061": "tt12317746",
  "44181": "tt13872472",
  "44521": "tt13872472",
}
const DEFAULT_LOGO_ID = {
  "tt0098492": "28145",
  "892": "127311",
  "tt0816407": "55556",
  "3579": "90919",
  "tt6153026": "138529",
  "45228": "138478",
  "tt0473578": "32761"
}

async function getImages(kitsuId, imdbId, type) {
  imdbId = OVERRIDES[kitsuId] || imdbId;
  if (!imdbId) {
    return {};
  }
  const cacheKey = DEFAULT_LOGO_ID[kitsuId] ? kitsuId : imdbId;
  return cacheWrapImages(cacheKey, () => retrieveImages(kitsuId, imdbId, type));
}

function retrieveImages(kitsuId, imdbId, type) {
  return retrieveFanartImages(kitsuId, imdbId, type)
      .catch(() => ({}))
      .then(images => retrieveMetahubImages(imdbId, images))
}

function retrieveFanartImages(kitsuId, imdbId, type) {
  if (type === 'movie') {
    return retrieveMovieImages(kitsuId, imdbId);
  }
  return retrieveSeriesImages(kitsuId, imdbId).catch(() => retrieveMovieImages(kitsuId, imdbId));
}

function retrieveSeriesImages(kitsuId, imdbId) {
  return getTvdbId(imdbId)
      .then(tvdbId => fanart.getShowImages(tvdbId))
      .then(response => response.thetvdb_id && response.thetvdb_id !== '0' ? response : {})
      .then(response => ({
          logo: logoUrl(kitsuId, imdbId, response.hdtvlogo, response.clearlogo),
          poster: firstUrl(response.tvposter),
          background: firstUrl(response.showbackground)
      }));
}

function retrieveMovieImages(kitsuId, imdbId) {
  return fanart.getMovieImages(imdbId)
      .then(response => ({
        logo: logoUrl(kitsuId, imdbId, response.hdmovielogo, response.movielogo),
        poster: firstUrl(response.movieposter),
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
      .then(response => response.body?.children?.map(series => series.children[0]?.value))
      .then(seriesIds => seriesIds?.sort((a, b) => a - b)?.[0])
      .then(tvdbId => tvdbId || Promise.reject("no id found"));
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

function logoUrl(kitsuId, imdbId, array1 = [], array2 = []) {
  const merged = [].concat(array1).concat(array2);
  const defaultLogoId = DEFAULT_LOGO_ID[kitsuId] || DEFAULT_LOGO_ID[imdbId];
  const defaultLogo = merged.find(e => e.id === defaultLogoId);
  return defaultLogo?.url || firstUrl(merged)
}

function firstUrl(array = []) {
  const entry = array.filter(e => !e.lang || e.lang === 'en')[0] || array[0];
  return entry?.url;
}

module.exports = { getImages };