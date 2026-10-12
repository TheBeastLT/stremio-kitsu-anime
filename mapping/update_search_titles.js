// Rebuilds static/data/search_titles.jsonl, the title search index's data: every Kitsu anime's titles (one Kitsu
// request a second, ~20 minutes) plus its synonyms from the latest anime-offline-database release. Run weekly by
// .github/workflows/search-titles.yml. One entry per line, sorted by id, so a rebuild only changes the lines that
// changed.
// Usage: node mapping/update_search_titles.js [--catalog <kitsu catalog .jsonl>] [--offline-database <release .jsonl>]
// (local inputs instead of fetching, e.g. search_eval/cache/catalog.jsonl)
const fs = require('fs');
const readline = require('readline');
const axios = require('axios');
const { toTitleRecord, readRecords, TITLES_PATH } = require('../lib/title_records');
const { SEARCHED_SUBTYPES } = require('../lib/title_search');
const { animeTitlePage, PAGE_SIZE } = require('../lib/kitsu_api');

const OFFLINE_DATABASE_URL = 'https://github.com/cedya77/anime-offline-database/releases/latest/download/'
    + 'anime-offline-database.jsonl';
const REQUEST_DELAY = 1000;
// A name the database gives more than this many Kitsu entries is a franchise-wide or generic name, not a title.
const MAX_SHARED_SYNONYM = 2;
// A rebuild is refused below this share of Kitsu's count or of the current file (a partial crawl or release).
const MIN_SHARE = 0.98;
const MIN_SYNONYMS_SHARE = 0.9;

const argValue = name => {
  const index = process.argv.indexOf(name);
  return index > 0 ? process.argv[index + 1] : undefined;
};

async function main() {
  const catalogFile = argValue('--catalog');
  const databaseFile = argValue('--offline-database');
  const catalog = catalogFile ? readJsonl(catalogFile) : await fetchCatalog();
  const current = readRecords();
  const synonyms = await fetchSynonyms(databaseFile)
      .then(fetched => {
        const before = current.filter(record => record.synonyms?.length).length;
        if (fetched.size < before * MIN_SYNONYMS_SHARE) {
          throw new Error(`only ${fetched.size} entries have synonyms (${before} before)`);
        }
        return fetched;
      })
      .catch(error => {
        console.warn(`Keeping the current synonyms, the offline database failed: ${error.message}`);
        return new Map(current.map(record => [record.id, record.synonyms || []]));
      });
  const records = catalog
      .filter(anime => SEARCHED_SUBTYPES.includes(anime.subtype))
      .map(anime => toTitleRecord(anime, synonyms.get(+anime.id)));
  if (records.length < current.length * MIN_SHARE) {
    throw new Error(`Only ${records.length} entries against ${current.length} in the titles file, not replacing it`);
  }
  const lines = records
      .sort((a, b) => a.id - b.id)
      .map(record => JSON.stringify(record));
  fs.writeFileSync(TITLES_PATH, lines.join('\n') + '\n');
  console.log(`Wrote ${lines.length} entries to ${TITLES_PATH}`);
}

const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

// a title page a second, retrying failures (Kitsu answers the odd request with a 5xx in a ~20 minute crawl)
let lastRequest = 0;
async function titlePage(offset) {
  for (let attempt = 1; ; attempt++) {
    await sleep(lastRequest + REQUEST_DELAY - Date.now());
    lastRequest = Date.now();
    try {
      return await animeTitlePage({ offset });
    } catch (error) {
      if (attempt >= 6) throw error;
      console.warn(`Retrying Kitsu titles at offset ${offset}: ${JSON.stringify(error).slice(0, 200)}`);
      await sleep(5000 * attempt);
    }
  }
}

// Every anime by id. Kitsu's offset paging can repeat a page, so entries are deduplicated and the count checked.
async function fetchCatalog() {
  const entries = new Map();
  let count = Infinity;
  for (let offset = 0; offset < count; offset += PAGE_SIZE) {
    const page = await titlePage(offset);
    count = page.count;
    if (!Number.isFinite(count)) throw new Error(`Kitsu's titles page at offset ${offset} has no count`);
    page.entries.forEach(anime => entries.set(anime.id, anime));
    if (!page.entries.length) break;
  }
  if (entries.size < count * MIN_SHARE) {
    throw new Error(`Only ${entries.size} of ${count} Kitsu entries fetched, not replacing the titles file`);
  }
  console.log(`Fetched ${entries.size} Kitsu entries (Kitsu counts ${count})`);
  return [...entries.values()];
}

// kitsu id -> [title, ...synonyms] from the offline database, streamed line by line (the release is ~60MB).
async function fetchSynonyms(file) {
  const input = file ? fs.createReadStream(file)
      : (await axios.get(OFFLINE_DATABASE_URL, { responseType: 'stream', timeout: 120000 })).data;
  const byKitsuId = new Map();
  const lines = readline.createInterface({ input, crlfDelay: Infinity });
  for await (const line of lines) {
    const entry = line.trim() && JSON.parse(line);
    if (!entry?.sources) continue; // the first line is the release's metadata
    const kitsuIds = entry.sources.map(source => source.match(/kitsu\.(?:app|io)\/anime\/(\d+)/)?.[1]).filter(Boolean);
    kitsuIds.forEach(id => {
      const names = byKitsuId.get(+id) || byKitsuId.set(+id, new Set()).get(+id);
      [entry.title, ...(entry.synonyms || [])].filter(Boolean).forEach(name => names.add(name));
    });
  }
  const key = name => name.toLowerCase().replace(/[^\p{L}\p{N}]+/gu, ' ').trim();
  const shared = new Map();
  byKitsuId.forEach(names => new Set([...names].map(key))
      .forEach(name => shared.set(name, (shared.get(name) || 0) + 1)));
  const synonyms = new Map();
  byKitsuId.forEach((names, id) => synonyms.set(id, [...names]
      .filter(name => shared.get(key(name)) <= MAX_SHARED_SYNONYM).sort()));
  console.log(`Read offline database synonyms for ${synonyms.size} Kitsu entries`);
  return synonyms;
}

const readJsonl = file => fs.readFileSync(file, 'utf8').split('\n').filter(Boolean).map(line => JSON.parse(line));

main().catch(error => {
  console.error(error.message || error);
  process.exit(1);
});
