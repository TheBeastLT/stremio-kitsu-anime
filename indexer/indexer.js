const express = require('express');
const indexer = express();
const repository = require('./repository');

indexer.post('/scrape', async (req, res) => {
  startScrape();
  res.send(200);
});

indexer.listen(process.env.PORT || 3001, async () => {
  console.log(`Kitsu indexer started on port ${process.env.PORT || 3001}!`);
  await repository.createIndices();
});

async function startScrape() {
  const lastScrape = repository.client
}