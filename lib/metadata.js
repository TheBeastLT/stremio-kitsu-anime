const { getImages } = require('./fanart')
const { getImdbId } = require('./metadataEnrich')

async function toStremioCatalogMeta(animeData, includes) {
  const meta = await _toStremioMeta(animeData, includes);
  return {
    id: meta.id,
    type: meta.type,
    animeType: meta.animeType,
    name: meta.name,
    aliases: meta.aliases,
    description: meta.description,
    releaseInfo: meta.releaseInfo,
    runtime: meta.runtime,
    imdbRating: meta.imdbRating,
    genres: meta.genres,
    logo: meta.logo,
    poster: meta.poster,
    background: meta.background,
    trailers: meta.trailers
  }
}

async function toStremioEntryMeta(animeData, includes) {
  return _toStremioMeta(animeData, includes);
}

async function _toStremioMeta(animeData, includes) {
  const attributes = animeData.attributes;
  const type = getType(animeData);

  let releaseInfo = `${attributes.startDate && attributes.startDate.match(/^\d+/)[0]}`;
  if (attributes.endDate && !attributes.endDate.startsWith(releaseInfo)) {
    releaseInfo = releaseInfo.concat(`-${attributes.endDate.match(/^\d+/)[0]}`);
  } else if (attributes.status === 'current') {
    releaseInfo = releaseInfo.concat('-');
  }

  let genres = [];
  if (animeData.relationships.genres.data && includes) {
    genres = animeData.relationships.genres.data
        .map((genre) => includes.find((include) => include.type === 'genres' && include.id === genre.id))
        .map((genre) => genre.attributes.name);
  }

  let videos;
  if (type === 'series' || attributes.episodeCount > 1) {
    const seriesStartTime = new Date(attributes.startDate).getTime();
    if (includes && animeData.relationships.episodes.data && animeData.relationships.episodes.data.length) {
      let lastReleaseDate = new Date(seriesStartTime)
      videos = includes
          .filter((include) => include.type === 'episodes')
          .map((ep, index, self) => ({
            id: `kitsu:${animeData.id}:${ep.attributes.number}`,
            title: ep.attributes.titles.en_us ||
                ep.attributes.titles.en ||
                ep.attributes.titles.en_jp ||
                ep.attributes.canonicalTitle ||
                `Episode ${ep.attributes.number}`,
            released: episodeReleased(ep, self[index + 1],lastReleaseDate),
            season: 1,
            episode: ep.attributes.number,
          }))
          .sort((a, b) => a.episode - b.episode);
    } else if (attributes.episodeCount) {
      videos = [...Array(attributes.episodeCount).keys()]
          .map((ep) => ep + 1)
          .map((ep) => ({
            id: `kitsu:${animeData.id}:${ep}`,
            title: `Episode ${ep}`,
            released: new Date(seriesStartTime + ep),
            season: 1,
            episode: ep,
          }))
    }
    if (videos && videos.length === 1 && ['movie', 'special', 'OVA', 'ONA'].includes(attributes.subtype)) {
      videos[0].id = `kitsu:${animeData.id}`;
    }
  }

  const titles = [attributes.titles.en_us, attributes.titles.en, attributes.titles.en_jp]
      .concat(attributes.abbreviatedTitles || [])
      .filter((v) => v)
      .reduce((array, next) => {
        if (!array.find((v) => v.toLowerCase() === next.toLowerCase())) {
          array.push(next);
        }
        return array;
      }, []);

  const imdbId = getImdbId(animeData.id);
  const fanartImages = await getImages(imdbId, type).catch(() => ({}));

  return {
    id: `kitsu:${animeData.id}`,
    kitsu_id: animeData.id,
    type: type,
    animeType: attributes.subtype,
    name: attributes.canonicalTitle,
    slug: attributes.slug,
    aliases: titles,
    genres: genres,
    logo: fanartImages.logo,
    poster: attributes.posterImage && attributes.posterImage.medium || fanartImages.poster,
    background: fanartImages.background || attributes.coverImage && attributes.coverImage.original,
    description: attributes.synopsis,
    releaseInfo: releaseInfo,
    year: releaseInfo,
    imdbRating: roundedRating(attributes.averageRating),
    userCount: attributes.userCount,
    status: attributes.status,
    runtime: Number.isInteger(attributes.episodeLength) && `${attributes.episodeLength} min` || null,
    trailers: attributes.youtubeVideoId && [{ source: attributes.youtubeVideoId, type: 'Trailer' }],
    videos: videos,
    links: kitsuLink(attributes)
  };
}

function getType(animeData) {
  if (animeData.attributes.subtype === 'movie') {
    return 'movie';
  }
  return 'series'
}

function roundedRating(rating) {
  return rating && Math.round(((rating / 10.0) + Number.EPSILON) * 100) / 100;
}

function kitsuLink(attributes) {
  return [{
    name: 'Kitsu',
    category: 'kitsu',
    url: `https://kitsu.io/anime/${attributes.slug}`
  }];
}

function episodeReleased(ep, nextEp, lastReleaseDate) {
  const airDate = ep.attributes.airdate && new Date(ep.attributes.airdate);
  const nextAirDate = nextEp && nextEp.attributes.airdate && new Date(nextEp.attributes.airdate) || airDate;
  const released = airDate && airDate.getTime() > lastReleaseDate.getTime() && airDate.getTime() <= nextAirDate.getTime()
      ? airDate
      : new Date(lastReleaseDate.getTime() + 1);
  lastReleaseDate.setTime(released.getTime());
  return released;
}

module.exports = { toStremioCatalogMeta, toStremioEntryMeta };