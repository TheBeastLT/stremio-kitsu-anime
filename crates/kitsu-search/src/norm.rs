use std::sync::LazyLock;

use fastjs_regex::Regex;
use unicode_normalization::UnicodeNormalization;

use crate::rx;

static POSSESSIVE: LazyLock<Regex> = LazyLock::new(|| rx(r"(\w)'s\b", "g"));
static SEPARATORS: LazyLock<Regex> = LazyLock::new(|| rx(r"[^\p{L}\p{N}]+", "gu"));
static LETTER_DIGIT: LazyLock<Regex> = LazyLock::new(|| rx(r"(\p{L})(\d)", "gu"));
static DIGIT_LETTER: LazyLock<Regex> = LazyLock::new(|| rx(r"(\d)(?!(?:st|nd|rd|th|d)\b)(\p{L})", "gu"));
static STANDALONE_X: LazyLock<Regex> = LazyLock::new(|| rx(r"(^| )x(?= \D| ?$)", "g"));

pub(crate) fn without_marks(text: &str) -> String {
    text.nfkd().filter(|c| !('\u{300}'..='\u{36f}').contains(c)).collect::<String>().to_lowercase()
}

pub(crate) fn squash(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn norm(text: &str) -> String {
    let spaced = without_marks(text).replace('×', " x ").replace('&', " and ");
    let plain = POSSESSIVE.replace(&spaced, "$1s");
    let words = SEPARATORS.replace(&plain, " ");
    let split = LETTER_DIGIT.replace(&words, "$1 $2");
    let split = DIGIT_LETTER.replace(&split, "$1 $2");
    let split = STANDALONE_X.replace(&split, " ");
    squash(&split.replace("ou", "o").replace("uu", "u"))
}
