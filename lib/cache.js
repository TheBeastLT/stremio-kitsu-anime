const { Keyv } = require('keyv');
const { KeyvMongo } = require('@keyv/mongo');
const { createCache } = require('cache-manager');

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

const MONGO_URI = process.env.MONGODB_URI;
const NO_CACHE = process.env.NO_CACHE || false;

const cache = initiateCache();

function initiateCache() {
  if (NO_CACHE) {
    return null;
  } else if (MONGO_URI) {
    const store = new Keyv({
      store: new KeyvMongo(MONGO_URI, { collection: 'kitsu_collection', socketTimeoutMS: 120000 }),
      useKeyPrefix: false,
      serialize: undefined,
      deserialize: undefined,
    });
    // Keyv throws errors nobody listens to; a failing Mongo should only skip the cache.
    store.on('error', (error) => console.warn(`Cache error: ${error.message}`));
    return createCache({ stores: [store], ttl: META_TTL });
  } else {
    return createCache({ ttl: META_TTL });
  }
}

function cacheWrap(key, method, ttl) {
  if (NO_CACHE || !cache) {
    return method();
  }
  return cache.wrap(key, method, ttl);
}

function cacheWrapCatalog(id, method) {
  return cacheWrap(`${CATALOG_KEY_PREFIX}:${id}`, method, CATALOG_TTL);
}

function cacheWrapMeta(id, method) {
  return cacheWrap(`${META_KEY_PREFIX}:${id}`, method, META_TTL);
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

module.exports = { cacheWrapCatalog, cacheWrapMeta, cacheWrapImages, cacheWrapIdMapping };

