const { Keyv } = require('keyv');
const { KeyvMongo } = require('@keyv/mongo');
const { createCache } = require('cache-manager');
const { createKeyv } = require('cacheable');
const { counters, cacheStoreDuration } = require('./metrics');

const GLOBAL_KEY_PREFIX = 'stremio-kitsu';
const META_KEY_PREFIX = `${GLOBAL_KEY_PREFIX}|meta`;
const CATALOG_KEY_PREFIX = `${GLOBAL_KEY_PREFIX}|catalog`;
const IMAGES_KEY_PREFIX = `${GLOBAL_KEY_PREFIX}|images`;
const ID_MAPPING_KEY_PREFIX = `${GLOBAL_KEY_PREFIX}|id_mapping`;

const META_TTL = process.env.META_TTL || 24 * 60 * 60 * 1000; // 1 day
const CATALOG_TTL = process.env.CATALOG_TTL || 24 * 60 * 60 * 1000; // 1 day
const IMAGES_TTL = 14 * 24 * 60 * 60 * 1000; // 14 days
const IMAGES_NON_EN_TTL = 4 * 24 * 60 * 60 * 1000; // 4 days
const IMAGES_NULL_TTL = 2 * 24 * 60 * 60 * 1000; // 2 days

const MEMORY_CACHE_SIZE = parseInt(process.env.MEMORY_CACHE_SIZE) || 2000;
const MONGO_URI = process.env.MONGODB_URI;
const NO_CACHE = process.env.NO_CACHE || false;

const cache = initiateCache();

function initiateCache() {
  // The most recently used entries also stay in memory, as Mongo is slow to send big metas (~100KB/s).
  const memory = createKeyv({ lruSize: MEMORY_CACHE_SIZE });
  if (NO_CACHE) {
    return null;
  } else if (MONGO_URI) {
    const store = persistentStore(new KeyvMongo(MONGO_URI, { collection: 'kitsu_collection', socketTimeoutMS: 120000 }));
    return createCache({ stores: [memory, store], ttl: META_TTL });
  } else {
    return createCache({ stores: [memory], ttl: META_TTL });
  }
}

// The Keyv wrapping the persistent (Mongo) store; cache-manager 7 reads it raw, which only Keyv 5 supports.
function persistentStore(adapter) {
  const store = new Keyv({ store: adapter, useKeyPrefix: false, serialize: undefined, deserialize: undefined });
  // Keyv throws errors nobody listens to; a failing Mongo should only skip the cache.
  store.on('error', (error) => console.warn(`Cache error: ${error.message}`));
  timeStoreOperation(store, 'get', 'get');
  timeStoreOperation(store, 'set', 'set');
  timeStoreOperation(store, 'getMany', 'mget');
  return store;
}

function timeStoreOperation(store, method, op) {
  const original = store[method].bind(store);
  store[method] = async (...args) => {
    const end = cacheStoreDuration.startTimer({ op });
    try {
      return await original(...args);
    } finally {
      end();
    }
  };
}

function kindOf(key) {
  if (key.startsWith(META_KEY_PREFIX)) return 'meta';
  if (key.startsWith(CATALOG_KEY_PREFIX)) return 'catalog';
  if (key.startsWith(IMAGES_KEY_PREFIX)) return 'images';
  if (key.startsWith(ID_MAPPING_KEY_PREFIX)) return 'id_mapping';
  return 'other';
}

function cacheWrap(key, method, ttl) {
  if (NO_CACHE || !cache) {
    return method();
  }
  let loaded = false;
  const counted = () => {
    loaded = true;
    return method();
  };
  const result = cache.wrap(key, counted, ttl);
  return result.then((value) => {
    counters.cache.inc({ kind: kindOf(key), result: loaded ? 'miss' : 'hit' });
    return value;
  });
}

function cacheWrapCatalog(id, method) {
  return cacheWrap(`${CATALOG_KEY_PREFIX}:${id}`, method, CATALOG_TTL);
}

function cacheWrapMeta(id, method) {
  return cacheWrap(`${META_KEY_PREFIX}:${id}`, method, META_TTL);
}

// Cached metas of many kitsu ids in one store query, undefined for the missing ones.
async function getCachedMetas(kitsuIds) {
  if (NO_CACHE || !cache) {
    return [];
  }
  return cache.mget(kitsuIds.map((kitsuId) => `${META_KEY_PREFIX}:${kitsuId}`));
}

function cacheWrapImages(id, method) {
  const ttl = (images) => {
    if (images.logoLang === 'en') return IMAGES_TTL;
    if (images.logoLang || images.logo) return IMAGES_NON_EN_TTL;
    return IMAGES_NULL_TTL;
  }
  return cacheWrap(`${IMAGES_KEY_PREFIX}:${id}`, method, ttl);
}

function cacheWrapIdMapping(id, method) {
  return cacheWrap(`${ID_MAPPING_KEY_PREFIX}:${id}`, method, IMAGES_TTL);
}

module.exports = { cacheWrapCatalog, cacheWrapMeta, getCachedMetas, cacheWrapImages, cacheWrapIdMapping, persistentStore };

