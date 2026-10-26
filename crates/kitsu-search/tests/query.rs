use kitsu_search::query::{
    compact, extra_family, extra_number_of, glue, movie_prefix_of, numerals_of, parse_query, punct_key, roman_numeral,
    season_of, seasonless_title, split_titles, title_base_of, title_movie_index_of,
};
use serde_json::Value;

fn check_parse(query: &str, expected: &str) {
    let expected: Value = serde_json::from_str(expected).unwrap();
    assert_eq!(serde_json::to_value(parse_query(query)).unwrap(), expected);
}

fn check_glue(title: &[&str], query: &[&str], initials_ok: bool, words: &[&str], initials: &[&str]) {
    let glued = glue(title, query, initials_ok);
    assert_eq!(glued.words, words);
    assert_eq!(glued.initials.iter().collect::<Vec<_>>(), initials);
}

#[test]
fn numerals_of_reads_roman_and_short_numbers() {
    assert_eq!(numerals_of("constructor ii 12 abc 12345"), [2, 12]);
}

#[test]
fn roman_numeral_out_of_range() {
    assert_eq!((roman_numeral(99), roman_numeral(0), roman_numeral(3)), (None, None, Some("iii")));
}

#[test]
fn title_movie_index_of_numbered_movie() {
    assert_eq!(title_movie_index_of("show 2 the movie", false), Some(2));
}

#[test]
fn title_movie_index_of_chapter_only_for_movies() {
    assert_eq!(
        (title_movie_index_of("show chapter 3", true), title_movie_index_of("show chapter 3", false)),
        (Some(3), None)
    );
}

#[test]
fn title_movie_index_of_movie_number() {
    assert_eq!(title_movie_index_of("show movie 3", false), Some(3));
}

#[test]
fn title_base_of_strips_season_year_extra_and_markers() {
    assert_eq!(title_base_of("show 2nd season 2019 ova recap", false), "show");
}

#[test]
fn title_base_of_empty() {
    assert_eq!(title_base_of("", false), "");
}

#[test]
fn extra_number_of_reads_the_number() {
    assert_eq!(extra_number_of("show ova 03"), Some(3));
}

#[test]
fn extra_family_groups_specials() {
    assert_eq!((extra_family("sp"), extra_family("specials"), extra_family("recap")), ("special", "special", "recap"));
}

#[test]
fn movie_prefix_of_cuts_at_colon() {
    assert_eq!(movie_prefix_of("Pokémon: The Movie 20 - Koko"), "pokemon");
}

#[test]
fn seasonless_title_drops_season_and_year() {
    assert_eq!(seasonless_title("Show Season 2 2019"), "show");
}

#[test]
fn punct_key_drops_possessive_apostrophe() {
    assert_eq!(punct_key("Kuroko’s Basket"), "kurokosbasket");
}

#[test]
fn compact_removes_spaces() {
    assert_eq!(compact("a b c"), "abc");
}

#[test]
fn season_of_bare_ordinal_beside_s_number() {
    assert_eq!(season_of("show 4th 2026 s 2"), Some(4));
}

#[test]
fn season_of_bare_ordinal_after_the_is_a_name() {
    assert_eq!(season_of("lupin the 3rd s 2"), Some(2));
}

#[test]
fn season_of_bare_ordinal_before_part_is_not_bare() {
    assert_eq!(season_of("show 4th part s 2"), Some(2));
}

#[test]
fn season_of_s_number() {
    assert_eq!(season_of("show s 2"), Some(2));
}

#[test]
fn season_of_season_number() {
    assert_eq!(season_of("show season 12"), Some(12));
}

#[test]
fn season_of_seasn_typo() {
    assert_eq!(season_of("show seasn 3"), Some(3));
}

#[test]
fn season_of_nth_season() {
    assert_eq!(season_of("show 2nd season"), Some(2));
}

