const test = require('node:test');
const assert = require('node:assert');
const http = require('node:http');

const serverless = require('../serverless');

async function get(path) {
  const server = http.createServer(serverless);
  await new Promise(resolve => server.listen(0, resolve));
  try {
    const response = await fetch(`http://localhost:${server.address().port}${path}`);
    return { status: response.status, headers: response.headers, body: response.status === 200 ? await response.json() : undefined };
  } finally {
    server.close();
  }
}

test('/search/{query}.json returns the best matching kitsu ids without metas', async () => {
  const { status, headers, body } = await get(`/search/${encodeURIComponent('overlord S3')}.json`);
  assert.strictEqual(status, 200);
  assert.strictEqual(body.ids[0], '41174');
  assert.ok(body.ids.length <= 20);
  assert.match(headers.get('cache-control'), /max-age=\d+/);
});

test('/search/{query}.json is empty when no title matches well enough', async () => {
  const { body } = await get(`/search/${encodeURIComponent('hello carbot 2018 S6')}.json`);
  assert.deepStrictEqual(body, { ids: [] });
});

test('/search/{query}.json reads a query with an encoded percent sign once', async () => {
  const { status, body } = await get(`/search/${encodeURIComponent('100% pascal-sensei')}.json`);
  assert.strictEqual(status, 200);
  assert.ok(Array.isArray(body.ids));
});

test('/search/{query}.json rejects a malformed query', async () => {
  const { status } = await get('/search/%E0%A4%A.json');
  assert.strictEqual(status, 400);
});
