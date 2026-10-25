// Responses of the Node app (app.js) to a fixed request sequence, for tests/http_parity.rs.
//   TZ=UTC FANART_APIKEY=test node crates/kitsu-addon/tests/fixtures/record_http.js
// Offline: Kitsu, Cinemeta and yuna answer from the meta/ recordings (every answer is saved next to the exchanges, so
// the Rust run gets the same inputs), the mapping and title records are the meta/ snapshots, fanart is synthetic and
// the cache is memory-only.
const fs = require('fs');
const http = require('http');
const path = require('path');
const zlib = require('zlib');

delete process.env.MONGODB_URI;
delete process.env.NO_CACHE;
process.env.METRICS_USER = 'prom';
process.env.METRICS_PASSWORD = 'secret';

const ROOT = path.join(__dirname, '../../../..');
const META = path.join(__dirname, 'meta');
const KITSU = path.join(__dirname, 'kitsu');
const OUTPUT = path.join(__dirname, 'http/exchanges.json.zst');
const AUTH = `Basic ${Buffer.from('prom:secret').toString('base64')}`;
const NOT_FOUND = (id) => JSON.stringify({
  errors: [{ title: 'Record not found', detail: `The record identified by ${id} could not be found.`, code: '404',
    status: '404' }],
});
const BAD_REQUEST = JSON.stringify({ errors: [{ title: 'Invalid field value', status: '400' }] });
const YUNA = {
  'https://relations.yuna.moe/api/v2/ids?source=myanimelist&id=1&include=kitsu': { status: 200, body: '{"kitsu":1,"myanimelist":1}' },
  'https://relations.yuna.moe/api/v2/ids?source=myanimelist&id=7&include=kitsu': { status: 200, body: '{"kitsu":null}' },
  'https://relations.yuna.moe/api/v2/ids?source=anilist&id=5&include=kitsu': { status: 500, body: 'upstream down' },
};

const REQUESTS = [
  ['GET', '/manifest.json'],
  ['GET', '/manifest.json', { 'accept-encoding': 'gzip' }],
  ['HEAD', '/manifest.json'],
  ['GET', '/MANIFEST.JSON'],
  ['GET', '/manifest.json/'],
  ['OPTIONS', '/manifest.json', { origin: 'https://web.stremio.com', 'access-control-request-method': 'GET',
    'access-control-request-headers': 'x-custom' }],
  ['OPTIONS', '/catalog/anime/kitsu-anime-trending.json'],
  ['GET', '/'],
  ['GET', '/', { 'accept-encoding': 'gzip' }],
  ['GET', '/configure'],
  ['GET', '/catalog/anime/kitsu-anime-trending.json'],
  ['GET', '/catalog/anime/kitsu-anime-trending.json?1700000000', { 'accept-encoding': 'gzip' }],
  ['GET', '/catalog/anime/kitsu-anime-airing/genre=Action&skip=20.json'],
  ['GET', '/catalog/anime/kitsu-anime-airing/skip=20&genre=Action.json'],
  ['GET', '/catalog/anime/kitsu-anime-airing/skip=abc.json'],
  ['GET', '/catalog/anime/kitsu-anime-popular/genre=Sci%20Fi&unknown=1.json'],
  ['GET', '/catalog/anime/kitsu-anime-list/search=.json'],
  ['GET', '/catalog/anime/kitsu-anime-list/search=cowboy%20bebop.json'],
  ['GET', '/catalog/anime/kitsu-anime-list/search=cowboy+bebop&skip=0.json'],
  ['GET', '/catalog/anime/kitsu-anime-list/search=tt0388629.json'],
  ['GET', '/catalog/anime/kitsu-anime-list/search=zzzzqqqq.json'],
  ['GET', '/catalog/anime/kitsu-anime-list/search=https%3A%2F%2Fexample.com.json'],
  ['GET', '/catalog/anime/kitsu-anime-list/search=a&search=b.json'],
  ['GET', '/catalog/anime/kitsu-anime-list/search=%E0%A4%A.json'],
  ['GET', '/catalog/anime/kitsu-anime-unknown.json'],
  ['GET', '/catalog/anime/kitsu-anime-unknown/search=one.json'],
  ['GET', '/meta/anime/kitsu:1.json'],
  ['GET', '/meta/anime/kitsu:1.json?1700000000', { 'accept-encoding': 'gzip' }],
  ['GET', '/meta/anime/kitsu%3A7442.json'],
  ['GET', '/meta/series/tt0213338.json'],
  ['GET', '/meta/series/tt0944947.json'],
  ['GET', '/meta/anime/foo.json'],
  ['GET', '/meta/anime/kitsu:99999999.json'],
  ['GET', '/meta/anime/mal:1.json'],
  ['GET', '/meta/anime/mal:0.json'],
  ['GET', '/meta/anime/mal:7.json'],
  ['GET', '/meta/anime/anilist:5.json'],
  ['GET', '/meta/anime/kitsu:1/extra=1.json'],
  ['GET', '/catalog/series/kitsu-anime-last-videos/lastVideosIds=kitsu%3A1%2Cmal%3A1%2Ckitsu%3A7442%2Ckitsu%3A99999999.json'],
  ['GET', '/catalog/series/kitsu-anime-last-videos/lastVideosIds=kitsu:2,kitsu:25.json'],
  ['GET', '/catalog/series/kitsu-anime-last-videos/lastVideosIds=kitsu:1&lastVideosIds=kitsu:2.json'],
  ['GET', `/catalog/anime/kitsu-anime-list/lastVideosIds=${[...Array.from({ length: 19 }, (_, i) => `kitsu:${i + 100}`),
    'kitsu:2', 'kitsu:25', 'kitsu:1'].join(',')}.json`],
  ['GET', '/subtitles/anime/kitsu:1:3/filename=Cowboy%20Bebop%2003.mkv&videoSize=123.json'],
  ['GET', '/subtitles/anime/kitsu:1:3/filename=a%20b.mkv.json', { accept: 'text/html' }],
  ['GET', '/subtitles/anime/kitsu:1:3.json', { accept: 'application/json' }],
  ['GET', '/subtitles/anime/kitsu:1.json'],
  ['HEAD', '/subtitles/anime/kitsu:1.json'],
  ['GET', '/subtitles/anime/kitsu:99999999.json'],
  ['GET', '/subtitles/anime/kitsu:1:999.json'],
  ['GET', '/subtitles/anime/foo.json'],
  ['GET', '/search/cowboy%20bebop.json'],
  ['GET', '/search/tt0388629.json'],
  ['GET', '/search/%20TT0213338%20.json'],
  ['GET', '/search/zzzzqqqq.json'],
  ['GET', '/search/%E0%A4%A.json'],
  ['GET', '/search/.json'],
  ['GET', '/search/a/b.json'],
  ['GET', '/nothing/here.json'],
  ['GET', '/nothing/here.json', { 'accept-encoding': 'gzip' }],
  ['GET', '/stream/movie/tt1.json'],
  ['POST', '/manifest.json'],
  ['GET', '/%E0'],
  ['GET', '/meta/anime/%E0.json'],
  ['GET', '/metrics'],
  ['GET', '/metrics', { authorization: 'Basic cHJvbTp3cm9uZw==' }],
  ['GET', '/metrics/'],
];

