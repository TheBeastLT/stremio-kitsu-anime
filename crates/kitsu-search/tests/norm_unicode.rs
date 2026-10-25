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
fn js_whitespace_is_trimmed() {
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
fn letter_added_after_regress_tables_u088f_is_kept() {
    check("a\u{88f}b", "a\u{88f}b");
}

#[test]
fn letter_added_after_regress_tables_ends_a_digit_run() {
    check("1\u{a7cf}", "1 \u{a7cf}");
}

#[test]
fn letter_added_after_regress_tables_follows_a_digit_split() {
    check("\u{10940}1", "\u{10940} 1");
}

#[test]
fn number_added_after_regress_tables_u11de0_is_kept() {
    check("a\u{11de0}", "a\u{11de0}");
}

#[test]
fn unassigned_armenian_u0558_is_not_decomposed() {
    check("1\u{558}a", "1 a");
}

#[test]
fn unassigned_armenian_u058b_is_not_decomposed() {
    check("a\u{58b}", "a");
}

#[test]
fn unassigned_mathematical_u1dfcd_is_not_decomposed() {
    check("a\u{1dfcd}", "a");
}

#[test]
fn unassigned_run_does_not_reorder_neighbouring_marks() {
    check("e\u{301}\u{558}\u{301}e", "e e");
}

#[test]
fn regress_tables_do_not_know_the_added_letters_yet() {
    let rx = fastjs_regex::Regex::new(r"^[\p{L}\p{N}]$", "u").unwrap();
    for &(start, end) in kitsu_search::unicode::NODE_ONLY_LETTERS.iter().chain(kitsu_search::unicode::NODE_ONLY_NUMBERS)
    {
        for c in (start..=end).filter_map(char::from_u32) {
            assert!(!rx.is_match(&c.to_string()), "{c:?} is known to regress now: shrink the table");
        }
    }
}

#[test]
fn number_added_after_regress_tables_is_not_a_letter_for_splitting() {
    check("a\u{11de0}1", "a\u{11de0}1");
}

#[test]
fn nfkd_table_is_checked_against_the_unicode_version_of_the_crate() {
    assert_eq!(
        unicode_normalization::UNICODE_VERSION,
        (18, 0, 0),
        "unicode-normalization changed its Unicode data: re-derive NFKD_UNASSIGNED_IN_NODE against Node"
    );
}

#[test]
fn nfkd_table_lists_only_code_points_the_crate_decomposes() {
    use unicode_normalization::UnicodeNormalization;
    for &(start, end) in kitsu_search::unicode::NFKD_UNASSIGNED_IN_NODE {
        for c in (start..=end).filter_map(char::from_u32) {
            let decomposed: String = std::iter::once(c).nfkd().collect();
            assert_ne!(decomposed, c.to_string(), "{c:?} is no longer decomposed by the crate: shrink the table");
        }
    }
}
