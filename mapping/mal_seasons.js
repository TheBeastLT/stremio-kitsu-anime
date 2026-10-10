// MAL seasons (winter = Jan-Mar, ..., fall = Oct-Dec) and Kitsu's season filter values (winter = Dec-Feb, ...).

const MAL_SEASONS = ['winter', 'spring', 'summer', 'fall'];
const KITSU_SEASONS = ['winter', 'spring', 'summer', 'fall'];

// The MAL season a date falls in, like '2026/fall'.
function malSeason(date = new Date()) {
  return `${date.getUTCFullYear()}/${MAL_SEASONS[Math.floor(date.getUTCMonth() / 3)]}`;
}

function previousMalSeason(season) {
  const [year, name] = season.split('/');
  const index = MAL_SEASONS.indexOf(name);
  return index === 0 ? `${parseInt(year) - 1}/fall` : `${year}/${MAL_SEASONS[index - 1]}`;
}

// The current MAL season and the `count - 1` before it, oldest first.
function recentMalSeasons(count, date = new Date()) {
  const seasons = [malSeason(date)];
  while (seasons.length < count) {
    seasons.unshift(previousMalSeason(seasons[0]));
  }
  return seasons;
}

// Kitsu { season, year } filters covering every month from `from` to `to`; December counts as next year's winter.
function kitsuSeasons(from, to) {
  const seasons = new Map();
  const month = new Date(Date.UTC(from.getUTCFullYear(), from.getUTCMonth(), 1));
  while (month <= to) {
    const shifted = new Date(Date.UTC(month.getUTCFullYear(), month.getUTCMonth() + 1, 1));
    const season = { season: KITSU_SEASONS[Math.floor(shifted.getUTCMonth() / 3)], year: shifted.getUTCFullYear() };
    seasons.set(`${season.year}/${season.season}`, season);
    month.setUTCMonth(month.getUTCMonth() + 1);
  }
  return [...seasons.values()];
}

// usage: node mapping/mal_seasons.js [count]   -> prints the recent MAL seasons, oldest first
if (require.main === module) {
  console.log(recentMalSeasons(parseInt(process.argv[2]) || 2).join(' '));
}

module.exports = { malSeason, previousMalSeason, recentMalSeasons, kitsuSeasons };
