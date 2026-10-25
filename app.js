const express = require('express');
const compression = require('compression');
const serverless = require('./serverless');
const { httpMetrics, metricsRouter } = require('./lib/metrics');

function createApp() {
  const app = express();
  app.use(compression());
  app.use(httpMetrics);
  app.use(metricsRouter);
  app.use((req, res, next) => serverless(req, res, next));
  return app;
}

module.exports = { createApp };
