const { test, mock } = require('node:test');
const assert = require('node:assert');
const kitsuApi = require('../lib/kitsu_api');
const { searchTitles, startNewTitlesRefresh } = require('../lib/title_index');

test('a restart searches the entries Kitsu created since the title file\'s rebuild without waiting an hour', async () => {
  const fresh = { id: '99999999', canonicalTitle: 'Zyxwvut Freshly Created', titles: { en_jp: 'Zyxwvut Freshly Created' },
    subtype: 'TV', status: 'upcoming', startDate: '2026-12-01' };
  const page = mock.method(kitsuApi, 'animeTitlePage', async () => ({ entries: [fresh] }));
  try {
    startNewTitlesRefresh();
    const deadline = Date.now() + 10000; // the index build takes a few seconds
    while (!searchTitles('zyxwvut freshly created', 1).length && Date.now() < deadline) {
      await new Promise(resolve => setTimeout(resolve, 50));
    }
    assert.ok(page.mock.callCount() > 0);
    assert.strictEqual(searchTitles('zyxwvut freshly created', 1)[0]?.id, 99999999);
  } finally {
    mock.restoreAll();
  }
});
