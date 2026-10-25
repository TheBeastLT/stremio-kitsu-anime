const { createApp } = require('./app');
const { startNewTitlesRefresh } = require('./lib/title_index');

const server = createApp().listen(process.env.PORT || 7000, () => {
  console.log(`Started addon at: http://localhost:${process.env.PORT || 7000}`);
});

// the search index's Kitsu entries created since the last weekly titles rebuild
startNewTitlesRefresh();

let shuttingDown = false;
function shutdown(signal) {
  if (shuttingDown) {
    process.exit(0);
  }
  shuttingDown = true;
  console.log(`${signal} received, shutting down`);
  server.close(() => process.exit(0));
  server.closeIdleConnections();
}

process.on('SIGTERM', () => shutdown('SIGTERM'));
process.on('SIGINT', () => shutdown('SIGINT'));
