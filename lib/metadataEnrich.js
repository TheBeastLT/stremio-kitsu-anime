const urlExists = require('url-exists');

const kitsuToImdbMappping = require('../static/data/imdb_mapping');
const imdbToKitsuMapping = Object.entries(kitsuToImdbMappping)
    .map(([kitsuId, value]) => ({
      kitsu_id: kitsuId,
      imdb_id: value.imdb_id,
      title: value.title,
      fromSeason: value.fromSeason === undefined ? 1 : value.fromSeason,
      fromEpisode: value.fromEpisode === undefined ? 1 : value.fromEpisode
    }))
    .filter((entry) => entry.imdb_id)
    .reduce((map, nextEntry) => {
      map[nextEntry.imdb_id] = (map[nextEntry.imdb_id] || []).concat(nextEntry)
          .sort((a, b) => {
            const seasonSort = a.fromSeason - b.fromSeason;
            if (seasonSort !== 0) {
              return seasonSort;
            }
            return a.fromEpisode - b.fromEpisode
          });
      return map;
    }, {});

async function enrichKitsuMetadata(metadata, retrieveImdbMetadata) {
  const imdbInfo = kitsuToImdbMappping[metadata.kitsu_id];
  if (imdbInfo && imdbInfo.imdb_id) {
    return sanitize({
      ...metadata,
      imdb_id: imdbInfo.imdb_id,
      background: await getBackgroundImage(imdbInfo.imdb_id, metadata.background),
      logo: await getLogoImage(imdbInfo.imdb_id),
      videos: await enrichKitsuEpisodes(metadata, imdbInfo, retrieveImdbMetadata)
    });
  }
  return metadata;
}

async function enrichKitsuEpisodes(metadata, imdbInfo, retrieveImdbMetadata) {
  if (metadata.type !== 'series' || !metadata.videos) {
    return metadata.videos;
  }
  const startSeason = imdbInfo.fromSeason || 1;
  const startEpisode = imdbInfo.fromEpisode || 1;
  const otherImdbEntries =  imdbToKitsuMapping[imdbInfo.imdb_id]
      .filter((entry) => entry.kitsu_id !== metadata.kitsu_id
          && entry.fromSeason >= startSeason
          && entry.fromEpisode >= startEpisode);
  const nextImdbEntry = otherImdbEntries && otherImdbEntries[0];
  const needsImdbMetadata = (nextImdbEntry && nextImdbEntry.fromSeason - startSeason > 1) // another sequential season doesn't exist
      || (!nextImdbEntry && metadata.videos.length > 50); // no other sequel and more than 50 episodes in series
  const imdbMetadata = needsImdbMetadata && await retrieveImdbMetadata(imdbInfo.imdb_id, metadata.type).catch(() => undefined);
  const perSeasonEpisodeCount = imdbMetadata && imdbMetadata.videos
      .filter((video) => (video.season === startSeason && video.episode >= startEpisode) || (video.season > startSeason && (!nextImdbEntry || nextImdbEntry.fromSeason > video.season)))
      .reduce((counts, next) => (counts[next.season - startSeason] = counts[next.season - startSeason] + 1 || 1, counts), []);

  if (perSeasonEpisodeCount.length) {
    return metadata.videos
        .map((video) => {
          const seasonIndex = ([...perSeasonEpisodeCount.keys()]
              .find((i) => perSeasonEpisodeCount.slice(0, i + 1)
              .reduce((a, b) => a + b) >= video.episode) + 1 || perSeasonEpisodeCount.length) - 1;
          return {
            ...video,
            imdbSeason: startSeason + seasonIndex,
            imdbEpisode: startEpisode - 1 + video.episode - perSeasonEpisodeCount.slice(0, seasonIndex).reduce((a, b) => a + b, 0)
          }
        });
  }

  return metadata.videos
      .map((video) => ({
        ...video,
        imdb_id: imdbInfo.imdb_id,
        imdbSeason: startSeason,
        imdbEpisode: startEpisode - 1 + video.episode // startEpisode is inclusive, so need -1
      }));
}

async function enrichImdbMetadata(metadata) {
  const kitsuEntries = imdbToKitsuMapping[metadata.imdb_id];
  if (kitsuEntries && kitsuEntries.length) {
    return sanitize({
      ...metadata,
      kitsu_id: kitsuEntries.length > 1 ? kitsuEntries.map((entry) => entry.kitsu_id) : kitsuEntries[0].kitsu_id,
      videos: enrichImdbEpisodes(metadata, kitsuEntries)
    });
  }
  return metadata;
}

function enrichImdbEpisodes(metadata, kitsuEntries) {
  if (metadata.type !== 'series' || !metadata.videos) {
    return metadata.videos;
  }
  const episodeCounter = kitsuEntries.reduce((counter, next) => (counter[next.kitsu_id] = 1, counter), {});
  return metadata.videos
      .map((video) => {
        const kitsuEntry = kitsuEntries.slice().reverse()
            .find((entry) => entry.fromSeason <= video.season && entry.fromEpisode <= video.episode);
        return !kitsuEntry ? video : {
          ...video,
          kitsu_id: kitsuEntry.kitsu_id,
          kitsuEpisode: episodeCounter[kitsuEntry.kitsu_id]++
        };
      })
}

async function getBackgroundImage(imdbId, coverImage) {
  return verifyImageOrFallback(coverImage, imdbId && `https://images.metahub.space/background/medium/${imdbId}/img`)
}

async function getLogoImage(imdbId) {
  return verifyImageOrFallback(undefined, imdbId && `https://images.metahub.space/logo/medium/${imdbId}/img`)
}

async function verifyImageOrFallback(kitsuImage, imdbImage) {
  return new Promise((resolve) => {
    if (!imdbImage) {
      resolve(kitsuImage);
    }
    urlExists(imdbImage, (err, exists) => {
      if (exists) {
        resolve(imdbImage)
      } else {
        resolve(kitsuImage);
      }
    })
  });
}

function sanitize(obj) {
  Object.keys(obj).forEach((key) => (obj[key] == null) && delete obj[key]);
  return obj;
}

module.exports = { enrichKitsuMetadata, enrichImdbMetadata };