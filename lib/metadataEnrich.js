const kitsuToImdbMappping = require('../static/data/imdb_mapping');
const imdbToKitsuMapping = Object.entries(kitsuToImdbMappping)
    .map(([kitsuId, value]) => ({
      kitsu_id: kitsuId,
      imdb_id: value.imdb_id,
      title: value.title,
      nonImdbEpisodes: value.nonImdbEpisodes,
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

function hasImdbMapping(imdbId) {
  return !!imdbToKitsuMapping[imdbId];
}

function getImdbMapping(kitsuId) {
  return kitsuToImdbMappping[kitsuId];
}

async function enrichKitsuMetadata(metadata, retrieveImdbMetadata) {
  const imdbInfo = kitsuToImdbMappping[metadata.kitsu_id];
  if (imdbInfo && imdbInfo.imdb_id) {
    return sanitize({
      ...metadata,
      imdb_id: imdbInfo.imdb_id,
      videos: await enrichKitsuEpisodes(metadata, imdbInfo, retrieveImdbMetadata)
    });
  }
  return metadata;
}

async function enrichKitsuEpisodes(metadata, imdbInfo, retrieveImdbMetadata) {
  if (!metadata.videos || !metadata.videos.length) {
    return metadata.videos;
  }
  const startSeason = Number.isInteger(imdbInfo.fromSeason) ? imdbInfo.fromSeason : 1;
  const startEpisode = Number.isInteger(imdbInfo.fromEpisode) ? imdbInfo.fromEpisode : 1;
  const otherImdbEntries = imdbToKitsuMapping[imdbInfo.imdb_id]
      .filter((entry) => entry.kitsu_id !== metadata.kitsu_id
          && entry.fromSeason >= startSeason
          && entry.fromEpisode >= startEpisode);
  const nextImdbEntry = otherImdbEntries && otherImdbEntries[0];
  const imdbMetadata = await retrieveImdbMetadata(imdbInfo.imdb_id, metadata.type).catch(() => undefined);
  const perSeasonEpisodeCount = imdbMetadata && imdbMetadata.videos && imdbMetadata.videos
      .filter((video) => video.episode)
      .filter((video) => (video.season === startSeason && video.episode >= startEpisode) || (video.season > startSeason
          && (!nextImdbEntry || nextImdbEntry.fromSeason > video.season)))
      .reduce(
          (counts, next) => (counts[next.season - startSeason] = counts[next.season - startSeason] + 1 || 1, counts),
          []);
  const videosMap = perSeasonEpisodeCount && imdbMetadata.videos.reduce((map, next) => (map[next.id] = next, map), {})
  let skippedEpisodes = 0;

  if (perSeasonEpisodeCount && perSeasonEpisodeCount.length) {
    let lastReleased;
    return metadata.videos
        .map(video => {
          if (imdbInfo.nonImdbEpisodes && imdbInfo.nonImdbEpisodes.includes(video.episode)) {
            skippedEpisodes++
            return video
          }
          const seasonIndex = ([...perSeasonEpisodeCount.keys()]
              .find((i) => perSeasonEpisodeCount.slice(0, i + 1)
                  .reduce((a, b) => a + b, 0) >= video.episode) + 1 || perSeasonEpisodeCount.length) - 1;
          const previousSeasonsEpisodeCount = perSeasonEpisodeCount.slice(0, seasonIndex).reduce((a, b) => a + b, 0);
          const season = startSeason + seasonIndex;
          const episode = startEpisode - 1 + video.episode - skippedEpisodes - previousSeasonsEpisodeCount;
          const imdbVideo = videosMap[`${imdbInfo.imdb_id}:${season}:${episode}`];
          const title = video.title.match(/Episode \d+/) && (imdbVideo?.title || imdbVideo?.name) || video.title;
          const thumbnail = video.thumbnail || imdbVideo?.thumbnail;
          const released = new Date(imdbVideo?.released || video.released.getTime());
          lastReleased = lastReleased?.getTime() > released.getTime() ? lastReleased : released;
          return {
            ...video,
            title,
            thumbnail,
            released: lastReleased,
            imdb_id: imdbInfo.imdb_id,
            imdbSeason: season,
            imdbEpisode: episode
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

async function enrichImdbMetadata(metadata, retrieveKitsuMetadata) {
  const kitsuEntries = imdbToKitsuMapping[metadata.id];
  if (kitsuEntries && kitsuEntries.length) {
    return sanitize({
      ...metadata,
      kitsu_id: kitsuEntries.length > 1 ? kitsuEntries.map((entry) => entry.kitsu_id) : kitsuEntries[0].kitsu_id,
      videos: await enrichImdbEpisodes(metadata, kitsuEntries, retrieveKitsuMetadata)
    });
  }
  return metadata;
}

async function enrichImdbEpisodes(metadata, kitsuEntries, retrieveKitsuMetadata) {
  if (metadata.type === 'movie') {
    return metadata.videos;
  }
  if (metadata.type === undefined || !metadata.videos || !metadata.videos.length) {
    return Promise.all(kitsuEntries.map((kitsuEntry) => retrieveKitsuMetadata(kitsuEntry.kitsu_id)
        .then((kitsuMetadata) => (kitsuMetadata.videos || [])
            .map((video) => ({
              title: video.title,
              season: kitsuEntry.fromSeason,
              episode: kitsuEntry.fromEpisode + video.episode - 1,
              kitsu_id: kitsuEntry.kitsu_id,
              kitsuEpisode: video.episode
            })))))
        .then((videos) => videos.reduce((a, b) => a.concat(b), []));
  }
  const episode = video => video.episode || video.number;
  const episodeCounter = kitsuEntries.reduce((counter, next) => (counter[next.kitsu_id] = 1, counter), {});
  return metadata.videos
      .sort((a, b) => a.season - b.season || episode(a) - episode(b))
      .map((video) => {
        const kitsuEntry = kitsuEntries.slice().reverse()
            .find((entry) => entry.fromSeason <= video.season && entry.fromEpisode <=  episode(video));
        if (!kitsuEntry) {
          return video
        }
        let kitsuEpisode = episodeCounter[kitsuEntry.kitsu_id]++
        while (kitsuEntry.nonImdbEpisodes && kitsuEntry.nonImdbEpisodes.includes(kitsuEpisode)) {
          kitsuEpisode = episodeCounter[kitsuEntry.kitsu_id]++
        }
        return {
          ...video,
          kitsu_id: kitsuEntry.kitsu_id,
          kitsuEpisode: kitsuEpisode
        };
      })
}

function sanitize(obj) {
  Object.keys(obj).forEach((key) => (obj[key] == null) && delete obj[key]);
  return obj;
}

module.exports = { enrichKitsuMetadata, enrichImdbMetadata, hasImdbMapping, getImdbMapping };