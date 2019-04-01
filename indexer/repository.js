const elasticsearch = require('elasticsearch');

const BONSAI_URL = process.env.BONSAI_URL;
const client = new elasticsearch.Client({
  host: BONSAI_URL,
  log: 'trace'
});

function createIndices() {
  client.indices.exists({index: 'kitsu'}, (err, res) => {
    if (res) {
      console.log('index already exists');
    } else {
      client.indices.create(
          {
            index: 'kitsu',
            body: {
              mappings: {
                anime: {
                  properties: {
                    id: { type: 'text' },
                    kitsu_id: { type: 'text' },
                    imdb_id: { type: 'text' },
                    type: { type: 'text' },
                    animeType: { type: 'text' },
                    name: { type: 'text' },
                    slug: { enabled: false },
                    aliases: { type: 'text' },
                    genres: { type: 'text' },
                    poster: { enabled: false },
                    background: { enabled: false },
                    logo: { enabled: false },
                    description: { enabled: false },
                    releaseInfo: { enabled: false },
                    year: { enabled: false },
                    imdbRating: { type: 'float' },
                    userCount: { type: 'integer' },
                    status: { type: 'text' },
                    runtime: { enabled: false },
                    trailers: { enabled: false },
                    videos: { enabled: false }
                  }
                }
              }
            }
          },
          (err) => { if (err) console.log(err); })
    }
  });
}

module.exports = { createIndices, client };