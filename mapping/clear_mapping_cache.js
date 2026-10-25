// Clears the addon's Mongo cache entries (meta, images, ...) of every kitsu and imdb id whose imdb_mapping.json row
// was added, changed or removed between two git refs, so the deployed addon serves the new mapping right away.
// Counts the matching keys unless --delete is given. Needs MONGODB_URI.
process.noDeprecation = true;
const { execFileSync } = require('child_process');
const { MongoClient } = require('mongodb');

const MAPPING_PATH = 'static/data/imdb_mapping.json';

function mappingAt(ref) {
  return JSON.parse(execFileSync('git', ['show', `${ref}:${MAPPING_PATH}`], { maxBuffer: 64 * 1024 * 1024 }));
}

// { kitsu: [...], imdb: [...] } of the rows that differ between the two mapping versions (both imdb ids of a row
// that moved to another imdb), plus every other kitsu entry of those imdb ids: a sibling row changes where an
// entry's episodes stop rolling over into later IMDb seasons.
function changedIds(before, after) {
  const byKitsu = rows => new Map(rows.map(row => [row.kitsu_id, row]));
  const [old, current] = [byKitsu(before), byKitsu(after)];
  const kitsu = new Set();
  const imdb = new Set();
  new Set([...old.keys(), ...current.keys()]).forEach(kitsuId => {
    const [was, is] = [old.get(kitsuId), current.get(kitsuId)];
    if (JSON.stringify(was) !== JSON.stringify(is)) {
      kitsu.add(String(kitsuId));
      [was?.imdb_id, is?.imdb_id].filter(Boolean).forEach(imdbId => imdb.add(imdbId));
    }
  });
  [...before, ...after].filter(row => imdb.has(row.imdb_id)).forEach(row => kitsu.add(String(row.kitsu_id)));
  return { kitsu: [...kitsu], imdb: [...imdb] };
}

async function clearCache(ids, { remove = false } = {}) {
  const all = [...ids.kitsu, ...ids.imdb];
  if (!all.length) {
    return { matched: 0, deleted: 0 };
  }
  const uri = process.env.MONGODB_URI.trim().replace(/^(['"])(.*)\1$/, '$2');
  const client = await MongoClient.connect(uri);
  try {
    const collection = client.db().collection('kitsu_collection');
    const filter = { key: { $regex: `:(${all.join('|')})$` } };
    const matched = await collection.countDocuments(filter);
    const deleted = remove ? (await collection.deleteMany(filter)).deletedCount : 0;
    return { matched, deleted };
  } finally {
    await client.close();
  }
}

// usage: node mapping/clear_mapping_cache.js <base ref> [head ref=HEAD] [--delete]
if (require.main === module) {
  const args = process.argv.slice(2);
  const [base, head = 'HEAD'] = args.filter(arg => arg !== '--delete');
  const ids = changedIds(mappingAt(base), mappingAt(head));
  console.log(`${base}..${head}: ${ids.kitsu.length} kitsu and ${ids.imdb.length} imdb ids changed`);
  clearCache(ids, { remove: args.includes('--delete') })
      .then(({ matched, deleted }) => console.log(`${matched} cached keys matched, ${deleted} deleted`))
      .catch(error => {
        console.error(`Cache clear failed: ${error.message}`);
        process.exit(1);
      });
}

module.exports = { changedIds, clearCache };
