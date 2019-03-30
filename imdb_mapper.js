const needle = require('needle');
const nameToImdb = require('name-to-imdb');
const imdbScrapper = require('imdb-scrapper');
const fs = require('fs');
const { toStremioEntryMeta } = require('./lib/metadata');
const addonInterface = require('./addon');

const MAX_SIZE = 20;

async function migrate(ids) {
  const imdbMappings = require('./static/data/imdb_mapping');

  while (ids.length) {
    const id = ids.shift();
    if (imdbMappings[id]) {
      console.log('skipping: ', imdbMappings[id]);
      continue;
    }

    const kitsuInfo = await _getKitsu(id)
        .then((response) => {
          const meta = toStremioEntryMeta(response.data, response.included);
          const tvdb_id = response.included && response.included
              .filter((include) => include.type === 'mappings')
              .find((mapping) => mapping.attributes.externalSite === 'thetvdb/series');
          const trakt_id = response.included && response.included
              .filter((include) => include.type === 'mappings')
              .find((mapping) => mapping.attributes.externalSite === 'trakt');

          meta.tvdb_id = tvdb_id && tvdb_id.attributes.externalId;
          meta.trakt_id = trakt_id && trakt_id.attributes.externalId;
          meta.episodeCount = response.data.attributes.episodeCount;
          return meta;
        })
        .catch((error) => undefined);

    if (!kitsuInfo || kitsuInfo.type === 'other') {
      continue;
    }

    console.log(`[${kitsuInfo.kitsu_id}] ${kitsuInfo.name}`);

    const imdbId = await _getTrakt(kitsuInfo.trakt_id)
        .catch((error) => _getTvdb(kitsuInfo.tvdb_id))
        .catch((error) => _getNameToImdb(kitsuInfo.name, kitsuInfo))
        .catch((error) => _getNameToImdb(kitsuInfo.aliases[0], kitsuInfo))
        .catch((error) => _getNameToImdb(kitsuInfo.slug, kitsuInfo))
        .catch((error) => _getCinemeta(kitsuInfo.name.replace(/\W+/g, ' '), kitsuInfo.type))
        .catch((error) => kitsuInfo.aliases[0] && _getCinemeta(kitsuInfo.aliases[0].replace(/\W+/g, ' '), kitsuInfo.type))
        .catch((error) => undefined);
    const metadata = imdbId && await imdbScrapper.scrapper(imdbId)
        .then((data) => _getMetaImdb(imdbId, kitsuInfo.type)
            .then((engTitle) => ({ ...data, title: engTitle.name })))
        .catch((error) => {}) || {};
    const episodeCount = metadata.episodeCount || parseInt(metadata.episodes) || metadata.videos && metadata.videos.filter((ep) => ep.season !== 0).length;
    const startFrom = kitsuInfo.type !== 'movie' && kitsuInfo.episodeCount !== episodeCount &&
        await findStartFrom(imdbId, metadata, kitsuInfo).catch((err) => undefined);

    imdbMappings[id] = {
      imdb_id: imdbId,
      title: metadata.title || "",
      fromSeason: startFrom && startFrom.fromSeason || undefined,
      fromEpisode: startFrom && startFrom.fromEpisode || undefined,
      toValidate: (kitsuInfo.type !== 'movie' && kitsuInfo.episodeCount !== episodeCount && !!!startFrom) ||
          kitsuInfo.type === 'movie' && !(kitsuInfo.name === metadata.title ||
              kitsuInfo.aliases.includes(metadata.title ) && kitsuInfo.releaseInfo.startsWith(metadata.year))
    };

    console.log(imdbMappings[id]);

    fs.writeFile("./static/data/imdb_mapping.json", JSON.stringify(imdbMappings), 'utf8', function (err) {
      if (err) {
        console.log("An error occurred while writing JSON Object to File.");
      }
    });
  }

  console.log('Finished mapping')
}

async function popularAnime500() {
  console.log('Retrieving popular anime...');
  const popularAnime = await _getCatalogEntries('kitsu-anime-popular', 500);
  console.log(`Retrieved ${popularAnime.length} popular anime`);
  console.log('Retrieving top rated anime...');
  const topRatedAnime = await _getCatalogEntries('kitsu-anime-rating', 500);
  console.log(`Retrieved ${topRatedAnime.length} top rated  anime`);
  console.log('Retrieving trending anime...');
  const trendingAnime = await _getCatalogEntries('kitsu-anime-trending', 50);
  console.log(`Retrieved ${trendingAnime.length} trending anime`);

  const ids = [popularAnime, topRatedAnime, trendingAnime]
    .reduce((a, b) => a.concat(b), [])
    .map((meta) => parseInt(meta.id.replace('kitsu:', ''), 10));

  return [...new Set(ids)];
}