const QUERY_STRINGS = [
  '', 'search=foo%20bar', 'search=a+b', 'a=1&a=2&a=3', '=x', 'x', '&&a=1&', 'a=%E0%A4%A', 'a=%zz', 'a=1=2',
  'skip=abc', 'genre=Sci%2DFi', '%61=b', 'a=%F0%9F%98%80', 'a%3Db=c', 'a=b&A=c', '1=a&b=2&0=c', 'a=%', 'a=%2', '=',
  'a=', '=&=', 'a+b=c+d', 'a=b%20+c', '%=1', 'a&b', 'a=1&a=', '%41%42=%43', 'k%zz%41=v', 'a=%41%zz', 'search=%25',
  'search=100%25%20pascal', 'lastVideosIds=kitsu%3A1%2Ckitsu%3A2', 'a=%ED%A0%80', 'a=%C0%AF', 'a=%+1',
  'genre=Slice%20of%20Life&skip=40', 'filename=My.Show.S01E01.mkv&videoSize=1234&videoHash=abc', 'a=b&&&c',
  'é=ü%C3%A9', Array.from({ length: 1005 }, (_, i) => `k${i}=v`).join('&'),
];

const unzstd = (file) => zlib.zstdDecompressSync(fs.readFileSync(file)).toString('utf8');
const zstd = (data) => zlib.zstdCompressSync(Buffer.from(data), {
  params: { [zlib.constants.ZSTD_c_compressionLevel]: 19 },
});

function stubModule(file, exports) {
  const filename = require.resolve(file);
  require.cache[filename] = { id: filename, filename, loaded: true, exports };
}

function images(kitsuId) {
  const base = `https://images.test/${kitsuId}`;
  switch (Number(kitsuId) % 4) {
    case 0:
      return Promise.resolve({ logo: `${base}/logo.png`, logoLang: 'en', poster: `${base}/poster.jpg`,
        background: `${base}/background.jpg` });
    case 1:
      return Promise.resolve({});
    case 2:
      return Promise.resolve({ background: `${base}/background.jpg` });
    default:
      return Promise.reject(new Error('fanart failed'));
  }
}

function catalogMetasBody(responses, ids) {
  const documents = ids.map((id) => responses.kitsu[id]).filter(Boolean).map((body) => JSON.parse(body));
  const genres = new Map();
  documents.flatMap((document) => document.included || []).filter((resource) => resource.type === 'genres')
      .forEach((resource) => genres.set(resource.id, resource));
  const data = documents.map((document) => ({ ...document.data, relationships: Object.fromEntries(
      Object.entries(document.data.relationships).map(([name, relationship]) => [name,
        name === 'genres' ? relationship : { links: relationship.links }])) }));
  return JSON.stringify({ data, included: [...genres.values()], meta: { count: data.length } });
}