#[test]
fn season_of_season_before_part() {
    assert_eq!(season_of("shingeki no kyojin 3 part 2"), Some(3));
}

#[test]
fn season_of_season_before_cour_non_ascii() {
    assert_eq!(season_of("進撃の巨人 3 cour 2"), Some(3));
}

#[test]
fn season_of_number_above_twenty_before_part_is_a_name() {
    assert_eq!(season_of("86 part 2"), Some(2));
}

#[test]
fn season_of_part_number() {
    assert_eq!(season_of("mushoku tensei part 2"), Some(2));
}

#[test]
fn season_of_cour_number() {
    assert_eq!(season_of("mushoku tensei cour 2"), Some(2));
}

#[test]
fn season_of_parte_number() {
    assert_eq!(season_of("show parte 3"), Some(3));
}

#[test]
fn season_of_nth_part() {
    assert_eq!(season_of("show 2nd part"), Some(2));
}

#[test]
fn season_of_nth_cour() {
    assert_eq!(season_of("show 3rd cor"), Some(3));
}

#[test]
fn season_of_ordinal_season() {
    assert_eq!(season_of("show second season"), Some(2));
}

#[test]
fn season_of_ordinal_part() {
    assert_eq!(season_of("show twentieth part"), Some(20));
}

#[test]
fn season_of_roman_part() {
    assert_eq!(season_of("show part iii"), Some(3));
}

#[test]
fn season_of_roman_season_longest_first() {
    assert_eq!(season_of("show season xviii"), Some(18));
}

#[test]
fn season_of_ending_roman() {
    assert_eq!(season_of("maou gakuin no futekigosha ii"), Some(2));
}

#[test]
fn season_of_ending_roman_needs_three_words() {
    assert_eq!(season_of("lupin iii"), None);
}

#[test]
fn season_of_ending_single_letter_roman_is_not_a_season() {
    assert_eq!(season_of("rockman zero x"), None);
}

#[test]
fn season_of_x_number() {
    assert_eq!(season_of("show x 2"), Some(2));
}

#[test]
fn season_of_x_glued_number() {
    assert_eq!(season_of("show x2"), Some(2));
}

#[test]
fn season_of_final_season() {
    assert_eq!(season_of("show final season"), Some(99));
}

#[test]
fn season_of_final_chapter() {
    assert_eq!(season_of("show final chapter"), Some(99));
}

#[test]
fn season_of_trailing_digit() {
    assert_eq!(season_of("show 2"), Some(2));
}

#[test]
fn season_of_trailing_digit_after_astral() {
    assert_eq!(season_of("𝔸𝔹 show 2"), Some(2));
}

#[test]
fn season_of_trailing_digit_after_a_number() {
    assert_eq!(season_of("ranma 1 2"), None);
}

#[test]
fn season_of_trailing_digit_after_a_year() {
    assert_eq!(season_of("show 2026 3"), None);
}

#[test]
fn season_of_trailing_digit_after_five_digits() {
    assert_eq!(season_of("show 12345 3"), Some(3));
}

#[test]
fn season_of_nothing() {
    assert_eq!(season_of("cowboy bebop"), None);
}

#[test]
fn parse_moves_season_before_alternative_title() {
    check_parse(
        "kuroko's basketball 2 (kuroko no baske)",
        r#"{"full":"kurokos basketball kuroko no baske 2","text":"kurokos basketball kuroko no baske 2","base":"kurokos basketball kuroko no baske 2","punctKey":"kurokosbasketballkurokonobaske2","season":2,"tvdbSeason":false,"bareMovie":false,"theater":false,"markers":[],"isPack":false,"wantsExtra":false}"#,
    );
}

#[test]
fn parse_keeps_part_before_alternative_title() {
    check_parse(
        "exogenesis symphony part 3 (redemption)",
        r#"{"full":"exogenesis symphony part 3 redemption","text":"exogenesis symphony part 3 redemption","base":"exogenesis symphony redemption","punctKey":"exogenesissymphonypart3redemption","season":3,"part":3,"tvdbSeason":false,"bareMovie":false,"theater":false,"markers":[],"isPack":false,"wantsExtra":false}"#,
    );
}

