const test = require('node:test');
const assert = require('node:assert');

const { suggestionImdbIds, firstTrusted } = require('../mapping/imdb_search');

test('suggestionImdbIds keeps only title ids, in order (Panlong: an editorial page ranked first)', () => {
  const data = { d: [{ id: '/family-entertainment-guide/' }, { id: 'tt27497321' }, { id: 'nm17979622' }, { id: 'tt15571732' }] };
  assert.deepStrictEqual(suggestionImdbIds(data), ['tt27497321', 'tt15571732']);
  assert.deepStrictEqual(suggestionImdbIds(undefined), []);
});

test('firstTrusted takes the first candidate that verifies (Jiang Ye: the 1st is not tagged Animation)', async () => {
  const tried = [];
  const verify = async imdbId => {
    tried.push(imdbId);
    return imdbId === 'tt32221651' ? { imdb_id: imdbId, _trusted: true } : { imdb_id: imdbId, reviewReason: 'no' };
  };
  const entry = await firstTrusted(['tt9193596', 'tt32221651', 'tt34117673'], verify);
  assert.deepStrictEqual(entry, { imdb_id: 'tt32221651', _trusted: true });
  assert.deepStrictEqual(tried, ['tt9193596', 'tt32221651']);
});

test('firstTrusted queues the first candidate for review when none verifies', async () => {
  const entry = await firstTrusted(['tt1', 'tt2'], async imdbId => ({ imdb_id: imdbId, reviewReason: `bad ${imdbId}` }));
  assert.deepStrictEqual(entry, { imdb_id: 'tt1', reviewReason: 'bad tt1' });
});