async function _getCatalogEntries(id, numberOfEntries, skip = 0) {
  return addonInterface.get('catalog', 'series', id, { skip: skip })
    .then((results) => results.metas)
    .then((metas) => skip + metas.length < numberOfEntries && metas.length >= MAX_SIZE
        ? _getCatalogEntries(id, numberOfEntries, skip + metas.length)
            .then((nextMetas) => metas.concat(nextMetas))
        : metas);
}

async function findStartFrom(imdbId, metadata, kitsuInfo) {
  const seasons = Array.from(Array(parseInt(metadata.seasons)),(val, index) => index + 1);
  const year = kitsuInfo.releaseInfo.split('-')[0];

  let found;
  while (seasons.length) {
    const season = seasons.shift();
    const episodes = await imdbScrapper.episodesPage(imdbId, season).then((result) => result.episodes);

    const releaseMatches = episodes.length >= kitsuInfo.episodeCount &&
        episodes.slice(0, kitsuInfo.episodeCount).some((ep) => ep.airDate.includes(year)) &&
        episodes.slice(kitsuInfo.episodeCount).every((ep) => ep.name.match(/ova|special/i));
    if (releaseMatches) {
      if (found) {
        // two seasons have possible matches needs manual validation
        found = undefined;
        break;
      }
      found = { fromSeason: season, fromEpisode: 1 };
    }
  }

  return found;
}

async function _getKitsu(id) {
  const url = `https://kitsu.io/api/edge/anime/${id}`;
  const query = { include: 'mappings'};
  return needle('get', url, query, { headers: { accept: 'application/vnd.api+json'} })
  .then((response) => {
    if (response.statusCode === 200 && response.body) {
      return JSON.parse(response.body);
    }
    throw new Error('No response from kitsu');
  });
}

async function _getTvdb(tvdbId) {
  if (!tvdbId) {
    return Promise.reject(new Error("no id"));
  }
  return _findImdbId(`https://www.thetvdb.com/dereferrer/series/${tvdbId}`);
}

async function _getTrakt(traktId) {
  if (!traktId) {
    return Promise.reject(new Error("no id"));
  }
  return _findImdbId(`https://trakt.tv/shows/${traktId}`);
}

async function _findImdbId(url) {
  return needle('get', url, { follow: 3 })
    .then((response) => {
      if (response.statusCode === 200 && response.body) {
        return response.body;
      }
      throw new Error('No response from tvdb');
    })
    .then((body) => body.match(/href=".*?imdb.com\/title\/(tt\d+)/)[1])
    .then((id) => {
      console.log(`found imdb: ${id}`);
      return id;
    });
}

async function _getCinemeta(title, type) {
  const url = `https://v3-cinemeta.strem.io/catalog/${type}/1/search=${title}.json`;
  return needle('get', url)
      .then((response) => {
        if (response.statusCode === 200 && response.body && response.body.metas[0].imdb_id) {
          return response.body.metas[0].imdb_id;
        }
        throw new Error('No response from kitsu');
      });
}

async function _getNameToImdb(title, kitsuInfo) {
  return new Promise((resolve, reject) => {
    nameToImdb(
        {
          name: title.replace(/\W+/g, ' ').trim().toLowerCase(),
          type: kitsuInfo.type,
          year: kitsuInfo.releaseInfo
        },
        function(err, res) {
          if (res) {
            resolve(res);
          } else {
            reject(err || new Error('failed imdbId search'));
          }
        }
    );
  });
}

async function _getMetaImdb(imdbId) {
  const url = `https://www.imdb.com/title/${imdbId}`;
  return needle('get', url, { follow: 3, headers: { 'accept-language': 'en-GB' } })
      .then((response) => {
        if (response.statusCode === 200 && response.body) {
          return response.body;
        }
        throw new Error('No response from kitsu');
      })
      .then((body) => {
        const titleMatch = body.match(/title_wrapper">\s+<h1[^>]+>(.*?)(?:&nbsp;|<\/h1>)/s);
        const epCountMatch = body.match(/bp_sub_heading">\s*?(\d+)/);
        return {
          name: titleMatch && titleMatch[1] || '',
          episodeCount: epCountMatch && parseInt(epCountMatch[1])
        };
      })
}

popularAnime500()
  .then((ids) => migrate(ids));