#[test]
fn parse_keeps_volume_before_alternative_title() {
    check_parse(
        "show volume 3 (redemption)",
        r#"{"full":"show volume 3 redemption","text":"show volume 3 redemption","base":"show volume 3 redemption","punctKey":"showvolume3redemption","tvdbSeason":false,"bareMovie":false,"theater":false,"markers":[],"isPack":false,"wantsExtra":false}"#,
    );
}

#[test]
fn parse_lookbehind_is_case_insensitive() {
    check_parse(
        "show PART 3 (redemption)",
        r#"{"full":"show part 3 redemption","text":"show part 3 redemption","base":"show redemption","punctKey":"showpart3redemption","season":3,"part":3,"tvdbSeason":false,"bareMovie":false,"theater":false,"markers":[],"isPack":false,"wantsExtra":false}"#,
    );
}

#[test]
fn parse_moves_season_before_non_ascii_alternative_title() {
    check_parse(
        "shingeki 2 (進撃の巨人だ)",
        r#"{"full":"shingeki 進撃の巨人た 2","text":"shingeki 進撃の巨人た 2","base":"shingeki 進撃の巨人た 2","punctKey":"shingeki2","season":2,"tvdbSeason":false,"bareMovie":false,"theater":false,"markers":[],"isPack":false,"wantsExtra":false}"#,
    );
}

#[test]
fn parse_moves_season_before_astral_alternative_title() {
    check_parse(
        "show 2 (𝔸𝔹ℂ𝔻)",
        r#"{"full":"show abcd 2","text":"show abcd 2","base":"show abcd 2","punctKey":"showabcd2","season":2,"tvdbSeason":false,"bareMovie":false,"theater":false,"markers":[],"isPack":false,"wantsExtra":false}"#,
    );
}

#[test]
fn parse_moves_season_before_letters_newer_than_regress() {
    check_parse(
        "show 2 (𐥀𐥁𐥂𐥃)",
        r#"{"full":"show 𐥀𐥁𐥂𐥃 2","text":"show 𐥀𐥁𐥂𐥃 2","base":"show 𐥀𐥁𐥂𐥃 2","punctKey":"show2","season":2,"tvdbSeason":false,"bareMovie":false,"theater":false,"markers":[],"isPack":false,"wantsExtra":false}"#,
    );
}

#[test]
fn parse_needs_four_letters_in_alternative_title() {
    check_parse(
        "show 2 (abc)",
        r#"{"full":"show 2 abc","text":"show 2 abc","base":"show 2 abc","punctKey":"show2abc","tvdbSeason":false,"bareMovie":false,"theater":false,"markers":[],"isPack":false,"wantsExtra":false}"#,
    );
}

#[test]
fn parse_episode_revision() {
    check_parse(
        "show 01v2-12v2",
        r#"{"full":"show 01 12","text":"show 01 12","base":"show","punctKey":"show0112","episode":1,"tvdbSeason":false,"bareMovie":false,"theater":false,"markers":[],"isPack":false,"wantsExtra":false}"#,
    );
}

#[test]
fn parse_dash_episode() {
    check_parse(
        "dr stone - 05",
        r#"{"full":"dr stone 05","text":"dr stone","base":"dr stone","punctKey":"drstone05","episode":5,"tvdbSeason":false,"bareMovie":false,"theater":false,"markers":[],"isPack":false,"wantsExtra":false}"#,
    );
}

#[test]
fn parse_trailing_episode() {
    check_parse(
        "dr stone new world 16 v2",
        r#"{"full":"dr stone new world 16 v 2","text":"dr stone new world 16 v 2","base":"dr stone new world 16 v 2","punctKey":"drstonenewworld16v2","episode":16,"tvdbSeason":false,"bareMovie":false,"theater":false,"markers":[],"isPack":false,"wantsExtra":false}"#,
    );
}

