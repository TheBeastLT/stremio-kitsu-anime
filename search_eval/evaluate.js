// Offline search evaluation (no requests): runs the title index (lib/title_index.js, built from
// static/data/search_titles.jsonl) over sample.jsonl and reports top-1 per bucket against `accepted` and `preferred`,
// next to the Kitsu text search the index replaced (its top results, saved in kitsu_search_tops.json).
// Label corrections in label_fixes.jsonl ({ query, verdict, correct, reason }) replace `accepted`/`preferred`;
// `garbage`, `absent` (not on Kitsu) and `ambiguous` rows are left out of the scores. --raw scores the original labels.
// Usage: node search_eval/evaluate.js [--raw] [--diff]   (--diff lists every query whose top result differs)
const fs = require('fs');
const path = require('path');

process.env.FANART_APIKEY = process.env.FANART_APIKEY || 'test';
const { searchTitles } = require('../lib/title_index');
const { readRecords } = require('../lib/title_records');

const readJsonl = file => fs.readFileSync(file, 'utf8').split('\n').filter(Boolean).map(line => JSON.parse(line));
const fixes = process.argv.includes('--raw') ? new Map()
    : new Map(readJsonl(path.join(__dirname, 'label_fixes.jsonl')).map(fix => [fix.query, fix]));
const sample = readJsonl(path.join(__dirname, 'sample.jsonl'))
    .filter(item => !['garbage', 'absent', 'ambiguous'].includes(fixes.get(item.query)?.verdict))
    .map(item => {
      const fix = fixes.get(item.query);
      return fix?.correct?.length ? { ...item, accepted: fix.correct, preferred: fix.correct[0] } : item;
    });
const kitsuTops = JSON.parse(fs.readFileSync(path.join(__dirname, 'kitsu_search_tops.json'), 'utf8'));
const titles = new Map(readRecords().map(record => [record.id, record.canonicalTitle]));

const buckets = {};
const changed = [];
for (const item of sample) {
  const bucket = buckets[item.bucket] || (buckets[item.bucket] = { n: 0, accepted: 0, preferred: 0, kitsu: 0, none: 0 });
  const top = searchTitles(item.query)[0]?.id;
  const kitsu = kitsuTops[item.query];
  bucket.n++;
  if (item.accepted.includes(top)) bucket.accepted++;
  if (top === item.preferred) bucket.preferred++;
  if (item.accepted.includes(kitsu)) bucket.kitsu++;
  if (top === undefined) bucket.none++;
  if (top !== kitsu) changed.push({ item, top, kitsu });
}

const pct = (count, n) => `${(100 * count / n).toFixed(1)}%`.padStart(7);
console.log(`${'bucket'.padEnd(64)}    n  accepted preferred  no result  kitsu search`);
const total = { n: 0, accepted: 0, preferred: 0, kitsu: 0, none: 0 };
Object.entries(buckets).forEach(([name, b]) => {
  Object.keys(total).forEach(key => total[key] += b[key]);
  console.log(`${name.padEnd(64)} ${String(b.n).padStart(4)} ${pct(b.accepted, b.n)}   ${pct(b.preferred, b.n)}  ${String(b.none).padStart(9)}  ${pct(b.kitsu, b.n)}`);
});
console.log(`${'overall'.padEnd(64)} ${String(total.n).padStart(4)} ${pct(total.accepted, total.n)}   ${pct(total.preferred, total.n)}  ${String(total.none).padStart(9)}  ${pct(total.kitsu, total.n)}`);

if (process.argv.includes('--diff')) {
  const title = id => id === undefined ? '-' : `${id} ${titles.get(id) || ''}`;
  console.log(`\n${changed.length} queries with a different top result than the Kitsu search (+ index right, - kitsu right, ~ neither):`);
  changed.forEach(({ item, top, kitsu }) => {
    const mark = item.accepted.includes(top) ? '+' : item.accepted.includes(kitsu) ? '-' : '~';
    console.log(`${mark} [${item.bucket}] ${item.query}: ${title(kitsu)} -> ${title(top)}`);
  });
}
