const { serveHTTP } = require("stremio-addon-sdk");
const addonInterface = require("./addon");
const { Repository } = require('./lib/repository');

serveHTTP(addonInterface, { port: process.env.PORT || 7000, cacheMaxAge: 0 })
    // .then((addon) => new Repository())
    // .then((repo) => repo.migrate(3));
