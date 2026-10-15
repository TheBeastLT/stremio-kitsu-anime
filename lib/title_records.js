// The title search's data file, static/data/search_titles.jsonl: one record per Kitsu anime (its titles plus
// anime-offline-database synonyms), one per line sorted by id. Rebuilt weekly by mapping/update_search_titles.js.
const fs = require('fs');
const path = require('path');

const TITLES_PATH = path.join(__dirname, '../static/data/search_titles.jsonl');
const ALIASES_PATH = path.join(__dirname, '../static/data/search_aliases.json');

function readRecords(file = TITLES_PATH) {
  return fs.readFileSync(file, 'utf8').split('\n').filter(Boolean).map(line => JSON.parse(line));
}

// The hand-kept aliases of static/data/search_aliases.json ({ kitsuId: [title] }): names search can't work out (a
// release group's franchise-wide season number, a transliteration nobody lists), added as Kitsu titles of their entry,
// so they keep their season numbers. Not written into the data file: its weekly rebuild would drop them.
function withAliases(records, aliases = JSON.parse(fs.readFileSync(ALIASES_PATH, 'utf8'))) {
  return records.map(record => aliases[record.id] ? {
    ...record,
    titles: { ...record.titles, ...Object.fromEntries(aliases[record.id].map((alias, i) => [`alias${i}`, alias])) },
  } : record);
}

// The scraper's query cleaning drops every character but ASCII letters and digits, so other titles could never match.
const hasAsciiAlnum = title => /[A-Za-z0-9]/.test(title || '');

// A title cut at its commas where a list of titles was split ("I Admire Magical Girls, and..." -> "and..." from MAL's
// comma-joined synonyms; "Little Ghosts, There, Here, and Where" -> "and Where" in Kitsu's abbreviated titles): a piece
// after a comma that starts in lowercase, is no more than three letters / digits or only numbers can't be a title.
function isTitleFragment(title, pieces) {
  return pieces.has(title.trim()) && (/^\p{Ll}/u.test(title.trim()) || title.replace(/[^A-Za-z0-9]/g, '').length <= 3
      || /^[\d\s.,&+-]+(?:and[\d\s.,-]+)?$/i.test(title.trim()));
}

// A record of a Kitsu anime (kitsu_api's animeTitlePage) and its synonyms.
function toTitleRecord(anime, synonyms = []) {
  const titles = Object.fromEntries(Object.entries(anime.titles || {}).filter(([, title]) => hasAsciiAlnum(title)));
  const all = [anime.canonicalTitle, ...Object.values(titles), ...(anime.abbreviatedTitles || []), ...synonyms]
      .filter(Boolean);
  const pieces = new Set(all.flatMap(title => title.split(',').slice(1).map(piece => piece.trim())));
  const kept = list => list.filter(title => hasAsciiAlnum(title) && !isTitleFragment(title, pieces));
  return {
    id: +anime.id,
    canonicalTitle: anime.canonicalTitle,
    titles,
    abbreviatedTitles: kept(anime.abbreviatedTitles || []),
    subtype: anime.subtype,
    status: anime.status,
    startDate: anime.startDate,
    episodeCount: anime.episodeCount,
    synonyms: kept(synonyms),
  };
}

module.exports = { TITLES_PATH, ALIASES_PATH, readRecords, withAliases, toTitleRecord };
