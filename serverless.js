const { getRouter } = require('stremio-addon-sdk');
const landingTemplate = require('stremio-addon-sdk/src/landingTemplate');
const addonInterface = require('./addon');
const { searchIds } = require('./lib/kitsu_api');
const { counters } = require('./lib/metrics');
const router = getRouter(addonInterface);

const CACHE_MAX_AGE = parseInt(process.env.CACHE_MAX_AGE) || 12 * 60 * 60; // 12 hours

router.get('/', (_, res) => {
  const landingHTML = landingTemplate(addonInterface.manifest);
  res.setHeader('content-type', 'text/html');
  res.end(landingHTML);
});

router.get(/^\/search\/(.+)\.json$/, (req, res) => {
  let ids;
  try {
    ids = searchIds(req.params[0]); // decoded by the router (a malformed one is its 400)
  } catch (error) {
    console.error(`Search failed for "${req.params[0]}": ${error.stack || error}`);
    res.statusCode = 500;
    return res.end();
  }
  counters.search.inc({ result: ids.length ? 'found' : 'empty' });
  res.setHeader('content-type', 'application/json; charset=utf-8');
  res.setHeader('access-control-allow-origin', '*');
  res.setHeader('cache-control', `max-age=${CACHE_MAX_AGE}, public`);
  res.end(JSON.stringify({ ids }));
});

module.exports = (req, res) => {
  // the SDK parses catalog extras from req.url, so a "?<timestamp>" would end up in the search term
  req.url = req.url.split('?')[0];
  router(req, res, (error) => {
    res.statusCode = error?.status || 404;
    res.end(res.statusCode === 404 ? JSON.stringify({ err: 'not found' }) : undefined);
  });
};