#[test]
fn parse_s_number_is_not_an_episode() {
    check_parse(
        "show S17",
        r#"{"full":"show s 17","text":"show s 17","base":"show","punctKey":"shows17","season":17,"tvdbSeason":true,"bareMovie":false,"theater":false,"markers":[],"isPack":false,"wantsExtra":false}"#,
    );
}

#[test]
fn parse_year_range() {
    check_parse(
        "show 2019 2020",
        r#"{"full":"show 2019 2020","text":"show","base":"show","punctKey":"show20192020","year":2019,"tvdbSeason":false,"bareMovie":false,"theater":false,"markers":[],"isPack":false,"wantsExtra":false}"#,
    );
}

#[test]
fn parse_year_not_first_occurrence() {
    check_parse(
        "show 12020 2020",
        r#"{"full":"show 12020 2020","text":"show 1 2020","base":"show 1 2020","punctKey":"show120202020","year":2020,"tvdbSeason":false,"bareMovie":false,"theater":false,"markers":[],"isPack":false,"wantsExtra":false}"#,
    );
}

#[test]
fn parse_movie_number() {
    check_parse(
        "pokemon movie 20",
        r#"{"full":"pokemon movie 20","text":"pokemon","base":"pokemon","punctKey":"pokemonmovie20","episode":20,"tvdbSeason":false,"movie":20,"bareMovie":false,"theater":false,"markers":[],"isPack":false,"wantsExtra":true}"#,
    );
}

#[test]
fn parse_bare_movie() {
    check_parse(
        "kakushigoto movie",
        r#"{"full":"kakushigoto movie","text":"kakushigoto movie","base":"kakushigoto","punctKey":"kakushigotomovie","tvdbSeason":false,"bareMovie":true,"theater":false,"markers":[],"isPack":false,"wantsExtra":true}"#,
    );
}

#[test]
fn parse_recap_marker() {
    check_parse(
        "show recap",
        r#"{"full":"show recap","text":"show","base":"show","punctKey":"showrecap","tvdbSeason":false,"bareMovie":false,"theater":false,"markers":["recap"],"isPack":false,"wantsExtra":true}"#,
    );
}

#[test]
fn parse_soshuhen_marker() {
    check_parse(
        "show soushuuhen",
        r#"{"full":"show soshuhen","text":"show","base":"show","punctKey":"showsoushuuhen","tvdbSeason":false,"bareMovie":false,"theater":false,"markers":["recap"],"isPack":false,"wantsExtra":true}"#,
    );
}

#[test]
fn parse_tokubetsu_hen_marker() {
    check_parse(
        "show tokubetsu-hen",
        r#"{"full":"show tokubetsu hen","text":"show","base":"show","punctKey":"showtokubetsuhen","tvdbSeason":false,"bareMovie":false,"theater":false,"markers":["recap"],"isPack":false,"wantsExtra":true}"#,
    );
}

#[test]
fn parse_directors_cut_marker() {
    check_parse(
        "show director's cut",
        r#"{"full":"show directors cut","text":"show","base":"show","punctKey":"showdirectorscut","tvdbSeason":false,"bareMovie":false,"theater":false,"markers":["reedit"],"isPack":false,"wantsExtra":true}"#,
    );
}

#[test]
fn parse_shin_henshuban_marker() {
    check_parse(
        "show shin henshuu-ban",
        r#"{"full":"show shin henshu ban","text":"show","base":"show","punctKey":"showshinhenshuuban","tvdbSeason":false,"bareMovie":false,"theater":false,"markers":["reedit"],"isPack":false,"wantsExtra":true}"#,
    );
}

