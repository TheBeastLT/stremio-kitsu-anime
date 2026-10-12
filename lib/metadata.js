const { getImages } = require('./fanart')
const { getGenreUrl } = require('./config')
const { getImdbMapping } = require("./metadataEnrich");

const allowedRelationships = [ 'prequel', 'sequel' ]

const CATALOG_META_FIELDS = ['id', 'kitsu_id', 'imdb_id', 'type', 'animeType', 'name', 'aliases', 'description',
  'releaseInfo', 'runtime', 'imdbRating', 'genres', 'logo', 'poster', 'background', 'trailers', 'links'];

async function toStremioCatalogMeta(animeData) {
  return toCatalogMeta(await _toStremioMeta(animeData));
}

// The catalog fields of a meta, either just built or a full one from the meta cache (no videos).
function toCatalogMeta(meta) {
  return Object.fromEntries(CATALOG_META_FIELDS.map((field) => [field,
    field === 'imdb_id' ? meta.imdb_id || getImdbMapping(meta.kitsu_id)?.imdb_id : meta[field]]));
}

async function toStremioEntryMeta(animeData, options) {
  return _toStremioMeta(animeData, options);
}

async function _toStremioMeta(animeData, { images = true, relations = false } = {}) {
  const type = getType(animeData);

  let releaseInfo = animeData.startDate && `${animeData.startDate.match(/^\d+/)[0]}`;
  if (animeData.endDate && !animeData.endDate.startsWith(releaseInfo)) {
    releaseInfo = releaseInfo && releaseInfo.concat(`-${animeData.endDate.match(/^\d+/)[0]}`);
  } else if (animeData.status === 'current') {
    releaseInfo = releaseInfo && releaseInfo.concat('-');
  }

  const genres = animeData.genres.data?.map(genre => genre.name);
  const fanartImages = images ? await getImages(animeData.id).catch(() => ({})) : {};

  let videos;
  let behaviorHints;
  if (animeData.subtype !== 'movie' || animeData.episodes.data?.length || animeData.episodeCount > 1) {
    const seriesStartTime = new Date(animeData.startDate).getTime();
    if (animeData.episodes.data?.length) {
      let lastReleaseDate = new Date(seriesStartTime)
      videos = animeData.episodes.data
          .map((ep, index, self) => ({
            id: `kitsu:${animeData.id}:${ep.number}`,
            title: ep.titles.en_us ||
                ep.titles.en ||
                ep.titles.en_jp ||
                ep.canonicalTitle ||
                `Episode ${ep.number}`,
            released: episodeReleased(ep, self[index + 1], lastReleaseDate),
            season: 1,
            episode: ep.number,
            thumbnail: ep.thumbnail?.original,
            overview: cleanDescription(ep.synopsis),
            runtime: ep.length > 0 ? `${ep.length} min` : undefined,
          }))
          .sort((a, b) => a.episode - b.episode);
    } else if (animeData.episodeCount) {
      videos = [...Array(animeData.episodeCount).keys()]
          .map((ep) => ep + 1)
          .map((ep) => ({
            id: `kitsu:${animeData.id}:${ep}`,
            title: `Episode ${ep}`,
            released: new Date(seriesStartTime),
            season: 1,
            episode: ep,
          }))
    }
    if (videos && videos.length === 1 && ['movie', 'special', 'OVA', 'ONA'].includes(animeData.subtype)) {
      videos[0].id = `kitsu:${animeData.id}`;
      behaviorHints = { defaultVideoId: videos[0].id }
    }
  }

  const titles = [animeData.titles.en_us, animeData.titles.en, animeData.titles.en_jp]
      .concat(animeData.abbreviatedTitles || [])
      .filter((v) => v)
      .reduce((array, next) => {
        if (!array.find((v) => v.toLowerCase() === next.toLowerCase())) {
          array.push(next);
        }
        return array;
      }, []);

  return cleanOutputObject({
    id: `kitsu:${animeData.id}`,
    kitsu_id: animeData.id,
    type: type,
    animeType: animeData.subtype,
    name: animeData.canonicalTitle,
    slug: animeData.slug,
    aliases: titles,
    genres: genres || [],
    logo: fanartImages.logo,
    poster: animeData?.posterImage?.medium || fanartImages.poster,
    background: fanartImages.background || animeData.coverImage && animeData.coverImage.original,
    description: cleanDescription(animeData.synopsis),
    releaseInfo: releaseInfo,
    year: releaseInfo,
    imdbRating: roundedRating(animeData.averageRating)?.toString(),
    userCount: animeData.userCount,
    status: animeData.status,
    runtime: Number.isInteger(animeData.episodeLength) && `${animeData.episodeLength} min` || null,
    trailers: animeData.youtubeVideoId && [{ source: animeData.youtubeVideoId, type: 'Trailer' }] || [],
    videos: videos,
    links: kitsuLinks(animeData, type),
    kitsuRelations: relations ? kitsuRelations(animeData) : undefined,
    behaviorHints
  });
}

function getType(animeData) {
  if (animeData.subtype === 'movie') {
    return 'movie';
  }
  return 'series'
}

function roundedRating(rating) {
  return rating && (Math.round(((rating / 10.0) + Number.EPSILON) * 10.0) / 10.0).toFixed(1);
}

function kitsuLinks(animeData, type) {
  const imdbRating = roundedRating(animeData.averageRating)
  const rating = imdbRating && [{
    name: `${imdbRating}`,
    category: 'imdb',
    url: `https://kitsu.io/anime/${animeData.slug}`
  }] || [];
  const franchise = (animeData.mediaRelationships.data || [])
      .filter(relationship => allowedRelationships.includes(relationship.role))
      .map(relationship => ({
        name: `${capitalize(relationship.role)}: ${relationship.destination.data.canonicalTitle}`,
        category: 'Franchise',
        url: `stremio:///detail/${type}/kitsu:${relationship.destination.data.id}`
      }));
  const genres = (animeData.genres.data || [])
      .map(genre => genre.name)
      .map(genre => ({
        name: genre,
        category: 'Genres',
        url: getGenreUrl(genre)
      }));
  return rating.concat(franchise).concat(genres);
}

function episodeReleased(ep, nextEp, lastReleaseDate) {
  const airDate = ep.airdate && new Date(ep.airdate);
  const nextAirDate = nextEp && nextEp.airdate && new Date(nextEp.airdate) || airDate;
  const released = airDate && airDate.getTime() > lastReleaseDate.getTime() && airDate.getTime() <= nextAirDate.getTime()
      ? airDate
      : new Date(lastReleaseDate.getTime());
  lastReleaseDate.setTime(released.getTime());
  return released;
}

function capitalize(input) {
  return input.charAt(0).toUpperCase() + input.slice(1);
}

function cleanDescription(description) {
  return description?.replace(/\n+(?:[([].+[)\]\n]|Source:.*)?(?:\n+Note(.|\n)+)?$/, '');
}

function kitsuRelations(animeData) {
  return (animeData.mediaRelationships.data || [])
      .filter(relationship => relationship.destination?.data?.type === 'anime')
      .map(relationship => ({ role: relationship.role, kitsuId: Number(relationship.destination.data.id) }));
}

function cleanOutputObject(object) {
    return Object.fromEntries(Object.entries(object).filter(([_, v]) => v != null));
}

module.exports = { toStremioCatalogMeta, toStremioEntryMeta, toCatalogMeta };
