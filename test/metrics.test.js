const { test } = require('node:test');
const assert = require('node:assert');
const http = require('node:http');

const metrics = require('../lib/metrics');

function basic(user, pass) {
  return `Basic ${Buffer.from(`${user}:${pass}`).toString('base64')}`;
}

function request(headers) {
  return new Promise((resolve, reject) => {
    const server = http.createServer((req, res) => metrics.metricsRouter(req, res, () => {
      res.statusCode = 418;
      res.end('next');
    }));
    server.listen(0, () => {
      const req = http.get({ port: server.address().port, path: '/metrics', headers }, (res) => {
        let body = '';
        res.on('data', (chunk) => { body += chunk; });
        res.on('end', () => {
          server.close();
          resolve({ status: res.statusCode, body });
        });
      });
      req.on('error', reject);
    });
  });
}

test('routeOf classifies catalog urls', () => {
  assert.strictEqual(metrics.routeOf('/catalog/anime/kitsu-anime-trending.json'), 'catalog');
});

test('routeOf classifies search urls', () => {
  assert.strictEqual(metrics.routeOf('/search/overlord%20S3.json'), 'search');
});

test('routeOf classifies unknown urls as other', () => {
  assert.strictEqual(metrics.routeOf('/anything/else?x=1'), 'other');
});

test('routeOf classifies malformed encoding as other', () => {
  assert.strictEqual(metrics.routeOf('/%E0%A4%A'), 'other');
});

test('routeOf classifies manifest, meta, subtitles, metrics and landing', () => {
  assert.strictEqual(metrics.routeOf('/manifest.json'), 'manifest');
  assert.strictEqual(metrics.routeOf('/meta/anime/kitsu:1.json'), 'meta');
  assert.strictEqual(metrics.routeOf('/subtitles/series/kitsu:1:1.json'), 'subtitles');
  assert.strictEqual(metrics.routeOf('/metrics'), 'metrics');
  assert.strictEqual(metrics.routeOf('/'), 'landing');
});

test('metricsRouter responds 404 when credentials are unset', async () => {
  delete process.env.METRICS_USER;
  delete process.env.METRICS_PASSWORD;
  const { status } = await request({ authorization: basic('u', 'p') });
  assert.strictEqual(status, 404);
});

test('metricsRouter responds 401 without or with wrong credentials', async () => {
  process.env.METRICS_USER = 'user';
  process.env.METRICS_PASSWORD = 'secret';
  assert.strictEqual((await request({})).status, 401);
  assert.strictEqual((await request({ authorization: basic('user', 'wrong') })).status, 401);
  assert.strictEqual((await request({ authorization: basic('other', 'secret') })).status, 401);
});

test('metricsRouter serves metrics with correct credentials', async () => {
  process.env.METRICS_USER = 'user';
  process.env.METRICS_PASSWORD = 'secret';
  const { status, body } = await request({ authorization: basic('user', 'secret') });
  assert.strictEqual(status, 200);
  assert.ok(body.includes('http_requests_total'));
});

test('metricsRouter passes other paths to next', async () => {
  process.env.METRICS_USER = 'user';
  process.env.METRICS_PASSWORD = 'secret';
  let called = false;
  metrics.metricsRouter({ path: '/manifest.json', headers: {} }, {}, () => { called = true; });
  assert.ok(called);
});

test('timedUpstream rethrows errors and records the error outcome', async () => {
  const error = new Error('e');
  await assert.rejects(metrics.timedUpstream('x', () => Promise.reject(error)), (err) => err === error);
  process.env.METRICS_USER = 'user';
  process.env.METRICS_PASSWORD = 'secret';
  const { body } = await request({ authorization: basic('user', 'secret') });
  assert.match(body, /upstream_request_duration_seconds_count\{[^}]*upstream="x"[^}]*outcome="error"[^}]*\} 1/);
});

test('timedUpstream returns the resolved value and records ok', async () => {
  assert.strictEqual(await metrics.timedUpstream('y', async () => 5), 5);
  process.env.METRICS_USER = 'user';
  process.env.METRICS_PASSWORD = 'secret';
  const { body } = await request({ authorization: basic('user', 'secret') });
  assert.match(body, /upstream_request_duration_seconds_count\{[^}]*upstream="y"[^}]*outcome="ok"[^}]*\} 1/);
});

test('counters expose cache, catalog, search and meta', () => {
  for (const name of ['cache', 'catalog', 'search', 'meta']) {
    assert.strictEqual(typeof metrics.counters[name].inc, 'function');
  }
});

test('routeOf classifies urls without a leading slash as other', () => {
  assert.strictEqual(metrics.routeOf('*'), 'other');
  assert.strictEqual(metrics.routeOf(''), 'other');
});

test('httpMetrics does not throw for an asterisk url', () => {
  const { EventEmitter } = require('node:events');
  const res = new EventEmitter();
  res.statusCode = 200;
  metrics.httpMetrics({ originalUrl: '*' }, res, () => {});
  assert.doesNotThrow(() => res.emit('finish'));
});

