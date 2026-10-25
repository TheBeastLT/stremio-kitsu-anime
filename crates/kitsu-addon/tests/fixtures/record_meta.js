// Goldens of lib/metadata.js and lib/metadataEnrich.js for tests/meta_parity.rs.
//   TZ=UTC FANART_APIKEY=test NO_CACHE=true node crates/kitsu-addon/tests/fixtures/record_meta.js [--fetch]
// --fetch records the Kitsu and Cinemeta responses (1 request per second) and snapshots the mapping rows and title
// records of the cases into meta/; without it the goldens are rebuilt from those files, with no network.
const fs = require('fs');
const path = require('path');
const zlib = require('zlib');

const ROOT = path.join(__dirname, '../../../..');
const DIR = path.join(__dirname, 'meta');
const RESPONSES = path.join(DIR, 'responses.json.zst');
const ROWS = path.join(DIR, 'mapping_rows.json');
const RECORDS = path.join(DIR, 'records.jsonl');
const GOLDENS = path.join(DIR, 'goldens.json.zst');
const MAX_RESPONSE = 1024 * 1024;

const KITSU_IDS = [
  12, 45690, 46215, 48291,
  7442, 8671, 13569, 41982, 42422, 44240, 46038, 47132, 7838, 7889, 7694, 8674, 13718,
  72, 2057, 2080, 765,
  11, 1555, 7975,
  73, 1092,
  7929, 11013,
  46150,
  2, 25,
  1,
  46474, 49240, 48105,
  43098, 46521,
  1621, 6708, 6709,
  1191, 8124,
  49444,
  497, 563,
  11491,
];
const RELATIONS_IDS = [7442, 12, 2];
const FAILING_CINEMETA = ['tt0374407'];
const UNMAPPED_IMDB = ['tt0111161'];
const MOVIE_IMDB = ['tt0275277', 'tt0113568', 'tt0111161'];

// Synthetic kitsu metas (as the node tests build them) and Cinemeta-like metas for the rules no recorded case reaches
const cinemetaVideos = (imdbId, seasons) => seasons.flatMap(([season, count]) => [...Array(count).keys()]
    .map((index) => ({ id: `${imdbId}:${season}:${index + 1}`, season, episode: index + 1,
      released: '2020-01-01T00:00:00.000Z' })));
const kitsuMeta = (kitsuId, episodeCount) => ({
  kitsu_id: kitsuId, type: 'series', genres: ['Action'], links: [],
  videos: [...Array(episodeCount).keys()].map((index) => ({
    id: `kitsu:${kitsuId}:${index + 1}`, title: `Episode ${index + 1}`, season: 1, episode: index + 1,
    released: new Date(Date.UTC(2020, 0, 1 + index)).toISOString(),
  })),
});
const imdbMeta = (imdbId, seasons) => ({ id: imdbId, type: 'series', videos: cinemetaVideos(imdbId, seasons) });
const SYNTHETIC = {
  run_steps_over_listed: [kitsuMeta('48291', 3), imdbMeta('tt0388629', [[0, 60]])],
  listed_with_null: [kitsuMeta('1092', 3), imdbMeta('tt0434694', [[0, 10]])],
  s1e7_catches_s2e7: [kitsuMeta('8124', 8), imdbMeta('tt0143353', [[1, 12], [2, 12]])],
  numeric_kitsu_id: [kitsuMeta(8124, 8), imdbMeta('tt0143353', [[1, 12], [2, 12]])],
  past_finished_cap: [kitsuMeta('72', 52), imdbMeta('tt0374407', [[0, 5], [1, 50], [2, 50]])],
  airing_s0_without_cap: [kitsuMeta('48105', 5), imdbMeta('tt22248376', [[0, 10], [1, 28], [2, 10]])],
  non_imdb_episode: [kitsuMeta('12', 592), imdbMeta('tt0388629', [[1, 591]])],
  mid_season_start: [kitsuMeta('41982', 10), imdbMeta('tt2560140', [[0, 2], [1, 25], [2, 12], [3, 22]])],
};
const withDates = (meta) => ({ ...meta, videos: meta.videos.map((video) => ({ ...video, released: new Date(video.released) })) });