#[test]
fn parse_anniversary_remaster_marker() {
    check_parse(
        "show 25th anniversary remastered edition",
        r#"{"full":"show 25th anniversary remastered edition","text":"show","base":"show","punctKey":"show25thanniversaryremasterededition","tvdbSeason":false,"bareMovie":false,"theater":false,"markers":["reedit"],"isPack":false,"wantsExtra":true}"#,
    );
}

#[test]
fn parse_anniversary_edition_marker() {
    check_parse(
        "show 10th anniversary edition",
        r#"{"full":"show 10th anniversary edition","text":"show","base":"show","punctKey":"show10thanniversaryedition","tvdbSeason":false,"bareMovie":false,"theater":false,"markers":["reedit"],"isPack":false,"wantsExtra":true}"#,
    );
}

#[test]
fn parse_anniversary_alone_is_no_marker() {
    check_parse(
        "baki 25th anniversary",
        r#"{"full":"baki 25th anniversary","text":"baki 25th anniversary","base":"baki 25th anniversary","punctKey":"baki25thanniversary","tvdbSeason":false,"bareMovie":false,"theater":false,"markers":[],"isPack":false,"wantsExtra":false}"#,
    );
}

#[test]
fn parse_both_markers() {
    check_parse(
        "show digest kanzenban",
        r#"{"full":"show digest kanzenban","text":"show","base":"show","punctKey":"showdigestkanzenban","tvdbSeason":false,"bareMovie":false,"theater":false,"markers":["recap","reedit"],"isPack":false,"wantsExtra":true}"#,
    );
}

#[test]
fn parse_theater_gekijouban() {
    check_parse(
        "gekijouban show",
        r#"{"full":"gekijoban show","text":"show","base":"show","punctKey":"gekijoubanshow","tvdbSeason":false,"bareMovie":false,"theater":true,"markers":[],"isPack":false,"wantsExtra":true}"#,
    );
}

#[test]
fn parse_theater_theatrical() {
    check_parse(
        "show theatrical",
        r#"{"full":"show theatrical","text":"show","base":"show","punctKey":"showtheatrical","tvdbSeason":false,"bareMovie":false,"theater":true,"markers":[],"isPack":false,"wantsExtra":true}"#,
    );
}

#[test]
fn parse_pack_plus_ova() {
    check_parse(
        "hyouka + ova",
        r#"{"full":"hyoka ova","text":"hyoka","base":"hyoka","punctKey":"hyouka+ova","tvdbSeason":false,"bareMovie":false,"theater":false,"markers":[],"isPack":true,"wantsExtra":false}"#,
    );
}

#[test]
fn parse_pack_and_specials() {
    check_parse(
        "baccano (and specials)",
        r#"{"full":"baccano and specials","text":"baccano and specials","base":"baccano and","punctKey":"baccanoandspecials","tvdbSeason":false,"bareMovie":false,"theater":false,"markers":[],"isPack":true,"wantsExtra":false}"#,
    );
}

#[test]
fn parse_pack_complete_with_extra() {
    check_parse(
        "show complete series ova",
        r#"{"full":"show complete series ova","text":"show complete series ova","base":"show","punctKey":"showcompleteseriesova","tvdbSeason":false,"bareMovie":false,"theater":false,"markers":[],"isPack":true,"wantsExtra":false}"#,
    );
}

#[test]
fn parse_pack_with_movies() {
    check_parse(
        "show with the movies",
        r#"{"full":"show with the movies","text":"show with the movies","base":"show with the","punctKey":"showwiththemovies","tvdbSeason":false,"bareMovie":false,"theater":false,"markers":[],"isPack":true,"wantsExtra":false}"#,
    );
}

#[test]
fn parse_pack_including_ovas() {
    check_parse(
        "show incl. 2 OVAs",
        r#"{"full":"show incl 2 ovas","text":"show incl 2","base":"show incl 2","punctKey":"showincl2ovas","season":2,"tvdbSeason":false,"bareMovie":false,"theater":false,"markers":[],"extraKind":"OVA","extraWord":"ovas","isPack":false,"wantsExtra":true}"#,
    );
}