function kitsuAnswer(responses, config) {
  const id = config.url.match(/^anime\/(\d+)$/)?.[1];
  if (id) {
    return responses.kitsu[id] ? { status: 200, body: responses.kitsu[id] } : { status: 404, body: NOT_FOUND(id) };
  }
  if (config.url === 'anime' && config.params.filter?.id) {
    return { status: 200, body: catalogMetasBody(responses, config.params.filter.id.split(',')) };
  }
  if (config.url === 'anime') {
    return config.params.page?.offset === 'abc'
        ? { status: 400, body: BAD_REQUEST }
        : { status: 200, body: fs.readFileSync(path.join(KITSU, 'entries_browse.json'), 'utf8') };
  }
  if (config.url === 'trending/anime') {
    return { status: 200, body: fs.readFileSync(path.join(KITSU, 'entries_trending.json'), 'utf8') };
  }
  return { status: 404, body: NOT_FOUND(config.url) };
}

function request(port, [method, target, headers = {}]) {
  return new Promise((resolve, reject) => {
    const req = http.request({ port, method, path: target, headers }, (res) => {
      const chunks = [];
      res.on('data', (chunk) => chunks.push(chunk));
      res.on('end', () => {
        const raw = Buffer.concat(chunks);
        const body = res.headers['content-encoding'] === 'gzip' ? zlib.gunzipSync(raw) : raw;
        const { date, ...stable } = res.headers;
        resolve({ method, path: target, requestHeaders: headers, status: res.statusCode, headers: stable,
          body: body.toString('utf8') });
      });
    });
    req.on('error', reject);
    req.end();
  });
}

async function main() {
  const responses = JSON.parse(unzstd(path.join(META, 'responses.json.zst')));
  const rows = JSON.parse(fs.readFileSync(path.join(META, 'mapping_rows.json'), 'utf8'));
  const records = fs.readFileSync(path.join(META, 'records.jsonl'), 'utf8').split('\n').filter(Boolean)
      .map((line) => JSON.parse(line));
  const upstream = { kitsu: {}, http: {} };

  stubModule(path.join(ROOT, 'static/data/imdb_mapping.json'), rows);
  const titleRecords = require(path.join(ROOT, 'lib/title_records'));
  stubModule(path.join(ROOT, 'lib/title_records'), {
    ...titleRecords, readRecords: () => records, withAliases: (list) => titleRecords.withAliases(list, {}),
  });
  stubModule(path.join(ROOT, 'lib/fanart'), { getImages: images });

  const axiosDir = path.join(path.dirname(require.resolve('kitsu')), '..', 'node_modules', 'axios');
  const buildURL = require(path.join(axiosDir, 'lib/helpers/buildURL'));
  const buildFullPath = require(path.join(axiosDir, 'lib/core/buildFullPath'));
  const settle = require(path.join(axiosDir, 'lib/core/settle'));
  require(axiosDir).defaults.adapter = (config) => new Promise((resolve, reject) => {
    const url = buildURL(buildFullPath(config.baseURL, config.url), config.params, config.paramsSerializer);
    const answer = kitsuAnswer(responses, config);
    upstream.kitsu[url] = answer;
    settle(resolve, reject, { data: answer.body, status: answer.status, statusText: `${answer.status}`, headers: {},
      config, request: {} });
  });

  const axios = require('axios');
  axios.get = async (url) => {
    const answer = YUNA[url] || responses.cinemeta[url] || { status: 404, body: 'Not Found' };
    upstream.http[url] = answer;
    if (answer.status < 200 || answer.status >= 300) {
      throw new Error(`Request failed with status code ${answer.status}`);
    }
    return { status: answer.status, data: JSON.parse(answer.body) };
  };
  console.warn = () => {};
  console.error = () => {};
  console.log = () => {};

  const { createApp } = require(path.join(ROOT, 'app'));
  const server = createApp().listen(0);
  await new Promise((resolve) => server.once('listening', resolve));
  const { port } = server.address();
  const exchanges = [];
  for (const entry of REQUESTS) {
    exchanges.push(await request(port, entry));
  }
  const metrics = await request(port, ['GET', '/metrics', { authorization: AUTH }]);
  server.close();
  const counters = metrics.body.split('\n')
      .filter((line) => /^kitsu_(http_requests_total|cache_requests_total|catalog_requests_total|search_requests_total|meta_requests_total|upstream_request_duration_seconds_count)\{/.test(line));
  fs.mkdirSync(path.dirname(OUTPUT), { recursive: true });
  const querystring = QUERY_STRINGS.map((query) => [query, Object.entries(require('querystring').parse(query))]);
  fs.writeFileSync(OUTPUT, zstd(JSON.stringify({ upstream, exchanges, counters, querystring }, null, 1)));
  process.stdout.write(`${exchanges.length} exchanges, ${Object.keys(upstream.kitsu).length} kitsu and `
      + `${Object.keys(upstream.http).length} http answers, ${counters.length} counter lines\n`);
}

main().then(() => process.exit(0), (error) => {
  process.stderr.write(`${error.stack || error}\n`);
  process.exit(1);
});