const zstd = (data) => zlib.zstdCompressSync(Buffer.from(data), {
  params: { [zlib.constants.ZSTD_c_compressionLevel]: 19 },
});
const unzstd = (file) => zlib.zstdDecompressSync(fs.readFileSync(file)).toString('utf8');
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const kitsuUrl = (id) => `https://kitsu.io/api/edge/anime/${id}?include=genres,episodes,mediaRelationships.destination`;
const cinemetaUrl = (type, imdbId) => `https://v3-cinemeta.strem.io/meta/${type}/${imdbId}.json`;

function allRows() {
  return JSON.parse(fs.readFileSync(path.join(ROOT, 'static/data/imdb_mapping.json'), 'utf8'));
}

function caseRows(rows) {
  const imdbIds = new Set(rows.filter((row) => KITSU_IDS.includes(row.kitsu_id) && row.imdb_id).map((row) => row.imdb_id));
  return rows.filter((row) => KITSU_IDS.includes(row.kitsu_id) || imdbIds.has(row.imdb_id));
}

function caseImdbIds(rows) {
  return [...new Set(rows.filter((row) => row.imdb_id).map((row) => row.imdb_id)), ...UNMAPPED_IMDB];
}

// Episode attributes the pipeline reads, for a Kitsu response too large to commit as is
const EPISODE_FIELDS = ['titles', 'canonicalTitle', 'number', 'airdate', 'synopsis', 'length', 'thumbnail'];
const VIDEO_FIELDS = ['id', 'title', 'name', 'season', 'episode', 'number', 'released', 'thumbnail', 'overview'];

function trimKitsu(body) {
  if (body.length <= MAX_RESPONSE) return body;
  const document = JSON.parse(body);
  document.included = document.included.map((resource) => resource.type !== 'episodes' ? resource : {
    ...resource,
    attributes: Object.fromEntries(EPISODE_FIELDS.filter((field) => field in resource.attributes)
        .map((field) => [field, resource.attributes[field]])),
    relationships: {},
  });
  return JSON.stringify(document);
}

function trimCinemeta(body) {
  if (body.length <= MAX_RESPONSE) return body;
  const document = JSON.parse(body);
  document.meta.videos = document.meta.videos.map((video) => Object.fromEntries(VIDEO_FIELDS
      .filter((field) => field in video).map((field) => [field, video[field]])));
  return JSON.stringify(document);
}

async function fetchResponses() {
  const rows = caseRows(allRows());
  const responses = { kitsu: {}, cinemeta: {} };
  let requests = 0;
  const get = async (url, accept) => {
    if (requests++) await sleep(1100);
    const response = await fetch(url, { headers: { Accept: accept } });
    const body = await response.text();
    console.log(response.status, body.length, url);
    return { status: response.status, body };
  };
  for (const id of KITSU_IDS) {
    const { status, body } = await get(kitsuUrl(id), 'application/vnd.api+json');
    if (status !== 200) throw new Error(`kitsu ${id}: ${status}`);
    responses.kitsu[id] = trimKitsu(body);
  }
  for (const imdbId of caseImdbIds(rows).filter((id) => !FAILING_CINEMETA.includes(id))) {
    const types = MOVIE_IMDB.includes(imdbId) ? ['series', 'movie'] : ['series'];
    for (const type of types) {
      const { status, body } = await get(cinemetaUrl(type, imdbId), 'application/json');
      responses.cinemeta[cinemetaUrl(type, imdbId)] = { status, body: status === 200 ? trimCinemeta(body) : body };
    }
  }
  console.log(`${requests} requests`);
  fs.mkdirSync(DIR, { recursive: true });
  fs.writeFileSync(RESPONSES, zstd(JSON.stringify(responses)));
  fs.writeFileSync(ROWS, JSON.stringify(rows, null, 2) + '\n');
  const kitsuIds = new Set(rows.map((row) => row.kitsu_id));
  const records = fs.readFileSync(path.join(ROOT, 'static/data/search_titles.jsonl'), 'utf8').split('\n')
      .filter((line) => line && kitsuIds.has(JSON.parse(line).id));
  fs.writeFileSync(RECORDS, records.join('\n') + '\n');
}

// The fanart images of a kitsu id, by id % 4: all three, none, a background only, a failed lookup
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

function stubModule(file, exports) {
  const filename = require.resolve(file);
  require.cache[filename] = { id: filename, filename, loaded: true, exports };
}

