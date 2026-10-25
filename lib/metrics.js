const crypto = require('node:crypto');
const client = require('prom-client');

const register = new client.Registry();
client.collectDefaultMetrics({ register });

const BUCKETS = [0.01, 0.05, 0.1, 0.25, 0.5, 1, 2.5, 5, 10];

const httpRequests = new client.Counter({
  name: 'http_requests_total',
  help: 'HTTP requests',
  labelNames: ['route', 'status'],
  registers: [register]
});

const httpDuration = new client.Histogram({
  name: 'http_request_duration_seconds',
  help: 'HTTP request duration',
  labelNames: ['route'],
  buckets: BUCKETS,
  registers: [register]
});

const upstreamDuration = new client.Histogram({
  name: 'upstream_request_duration_seconds',
  help: 'Upstream request duration',
  labelNames: ['upstream', 'outcome'],
  buckets: BUCKETS,
  registers: [register]
});

const cacheStoreDuration = new client.Histogram({
  name: 'cache_store_operation_duration_seconds',
  help: 'Cache store operation duration',
  labelNames: ['op'],
  buckets: BUCKETS,
  registers: [register]
});

const counters = {
  cache: new client.Counter({
    name: 'cache_requests_total',
    help: 'Cache lookups',
    labelNames: ['kind', 'result'],
    registers: [register]
  }),
  catalog: new client.Counter({
    name: 'catalog_requests_total',
    help: 'Catalog requests',
    labelNames: ['catalog', 'kind'],
    registers: [register]
  }),
  search: new client.Counter({
    name: 'search_requests_total',
    help: 'Search requests',
    labelNames: ['result'],
    registers: [register]
  }),
  meta: new client.Counter({
    name: 'meta_requests_total',
    help: 'Meta requests',
    labelNames: ['id_type', 'outcome'],
    registers: [register]
  })
};

const ROUTES = new Set(['manifest', 'catalog', 'meta', 'subtitles', 'search', 'metrics']);

function routeOf(url) {
  let path;
  try {
    path = decodeURIComponent(String(url).split('?')[0]);
  } catch (error) {
    return 'other';
  }
  if (path === '/' || path === '/configure') {
    return 'landing';
  }
  const segment = path.split('/')[1].replace(/\.json$/, '');
  return ROUTES.has(segment) ? segment : 'other';
}

function httpMetrics(req, res, next) {
  const end = httpDuration.startTimer();
  res.on('finish', () => {
    const route = routeOf(req.originalUrl || req.url);
    httpRequests.inc({ route, status: res.statusCode });
    end({ route });
  });
  next();
}

function digest(value) {
  return crypto.createHash('sha256').update(String(value)).digest();
}

function safeEqual(a, b) {
  return crypto.timingSafeEqual(digest(a), digest(b));
}

function metricsRouter(req, res, next) {
  const path = req.path || String(req.url || '').split('?')[0];
  if (path !== '/metrics') {
    return next();
  }
  const user = process.env.METRICS_USER;
  const password = process.env.METRICS_PASSWORD;
  if (!user || !password) {
    res.statusCode = 404;
    return res.end();
  }
  const header = req.headers.authorization || '';
  const [scheme, encoded = ''] = header.split(' ');
  const decoded = Buffer.from(encoded, 'base64').toString();
  const separator = decoded.indexOf(':');
  const givenUser = separator < 0 ? decoded : decoded.slice(0, separator);
  const givenPassword = separator < 0 ? '' : decoded.slice(separator + 1);
  const authorized = scheme === 'Basic'
      && safeEqual(givenUser, user)
      && safeEqual(givenPassword, password);
  if (!authorized) {
    res.statusCode = 401;
    res.setHeader('WWW-Authenticate', 'Basic realm="metrics"');
    return res.end();
  }
  return register.metrics()
      .then((body) => {
        res.statusCode = 200;
        res.setHeader('Content-Type', register.contentType);
        res.end(body);
      })
      .catch((error) => {
        console.error('Failed serving metrics:', error?.message || error);
        res.statusCode = 500;
        res.end();
      });
}

async function timedUpstream(name, fn) {
  const end = upstreamDuration.startTimer({ upstream: name });
  try {
    const result = await fn();
    end({ outcome: 'ok' });
    return result;
  } catch (error) {
    end({ outcome: 'error' });
    throw error;
  }
}

module.exports = {
  routeOf,
  httpMetrics,
  metricsRouter,
  timedUpstream,
  counters,
  cacheStoreDuration,
  register
};
