const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const { createTitleIndex, MIN_CONFIDENCE, _internals } = require('../../lib/title_search');
const { norm, parseQuery } = require('../../lib/title_query');
const { readRecords, withAliases } = require('../../lib/title_records');
const { getImdbMapping } = require('../../lib/metadataEnrich');

const { distance, MAX_QUERY_LENGTH } = _internals;
const OUT = __dirname;
const DECIMALS = 12;
const CANDIDATE_QUERIES = 200;
const CANDIDATE_STEP = 15;
const JW_PAIRS = 5000;
const LOG_VALUES = 200;
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

function random(seed) {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6D2B79F5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

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

function mutate(word, rand) {
  const chars = [...word];
  if (chars.length < 2) return `${word}${word}`;
  const at = Math.floor(rand() * (chars.length - 1));
  const kind = Math.floor(rand() * 4);
  if (kind === 0) [chars[at], chars[at + 1]] = [chars[at + 1], chars[at]];
  else if (kind === 1) chars.splice(at, 1);
  else if (kind === 2) chars.splice(at, 0, chars[at]);
  else chars[at] = 'x';
  return chars.join('');
}

function jwPairs(index, queries, titles) {
  const rand = random(20261025);
  const pick = list => list[Math.floor(rand() * list.length)];
  const normed = unique(titles.flatMap(title => norm(title).split(' ')).filter(Boolean));
  const queryWords = unique(queries.slice(0, 3000).flatMap(query => norm(query).split(' ')).filter(Boolean));
  const raw = unique(titles.flatMap(title => title.split(/\s+/)).filter(Boolean));
  const nonAscii = unique([...normed, ...raw]).filter(word => /[^\x00-\x7f]/.test(word) && !/[\u{10000}-\u{10ffff}]/u.test(word));
  const astral = unique([...normed, ...raw]).filter(word => /[\u{10000}-\u{10ffff}]/u.test(word));
  const synthetic = ['😀', '😀😁', 'a😀b', '𠮷野家', '𠮷', 'ab𝒳yz', '😀😀😀😀', '𝒳𝒴𝒵'];
  const astralPool = [...astral, ...synthetic];
  const byLead = new Map();
  normed.forEach(word => (byLead.get(word.slice(0, 2)) || byLead.set(word.slice(0, 2), []).get(word.slice(0, 2))).push(word));
  const leads = [...byLead.values()].filter(list => list.length > 1);
  const pairs = [
    ['', ''], ['', 'a'], ['a', ''], ['a', 'a'], ['a', 'b'], ['ab', 'ba'], ['abc', 'abd'], ['martha', 'marhta'],
    ['dixon', 'dicksonx'], ['jellyfish', 'smellyfish'], ['Naruto', 'naruto'], ['naruto', 'narutoo'],
    ['naruto shippuden', 'naruto shippuuden'], ['one piece', 'one piece film z'], ['x', 'xyz'], ['😀', '😀'],
    ['😀', '😁'], ['𠮷野家', '吉野家'],
  ];
  const add = (count, make) => { for (let i = 0; i < count; i++) pairs.push(make()); };
  add(1400, () => [pick(normed), pick(normed)]);
  add(300, () => [pick(queryWords), pick(normed)]);
  add(900, () => { const list = pick(leads); return [pick(list), pick(list)]; });
  add(500, () => { const word = pick(normed); return [word, mutate(word, rand)]; });
  add(500, () => [pick(nonAscii), pick(nonAscii)]);
  add(200, () => { const word = pick(nonAscii); return [word, mutate(word, rand)]; });
  add(150, () => [pick(astralPool), pick(astralPool)]);
  add(100, () => { const word = pick(astralPool); return [word, mutate(word, rand)]; });
  add(50, () => [pick(astralPool), pick(raw)]);
  add(300, () => [norm(pick(queries)), norm(pick(titles))]);
  add(300, () => {
    const base = parseQuery(pick(queries)).base || 'x';
    const { entries, candidatesFor } = index._internals;
    const candidate = entries[pick(candidatesFor(parseQuery(base), base.split(' ')).concat([0]))];
    return [base, pick(candidate.titles).base];
  });
  while (pairs.length < JW_PAIRS) pairs.push([pick(raw), pick(raw)]);
  return pairs.slice(0, JW_PAIRS).map(([a, b]) => ({ a, b, distance: distance(a, b) }));
}

function logValues(index) {
  const { entries, entriesWithWord } = index._internals;
  const counts = unique([...entriesWithWord.values()].map(list => list.length)).sort((a, b) => a - b);
  const wanted = LOG_VALUES - 1;
  const picked = unique(Array.from({ length: wanted }, (_, i) => counts[Math.round(i * (counts.length - 1) / (wanted - 1))]));
  return [0, ...picked].map(count => entries.length / (1 + count))
      .map(x => `{"x":${x.toPrecision(17)},"ln":${Math.log(x).toPrecision(17)}}`);
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

const NON_ASCII_ROWS = 5;
const ASTRAL_ROWS = 2;
const isAstral = text => /[\u{10000}-\u{10ffff}]/u.test(JSON.parse(text).a + JSON.parse(text).b);
const isNonAscii = text => !isAstral(text) && /[^\x00-\x7f]/.test(JSON.parse(text).a + JSON.parse(text).b);

function smokeDump(files) {
  return Object.entries(files).flatMap(([kind, lines]) => {
    const step = Math.max(1, Math.floor(lines.length / SMOKE_PER_KIND));
    const chosen = kind === 'jw'
        ? [...lines.filter(isAstral).slice(0, ASTRAL_ROWS), ...lines.filter(isNonAscii).slice(0, NON_ASCII_ROWS)]
        : [];
    const rest = lines.filter((text, i) => i % step === 0 && !chosen.includes(text));
    return [...chosen, ...rest].slice(0, SMOKE_PER_KIND).map(text => `{"kind":${JSON.stringify(kind)},"row":${text}}`);
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
    jw: jwPairs(index, queries, titles).map(pair => line(pair)),
    log: logValues(index),
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