async function recordGoldens() {
  const responses = JSON.parse(unzstd(RESPONSES));
  const rows = JSON.parse(fs.readFileSync(ROWS, 'utf8'));
  const records = fs.readFileSync(RECORDS, 'utf8').split('\n').filter(Boolean).map((line) => JSON.parse(line));

  stubModule(path.join(ROOT, 'static/data/imdb_mapping.json'), rows);
  const titleRecords = require(path.join(ROOT, 'lib/title_records'));
  stubModule(path.join(ROOT, 'lib/title_records'), { ...titleRecords, readRecords: () => records });
  stubModule(path.join(ROOT, 'lib/fanart'), { getImages: images });

  const Kitsu = require('kitsu');
  const axiosDir = path.join(path.dirname(require.resolve('kitsu')), '..', 'node_modules', 'axios');
  const settle = require(path.join(axiosDir, 'lib/core/settle'));
  require(axiosDir).defaults.adapter = (config) => new Promise((resolve, reject) => {
    const id = config.url.match(/^anime\/(\d+)$/)?.[1];
    const body = responses.kitsu[id];
    if (!body) return reject(new Error(`no recorded kitsu response for ${config.url}`));
    settle(resolve, reject, { data: body, status: 200, statusText: '200', headers: {}, config, request: {} });
  });
  let lastData;
  const kitsuGet = Kitsu.prototype.get;
  Kitsu.prototype.get = async function (...args) {
    const result = await kitsuGet.apply(this, args);
    lastData = JSON.parse(JSON.stringify(result.data));
    return result;
  };

  const axios = require('axios');
  axios.get = async (url) => {
    const recorded = responses.cinemeta[url];
    if (!recorded) throw new Error(`no recorded cinemeta response for ${url}`);
    if (recorded.status < 200 || recorded.status >= 300) {
      throw new Error(`Request failed with status code ${recorded.status}`);
    }
    return { status: recorded.status, data: JSON.parse(recorded.body) };
  };
  console.warn = () => {};

  const kitsu = require(path.join(ROOT, 'lib/kitsu_api'));
  const cinemeta = require(path.join(ROOT, 'lib/cinemeta_api'));
  const { toStremioEntryMeta, toStremioCatalogMeta, toCatalogMeta } = require(path.join(ROOT, 'lib/metadata'));
  const enrich = require(path.join(ROOT, 'lib/metadataEnrich'));
  const json = (value) => JSON.parse(JSON.stringify(value));

  const goldens = { kitsu: {}, imdb: {} };
  for (const id of KITSU_IDS) {
    const entry = await kitsu.animeData(`${id}`);
    const data = lastData;
    const golden = {
      entry: json(entry),
      catalog: json(await toStremioCatalogMeta(json(data))),
      enriched: json(await enrich.enrichKitsuMetadata(entry, cinemeta.getCinemetaMetadata)),
      hasImdbMapping: enrich.hasImdbMapping(`${id}`),
      episodeCap: enrich.episodeCap(`${id}`) ?? null,
    };
    golden.cachedCatalog = json(toCatalogMeta(golden.enriched));
    if (RELATIONS_IDS.includes(id)) {
      golden.relations = json(await toStremioEntryMeta(json(data), { images: false, relations: true }));
    }
    goldens.kitsu[id] = golden;
  }
  for (const imdbId of caseImdbIds(rows)) {
    const metadata = await cinemeta.getCinemetaMetadata(imdbId);
    goldens.imdb[imdbId] = {
      cinemeta: json(metadata),
      enriched: json(await enrich.enrichImdbMetadata(json(metadata), kitsu.animeData)),
      hasImdbMapping: enrich.hasImdbMapping(imdbId),
      kitsuIds: enrich.kitsuIdsForImdb(imdbId),
    };
  }
  goldens.synthetic = {};
  for (const [name, [kitsuInput, imdbInput]] of Object.entries(SYNTHETIC)) {
    goldens.synthetic[name] = {
      kitsu: kitsuInput,
      imdb: imdbInput,
      enrichedKitsu: json(await enrich.enrichKitsuMetadata(withDates(kitsuInput), async () => json(imdbInput))),
      enrichedImdb: json(await enrich.enrichImdbMetadata(json(imdbInput), kitsu.animeData)),
    };
  }
  fs.writeFileSync(GOLDENS, zstd(JSON.stringify(goldens)));
  console.log(`${Object.keys(goldens.kitsu).length} kitsu and ${Object.keys(goldens.imdb).length} imdb goldens`);
}

(async () => {
  if (process.argv.includes('--fetch')) {
    await fetchResponses();
  }
  await recordGoldens();
  process.exit(0);
})().catch((error) => {
  console.error(error);
  process.exit(1);
});
