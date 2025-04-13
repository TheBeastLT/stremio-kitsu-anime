const cacheManager = require('cache-manager');
const mangodbStore = require('cache-manager-mongodb');

const GLOBAL_KEY_PREFIX = 'stremio-kitsu';
const META_KEY_PREFIX = `${GLOBAL_KEY_PREFIX}|meta`;
const CATALOG_KEY_PREFIX = `${GLOBAL_KEY_PREFIX}|catalog`;
const IMAGES_KEY_PREFIX = `${GLOBAL_KEY_PREFIX}|images`;
const ID_MAPPING_KEY_PREFIX = `${GLOBAL_KEY_PREFIX}|id_mapping`;

const META_TTL = process.env.META_TTL || 24 * 60 * 60; // 1 day
const CATALOG_TTL = process.env.CATALOG_TTL || 24 * 60 * 60; // 1 day
const IMAGES_TTL = 14 * 24 * 60 * 60; // 14 days
const IMAGES_NON_EN_TTL = 4 * 24 * 60 * 60; // 4 days
const IMAGES_NULL_TTL = 2 * 24 * 60 * 60; // 2 days

const MONGO_URI = process.env.MONGODB_URI;
const NO_CACHE = process.env.NO_CACHE || false;

const cache = initiateCache();

function initiateCache() {
  if (NO_CACHE) {
    return null;
  } else if (!NO_CACHE && MONGO_URI) {
    return cacheManager.caching({
      store: mangodbStore,
      uri: MONGO_URI,
      options: {
        collection: 'kitsu_collection',
        socketTimeoutMS: 120000,
        useNewUrlParser: true,
        useUnifiedTopology: false,
        autoReconnect: true,
        poolSize : 20,
        ttl: META_TTL
      },
      ttl: META_TTL,
      ignoreCacheErrors: true
    });
  } else {
    return cacheManager.caching({
      store: 'memory',
      ttl: META_TTL
    });
  }
}

function cacheWrap(key, method, options) {
  if (NO_CACHE || !cache) {
    return method();
  }
  return cache.wrap(key, method, options);
}

function cacheWrapCatalog(id, method) {
  return cacheWrap(`${CATALOG_KEY_PREFIX}:${id}`, method, { ttl: CATALOG_TTL });
}

function cacheWrapMeta(id, method) {
  return cacheWrap(`${META_KEY_PREFIX}:${id}`, method, { ttl: META_TTL });
}

function cacheWrapImages(id, method) {
  const ttl = (images) => {
    if (images.logoLang === 'en') return IMAGES_TTL;
    if (images.logoLang || images.logo) return IMAGES_NON_EN_TTL;
    return IMAGES_NULL_TTL;
  }
  return cacheWrap(`${IMAGES_KEY_PREFIX}:${id}`, method, { ttl });
}

function cacheWrapIdMapping(id, method) {
  return cacheWrap(`${ID_MAPPING_KEY_PREFIX}:${id}`, method, { ttl: IMAGES_TTL });
}

module.exports = { cacheWrapCatalog, cacheWrapMeta, cacheWrapImages, cacheWrapIdMapping };

