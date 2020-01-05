const urlExists = require('url-exists');
const imdbMappping = require('../static/data/imdb_mapping');

function toStremioCatalogMeta(animeData, includes) {
  const meta = _toStremioMeta(animeData, includes);
  return {
    id: meta.id,
    type: meta.type,
    name: meta.name,
    description: meta.description,
    releaseInfo: meta.releaseInfo,
    imdbRating: meta.imdbRating,
    genres: meta.genres,
    poster: meta.poster,
  }
}

async function toStremioEntryMeta(animeData, includes) {
  const meta = _toStremioMeta(animeData, includes);
  return {
    ...meta,
    background: await getBackgroundImage(meta.imdb_id, meta.background),
    logo: await getLogoImage(meta.imdb_id),
  };
}

function _toStremioMeta(animeData, includes) {
  const type = getType(animeData);

  let releaseInfo = `${animeData.attributes.startDate && animeData.attributes.startDate.match(/^\d+/)[0]}`;
  if (animeData.attributes.endDate && !animeData.attributes.endDate.startsWith(releaseInfo)) {
    releaseInfo = releaseInfo.concat(`-${animeData.attributes.endDate.match(/^\d+/)[0]}`);
  } else if (animeData.attributes.status === 'current') {
    releaseInfo = releaseInfo.concat('-');
  }

  let genres = [];
  if (animeData.relationships.genres.data && includes) {
    genres = animeData.relationships.genres.data
        .map((genre) => includes.find((include) => include.type === 'genres' && include.id === genre.id))
        .map((genre) => genre.attributes.name);
  }

  let videos;
  if (type === 'series' && animeData.relationships.episodes.data && includes) {
    const seriesStartTime = new Date(animeData.attributes.startDate).getTime();
    videos = includes
        .filter((include) => include.type === 'episodes')
        .map((ep) => ({
          id: `kitsu:${animeData.id}:${ep.attributes.number}`,
          title: ep.attributes.titles.en_us ||
              ep.attributes.titles.en ||
              ep.attributes.titles.en_jp ||
              ep.attributes.canonicalTitle ||
              `Episode ${ep.attributes.number}`,
          released: new Date( seriesStartTime + ep.attributes.number),
          season: 1,
          episode: ep.attributes.number,
        }));
  }

  const imdbInfo = imdbMappping[animeData.id] || {};

  const titles = [animeData.attributes.titles.en_us, animeData.attributes.titles.en, animeData.attributes.titles.en_jp]
      .concat(imdbInfo.title)
      .concat(animeData.attributes.abbreviatedTitles || [])
      .filter((v) => v)
      .reduce((array, next) => {
        if (array.find((v) => v.toLowerCase() === next.toLowerCase())) {
          return array.push(next);
        }
        return array;
      }, []);

  return sanitize({
    id: `kitsu:${animeData.id}`,
    kitsu_id: animeData.id,
    imdb_id: imdbInfo.imdb_id,
    imdbFromSeason: imdbInfo.fromSeason,
    imdbFromEpisode: imdbInfo.fromEpisode,
    type: type,
    animeType: animeData.attributes.subtype,
    name: animeData.attributes.canonicalTitle,
    slug: animeData.attributes.slug,
    aliases: titles,
    genres: genres,
    poster: animeData.attributes.posterImage && animeData.attributes.posterImage.small,
    background: animeData.attributes.coverImage && animeData.attributes.coverImage.original,
    description: animeData.attributes.synopsis,
    releaseInfo: releaseInfo,
    year: releaseInfo,
    imdbRating: animeData.attributes.averageRating && animeData.attributes.averageRating / 10.0,
    userCount: animeData.attributes.userCount,
    status: animeData.attributes.status,
    runtime: animeData.attributes.episodeLength && `${animeData.attributes.episodeLength} min`,
    trailers: animeData.attributes.youtubeVideoId && [{ source: animeData.attributes.youtubeVideoId, type: 'Trailer'}],
    videos: videos
  });
}

function getType(animeData) {
  if (animeData.attributes.subtype === 'movie') {
    return 'movie';
  } else if (animeData.attributes.subtype === 'special') {
    if (animeData.attributes.episodeCount > 1) {
      return 'series'
    }
    return 'movie'
  }
  return 'series'
}

async function getBackgroundImage(imdbId, coverImage) {
  return verifyImageOrFallback(imdbId, coverImage, `https://images.metahub.space/background/medium/${imdbId}/img`)
}

async function getLogoImage(imdbId) {
  return verifyImageOrFallback(imdbId, undefined, `https://images.metahub.space/logo/medium/${imdbId}/img`)
}

async function verifyImageOrFallback(imdbId, kitsuImage, imdbImage) {
  return new Promise((resolve) => {
    if (!imdbId) {
      resolve(kitsuImage || undefined);
    }
    urlExists(imdbImage, (err, exists) => {
      if (exists) {
        resolve(imdbImage)
      } else {
        resolve(kitsuImage || undefined);
      }
    })
  });
}

function sanitize(obj) {
  Object.keys(obj).forEach((key) => (obj[key] == null) && delete obj[key]);
  return obj;
}

module.exports = { toStremioCatalogMeta, toStremioEntryMeta };