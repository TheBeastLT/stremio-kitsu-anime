const { MongoClient } = require('mongodb');

const NAMESPACE_EXISTS = 48;
const CREATE_OPTIONS = { storageEngine: { wiredTiger: { configString: 'block_compressor=zstd' } } };

function connectClient(uri) {
  return MongoClient.connect(uri);
}

async function createCollection(uri, name, connect) {
  const client = await connect(uri);
  try {
    await client.db().createCollection(name, CREATE_OPTIONS);
    return 'created';
  } catch (error) {
    if (error?.code === NAMESPACE_EXISTS) {
      return 'exists';
    }
    throw error;
  } finally {
    await client.close().catch(() => {});
  }
}

async function ensureCacheCollection(uri, name, { timeoutMs = 10000, connect = connectClient } = {}) {
  let timer;
  const timeout = new Promise((_, reject) => {
    timer = setTimeout(() => reject(new Error(`timed out after ${timeoutMs}ms`)), timeoutMs);
  });
  const work = createCollection(uri, name, connect);
  work.catch(() => {});
  try {
    return await Promise.race([work, timeout]);
  } catch (error) {
    console.error(`Failed ensuring cache collection ${name}:`, error?.message || error);
    return 'failed';
  } finally {
    clearTimeout(timer);
  }
}

module.exports = { ensureCacheCollection };
