// Title search for the imdb id of a seasonal entry that Fribb has none for. Search results rank the wanted title
// below older namesakes often enough (Jiang Ye, Keroro Gunsou☆: 2nd), so callers verify the candidates in order.
const axios = require('axios');
const googleSr = require('google-sr');

const MAX_SEARCH_CANDIDATES = 5;

// Title imdb ids for a search, best match first, at most MAX_SEARCH_CANDIDATES.
async function searchImdbIds(title, type) {
  const query = `${title} imdb`;
  return googleSr.search({ query })
      .then(results => results
          .filter(result => result?.link?.match(/imdb.com\/.*title\//))
          .map(result => result.link.match(/(tt\d+)/)?.[1])
          .filter(Boolean))
      .then(ids => ids.length ? ids : Promise.reject('No results'))
      .catch((err) => getImdbIdsFromImdbSuggestions(title))
      .catch((err) => getImdbIdFromTrakt(title, type).then(id => [id]))
      .then(ids => [...new Set(ids)].slice(0, MAX_SEARCH_CANDIDATES));
}

async function getImdbIdsFromImdbSuggestions(title) {
  const letter = title.slice(0,1).toLowerCase().normalize('NFKD');
  const query = encodeURIComponent(title.trim());
  return axios.get(`https://v2.sg.media-imdb.com/suggestion/${letter}/${query}.json`)
      .then(response => suggestionImdbIds(response.data))
      .then(ids => ids.length ? ids : Promise.reject("No imdb result"));
}

// The suggestions also list people (nm...) and editorial pages (/family-entertainment-guide/), which can't be mapped.
function suggestionImdbIds(data) {
  return (data?.d || []).map(item => item.id).filter(id => /^tt\d+$/.test(id));
}

async function getImdbIdFromTrakt(title, type) {
    const traktType = type === 'movie' ? 'movie' : 'show';
    const query = encodeURIComponent(title.trim());
    const headers = { 'trakt-api-key': process.env.TRAKT_CLIENT_ID }
    return axios.get(`https://api.trakt.tv/search/${traktType}?query=${query}`, { headers })
        .then(response => {
            if (Array.isArray(response.data)) {
                return response.data.map(result => result[traktType])[0]?.ids?.imdb;
            }
            return Promise.reject('No imdb match found in trakt');
        });
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
