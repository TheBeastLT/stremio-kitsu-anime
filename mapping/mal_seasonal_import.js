const fs = require('fs');
const axios = require('axios');
const cheerio = require('cheerio');
const googleSr = require('google-sr');
const { queryIdMapping } = require('../lib/id_convert');
const { getImdbMapping } = require('../lib/metadataEnrich');
const { search, animeData } = require('../lib/kitsu_api');
const { getTvdbId } = require("../lib/fanart");

async function importMalSeason(season) {
  const imdbMapping = require('../static/data/imdb_mapping');
  const malEntries = await getMalSeasonalEntries(season);
  const newMappingEntries = await sequence(malEntries.map(malEntry => () => createImdbMappingEntry(malEntry)));
  const newMappings = newMappingEntries
      .filter(entry => entry && !imdbMapping[entry.kitsuId])
      .reduce((map, entry) => (map[entry.kitsuId] = entry, map), {})
  writeToFile(newMappings, "./static/data/new_mappings.json")
  console.log("Ids to remove from cache:")
  console.log(newMappingEntries.map(e => [e?.kitsuId, e?.imdb_id]).flat().filter(x => x).join('|'))
  return season;
}

async function createImdbMappingEntry(malEntry) {
  const kitsuIdFromMal = await queryIdMapping(`mal`, malEntry.malId).catch((err) => undefined);
  const kitsuIdFromSearch = !kitsuIdFromMal && await search(malEntry.title)
      .then(metas => metas[0]?.kitsu_id)
      .catch((err) => undefined);
  const kitsuId = kitsuIdFromMal || kitsuIdFromSearch;
  if (!kitsuId) {
    console.log(`No kitsuId found for: ${JSON.stringify(malEntry)}`);
    return kitsuId;
  }

  const kitsuMetadata = await animeData(kitsuId).catch(() => undefined);
  const prequelKitsuId = kitsuMetadata && kitsuMetadata.links
      .filter(link => link.name.startsWith("Prequel"))
      .map(link => link.url.match(/kitsu:(\d+)$/)[1])[0];
  const prequelImdbMapping = getImdbMapping(prequelKitsuId);
  if (prequelImdbMapping) {
    return {
      malId: malEntry.malId,
      malTitle: malEntry.title,
      kitsuId: kitsuId,
      kitsuTitle: kitsuMetadata?.name,
      animeType: kitsuMetadata?.animeType,
      imdb_id: prequelImdbMapping.imdb_id,
      title: prequelImdbMapping.title,
      fromSeason: prequelImdbMapping.fromSeason + 1,
      fromEpisode: 1,
    };
  }

  const type = kitsuMetadata?.animeType === 'movie' ? 'movie' : 'series';
  const foundImdbId = await searchImdbId(malEntry.title, type).catch((err) => undefined);
  if (!foundImdbId) {
    console.log(`No imdbId found for: ${JSON.stringify(malEntry)}`);
    return {
      malId: malEntry.malId,
      malTitle: malEntry.title,
      kitsuId: kitsuId,
      kitsuTitle: kitsuMetadata?.name,
      animeType: kitsuMetadata?.animeType,
    };
  }
  const imdbMeta = await getImdbMeta(foundImdbId).catch((err) => undefined);
  const hasVideos = kitsuMetadata?.videos?.length > 1 || kitsuMetadata?.animeType === 'TV';
  const tvdbId = await getTvdbId({ imdb_id: foundImdbId }).catch(() => false);
  return {
    malId: malEntry.malId,
    malTitle: malEntry.title,
    kitsuId: kitsuId,
    kitsuTitle: kitsuMetadata?.name,
    animeType: kitsuMetadata?.animeType,
    tvdb_id: tvdbId,
    imdb_id: foundImdbId,
    title: imdbMeta?.title,
    fromSeason: hasVideos ? 1 : undefined,
    fromEpisode: hasVideos ? 1 : undefined,
  };
}

async function getMalSeasonalEntries(season) {
  const requestUrl = `https://myanimelist.net/anime/season/${season}`
  return axios.get(requestUrl)
      .then(response => response.data)
      .then(body => {
        const $ = cheerio.load(body);
        return $('a.link-title')
            .map((i, element) => {
              const row = $(element);
              return {
                title: row.text(),
                malId: row.attr('href').match(/anime\/(\d+)/)[1],
              };
            }).get();
      });
}

async function searchImdbId(title, type) {
  const query = `${title} imdb`;
  return googleSr.search({ query })
      .then(response => response.length ? response : Promise.reject('No results'))
      .then(results => results
          .filter(result => result?.link?.match(/imdb.com\/.*title\//))
          .map(result => result.link.match(/(tt\d+)/)[1])[0])
      .catch((err) => getImdbIdFromImdbSuggestions(title))
      .catch((err) => getImdbIdFromTrakt(title, type));
}

async function getImdbIdFromImdbSuggestions(title) {
  const letter = title.slice(0,1).toLowerCase().normalize('NFKD');
  const query = encodeURIComponent(title.trim());
  return axios.get(`https://v2.sg.media-imdb.com/suggestion/${letter}/${query}.json`)
      .then(response => response.data?.d?.[0]?.id)
      .then(result => result ? result : Promise.reject("No imdb result"));
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

async function getImdbMeta(imdbId) {
  const url = `https://www.imdb.com/title/${imdbId}`;
  const config = {
    headers: {
      'user-agent': 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/108.0.0.0 Safari/537.36',
      'accept-language': 'en-GB'
    }
  };
  return axios.get(url, config)
      .then(response => response.data)
      .then(body => {
        const $ = cheerio.load(body);
        const title = $('h1').text().trim();
        return {
          title: title || undefined,
        };
      })
}

function findMappingInsertIndex(imdbMappingEntries, mapping) {
  const kitsuId = parseInt(mapping.kitsuId);
  const imdbId = mapping.imdb_id;
  let insertIndex = 0;
  let franchiseIndex = 0;
  for (let i = imdbMappingEntries.length - 1; i >= 0; i--) {
    const imdbMappingEntry = imdbMappingEntries[i];
    const imdbMapping = imdbMappingEntry[1];
    if (imdbId === imdbMapping && imdbMapping.fromSeason !== 0) {
      franchiseIndex = i + 1;
      break;
    }
    if (!insertIndex && parseInt(imdbMappingEntry[0]) < kitsuId) {
      insertIndex = i + 1;
    }
  }
  return franchiseIndex || insertIndex;
}

function writeImdbMappingToFile(imdbMappingEntries) {
  const imdbMappings = imdbMappingEntries.reduce((map, entry) => (map[entry[0]] = entry[1], map), {})
  writeToFile(imdbMappings, "./static/data/imdb_mapping.json")
}

function writeToFile(object, path) {
  fs.writeFile(path, JSON.stringify(object, null, 2), 'utf8', function (err) {
    if (err) {
      console.log("An error occurred while writing JSON Object to File:", err);
    }
  });
}

async function sequence(promises) {
  return promises.reduce((promise, func) =>
      promise.then(result => func().then(Array.prototype.concat.bind(result))), Promise.resolve([]));
}

importMalSeason('2025/autumn').then(season => `Finished importing MAL ${season}`);