// Title search for the imdb id of a seasonal entry that Fribb has none for. Search results rank the wanted title
// below older namesakes often enough (Jiang Ye, Keroro Gunsou☆: 2nd), so callers verify the candidates in order.
const axios = require('axios');

const MAX_SEARCH_CANDIDATES = 5;

// Title imdb ids from IMDb's own search suggestions, best match first, at most MAX_SEARCH_CANDIDATES.
async function searchImdbIds(title) {
  const letter = title.slice(0,1).toLowerCase().normalize('NFKD');
  const query = encodeURIComponent(title.trim());
  return axios.get(`https://v2.sg.media-imdb.com/suggestion/${letter}/${query}.json`)
      .then(response => [...new Set(suggestionImdbIds(response.data))].slice(0, MAX_SEARCH_CANDIDATES));
}

// The suggestions also list people (nm...) and editorial pages (/family-entertainment-guide/), which can't be mapped.
function suggestionImdbIds(data) {
  return (data?.d || []).map(item => item.id).filter(id => /^tt\d+$/.test(id));
}

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

module.exports = { searchImdbIds, suggestionImdbIds, firstTrusted, MAX_SEARCH_CANDIDATES };