const { mock } = require('node:test');
const { createApp } = require('../app');
const kitsuApi = require('../lib/kitsu_api');

async function withApp(run) {
  process.env.METRICS_USER = 'user';
  process.env.METRICS_PASSWORD = 'secret';
  const server = createApp().listen(0);
  await new Promise((resolve) => server.once('listening', resolve));
  const port = server.address().port;
  const get = (path, headers = {}) => new Promise((resolve, reject) => {
    http.get({ port, path, headers }, (res) => {
      let body = '';
      res.on('data', (chunk) => { body += chunk; });
      res.on('end', () => resolve({ status: res.statusCode, body }));
    }).on('error', reject);
  });
  try {
    return await run(get);
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
}

const scrape = (get) => get('/metrics', { authorization: basic('user', 'secret') }).then(({ body }) => body);

test('catalog search counts the catalog, search kind and a found result', async () => {
  mock.method(kitsuApi, 'animeCatalogMetas', async () => []);
  try {
    const body = await withApp(async (get) => {
      await get('/catalog/anime/kitsu-anime-list/search=overlord%20S3.json');
      return scrape(get);
    });
    assert.match(body, /catalog_requests_total\{[^}]*catalog="kitsu-anime-list"[^}]*kind="search"[^}]*\} [1-9]/);
    assert.match(body, /search_requests_total\{result="found"\} [1-9]/);
  } finally {
    mock.restoreAll();
  }
});

test('catalog search without a match counts an empty result', async () => {
  mock.method(kitsuApi, 'animeCatalogMetas', async () => []);
  try {
    const body = await withApp(async (get) => {
      await get('/catalog/anime/kitsu-anime-list/search=hello%20carbot%202018%20S6.json');
      return scrape(get);
    });
    assert.match(body, /search_requests_total\{result="empty"\} [1-9]/);
  } finally {
    mock.restoreAll();
  }
});

test('catalog browse counts the browse kind', async () => {
  mock.method(kitsuApi, 'animeEntries', async () => []);
  try {
    const body = await withApp(async (get) => {
      await get('/catalog/anime/kitsu-anime-popular.json');
      return scrape(get);
    });
    assert.match(body, /catalog_requests_total\{[^}]*catalog="kitsu-anime-popular"[^}]*kind="browse"[^}]*\} [1-9]/);
  } finally {
    mock.restoreAll();
  }
});

test('meta for an unmapped imdb id counts imdb not_found and stays a 404', async () => {
  const { status, body } = await withApp(async (get) => {
    const response = await get('/meta/anime/tt0000000.json');
    return { status: response.status, body: await scrape(get) };
  });
  assert.strictEqual(status, 404);
  assert.match(body, /meta_requests_total\{[^}]*id_type="imdb"[^}]*outcome="not_found"[^}]*\} [1-9]/);
});

test('timedUpstream wrapping a stub marks the kitsu upstream ok', async () => {
  const stub = async () => ({ data: [] });
  assert.deepStrictEqual(await metrics.timedUpstream('kitsu', stub), { data: [] });
  const body = await withApp(scrape);
  assert.match(body, /upstream_request_duration_seconds_count\{[^}]*upstream="kitsu"[^}]*outcome="ok"[^}]*\} [1-9]/);
});

test('search route counts found and empty results', async () => {
  const body = await withApp(async (get) => {
    await get(`/search/${encodeURIComponent('overlord S3')}.json`);
    await get(`/search/${encodeURIComponent('hello carbot 2018 S6')}.json`);
    return scrape(get);
  });
  assert.match(body, /search_requests_total\{result="found"\} [1-9]/);
  assert.match(body, /search_requests_total\{result="empty"\} [1-9]/);
});

test('search route counts nothing for a malformed query', async () => {
  const read = (body, result) => Number(body.match(new RegExp(`search_requests_total\\{result="${result}"\\} (\\d+)`))?.[1] || 0);
  const { before, after, status } = await withApp(async (get) => {
    const before = await scrape(get);
    const { status } = await get('/search/%E0%A4%A.json');
    return { before, after: await scrape(get), status };
  });
  assert.strictEqual(status, 400);
  assert.strictEqual(read(after, 'found'), read(before, 'found'));
  assert.strictEqual(read(after, 'empty'), read(before, 'empty'));
});

test('unknown catalog ids are labelled unknown', async () => {
  mock.method(kitsuApi, 'animeEntries', async () => []);
  try {
    const body = await withApp(async (get) => {
      await get('/catalog/anime/not-a-real-catalog.json');
      return scrape(get);
    });
    assert.match(body, /catalog_requests_total\{[^}]*catalog="unknown"[^}]*\} [1-9]/);
    assert.doesNotMatch(body, /catalog="not-a-real-catalog"/);
  } finally {
    mock.restoreAll();
  }
});
