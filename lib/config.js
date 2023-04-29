const ADDON_URL = process.env.ADDON_URL || "https://anime-kitsu.strem.fun"

function getGenreUrl(genre) {
  return `stremio:///discover/${encodeURIComponent(ADDON_URL)}%2Fmanifest.json/anime/kitsu-anime-popular?genre=${genre}`
}

module.exports = { ADDON_URL, getGenreUrl };