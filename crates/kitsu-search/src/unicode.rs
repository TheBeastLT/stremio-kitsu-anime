use std::fmt::Write;

pub const NODE_ONLY_LETTERS: &[(u32, u32)] = &[
    (0x88F, 0x88F),
    (0xC5C, 0xC5C),
    (0xCDC, 0xCDC),
    (0xA7CE, 0xA7CF),
    (0xA7D2, 0xA7D2),
    (0xA7D4, 0xA7D4),
    (0xA7F1, 0xA7F1),
    (0x10940, 0x10959),
    (0x10EC5, 0x10EC7),
    (0x11DB0, 0x11DDB),
    (0x16EA0, 0x16EB8),
    (0x16EBB, 0x16ED3),
    (0x16FF2, 0x16FF3),
    (0x187F8, 0x187FF),
    (0x18D09, 0x18D1E),
    (0x18D80, 0x18DF2),
    (0x1E6C0, 0x1E6DE),
    (0x1E6E0, 0x1E6E2),
    (0x1E6E4, 0x1E6E5),
    (0x1E6E7, 0x1E6ED),
    (0x1E6F0, 0x1E6F4),
    (0x1E6FE, 0x1E6FF),
    (0x2B73A, 0x2B73F),
    (0x2CEA2, 0x2CEAD),
    (0x323B0, 0x33479),
];

pub const NODE_ONLY_NUMBERS: &[(u32, u32)] = &[(0x11DE0, 0x11DE9), (0x16FF4, 0x16FF6)];

pub const NFKD_UNASSIGNED_IN_NODE: &[(u32, u32)] = &[
    (0x558, 0x558),
    (0x58B, 0x58C),
    (0x209D, 0x209F),
    (0x107BB, 0x107BF),
    (0x1B123, 0x1B126),
    (0x1D6A6, 0x1D6A6),
    (0x1DF95, 0x1DF95),
    (0x1DFCD, 0x1DFFF),
];

pub fn in_ranges(ranges: &[(u32, u32)], c: char) -> bool {
    let c = c as u32;
    ranges
        .binary_search_by(|&(start, end)| {
            if c < start {
                std::cmp::Ordering::Greater
            } else if c > end {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

pub fn class_members(ranges: &[(u32, u32)]) -> String {
    let mut class = String::new();
    for &(start, end) in ranges {
        let _ = write!(class, "\\u{{{start:X}}}-\\u{{{end:X}}}");
    }
    class
}
