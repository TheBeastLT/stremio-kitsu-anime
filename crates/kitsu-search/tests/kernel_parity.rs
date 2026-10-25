use std::fmt::Write;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::sync::LazyLock;

use kitsu_search::index::Index;
use kitsu_search::jaro_winkler::distance;
use kitsu_search::math::ln;
use kitsu_search::query::parse_query;
use kitsu_search::records::Sources;
use kitsu_search::score::{Query, candidates, title_score};
use serde::Deserialize;
use serde_json::value::RawValue;
use sha2::{Digest, Sha256};

mod common;

use common::golden;

const FMA_NOTE: &str = "Node on aarch64 contracts fdlibm's multiply-adds into FMAs, so Math.log may differ from the \
                        uncontracted port by one ulp";
const SEED: u32 = 20_261_025;
const ALPHABET: [u16; 12] = [0x61, 0x62, 0x63, 0x64, 0x65, 0x20, 0x78, 0xe9, 0x4e00, 0xd83d, 0xde00, 0xde01];

fn data(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../static/data").join(name)
}

fn rows<T: serde::de::DeserializeOwned>(name: &str) -> Option<Vec<T>> {
    let path = golden(name)?;
    let lines = BufReader::new(File::open(path).unwrap()).lines();
    Some(lines.map(|line| serde_json::from_str(&line.unwrap()).unwrap()).collect())
}

fn number(raw: &RawValue) -> f64 {
    raw.get().parse().unwrap_or_else(|error| panic!("not a number {}: {error}", raw.get()))
}

fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn round_up(digits: &mut [u8]) {
    for digit in digits.iter_mut().rev() {
        if *digit == b'.' {
            continue;
        }
        if *digit == b'9' {
            *digit = b'0';
        } else {
            *digit += 1;
            return;
        }
    }
    panic!("carry past the leading digit");
}

fn js_fixed_12(value: f64) -> f64 {
    assert!((0.0..10.0).contains(&value), "{value}");
    let exact = format!("{value:.1100}");
    let point = exact.find('.').unwrap();
    let mut kept = exact.as_bytes()[..point + 13].to_vec();
    if exact.as_bytes()[point + 13] >= b'5' {
        round_up(&mut kept);
    }
    String::from_utf8(kept).unwrap().parse().unwrap()
}

#[derive(Deserialize)]
struct JwRow {
    a: String,
    b: String,
    distance: Box<RawValue>,
}

#[test]
fn jaro_winkler_equals_every_golden_pair() {
    let Some(rows) = rows::<JwRow>("jw.jsonl") else { return };
    assert_eq!(rows.len(), 5000);
    let differences: Vec<String> = rows
        .iter()
        .filter_map(|row| {
            let got = distance(&utf16(&row.a), &utf16(&row.b));
            let expected = number(&row.distance);
            (js_fixed_12(got).to_bits() != expected.to_bits())
                .then(|| format!("{:?} {:?}: got {got}, expected {expected}", row.a, row.b))
        })
        .collect();
    assert!(
        differences.is_empty(),
        "{} differences: {:#?}",
        differences.len(),
        &differences[..differences.len().min(20)]
    );
}

#[derive(Deserialize)]
struct LogRow {
    x: Box<RawValue>,
    ln: Box<RawValue>,
}

fn ulps(a: f64, b: f64) -> u64 {
    a.to_bits().abs_diff(b.to_bits())
}

