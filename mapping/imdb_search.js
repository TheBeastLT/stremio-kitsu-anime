// Title search for the imdb id of a seasonal entry that Fribb has none for. Search results rank the wanted title
// below older namesakes often enough (Jiang Ye, Keroro Gunsou☆: 2nd), so callers verify the candidates in order.
const axios = require('axios');

const MAX_SEARCH_CANDIDATES = 5;

// Title imdb ids from IMDb's own search suggestions, best match first, at most MAX_SEARCH_CANDIDATES.
async function searchImdbIds(title) {
  return suggestions(title).then(data => [...new Set(suggestionImdbIds(data))].slice(0, MAX_SEARCH_CANDIDATES));
}

// Imdb ids of titles named exactly like one of `titles` (ignoring case, spacing and punctuation) from about `year`,
// searched with the year added: small films and shorts only surface that way, below other years' namesakes
// (Firstline (2024) for First Line, An tu! An tu! (2018)). Only exact names are kept, as a year search also lists
// that year's unrelated titles.
async function searchImdbIdsByYear(titles, year) {
  for (const title of titles) {
    const ids = namedImdbIds(await suggestions(`${title} ${year}`).catch(() => undefined), titles, year);
    if (ids.length) {
      return ids;
    }
  }
  return [];
}

// The suggestions' title ids named exactly like one of `titles`, a year apart at most.
function namedImdbIds(data, titles, year) {
  const names = new Set(titles.map(compactTitle));
  return [...new Set((data?.d || [])
      .filter(item => /^tt\d+$/.test(item.id) && names.has(compactTitle(item.l)) && Math.abs(item.y - year) <= 1)
      .map(item => item.id))];
}

async function suggestions(query) {
  const letter = query.slice(0,1).toLowerCase().normalize('NFKD');
  return axios.get(`https://v2.sg.media-imdb.com/suggestion/${letter}/${encodeURIComponent(query.trim())}.json`)
      .then(response => response.data);
}

// The suggestions also list people (nm...) and editorial pages (/family-entertainment-guide/), which can't be mapped.
function suggestionImdbIds(data) {
  return (data?.d || []).map(item => item.id).filter(id => /^tt\d+$/.test(id));
}

const compactTitle = title => (title || '').toLowerCase().normalize('NFKD').replace(/[^a-z0-9]+/g, '');

// The entry `verify` builds for the first candidate it trusts, else the one for the first candidate (to review).
async function firstTrusted(imdbIds, verify) {
  let first;
  for (const imdbId of imdbIds) {
    const entry = await verify(imdbId);
    if (entry._trusted) {
      return entry;
    }
    first = first || entry;
  }
  return first;
}

module.exports = {
  searchImdbIds, searchImdbIdsByYear, suggestionImdbIds, namedImdbIds, firstTrusted, MAX_SEARCH_CANDIDATES,
};
