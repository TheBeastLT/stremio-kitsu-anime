const fs = require('fs');
const path = require('path');
const addonInterface = require('../../../../addon');

fs.writeFileSync(path.join(__dirname, 'manifest.json'), JSON.stringify(addonInterface.manifest));
