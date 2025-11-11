const { Elysia } = require('elysia');
const { getRouter } = require('stremio-addon-sdk');
const landingTemplate = require('stremio-addon-sdk/src/landingTemplate');
const addonInterface = require('./addon');
const { connect } = require('elysia-connect-middleware');

const router = getRouter(addonInterface);

const app = new Elysia();

router.get('/', (_, res) => {
  const landingHTML = landingTemplate(addonInterface.manifest);
  res.setHeader('content-type', 'text/html');
  res.end(landingHTML);
});

const handler = (req, res) => {
  router(req, res, () => {
    res.statusCode = 404;
    res.end();
  });
};

app.use(connect(handler));

app.listen(process.env.PORT || 7000, () => {
  console.log(`Started addon at: http://localhost:${process.env.PORT || 7000}`);
});
