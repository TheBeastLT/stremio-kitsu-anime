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

test('cacheWrapMeta counts a miss when the loader runs and a hit afterwards', async () => {
  const { cacheWrapMeta } = require('../lib/cache');
  const { register } = require('../lib/metrics');
  const count = async (result) => {
    const metric = (await register.getMetricsAsJSON()).find((m) => m.name === 'cache_requests_total');
    const entry = metric.values.find((v) => v.labels.kind === 'meta' && v.labels.result === result);
    return entry ? entry.value : 0;
  };
  const id = `counted-${process.hrtime.bigint()}`;
  const misses = await count('miss');
  const hits = await count('hit');

  await cacheWrapMeta(id, async () => ({ id }));
  assert.strictEqual(await count('miss'), misses + 1);
  assert.strictEqual(await count('hit'), hits);

  await cacheWrapMeta(id, async () => ({ id }));
  assert.strictEqual(await count('miss'), misses + 1);
  assert.strictEqual(await count('hit'), hits + 1);
});

test('persistent store operations are timed by op', async () => {
  const { register } = require('../lib/metrics');
  const store = persistentStore(new Map());
  await store.set('timed:1', 'v');
  await store.get('timed:1');
  await store.getMany(['timed:1', 'timed:2']);
  const metric = (await register.getMetricsAsJSON()).find((m) => m.name === 'cache_store_operation_duration_seconds');
  const ops = new Set(metric.values.filter((v) => v.metricName.endsWith('_count')).map((v) => v.labels.op));
  assert.ok(ops.has('set'));
  assert.ok(ops.has('get'));
  assert.ok(ops.has('mget'));
});
