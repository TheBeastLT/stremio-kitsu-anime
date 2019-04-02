const imdbMappping = require('../static/data/imdb_mapping');

function toStremioMeta(animeData, includes) {
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
    videos = includes
        .filter((include) => include.type === 'episodes')
        .map((ep) => ({
          id: `kitsu:${animeData.id}:${ep.attributes.number || ep.attributes.relativeNumber}`,
          title: ep.attributes.titles.en_us ||
              ep.attributes.titles.en ||
              ep.attributes.titles.en_jp ||
              ep.attributes.canonicalTitle ||
              `Episode ${ep.attributes.relativeNumber || ep.attributes.number}`,
          released: new Date(new Date(animeData.attributes.startDate).getTime() + ep.attributes.number || ep.attributes.relativeNumber),
          season: 1,
          episode: ep.attributes.number || ep.attributes.relativeNumber,
    }));
  }

  const imdbInfo = imdbMappping[animeData.id] || {};

  const titles = [animeData.attributes.titles.en_us, animeData.attributes.titles.en, animeData.attributes.titles.en_jp]
      .concat(imdbInfo.title)
      .concat(animeData.attributes.abbreviatedTitles || [])
      .filter((v) => v)
      .filter((v, i, a) => a.indexOf(v) === i);

  return {
    id: `kitsu:${animeData.id}`,
    kitsu_id: animeData.id,
    imdb_id: imdbInfo.imdb_id,
    type: type,
    animeType: animeData.attributes.subtype,
    name: titles[0],
    slug: animeData.attributes.slug,
    aliases: titles.slice(1),
    genres: genres,
    poster: animeData.attributes.posterImage && animeData.attributes.posterImage.small,
    background: imdbInfo.imdb_id && `https://images.metahub.space/background/medium/${imdbInfo.imdb_id}/img` ||
        animeData.attributes.coverImage && animeData.attributes.coverImage.original,
    logo: imdbInfo.imdb_id && `https://images.metahub.space/logo/medium/${imdbInfo.imdb_id}/img`,
    description: animeData.attributes.synopsis,
    releaseInfo: releaseInfo,
    year: releaseInfo,
    imdbRating: animeData.attributes.averageRating && parseInt(animeData.attributes.averageRating) / 10.0,
    status: animeData.attributes.status,
    runtime: animeData.attributes.episodeLength && `${animeData.attributes.episodeLength} min`,
    trailers: animeData.attributes.youtubeVideoId && [{ source: animeData.attributes.youtubeVideoId, type: 'Trailer'}],
    videos: videos
  }
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

module.exports = { toStremioMeta };