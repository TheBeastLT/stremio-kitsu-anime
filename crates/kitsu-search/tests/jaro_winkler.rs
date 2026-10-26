use kitsu_search::score::jaro_winkler;

fn jw(a: &str, b: &str) -> f64 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    jaro_winkler(&a, &b)
}

fn assert_close(got: f64, expected: f64) {
    assert!((got - expected).abs() < 1e-12, "got {got}, expected {expected}");
}

#[test]
fn two_empty_strings_score_zero() {
    assert_close(jw("", ""), 0.0);
}

#[test]
fn identical_strings_score_one() {
    assert_close(jw("naruto", "naruto"), 1.0);
}

#[test]
fn different_single_characters_score_zero() {
    assert_close(jw("a", "b"), 0.0);
}

#[test]
fn a_zero_window_matches_only_the_same_position() {
    assert_close(jw("ab", "ba"), 0.0);
}

#[test]
fn transpositions_count_half() {
    assert_close(jw("martha", "marhta"), 0.961_111_111_111_111_1);
}

#[test]
fn the_window_follows_the_longer_string() {
    assert_close(jw("dixon", "dicksonx"), 0.813_333_333_333_333_2);
}

#[test]
fn the_prefix_boost_stops_at_four() {
    assert_close(jw("abcdefgh", "abcdefgx"), 0.95);
}

#[test]
fn repeated_letters_match_once() {
    assert_close(jw("aaa", "aa"), 0.911_111_111_111_111_1);
}

#[test]
fn a_prefix_of_a_longer_string_is_boosted() {
    assert_close(jw("ab", "abcde"), 0.84);
}

#[test]
fn reversed_strings_match_half() {
    assert_close(jw("abcd", "dcba"), 0.5);
}

#[test]
fn no_boost_without_a_shared_first_letter() {
    assert_close(jw("crate", "trace"), 0.733_333_333_333_333_4);
}

#[test]
fn a_one_letter_prefix_is_boosted_once() {
    assert_close(jw("a", "ab"), 0.850_000_000_000_000_1);
}

#[test]
fn long_strings_score_as_short_ones() {
    let a = "naruto shippuden ".repeat(10);
    let b = format!("{}x", "naruto shipuuden ".repeat(9));
    assert_close(jw(&a, &b), 0.913_429_890_013_290_6);
}

#[test]
fn astral_characters_compare_as_whole_chars() {
    assert_close(jw("😀", "😁"), 0.0);
    assert_close(jw("😀a", "😁a"), 0.666_666_666_666_666_6);
}