#[test]
fn parse_pack_comma_films() {
    check_parse(
        "show, films",
        r#"{"full":"show films","text":"show films","base":"show","punctKey":"showfilms","tvdbSeason":false,"bareMovie":false,"theater":false,"markers":[],"isPack":true,"wantsExtra":false}"#,
    );
}

#[test]
fn parse_pack_words_off_base() {
    check_parse(
        "naruto classico temporada completa dublado",
        r#"{"full":"naruto classico temporada completa dublado","text":"naruto classico temporada completa dublado","base":"naruto","punctKey":"narutoclassicotemporadacompletadublado","tvdbSeason":false,"bareMovie":false,"theater":false,"markers":[],"isPack":false,"wantsExtra":false}"#,
    );
}

#[test]
fn parse_extra_ova() {
    check_parse(
        "ano natsu de matteru - ova",
        r#"{"full":"ano natsu de matteru ova","text":"ano natsu de matteru","base":"ano natsu de matteru","punctKey":"anonatsudematteruova","tvdbSeason":false,"bareMovie":false,"theater":false,"markers":[],"extraKind":"OVA","extraWord":"ova","isPack":false,"wantsExtra":true}"#,
    );
}

#[test]
fn parse_extra_special_number() {
    check_parse(
        "show special 01",
        r#"{"full":"show special 01","text":"show","base":"show","punctKey":"showspecial01","episode":1,"tvdbSeason":false,"bareMovie":false,"theater":false,"markers":[],"extraKind":"special","extraWord":"special","extraNumber":1,"isPack":false,"wantsExtra":true}"#,
    );
}

#[test]
fn parse_extra_oav_with_p_number() {
    check_parse(
        "show oav (p1)",
        r#"{"full":"show oav p 1","text":"show","base":"show","punctKey":"showoavp1","tvdbSeason":false,"bareMovie":false,"theater":false,"markers":[],"extraKind":"OVA","extraWord":"oav","isPack":false,"wantsExtra":true}"#,
    );
}

#[test]
fn parse_extra_picture_drama() {
    check_parse(
        "show picture drama",
        r#"{"full":"show picture drama","text":"show","base":"show","punctKey":"showpicturedrama","tvdbSeason":false,"bareMovie":false,"theater":false,"markers":[],"extraKind":"special","extraWord":"picture drama","isPack":false,"wantsExtra":true}"#,
    );
}

#[test]
fn parse_extra_sp_zero() {
    check_parse(
        "show sp 0",
        r#"{"full":"show sp 0","text":"show","base":"show","punctKey":"showsp0","tvdbSeason":false,"bareMovie":false,"theater":false,"markers":[],"extraKind":"special","extraWord":"sp","extraNumber":0,"isPack":false,"wantsExtra":true}"#,
    );
}

#[test]
fn parse_pilot_wants_extra() {
    check_parse(
        "show pilot",
        r#"{"full":"show pilot","text":"show pilot","base":"show pilot","punctKey":"showpilot","tvdbSeason":false,"bareMovie":false,"theater":false,"markers":[],"isPack":false,"wantsExtra":true}"#,
    );
}

#[test]
fn parse_season_part() {
    check_parse(
        "shingeki no kyojin 3 part 2",
        r#"{"full":"shingeki no kyojin 3 part 2","text":"shingeki no kyojin 3 part 2","base":"shingeki no kyojin 3","punctKey":"shingekinokyojin3part2","season":3,"part":2,"tvdbSeason":false,"bareMovie":false,"theater":false,"markers":[],"isPack":false,"wantsExtra":false}"#,
    );
}

#[test]
fn parse_lone_part_matching_season() {
    check_parse(
        "mushoku tensei cour 2",
        r#"{"full":"mushoku tensei cor 2","text":"mushoku tensei cor 2","base":"mushoku tensei","punctKey":"mushokutenseicour2","season":2,"part":2,"tvdbSeason":false,"bareMovie":false,"theater":false,"markers":[],"isPack":false,"wantsExtra":false}"#,
    );
}

