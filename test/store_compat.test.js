const test = require('node:test');
const assert = require('node:assert');
const fs = require('node:fs');
const { BSON } = require('mongodb');
const { record, nodeCache, NODE_WRITTEN, RUST_WRITTEN } = require('../crates/kitsu-addon/tests/fixtures/record_store');

const rustWritten = JSON.parse(fs.readFileSync(RUST_WRITTEN, 'utf8'));
const nodeWritten = JSON.parse(fs.readFileSync(NODE_WRITTEN, 'utf8'));
const bytes = (base64) => Buffer.from(base64, 'base64');
const typed = (document) => BSON.EJSON.stringify(BSON.deserialize(document, { promoteValues: false }).value.value, { relaxed: false });
const unreachable = async () => assert.fail('the loader ran, so the stored document was a miss');

async function readThroughNode(key, document) {
  const cache = nodeCache(new Map([[key, document]]));
  const [fromMget] = await cache.mget([key]);
  return { fromWrap: await cache.wrap(key, unreachable), fromMget };
}

test('Node reads every document Rust rewrote as it reads its own', async () => {
  const fresh = await record();
  const rewritten = new Map(rustWritten.cases.map((entry) => [entry.key, bytes(entry.bson)]));
  for (const nodeCase of fresh.cases.filter((entry) => !entry.expired)) {
    const rust = rewritten.get(nodeCase.key);
    const node = await readThroughNode(nodeCase.key, bytes(nodeCase.bson));
    assert.ok(rust, nodeCase.key);
    assert.deepStrictEqual(await readThroughNode(nodeCase.key, rust), node, nodeCase.key);
    assert.strictEqual(typed(rust), typed(bytes(nodeCase.bson)), nodeCase.key);
  }
});

test('a meta Rust built itself reads in Node as Rust serves it, with Date episodes', async () => {
  const native = rustWritten.cases.find((entry) => entry.key === 'stremio-kitsu|meta:3');
  const { fromWrap } = await readThroughNode(native.key, bytes(native.bson));
  assert.strictEqual(JSON.stringify(fromWrap), native.served);
  assert.ok(fromWrap.meta.videos[0].released instanceof Date);
});

test('the committed Node fixture holds the payloads the recorder writes now', async () => {
  const fresh = await record();
  assert.deepStrictEqual(
    nodeWritten.cases.map((entry) => [entry.key, typed(bytes(entry.bson)), entry.served]),
    fresh.cases.map((entry) => [entry.key, typed(bytes(entry.bson)), entry.served]));
});
