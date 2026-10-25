process.env.METRICS_USER = 'prom';
process.env.METRICS_PASSWORD = 'secret';

const { describe, it, before, after } = require('node:test');
const assert = require('node:assert');
const http = require('node:http');
const zlib = require('node:zlib');
const { createApp } = require('../app');

const AUTH = `Basic ${Buffer.from('prom:secret').toString('base64')}`;

function request(port, path, headers = {}) {
  return new Promise((resolve, reject) => {
    http.get({ port, path, headers }, (res) => {
      const chunks = [];
      res.on('data', (chunk) => chunks.push(chunk));
      res.on('end', () => resolve({ status: res.statusCode, headers: res.headers, raw: Buffer.concat(chunks) }));
    }).on('error', reject);
  });
}

const gunzip = (response) => zlib.gunzipSync(response.raw).toString();

describe('app', () => {
  let server;
  let port;

  before(async () => {
    server = createApp().listen(0);
    await new Promise((resolve) => server.once('listening', resolve));
    port = server.address().port;
  });

  after(() => new Promise((resolve) => server.close(resolve)));

  it('gzips the manifest when accepted', async () => {
    const response = await request(port, '/manifest.json', { 'accept-encoding': 'gzip' });
    assert.strictEqual(response.status, 200);
    assert.strictEqual(response.headers['content-encoding'], 'gzip');
    assert.strictEqual(JSON.parse(gunzip(response)).id, 'community.anime.kitsu');
  });

  it('gzips the landing page', async () => {
    const response = await request(port, '/', { 'accept-encoding': 'gzip' });
    assert.strictEqual(response.status, 200);
    assert.strictEqual(response.headers['content-encoding'], 'gzip');
    assert.match(gunzip(response), /<html/i);
  });

  it('serves a plain body without accept-encoding', async () => {
    const response = await request(port, '/manifest.json', { 'accept-encoding': 'identity' });
    assert.strictEqual(response.headers['content-encoding'], undefined);
    assert.strictEqual(JSON.parse(response.raw.toString()).id, 'community.anime.kitsu');
  });

  it('serves a small 404 body correctly', async () => {
    const response = await request(port, '/nothing/here.json', { 'accept-encoding': 'gzip' });
    assert.strictEqual(response.status, 404);
    const body = response.headers['content-encoding'] === 'gzip' ? gunzip(response) : response.raw.toString();
    assert.deepStrictEqual(JSON.parse(body), { err: 'not found' });
  });

  it('rejects metrics without credentials', async () => {
    const response = await request(port, '/metrics');
    assert.strictEqual(response.status, 401);
  });

  it('exposes request counters with credentials', async () => {
    await request(port, '/manifest.json');
    const response = await request(port, '/metrics', { authorization: AUTH });
    assert.strictEqual(response.status, 200);
    assert.match(response.raw.toString(), /http_requests_total\{route="manifest",status="200"\} \d+/);
  });
});
