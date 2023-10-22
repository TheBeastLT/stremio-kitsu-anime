const OPENSUBTITLES_ADDON_URL = 'https://opensubtitles-v3.strem.io';

async function getRedirectUrl(metadata, args) {
  const parts = args.id.split(':');
  const kitsuId = parts[1];
  const episode = parts[2] ? parseInt(parts[2], 10) : undefined;
  const videoId = getImdbVideoId(metadata, kitsuId, episode);
  if (!videoId) {
    throw new Error('No imdb mapping found');
  }
  const type = Array.isArray(metadata.videos) ? 'series' : 'movie';
  const extrasPrefix = Object.keys(args.extra || {}).length ? '/' : '';
  const extras = extrasPrefix + Object.entries(args.extra || {})
      .map(([key, value]) => `${key}=${value}`)
      .join('&')
  const url = `${OPENSUBTITLES_ADDON_URL}/subtitles/${type}/${videoId}${extras}.json`
  console.log(url)
  return url;
}

function getImdbVideoId(metadata, kitsuId, kitsuEpisode) {
  if (Number.isInteger(kitsuEpisode)) {
    const videoEpisode = metadata.videos.find((video) => video.episode === kitsuEpisode);
    if (videoEpisode && videoEpisode.imdb_id) {
      const imdbId = videoEpisode.imdb_id || metadata.imdb_id;
      const season = videoEpisode.imdbSeason;
      const episode = videoEpisode.imdbEpisode;
      return `${imdbId}:${season}:${episode}`
    }
  } else {
    return metadata.imdb_id;
  }
}

module.exports = { getRedirectUrl };