const { createApp } = require('./app');
const { startNewTitlesRefresh } = require('./lib/title_index');
const { ensureCacheCollection } = require('./lib/cache_collection');

async function start() {
  if (process.env.MONGODB_URI && !process.env.NO_CACHE) {
    await ensureCacheCollection(process.env.MONGODB_URI, 'kitsu_collection');
  }
  createApp().listen(process.env.PORT || 7000, () => {
    console.log(`Started addon at: http://localhost:${process.env.PORT || 7000}`);
  });

  // the search index's Kitsu entries created since the last weekly titles rebuild
  startNewTitlesRefresh();
}

start();
