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

function nextMalSeason(season) {
  const [year, name] = season.split('/');
  const index = MAL_SEASONS.indexOf(name);
  return index === MAL_SEASONS.length - 1 ? `${parseInt(year) + 1}/winter` : `${year}/${MAL_SEASONS[index + 1]}`;
}

// The current MAL season and the `count - 1` before it, oldest first. With `upcoming`, in the current season's last
// month (where seasonWindow's pre-season padding starts) the next season too, so its titles are mapped before they air.
function recentMalSeasons(count, date = new Date(), { upcoming = false } = {}) {
  const seasons = [malSeason(date)];
  while (seasons.length < count) {
    seasons.unshift(previousMalSeason(seasons[0]));
  }
  if (upcoming && date.getUTCMonth() % 3 === 2) {
    seasons.push(nextMalSeason(seasons[seasons.length - 1]));
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

// usage: node mapping/mal_seasons.js [count] [--upcoming]   -> prints the recent MAL seasons, oldest first
if (require.main === module) {
  const upcoming = process.argv.includes('--upcoming');
  console.log(recentMalSeasons(parseInt(process.argv[2]) || 2, new Date(), { upcoming }).join(' '));
}

module.exports = { malSeason, previousMalSeason, nextMalSeason, recentMalSeasons, kitsuSeasons };