#[test]
fn parse_cour_beside_s_number() {
    check_parse(
        "bleach thousand-year blood war (cour 2) 2023 S17",
        r#"{"full":"bleach thosand year blood war cor 2 2023 s 17","text":"bleach thosand year blood war cor 2 s 17","base":"bleach thosand year blood war","punctKey":"bleachthousandyearbloodwarcour22023s17","year":2023,"season":17,"part":2,"tvdbSeason":true,"bareMovie":false,"theater":false,"markers":[],"isPack":false,"wantsExtra":false}"#,
    );
}

#[test]
fn parse_part_zero_is_no_part() {
    check_parse(
        "show part 0 s 2",
        r#"{"full":"show part 0 s 2","text":"show part 0 s 2","base":"show","punctKey":"showpart0s2","season":2,"tvdbSeason":true,"bareMovie":false,"theater":false,"markers":[],"isPack":false,"wantsExtra":false}"#,
    );
}

#[test]
fn parse_release_revision_is_not_a_season() {
    check_parse(
        "duel masters victory v3",
        r#"{"full":"duel masters victory v 3","text":"duel masters victory v 3","base":"duel masters victory v 3","punctKey":"duelmastersvictoryv3","tvdbSeason":false,"bareMovie":false,"theater":false,"markers":[],"isPack":false,"wantsExtra":false}"#,
    );
}

#[test]
fn parse_tvdb_season() {
    check_parse(
        "show season 2",
        r#"{"full":"show season 2","text":"show season 2","base":"show","punctKey":"showseason2","season":2,"tvdbSeason":true,"bareMovie":false,"theater":false,"markers":[],"isPack":false,"wantsExtra":false}"#,
    );
}

#[test]
fn parse_number_pair_off_base() {
    check_parse(
        "ranma 1 2",
        r#"{"full":"ranma 1 2","text":"ranma 1 2","base":"ranma","punctKey":"ranma12","tvdbSeason":false,"bareMovie":false,"theater":false,"markers":[],"isPack":false,"wantsExtra":false}"#,
    );
}

#[test]
fn parse_only_extra_words() {
    check_parse(
        "ova",
        r#"{"full":"ova","text":"","base":"","punctKey":"ova","tvdbSeason":false,"bareMovie":false,"theater":false,"markers":[],"extraKind":"OVA","extraWord":"ova","isPack":false,"wantsExtra":true}"#,
    );
}

#[test]
fn parse_non_ascii_title() {
    check_parse(
        "Ōkami to Kōshinryō II",
        r#"{"full":"okami to koshinryo ii","text":"okami to koshinryo ii","base":"okami to koshinryo ii","punctKey":"okamitokoshinryoii","season":2,"tvdbSeason":false,"bareMovie":false,"theater":false,"markers":[],"isPack":false,"wantsExtra":false}"#,
    );
}

#[test]
fn parse_astral_title() {
    check_parse(
        "𝔸𝔹 show 2nd season",
        r#"{"full":"ab show 2nd season","text":"ab show 2nd season","base":"ab show","punctKey":"abshow2ndseason","season":2,"tvdbSeason":false,"bareMovie":false,"theater":false,"markers":[],"isPack":false,"wantsExtra":false}"#,
    );
}

#[test]
fn parse_punct_key() {
    check_parse(
        "Working'!! ＋ Gintama’ ☆",
        r#"{"full":"working gintama","text":"working gintama","base":"working gintama","punctKey":"working'!!+gintama'☆","tvdbSeason":false,"bareMovie":false,"theater":false,"markers":[],"isPack":false,"wantsExtra":false}"#,
    );
}

#[test]
fn split_slash() {
    assert_eq!(
        split_titles("Doupo Cangqiong / Dou Po Cang Qiong / Battle Through The Heavens"),
        ["Doupo Cangqiong", "Dou Po Cang Qiong", "Battle Through The Heavens"]
    );
}

