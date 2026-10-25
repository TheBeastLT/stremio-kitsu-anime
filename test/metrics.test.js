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
