const test = require('node:test');
const assert = require('node:assert');

const { ensureCacheCollection } = require('../lib/cache_collection');

const ZSTD_OPTIONS = { storageEngine: { wiredTiger: { configString: 'block_compressor=zstd' } } };

function fakeConnect(createCollection) {
  const calls = { created: [], closed: 0 };
  const connect = async () => ({
    db: () => ({
      createCollection: async (name, options) => {
        calls.created.push([name, options]);
        return createCollection();
      }
    }),
    close: async () => { calls.closed++; }
  });
  return { connect, calls };
}

async function silenced(fn) {
  const original = console.error;
  const logged = [];
  console.error = (...args) => logged.push(args.join(' '));
  try {
    return { result: await fn(), logged };
  } finally {
    console.error = original;
  }
}

test('creates the collection with the zstd compressor', async () => {
  const { connect, calls } = fakeConnect(async () => {});
  assert.strictEqual(await ensureCacheCollection('mongodb://fake/db', 'kitsu_collection', { connect }), 'created');
  assert.deepStrictEqual(calls.created, [['kitsu_collection', ZSTD_OPTIONS]]);
  assert.strictEqual(calls.closed, 1);
});

test('reports exists when the collection is already there', async () => {
  const { connect, calls } = fakeConnect(async () => { throw Object.assign(new Error('exists'), { code: 48 }); });
  assert.strictEqual(await ensureCacheCollection('mongodb://fake/db', 'kitsu_collection', { connect }), 'exists');
  assert.strictEqual(calls.closed, 1);
});

test('reports failed and logs on any other error', async () => {
  const { connect, calls } = fakeConnect(async () => { throw new Error('boom'); });
  const { result, logged } = await silenced(() => ensureCacheCollection('mongodb://fake/db', 'kitsu_collection', { connect }));
  assert.strictEqual(result, 'failed');
  assert.ok(logged.some((line) => line.includes('boom')));
  assert.strictEqual(calls.closed, 1);
});

test('reports failed and logs when connecting exceeds the timeout', async () => {
  const connect = () => new Promise(() => {});
  const { result, logged } = await silenced(() => ensureCacheCollection('mongodb://fake/db', 'kitsu_collection', { timeoutMs: 20, connect }));
  assert.strictEqual(result, 'failed');
  assert.strictEqual(logged.length, 1);
});
