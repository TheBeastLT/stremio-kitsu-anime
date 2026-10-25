const PREFIX_SCALE: f64 = 0.1;
const BOOST_THRESHOLD: f64 = 0.7;
const MAX_PREFIX: usize = 4;
const STACK_FLAGS: usize = 256;

/// The npm `jaro-winkler` 0.2.8 `distance` (a similarity, 1 for equal strings) over UTF-16 code units, operation for
/// operation: its match window `floor(max / 2) - 1` may be negative, and the prefix boost only applies above 0.7.
pub fn distance(s1: &[u16], s2: &[u16]) -> f64 {
    if s1.is_empty() || s2.is_empty() {
        return 0.0;
    }
    if s1 == s2 {
        return 1.0;
    }
    let range = (s1.len().max(s2.len()) / 2) as i64 - 1;
    let last = s2.len() as i64 - 1;
    let mut stack = [false; STACK_FLAGS];
    let mut heap = Vec::new();
    let flags = if s1.len() + s2.len() <= STACK_FLAGS {
        &mut stack[..s1.len() + s2.len()]
    } else {
        heap.resize(s1.len() + s2.len(), false);
        &mut heap[..]
    };
    let (s1_matches, s2_matches) = flags.split_at_mut(s1.len());
    let mut m: u32 = 0;
    for (i, &unit) in s1.iter().enumerate() {
        let at = i as i64;
        let low = if at >= range { at - range } else { 0 };
        let high = if at + range <= last { at + range } else { last };
        if low > high {
            continue;
        }
        let window = low as usize..=high as usize;
        let taken = &mut s2_matches[window.clone()];
        if let Some(j) = s2[window].iter().zip(taken.iter()).position(|(&other, &taken)| other == unit && !taken) {
            m += 1;
            s1_matches[i] = true;
            taken[j] = true;
        }
    }
    if m == 0 {
        return 0.0;
    }
    let mut k = 0;
    let mut transpositions: u32 = 0;
    for i in 0..s1.len() {
        if s1_matches[i] {
            let mut j = k;
            while j < s2.len() {
                if s2_matches[j] {
                    k = j + 1;
                    break;
                }
                j += 1;
            }
            if Some(&s1[i]) != s2.get(j) {
                transpositions += 1;
            }
        }
    }
    let m = f64::from(m);
    let mut weight = (m / s1.len() as f64 + m / s2.len() as f64 + (m - f64::from(transpositions) / 2.0) / m) / 3.0;
    if weight > BOOST_THRESHOLD {
        let mut l = 0;
        while s1.get(l) == s2.get(l) && l < MAX_PREFIX {
            l += 1;
        }
        weight = weight + l as f64 * PREFIX_SCALE * (1.0 - weight);
    }
    weight
}
