use kitsu_search::jaro_winkler::distance;

fn jw(a: &str, b: &str) -> u64 {
    let a: Vec<u16> = a.encode_utf16().collect();
    let b: Vec<u16> = b.encode_utf16().collect();
    distance(&a, &b).to_bits()
}

#[test]
fn two_empty_strings_score_zero() {
    assert_eq!(jw("", ""), 0.0f64.to_bits());
}

#[test]
fn identical_strings_score_one() {
    assert_eq!(jw("naruto", "naruto"), 1.0f64.to_bits());
}

#[test]
fn single_characters_have_a_negative_window() {
    assert_eq!(jw("a", "b"), 0.0f64.to_bits());
}

#[test]
fn a_zero_window_matches_only_the_same_position() {
    assert_eq!(jw("ab", "ba"), 0.0f64.to_bits());
}

#[test]
fn transpositions_count_half() {
    assert_eq!(jw("martha", "marhta"), 0.961_111_111_111_111_1f64.to_bits());
}

#[test]
fn the_window_follows_the_longer_string() {
    assert_eq!(jw("dixon", "dicksonx"), 0.813_333_333_333_333_2f64.to_bits());
}

#[test]
fn the_prefix_boost_stops_at_four() {
    assert_eq!(jw("abcdefgh", "abcdefgx"), 0.95f64.to_bits());
}

#[test]
fn astral_characters_compare_as_code_units() {
    assert_eq!(jw("😀", "😁"), 0.666_666_666_666_666_6f64.to_bits());
}

#[test]
fn a_shared_high_surrogate_counts_as_a_prefix() {
    assert_eq!(jw("😀a", "😁a"), 0.799_999_999_999_999_9f64.to_bits());
}

#[test]
fn repeated_letters_match_once() {
    assert_eq!(jw("aaa", "aa"), 0.911_111_111_111_111_1f64.to_bits());
}

#[test]
fn a_prefix_of_a_longer_string_is_boosted() {
    assert_eq!(jw("ab", "abcde"), 0.84f64.to_bits());
}

#[test]
fn reversed_strings_match_half() {
    assert_eq!(jw("abcd", "dcba"), 0.5f64.to_bits());
}

#[test]
fn no_boost_without_a_shared_first_letter() {
    assert_eq!(jw("crate", "trace"), 0.733_333_333_333_333_4f64.to_bits());
}

#[test]
fn a_one_letter_prefix_is_boosted_once() {
    assert_eq!(jw("a", "ab"), 0.850_000_000_000_000_1f64.to_bits());
}

#[test]
fn long_strings_score_as_short_ones() {
    let a = "naruto shippuden ".repeat(10);
    let b = format!("{}x", "naruto shipuuden ".repeat(9));
    assert_eq!(jw(&a, &b), 0.913_429_890_013_290_6f64.to_bits());
}