#[allow(clippy::many_single_char_names, clippy::float_cmp)]
fn ln_contracted(x: f64) -> f64 {
    const LN2_HI: f64 = f64::from_bits(0x3fe6_2e42_fee0_0000);
    const LN2_LO: f64 = f64::from_bits(0x3dea_39ef_3579_3c76);
    const LG: [f64; 7] = [
        f64::from_bits(0x3fe5_5555_5555_5593),
        f64::from_bits(0x3fd9_9999_9997_fa04),
        f64::from_bits(0x3fd2_4924_9422_9359),
        f64::from_bits(0x3fcc_71c5_1d8e_78af),
        f64::from_bits(0x3fc7_4664_96cb_03de),
        f64::from_bits(0x3fc3_9a09_d078_c69f),
        f64::from_bits(0x3fc2_f112_df3e_5244),
    ];
    let high = |x: f64| (x.to_bits() >> 32) as u32 as i32;
    let mut x = x;
    let mut hx = high(x);
    let mut k: i32 = 0;
    if hx < 0x0010_0000 {
        if x == 0.0 {
            return f64::NEG_INFINITY;
        }
        if hx < 0 {
            return f64::NAN;
        }
        k -= 54;
        x *= f64::from_bits(0x4350_0000_0000_0000);
        hx = high(x);
    }
    if hx >= 0x7ff0_0000 {
        return x + x;
    }
    k += (hx >> 20) - 1023;
    hx &= 0x000f_ffff;
    let i = (hx + 0x95f64) & 0x0010_0000;
    x = f64::from_bits((u64::from((hx | (i ^ 0x3ff0_0000)) as u32) << 32) | (x.to_bits() & 0xffff_ffff));
    k += i >> 20;
    let f = x - 1.0;
    let dk = f64::from(k);
    if (0x000f_ffff & (2 + hx)) < 3 {
        if f == 0.0 {
            return if k == 0 { 0.0 } else { dk.mul_add(LN2_HI, dk * LN2_LO) };
        }
        let r = f * f * (-0.333_333_333_333_333_3f64).mul_add(f, 0.5);
        return if k == 0 { f - r } else { dk.mul_add(LN2_HI, -((-dk).mul_add(LN2_LO, r) - f)) };
    }
    let s = f / (2.0 + f);
    let z = s * s;
    let w = z * z;
    let t1 = w * w.mul_add(w.mul_add(LG[5], LG[3]), LG[1]);
    let t2 = z * w.mul_add(w.mul_add(w.mul_add(LG[6], LG[4]), LG[2]), LG[0]);
    let r = t2 + t1;
    if ((hx - 0x6147a) | (0x6b851 - hx)) > 0 {
        let hfsq = 0.5 * f * f;
        if k == 0 {
            return f - (-s).mul_add(hfsq + r, hfsq);
        }
        return dk.mul_add(LN2_HI, -((hfsq - s.mul_add(hfsq + r, dk * LN2_LO)) - f));
    }
    if k == 0 {
        return (-s).mul_add(f - r, f);
    }
    dk.mul_add(LN2_HI, -(s.mul_add(f - r, -(dk * LN2_LO)) - f))
}

#[test]
fn contracted_fdlibm_equals_every_golden_log_on_aarch64() {
    if !cfg!(target_arch = "aarch64") {
        return;
    }
    let Some(rows) = rows::<LogRow>("log.jsonl") else { return };
    assert_eq!(rows.len(), 200);
    for row in rows {
        let x = number(&row.x);
        assert_eq!(ln_contracted(x).to_bits(), number(&row.ln).to_bits(), "ln({x})");
    }
}

#[test]
fn ln_matches_every_golden_log_for_the_platform() {
    let Some(rows) = rows::<LogRow>("log.jsonl") else { return };
    assert_eq!(rows.len(), 200);
    let allowed = u64::from(!cfg!(target_arch = "x86_64"));
    for row in &rows {
        let (got, expected) = (ln(number(&row.x)), number(&row.ln));
        assert!(
            ulps(got, expected) <= allowed,
            "ln({}): got {got:?}, Node {expected:?}, more than {allowed} ulp apart; {FMA_NOTE}",
            row.x.get()
        );
    }
}

#[test]
fn ln_handles_special_values() {
    assert_eq!(ln(0.0), f64::NEG_INFINITY);
    assert_eq!(ln(-0.0), f64::NEG_INFINITY);
    assert!(ln(-1.0).is_nan());
    assert!(ln(f64::NAN).is_nan());
    assert_eq!(ln(f64::INFINITY), f64::INFINITY);
    assert_eq!(ln(1.0).to_bits(), 0.0f64.to_bits());
}

struct Mulberry32(u32);

impl Mulberry32 {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_add(0x6D2B_79F5);
        let mut t = self.0;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 0x3d));
        t ^ (t >> 14)
    }
}

fn from_words(high: u32, low: u32) -> f64 {
    f64::from_bits((u64::from(high) << 32) | u64::from(low))
}

fn log_inputs() -> Vec<f64> {
    let mut next = Mulberry32(SEED);
    let mut inputs = Vec::new();
    for _ in 0..1_000_000 {
        let high = next.next() & 0x7fff_ffff;
        let low = next.next();
        if high < 0x7ff0_0000 {
            inputs.push(from_words(high, low));
        }
    }
    for _ in 0..200_000 {
        let offset = i64::from(next.next() % 0x20_0000) - 0x10_0000;
        inputs.push(if offset < 0 {
            from_words(0x3fef_ffff, (0x1_0000_0000 + offset) as u32)
        } else {
            from_words(0x3ff0_0000, offset as u32)
        });
    }
    for i in 1..=200_000 {
        inputs.push(f64::from(i));
        inputs.push(1.0 / f64::from(i));
    }
    for total in [1.0, 100.0, 19160.0, 20000.0, 25000.0] {
        for count in 0..=30_000 {
            inputs.push(total / f64::from(1 + count));
        }
    }
    inputs
}

fn digest(values: impl Iterator<Item = f64>) -> (usize, String) {
    let mut hash = Sha256::new();
    let mut count = 0;
    for value in values {
        hash.update(value.to_le_bytes());
        count += 1;
    }
    let hex = hash.finalize().iter().fold(String::new(), |mut hex, byte| {
        write!(hex, "{byte:02x}").unwrap();
        hex
    });
    (count, hex)
}

