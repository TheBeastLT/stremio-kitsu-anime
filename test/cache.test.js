const test = require('node:test');
const assert = require('node:assert');

const { createCache } = require('cache-manager');
const { createKeyv } = require('cacheable');
const { persistentStore } = require('../lib/cache');

// The production stack with a Map standing in for Mongo: memory tier first, the persistent store second.
function twoTierCache(persistent) {
  return createCache({ stores: [createKeyv({ lruSize: 10 }), persistentStore(persistent)], ttl: 60000 });
}

test('wrap builds a value once and serves it from the persistent store after a restart', async () => {
  const persistent = new Map();
  let builds = 0;
  const build = async () => ({ id: 'kitsu:1', builds: ++builds });

  assert.deepStrictEqual(await twoTierCache(persistent).wrap('meta:1', build), { id: 'kitsu:1', builds: 1 });
  // a fresh memory tier, like a restarted container: the value has to come back from the persistent store
  const restarted = twoTierCache(persistent);
  assert.deepStrictEqual(await restarted.wrap('meta:1', build), { id: 'kitsu:1', builds: 1 });
  assert.strictEqual(builds, 1);
});

test('mget reads cached values from the persistent store and leaves misses undefined', async () => {
  const persistent = new Map();
  await twoTierCache(persistent).wrap('meta:1', async () => ({ id: 'kitsu:1' }));

  assert.deepStrictEqual(await twoTierCache(persistent).mget(['meta:1', 'meta:2']), [{ id: 'kitsu:1' }, undefined]);
});
