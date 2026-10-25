use std::sync::LazyLock;

use fastjs_regex::Regex;
use unicode_normalization::UnicodeNormalization;

use crate::unicode::{NFKD_UNASSIGNED_IN_NODE, NODE_ONLY_LETTERS, NODE_ONLY_NUMBERS, class_members, in_ranges};

fn rx(source: &str, flags: &str) -> Regex {
    Regex::new(source, flags).unwrap_or_else(|error| panic!("invalid pattern {source}: {error}"))
}

static COMBINING: LazyLock<Regex> = LazyLock::new(|| rx("[\u{300}-\u{36f}]", "g"));
static TIMES: LazyLock<Regex> = LazyLock::new(|| rx("×", "g"));
static AMPERSAND: LazyLock<Regex> = LazyLock::new(|| rx("&", "g"));
static POSSESSIVE: LazyLock<Regex> = LazyLock::new(|| rx(r"(\w)'s\b", "g"));
static LETTERS: LazyLock<String> = LazyLock::new(|| class_members(NODE_ONLY_LETTERS));
static NUMBERS: LazyLock<String> = LazyLock::new(|| class_members(NODE_ONLY_NUMBERS));
static SEPARATORS: LazyLock<Regex> =
    LazyLock::new(|| rx(&format!(r"[^\p{{L}}\p{{N}}{}{}]+", *LETTERS, *NUMBERS), "gu"));
static LETTER_DIGIT: LazyLock<Regex> = LazyLock::new(|| rx(&format!(r"([\p{{L}}{}])(\d)", *LETTERS), "gu"));
static DIGIT_LETTER: LazyLock<Regex> =
    LazyLock::new(|| rx(&format!(r"(\d)(?!(?:st|nd|rd|th|d)\b)([\p{{L}}{}])", *LETTERS), "gu"));
static STANDALONE_X: LazyLock<Regex> = LazyLock::new(|| rx(r"(^| )x(?= \D| ?$)", "g"));
static OU: LazyLock<Regex> = LazyLock::new(|| rx("ou", "g"));
static UU: LazyLock<Regex> = LazyLock::new(|| rx("uu", "g"));
static WHITESPACE: LazyLock<Regex> = LazyLock::new(|| rx(r"\s+", "g"));

const JS_WHITESPACE: &[char] = &[
    '\u{9}', '\u{a}', '\u{b}', '\u{c}', '\u{d}', '\u{20}', '\u{a0}', '\u{1680}', '\u{2000}', '\u{2001}', '\u{2002}',
    '\u{2003}', '\u{2004}', '\u{2005}', '\u{2006}', '\u{2007}', '\u{2008}', '\u{2009}', '\u{200a}', '\u{2028}',
    '\u{2029}', '\u{202f}', '\u{205f}', '\u{3000}', '\u{feff}',
];

pub(crate) fn nfkd(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut run = String::new();
    for c in text.chars() {
        if in_ranges(NFKD_UNASSIGNED_IN_NODE, c) {
            out.extend(run.drain(..).nfkd());
            out.push(c);
        } else {
            run.push(c);
        }
    }
    out.extend(run.nfkd());
    out
}

pub fn norm(text: &str) -> String {
    let decomposed = nfkd(text);
    let stripped = COMBINING.replace(&decomposed, "");
    let lowered = stripped.to_lowercase();
    let spaced = TIMES.replace(&lowered, " x ");
    let spaced = AMPERSAND.replace(&spaced, " and ");
    let plain = POSSESSIVE.replace(&spaced, "$1s");
    let words = SEPARATORS.replace(&plain, " ");
    let split = LETTER_DIGIT.replace(&words, "$1 $2");
    let split = DIGIT_LETTER.replace(&split, "$1 $2");
    let split = STANDALONE_X.replace(&split, " ");
    let folded = OU.replace(&split, "o");
    let folded = UU.replace(&folded, "u");
    WHITESPACE.replace(js_trim(&folded), " ").into_owned()
}

pub fn js_trim(text: &str) -> &str {
    text.trim_matches(JS_WHITESPACE)
}