#[test]
fn split_aka() {
    assert_eq!(split_titles("Show One a.k.a. Show Two"), ["Show One", "Show Two"]);
}

#[test]
fn split_pipe() {
    assert_eq!(split_titles("One | Two"), ["One", "Two"]);
}

#[test]
fn split_bracketed() {
    assert_eq!(split_titles("Wu Ni (Martial Inverse)"), ["Wu Ni", "Martial Inverse"]);
}

#[test]
fn split_bracketed_tv_stays() {
    assert_eq!(split_titles("Show (TV)"), ["Show (TV)"]);
}

#[test]
fn split_bracketed_year_stays() {
    assert_eq!(split_titles("Show (2019)"), ["Show (2019)"]);
}

#[test]
fn split_bracketed_extra() {
    assert_eq!(
        split_titles("shingeki no kyojin ovas (attack on titan)"),
        ["shingeki no kyojin ovas", "attack on titan ovas"]
    );
}

#[test]
fn split_bracketed_astral() {
    assert_eq!(split_titles("Show (𝔸𝔹ℂ𝔻) rest"), ["Show rest", "𝔸𝔹ℂ𝔻 rest"]);
}

#[test]
fn split_bracketed_empty_name() {
    assert_eq!(split_titles("(Martial Inverse)"), ["(Martial Inverse)"]);
}

#[test]
fn split_double_space() {
    assert_eq!(split_titles("cloak  dagger"), ["cloak  dagger"]);
}

#[test]
fn split_double_space_two_words_each() {
    assert_eq!(split_titles("cloak and  dagger here"), ["cloak and", "dagger here"]);
}

#[test]
fn split_single() {
    assert_eq!(split_titles("cowboy bebop"), ["cowboy bebop"]);
}

#[test]
fn glue_adjacent_words() {
    check_glue(&["shadow", "side", "x"], &["shadowside"], true, &["shadowside", "x"], &[]);
}

#[test]
fn glue_not_when_first_word_is_in_query() {
    check_glue(&["shadow", "side"], &["shadow", "shadowside"], true, &["shadow", "side"], &[]);
}

#[test]
fn glue_initials() {
    check_glue(&["black", "and", "white"], &["bw"], true, &["bw"], &["bw"]);
}

#[test]
fn glue_initials_off() {
    check_glue(&["black", "and", "white"], &["bw"], false, &["black", "and", "white"], &[]);
}

#[test]
fn glue_leading_the_needs_four_words() {
    check_glue(&["the", "hidden", "one", "year", "war"], &["thoyw"], true, &["thoyw"], &["thoyw"]);
}

#[test]
fn glue_leading_the_four_words() {
    check_glue(&["the", "ab", "cd", "ef"], &["tace"], true, &["tace"], &["tace"]);
}

#[test]
fn glue_initials_at_most_four_words() {
    check_glue(&["ab", "cd", "ef", "gh", "ij"], &["acegi"], true, &["ab", "cd", "ef", "gh", "ij"], &[]);
}

#[test]
fn glue_initials_four_words() {
    check_glue(&["ab", "cd", "ef", "gh", "ij"], &["aceg"], true, &["aceg", "ij"], &["aceg"]);
}

#[test]
fn glue_initials_not_when_last_word_is_in_query() {
    check_glue(&["ab", "cd"], &["ac", "cd"], true, &["ab", "cd"], &[]);
}

#[test]
fn glue_initials_need_a_lowercase_ascii_start() {
    check_glue(&["élan", "vital"], &["év"], true, &["élan", "vital"], &[]);
}

#[test]
fn glue_astral_initial_is_a_whole_character() {
    check_glue(&["best", "𝔸wishes", "x"], &["b𝔸"], true, &["b𝔸", "x"], &["b𝔸"]);
}