#[derive(Deserialize)]
struct Sweep {
    count: usize,
    sha256: String,
}

#[derive(Deserialize)]
struct Sweeps {
    log: Sweep,
    jw: Sweep,
}

fn sweeps() -> Option<Sweeps> {
    let path = golden("kernel_sweep.json")?;
    Some(serde_json::from_reader(File::open(path).unwrap()).unwrap())
}

#[test]
fn contracted_fdlibm_equals_node_on_a_wide_sweep_on_aarch64() {
    if !cfg!(target_arch = "aarch64") {
        return;
    }
    let Some(sweeps) = sweeps() else { return };
    let inputs = log_inputs();
    let (count, sha256) = digest(inputs.iter().map(|&x| ln_contracted(x)));
    assert_eq!((count, sha256), (sweeps.log.count, sweeps.log.sha256));
}

#[test]
fn ln_is_fdlibm_on_a_wide_sweep() {
    let inputs = log_inputs();
    assert!(inputs.iter().all(|&x| ulps(ln(x), ln_contracted(x)) <= 1), "{FMA_NOTE}");
    if !cfg!(target_arch = "x86_64") {
        return;
    }
    let Some(sweeps) = sweeps() else { return };
    let (count, sha256) = digest(inputs.iter().map(|&x| ln(x)));
    assert_eq!((count, sha256), (sweeps.log.count, sweeps.log.sha256));
}

fn jw_pairs() -> Vec<(Vec<u16>, Vec<u16>)> {
    let mut next = Mulberry32(SEED + 1);
    let word = |next: &mut Mulberry32| -> Vec<u16> {
        let length = next.next() % 25;
        (0..length).map(|_| ALPHABET[(next.next() % ALPHABET.len() as u32) as usize]).collect()
    };
    let mut pairs = Vec::new();
    for _ in 0..300_000 {
        let a = word(&mut next);
        if next.next() % 2 == 1 {
            let b = word(&mut next);
            pairs.push((a, b));
            continue;
        }
        let mut units = a.clone();
        let at = if units.is_empty() { 0 } else { (next.next() % units.len() as u32) as usize };
        match next.next() % 3 {
            0 => {
                if at < units.len() {
                    units.remove(at);
                }
            }
            1 => units.insert(at, ALPHABET[(next.next() % ALPHABET.len() as u32) as usize]),
            _ => {
                if units.len() > 1 {
                    let other = (at + 1) % units.len();
                    units.swap(at, other);
                }
            }
        }
        pairs.push((a, units));
    }
    pairs
}

#[test]
fn jaro_winkler_equals_node_on_a_wide_sweep() {
    let Some(sweeps) = sweeps() else { return };
    let pairs = jw_pairs();
    let (count, sha256) = digest(pairs.iter().map(|(a, b)| distance(a, b)));
    assert_eq!((count, sha256), (sweeps.jw.count, sweeps.jw.sha256));
}

static INDEX: LazyLock<Index> = LazyLock::new(|| {
    let sources =
        Sources::read(&data("search_titles.jsonl"), &data("search_aliases.json"), &data("imdb_mapping.json")).unwrap();
    Index::build(&sources.records, &sources.mapping, &sources.aliases)
});

#[derive(Deserialize)]
struct Candidate {
    id: u64,
    score: Box<RawValue>,
}

#[derive(Deserialize)]
struct CandidatesRow {
    query: String,
    candidates: Vec<Candidate>,
}

fn scored(query: &str) -> Vec<(u64, f64)> {
    let parsed = parse_query(query);
    if parsed.base.is_empty() {
        return Vec::new();
    }
    let query = Query::new(&INDEX, &parsed);
    candidates(&INDEX, &query)
        .into_iter()
        .map(|entry| (INDEX.entries()[entry as usize].id, title_score(&INDEX, entry, &query).score))
        .collect()
}

#[test]
fn candidates_and_title_scores_equal_every_golden_query() {
    let Some(rows) = rows::<CandidatesRow>("candidates.jsonl") else { return };
    assert_eq!(rows.len(), 200);
    let mut differences = Vec::new();
    for row in &rows {
        let got = scored(&row.query);
        let ids: Vec<u64> = got.iter().map(|(id, _)| *id).collect();
        let expected: Vec<u64> = row.candidates.iter().map(|candidate| candidate.id).collect();
        if ids != expected {
            differences.push(format!("{:?}: ids differ ({} got, {} expected)", row.query, ids.len(), expected.len()));
            continue;
        }
        for ((id, score), candidate) in got.iter().zip(&row.candidates) {
            let expected = number(&candidate.score);
            if js_fixed_12(*score).to_bits() != expected.to_bits() {
                differences.push(format!("{:?} {id}: got {score}, expected {expected}", row.query));
            }
        }
    }
    assert!(
        differences.is_empty(),
        "{} differences: {:#?}",
        differences.len(),
        &differences[..differences.len().min(20)]
    );
}
