const fs = require('fs');
const path = require('path');
const Kitsu = require('kitsu');
const axiosDir = path.join(path.dirname(require.resolve('kitsu')), '..', 'node_modules', 'axios');
const kitsuAxios = require(axiosDir);
const buildURL = require(path.join(axiosDir, 'lib/helpers/buildURL'));
const buildFullPath = require(path.join(axiosDir, 'lib/core/buildFullPath'));
const settle = require(path.join(axiosDir, 'lib/core/settle'));

const dir = path.join(__dirname, 'kitsu');
const NOT_FOUND_BODY = JSON.stringify({
  errors: [{
    title: 'Record not found',
    detail: 'The record identified by 99999999 could not be found.',
    code: '404',
    status: '404',
  }],
});

let current;
const missing = [];
const captured = {};
const Captured = Symbol('captured');

kitsuAxios.defaults.adapter = (config) => new Promise((resolve, reject) => {
  const url = buildURL(buildFullPath(config.baseURL, config.url), config.params, config.paramsSerializer);
  captured.url = url;
  const status = current.status || 200;
  const file = path.join(dir, `${current.name}.json`);
  const body = current.body || (fs.existsSync(file) && fs.readFileSync(file, 'utf8'));
  if (!body) {
    missing.push(url);
    return reject(new Error(`no recorded body for ${current.name}`));
  }
  settle(resolve, reject, { data: body, status, statusText: `${status}`, headers: {}, config, request: {} });
});

const get = Kitsu.prototype.get;
Kitsu.prototype.get = async function (...args) {
  const result = await get.apply(this, args);
  const { headers, ...document } = result;
  captured.document = JSON.parse(JSON.stringify(document));
  throw Captured;
};

const kitsu = require('../../../../lib/kitsu_api');

const cases = [
  { name: 'anime_data', run: () => kitsu.animeData('7442', { images: false }) },
  {
    name: 'catalog_metas',
    run: () => kitsu.animeCatalogMetas(['1', '7442', '12', '21', '11', '13', '1376', '1555', '3936', '41370', '12268',
      '6448', '8271', '10740', '11469', '42323', '99999999']),
  },
  {
    name: 'entries_browse',
    run: () => kitsu.animeEntries({ offset: 20, genre: 'Action', sort: '-average_rating', status: 'current' }),
  },
  { name: 'entries_trending', run: () => kitsu.animeEntries({ trending: true }) },
  { name: 'title_page', run: () => kitsu.animeTitlePage({ offset: 0, sort: 'id' }) },
  { name: 'not_found', status: 404, body: NOT_FOUND_BODY, run: () => kitsu.animeData('99999999', { images: false }) },
];

(async () => {
  for (const testCase of cases) {
    current = testCase;
    delete captured.url;
    delete captured.document;
    const rejection = await testCase.run().then(() => undefined, (error) => error);
    if (missing.length) {
      continue;
    }
    const golden = rejection === Captured || captured.document
        ? { url: captured.url, document: captured.document }
        : { url: captured.url, rejection };
    fs.writeFileSync(path.join(dir, `${testCase.name}.golden.json`), JSON.stringify(golden, null, 2) + '\n');
  }
  if (missing.length) {
    console.log(missing.join('\n'));
    process.exitCode = 1;
  }
})();
