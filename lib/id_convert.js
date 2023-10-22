const axios = require('axios');
const { cacheWrapIdMapping } = require('./cache');

const ID_MAPPING_URL = 'https://1fe84bc728af-stremio-anime-catalogs.baby-beamup.club/map.json';
const VALID_FOR = 12 * 60 * 60 * 1000; // 12 hours
let fetchedAt;
let mapping = {}

async function mapToKitsuId(fullId) {
  const idType = fullId.split(":")[0];
  const id = fullId.split(":")[1];
  if (idType === 'kitsu') {
    return id;
  }
  if (!fetchedAt || new Date() - fetchedAt > VALID_FOR) {
    mapping = await getMappingJson();
    fetchedAt = new Date()
    console.log(`Refreshed id mapping list at ${fetchedAt.toISOString()}`)
  }
  if (mapping[idType] && mapping[idType][id]) {
    return mapping[idType][id];
  }
  return cacheWrapIdMapping(fullId, () => queryIdMapping(idType, id))
}

async function queryIdMapping(idType, id) {
  const yunaType = idType === 'mal' ? 'myanimelist' : idType;
  const url = `https://relations.yuna.moe/api/ids?source=${yunaType}&id=${id}`
  return axios.get(url, { timeout: 30000 })
      .then(response => response.data && response.data.kitsu)
      .then(kitsuId => kitsuId
          ? Promise.resolve(kitsuId)
          : Promise.reject(`No kitsu id found for: ${idType}:${id}`))
}

async function getMappingJson(retry = 2) {
  return axios.get(ID_MAPPING_URL, { timeout: 30000 })
      .then(response => response.data)
      .catch(error => {
        if (retry === 0) {
          console.log(`Failed retrieving id mapping list: ${error.message}`);
          throw error;
        }
        return getMappingJson(retry - 1);
      });
}

module.exports = { mapToKitsuId, queryIdMapping };