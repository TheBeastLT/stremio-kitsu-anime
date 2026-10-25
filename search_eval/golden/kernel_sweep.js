const crypto = require('crypto');
const distance = require('jaro-winkler');

const SEED = 20261025;
const RANDOM_DOUBLES = 1000000;
const NEAR_ONE = 200000;
const INTEGERS = 200000;
const IDF_TOTALS = [1, 100, 19160, 20000, 25000];
const IDF_COUNTS = 30000;
const JW_PAIRS = 300000;
const JW_MAX_LENGTH = 24;
const ALPHABET = [0x61, 0x62, 0x63, 0x64, 0x65, 0x20, 0x78, 0xe9, 0x4e00, 0xd83d, 0xde00, 0xde01];

function mulberry32(seed) {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6D2B79F5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return (t ^ (t >>> 14)) >>> 0;
  };
}

const view = new DataView(new ArrayBuffer(8));
const fromWords = (high, low) => {
  view.setUint32(0, high);
  view.setUint32(4, low);
  return view.getFloat64(0);
};

function digest(values) {
  const hash = crypto.createHash('sha256');
  const out = Buffer.alloc(8);
  let count = 0;
  for (const value of values) {
    out.writeDoubleLE(value);
    hash.update(out);
    count++;
  }
  return { count, sha256: hash.digest('hex') };
}

function* logInputs() {
  const next = mulberry32(SEED);
  for (let i = 0; i < RANDOM_DOUBLES; i++) {
    const high = next() & 0x7fffffff;
    const low = next();
    if (high >= 0x7ff00000) continue;
    yield fromWords(high, low);
  }
  for (let i = 0; i < NEAR_ONE; i++) {
    const offset = (next() % 0x200000) - 0x100000;
    yield offset < 0 ? fromWords(0x3fefffff, (0x100000000 + offset) >>> 0) : fromWords(0x3ff00000, offset);
  }
  for (let i = 1; i <= INTEGERS; i++) {
    yield i;
    yield 1 / i;
  }
  for (const total of IDF_TOTALS) {
    for (let count = 0; count <= IDF_COUNTS; count++) yield total / (1 + count);
  }
}

function* jwPairs() {
  const next = mulberry32(SEED + 1);
  const word = () => {
    const length = next() % (JW_MAX_LENGTH + 1);
    return String.fromCharCode(...Array.from({ length }, () => ALPHABET[next() % ALPHABET.length]));
  };
  for (let i = 0; i < JW_PAIRS; i++) {
    const a = word();
    if (next() % 2) {
      yield [a, word()];
      continue;
    }
    const units = a.split('');
    const at = units.length ? next() % units.length : 0;
    const kind = next() % 3;
    if (kind === 0) units.splice(at, 1);
    else if (kind === 1) units.splice(at, 0, String.fromCharCode(ALPHABET[next() % ALPHABET.length]));
    else if (units.length > 1) [units[at], units[(at + 1) % units.length]] = [units[(at + 1) % units.length], units[at]];
    yield [a, units.join('')];
  }
}

console.log(JSON.stringify({
  log: digest((function* () { for (const x of logInputs()) yield Math.log(x); })()),
  jw: digest((function* () { for (const [a, b] of jwPairs()) yield distance(a, b); })()),
}));
