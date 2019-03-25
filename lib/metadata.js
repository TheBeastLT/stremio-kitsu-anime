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
    videos = animeData.relationships.episodes.data
    .map((ep) => includes.find((include) => include.type === 'episodes' && include.id === ep.id))
    .map((ep) => ({
      id: `kitsu:${animeData.id}:${ep.attributes.relativeNumber || ep.attributes.number}`,
      title: ep.attributes.titles.en_us ||
          ep.attributes.titles.en ||
          ep.attributes.titles.en_jp ||
          ep.attributes.canonicalTitle ||
          `Episode ${ep.attributes.relativeNumber || ep.attributes.number}`,
      released: new Date(new Date(animeData.attributes.startDate).getTime() + ep.attributes.relativeNumber || ep.attributes.number),
      season: 1,
      episode: ep.attributes.relativeNumber || ep.attributes.number,
    }));
  }

  return {
    id: `kitsu:${animeData.id}`,
    kitsuId: animeData.id,
    type: type,
    animeType: animeData.attributes.subtype,
    name: animeData.attributes.titles.en_us ||
        animeData.attributes.titles.en ||
        animeData.attributes.titles.en_jp ||
        animeData.attributes.canonicalTitle,
    genres: genres,
    poster: animeData.attributes.posterImage && animeData.attributes.posterImage.original,
    background: animeData.attributes.coverImage && animeData.attributes.coverImage.original,
    // logo: ,
    description: animeData.attributes.synopsis,
    releaseInfo: releaseInfo,
    status: animeData.attributes.status,
    runtime: animeData.attributes.episodeLength && `${animeData.attributes.episodeLength} min`,
    trailers: animeData.attributes.youtubeVideoId && [{ source: animeData.attributes.youtubeVideoId, type: 'Trailer'}],
    videos: videos
  }
}

function getType(animeData) {
  if (animeData.attributes.subtype === 'movie') {
    return 'movie';
  } else if (['TV', 'OVA', 'ONA'].includes(animeData.attributes.subtype)) {
    return 'series';
  } else if (animeData.attributes.subtype === 'special' && animeData.relationships.episodes.data) {
    if (animeData.relationships.episodes.data.length > 1) {
      return 'series'
    }
    return 'movie'
  }
  return 'other'
}

module.exports = { toStremioMeta };