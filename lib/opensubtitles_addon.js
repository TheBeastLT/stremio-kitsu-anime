const needle = require('needle');

const OPENSUBTITLES_ADDON_URL = 'https://opensubtitles.strem.io';
const DEFAULT_TIMEOUT = 10000; // 10s

async function getSubtitles(metadata, kitsuEpisode) {
  const videoEpisode = metadata.videos.find((video) => video.episode === kitsuEpisode);
  if (!videoEpisode) {
    return [];
  }

  const type = metadata.type;
  const imdbId = videoEpisode.imdb_id || metadata.imdb_id;
  const season = videoEpisode.season;
  const episode = videoEpisode.episode;
  const url = `${OPENSUBTITLES_ADDON_URL}/subtitles/${type}/${imdbId}:${season}:${episode}.json`

  return needle('get', url, { open_timeout: DEFAULT_TIMEOUT })
      .then(response => {
        if (response.body && response.body.subtitles) {
          return response.body.subtitles;
        } else {
          throw new Error('No subtitle results');
        }
      })
}

module.exports = { getSubtitles };