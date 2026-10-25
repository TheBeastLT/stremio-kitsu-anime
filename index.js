const { createApp } = require('./app');
const { startNewTitlesRefresh } = require('./lib/title_index');

createApp().listen(process.env.PORT || 7000, () => {
  console.log(`Started addon at: http://localhost:${process.env.PORT || 7000}`);
});

// the search index's Kitsu entries created since the last weekly titles rebuild
startNewTitlesRefresh();
