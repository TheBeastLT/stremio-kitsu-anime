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
                    slug: { type: 'text', enabled: false },
                    aliases: { type: 'text' },
                    genres: { type: 'text' },
                    poster: { type: 'text', enabled: false },
                    background: { type: 'text', enabled: false },
                    logo: { type: 'text', enabled: false },
                    description: { type: 'text', enabled: false },
                    releaseInfo: { type: 'text', enabled: false },
                    year: { type: 'text', enabled: false },
                    imdbRating: { type: 'float' },
                    userCount: { type: 'integer' },
                    status: { type: 'text' },
                    runtime: { type: 'text', enabled: false },
                    trailers: { type: 'object', enabled: false },
                    videos: { type: 'object', enabled: false }
                  }
                }
              }
            }
          },
          (err) => { if (err) console.log(err); })
    }
  });
}

