use kitsu_search::norm::norm;

fn check(input: &str, expected: &str) {
    assert_eq!(norm(input), expected);
}

#[test]
fn greek_capital_sigma_inside_a_word_stays_medial() {
    check("ΟΔΥΣΣΕΥΣ", "οδυσσευς");
}

#[test]
fn final_sigma_before_a_space() {
    check("ΑΣ ΒΑ", "ας βα");
}

#[test]
fn final_sigma_before_punctuation() {
    check("ΑΣ.", "ας");
}

#[test]
fn lone_sigma_stays_medial() {
    check("Σ", "σ");
}

#[test]
fn final_sigma_after_a_latin_letter() {
    check("aΣ", "aς");
}

#[test]
fn final_sigma_before_a_digit() {
    check("ΑΣ1", "ας 1");
}

#[test]
fn final_sigma_before_a_combining_mark() {
    check("ΑΣ\u{301}", "ας");
}

#[test]
fn final_sigma_before_a_soft_hyphen() {
    check("ΑΣ\u{ad}", "ας");
}

#[test]
fn sigma_before_a_right_quote_and_letter_stays_medial() {
    check("ΑΣ\u{2019}b", "ασ b");
}

#[test]
fn final_sigma_before_a_hyphen() {
    check("ΑΣ-Β", "ας β");
}

#[test]
fn final_sigma_before_an_underscore() {
    check("ΑΣ_Β", "ας β");
}

#[test]
fn sigma_before_a_zero_width_space_and_letter_stays_medial() {
    check("ΑΣ\u{200b}Β", "ασ β");
}

#[test]
fn capital_i_with_dot_above() {
    check("İstanbul", "istanbul");
}

#[test]
fn capital_i_with_combining_dot() {
    check("I\u{307}", "i");
}

#[test]
fn titlecase_digraph_dz() {
    check("ǅ", "dz");
}

#[test]
fn ligature_ffi() {
    check("ﬃ", "ffi");
}

#[test]
fn roman_numeral_twelve() {
    check("Ⅻ", "xii");
}

#[test]
fn fullwidth_letters_and_digits() {
    check("ＡＢＣ１２", "abc 12");
}

#[test]
fn vulgar_fraction_one_half() {
    check("½", "1 2");
}

#[test]
fn unicode_whitespace_is_trimmed() {
    check("\u{2003} a\u{a0}b\u{3000}", "a b");
}

#[test]
fn astral_mathematical_bold_a() {
    check("\u{1d400}", "a");
}

#[test]
fn georgian_mtavruli_lowercases_to_mkhedruli_only_as_nfkd_leaves_it() {
    check("Ⴀ", "ⴀ");
}

#[test]
fn cyrillic_small_letter_rounded_ve_is_kept() {
    check("\u{1c80}", "\u{1c80}");
}

#[test]
fn letter_added_in_unicode_17_u088f_is_kept() {
    check("a\u{88f}b", "a\u{88f}b");
}

#[test]
fn letter_added_in_unicode_17_ends_a_digit_run() {
    check("1\u{a7cf}", "1 \u{a7cf}");
}

#[test]
fn letter_added_in_unicode_17_follows_a_digit_split() {
    check("\u{10940}1", "\u{10940} 1");
}

#[test]
fn number_added_in_unicode_17_u11de0_is_kept() {
    check("a\u{11de0}", "a\u{11de0}");
}

#[test]
fn number_added_in_unicode_17_is_not_a_letter_for_splitting() {
    check("a\u{11de0}1", "a\u{11de0}1");
}

#[test]
fn armenian_u0558_is_decomposed_although_node_kept_it_unassigned() {
    check("1\u{558}a", "1 \u{567}a");
}
