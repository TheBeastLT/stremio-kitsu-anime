const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const { createTitleIndex, MIN_CONFIDENCE, _internals } = require('../../lib/title_search');
const { norm, parseQuery } = require('../../lib/title_query');
const { readRecords, withAliases } = require('../../lib/title_records');
const { getImdbMapping } = require('../../lib/metadataEnrich');

const { MAX_QUERY_LENGTH } = _internals;
const OUT = __dirname;
const DECIMALS = 12;
const CANDIDATE_QUERIES = 200;
const CANDIDATE_STEP = 15;
const SMOKE_PER_KIND = 20;
const TOP = 20;

const fixed = value => Number(value.toFixed(DECIMALS));
const replacer = (key, value) => {
  if (value instanceof Map || value instanceof Set) throw new Error(`unserialisable ${value.constructor.name} at ${key}`);
  if (typeof value !== 'number') return value;
  if (!Number.isFinite(value)) throw new Error(`non-finite number at ${key}`);
  return Number.isInteger(value) ? value : fixed(value);
};
const line = value => JSON.stringify(value, replacer);

const unique = list => [...new Set(list)];
const writeLines = (name, lines) => fs.writeFileSync(path.join(OUT, name), lines.map(item => `${item}\n`).join(''));

function loadQueries() {
  const rows = fs.readFileSync(path.join(__dirname, '../sample.jsonl'), 'utf8').split('\n').filter(Boolean)
      .map(text => JSON.parse(text));
  const sampleQueries = unique(rows.map(row => row.query));
  return { sampleQueries, queries: unique([...sampleQueries, ...rows.flatMap(row => row.examples || [])]) };
}

function titlesOf(records) {
  return unique(records.flatMap(record => [
    record.canonicalTitle, ...Object.values(record.titles || {}), ...(record.abbreviatedTitles || []),
    ...(record.synonyms || []),
  ]).filter(title => typeof title === 'string' && title));
}

function indexDump(index) {
  const { entries, entriesWithWord } = index._internals;
  const words = [...entriesWithWord.keys()].sort();
  const posting = word => ({ word, entries: entriesWithWord.get(word), ids: entriesWithWord.get(word).map(i => entries[i].id) });
  const hash = crypto.createHash('sha256');
  words.forEach(word => hash.update(`${word}\t${entriesWithWord.get(word).join(',')}\n`));
  return {
    entries: entries.length,
    variants: entries.reduce((sum, entry) => sum + entry.titles.length, 0),
    words: words.length,
    postingsSha256: hash.digest('hex'),
    first: words.slice(0, 5).map(posting),
    last: words.slice(-5).map(posting),
  };
}

const codeUnits = text => Array.from({ length: text.length }, (_, i) => text.charCodeAt(i).toString(16).padStart(4, '0'))
    .join('');

function entriesDump(index) {
  return index._internals.entries.map(entry => line({ ...entry, absTo: entry.absTo === Infinity ? 'Infinity' : entry.absTo }));
}

function trigramsDump(index) {
  const trigrams = index._internals.trigramIndex();
  const grams = [...trigrams.keys()].sort();
  const posting = gram => ({ gram: codeUnits(gram), entries: trigrams.get(gram) });
  const hash = crypto.createHash('sha256');
  grams.forEach(gram => hash.update(`${codeUnits(gram)}\t${trigrams.get(gram).join(',')}\n`));
  return {
    grams: grams.length,
    postingsSha256: hash.digest('hex'),
    first: grams.slice(0, 5).map(posting),
    last: grams.slice(-5).map(posting),
  };
}

function searchDump(index, queries) {
  return queries.map(query => {
    const results = index.search(query, { limit: TOP, minConfidence: 0 });
    const confidence = results[0]?.confidence;
    const confident = confidence >= MIN_CONFIDENCE;
    return line({
      query,
      ids: confident ? results.map(({ id, score }) => ({ id, score })) : [],
      confidence: confidence === undefined ? null : confidence,
    });
  });
}

function candidatesDump(index, sampleQueries) {
  const { entries, candidatesFor, scoreTitles } = index._internals;
  const sampled = sampleQueries.filter((_, i) => i % CANDIDATE_STEP === 0).slice(0, CANDIDATE_QUERIES);
  return sampled.map(query => {
    const trimmed = query.length > MAX_QUERY_LENGTH ? query.slice(0, MAX_QUERY_LENGTH).replace(/\s+\S*$/, '') : query;
    const parsed = parseQuery(trimmed);
    const queryWords = parsed.base ? parsed.base.split(' ') : [];
    const candidates = parsed.base ? candidatesFor(parsed, queryWords)
        .map(i => ({ id: entries[i].id, score: scoreTitles(entries[i], parsed, queryWords).score })) : [];
    return line({ query, candidates });
  });
}

function smokeDump(files) {
  return Object.entries(files).flatMap(([kind, lines]) => {
    const step = Math.max(1, Math.floor(lines.length / SMOKE_PER_KIND));
    return lines.filter((_, i) => i % step === 0).slice(0, SMOKE_PER_KIND).map(text => `{"kind":${JSON.stringify(kind)},"row":${text}}`);
  });
}

function main() {
  const records = withAliases(readRecords());
  const index = createTitleIndex(records, { imdbMapping: getImdbMapping });
  const { sampleQueries, queries } = loadQueries();
  const titles = titlesOf(records);

  const files = {
    norm: titles.map(title => line({ title, norm: norm(title) })),
    parse_query: queries.map(query => line({ query, parsed: parseQuery(query) })),
    search: searchDump(index, queries),
    candidates: candidatesDump(index, sampleQueries),
  };
  Object.entries(files).forEach(([kind, lines]) => writeLines(`${kind}.jsonl`, lines));
  fs.writeFileSync(path.join(OUT, 'index.json'), `${JSON.stringify(indexDump(index), null, 1)}\n`);
  writeLines('smoke.jsonl', smokeDump({ ...files, index: [line({ ...indexDump(index), first: undefined, last: undefined })] }));
  writeLines('entries.jsonl', entriesDump(index));
  fs.writeFileSync(path.join(OUT, 'trigrams.json'), `${JSON.stringify(trigramsDump(index), null, 1)}\n`);

  const sums = [...Object.keys(files).map(kind => `${kind}.jsonl`), 'index.json', 'entries.jsonl', 'trigrams.json', 'smoke.jsonl', 'dump.js'].map(name => {
    const content = fs.readFileSync(path.join(OUT, name));
    const lines = content.toString('utf8').split('\n').length - 1;
    return `${crypto.createHash('sha256').update(content).digest('hex')}  ${lines}  ${name}`;
  });
  fs.writeFileSync(path.join(OUT, 'SHA256SUMS'), `${sums.join('\n')}\n`);
  console.log(sums.join('\n'));
}

main();
