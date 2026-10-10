const axios = require('axios');
const { cacheWrapIdMapping } = require('./cache');

async function mapToKitsuId(fullId) {
  const idType = fullId.split(":")[0];
  const id = fullId.split(":")[1];
  if (idType === 'kitsu') {
    return id;
  }

  return cacheWrapIdMapping(fullId, () => queryIdMapping(idType, id));
}

async function queryIdMapping(idType, id) {
  if (Number.parseInt(id) <= 0) {
    // yuna answers invalid ids like mal:0 with a 400
    return Promise.reject(`No kitsu id found for: ${idType}:${id}`);
  }
  const yunaType = idType === 'mal' ? 'myanimelist' : idType;
  const url = `https://relations.yuna.moe/api/v2/ids?source=${yunaType}&id=${id}&include=kitsu`
  return axios.get(url, { timeout: 30000 })
      .then(response => response.data?.kitsu)
      .then(kitsuId => kitsuId
          ? Promise.resolve(kitsuId)
          : Promise.reject(`No kitsu id found for: ${idType}:${id}`))
}

module.exports = { mapToKitsuId, queryIdMapping };
