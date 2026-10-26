use std::sync::LazyLock;

use fastjs_regex::Regex;
use indexmap::IndexSet;
use serde::Serialize;

use crate::norm::{norm, squash, without_marks};
use crate::rx;

pub type Season = u32;

pub const FINAL_SEASON: Season = 99;
pub const SEQUEL_MARKS: [char; 6] = ['!', '+', '◎', '☆', '★', '\''];

#[rustfmt::skip]
const ORDINAL_WORDS: [&str; 20] = [
    "first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth", "ninth", "tenth", "eleventh",
    "twelfth", "thirteenth", "fourteenth", "fifteenth", "sixteenth", "seventeenth", "eighteenth", "nineteenth",
    "twentieth",
];
const ROMAN_WORDS: [&str; 20] = [
    "i", "ii", "iii", "iv", "v", "vi", "vii", "viii", "ix", "x", "xi", "xii", "xiii", "xiv", "xv", "xvi", "xvii",
    "xviii", "xix", "xx",
];
const EXTRA: &str = "(?:ovas?|oads?|oavs?|specials?|sp|extras?|recaps?|picture dramas?|omake|bonus)";
const INITIALS_FILLER: [&str; 5] = ["and", "the", "no", "of", "to"];

fn longest_first(words: impl IntoIterator<Item = &'static str>) -> String {
    let mut words: Vec<&str> = words.into_iter().collect();
    words.sort_by_key(|word| std::cmp::Reverse(word.len()));
    words.join("|")
}

fn ordinal(word: &str) -> Option<Season> {
    ORDINAL_WORDS.iter().position(|candidate| *candidate == word).map(|index| index as Season + 1)
}

fn roman(word: &str) -> Option<Season> {
    ROMAN_WORDS.iter().position(|candidate| *candidate == word).map(|index| index as Season + 1)
}

static ORDINAL_PATTERN: LazyLock<String> = LazyLock::new(|| ORDINAL_WORDS.join("|"));
static ROMAN_PATTERN: LazyLock<String> = LazyLock::new(|| longest_first(ROMAN_WORDS));
// i, v and x alone are too often plain words or names ("Rockman X") to read as a season ending a title
static MULTI_LETTER_ROMAN_PATTERN: LazyLock<String> =
    LazyLock::new(|| longest_first(ROMAN_WORDS.into_iter().filter(|word| word.len() > 1)));

static SEASON_BEFORE_ALTERNATIVE_TITLE: LazyLock<Regex> = LazyLock::new(|| {
    rx(
        r"(?<!\b(?:part|parte|season|cou?r|vol|volume|episode|ep|movie|film|ova|special|sp))\s(\d{1,2})\s*(\([^()]*\p{L}{4}[^()]*\))\s*$",
        "iu",
    )
});
const MOVIE_SOURCE: &str = r"\b(?:the )?(?:movie|film|gekijouban|gekijoban)s? ?(\d{1,3})\b";
static MOVIE_RX: LazyLock<Regex> = LazyLock::new(|| rx(MOVIE_SOURCE, ""));
static STRIP_MOVIE: LazyLock<Regex> = LazyLock::new(|| rx(MOVIE_SOURCE, "g"));
static STRIP_SEASON: LazyLock<Regex> = LazyLock::new(|| {
    rx(
        &format!(
            r"\b(?:s ?\d{{1,2}}|(?:season|part|cou?r) ?\d{{1,2}}|\d{{1,2}}(?:st|nd|rd|th) (?:season|part|cou?r)|(?:{}) (?:season|part)|final season|season)\b",
            *ORDINAL_PATTERN
        ),
        "g",
    )
});
static PACK_WORDS: LazyLock<Regex> = LazyLock::new(|| {
    rx(
        r"\b(?:\+? ?(?:ovas?|oads?|movies?|films?|specials?|sp|extras?)|complete|batch|collection|bd|dvd|english|dubbed|dual audio|high quality|tv|series|dublado|legendado|latino|classico|(?:serie|temporada) completa|capitulos|episodios)\b",
        "g",
    )
});
// "hyouka + ova", "baccano (and specials)", "complete ... ova": the series with its extras, not the extra
static PACK_RX: LazyLock<Regex> = LazyLock::new(|| {
    rx(
        &format!(
            r"(?:\+|&|\band\b|\bwith\b|\bincl\w*|\bplus\b|,)\s*(?:the\s+|all\s+)?(?:\d+\s+)?(?:{EXTRA}|movies?|films?)\b|\bcomplete\b.*\b{EXTRA}\b"
        ),
        "i",
    )
});
pub static EXTRA_RX: LazyLock<Regex> = LazyLock::new(|| rx(&format!(r"\b({EXTRA})\b(?: ?(\d{{1,2}})\b)?"), ""));
static ENDING_EXTRA_RX: LazyLock<Regex> = LazyLock::new(|| rx(&format!(r"\b({EXTRA})(?: \d{{1,2}})?$"), ""));
static STRIP_EXTRA: LazyLock<Regex> = LazyLock::new(|| rx(&format!(r"\b{EXTRA}\b"), "g"));
static ENDING_RAW_EXTRA_RX: LazyLock<Regex> = LazyLock::new(|| rx(&format!(r"\s({EXTRA}(?:\s\d{{1,2}})?)\s*$"), "i"));
// Recap and re-edit markers, with the Japanese terms Kitsu's romanised titles use for them, spelled as norm() leaves
// them (long vowels folded: "soushuuhen" -> "soshuhen"): soushuuhen 総集編 recap / compilation, tokubetsu-hen 特別編
// special edition, (shin) henshuu-ban (新)編集版 (newly) edited version, kanzenban 完全版 complete version; gekijouban
// 劇場版 (THEATER) is the theatrical version. An anniversary only before a remaster or edition: alone it names many
// entries ("Baki 25th Anniversary").
static RECAP: LazyLock<Regex> = LazyLock::new(|| {
    rx(r"\b(?:recaps?|recapitulation|soshu ?hen|digest|summary|highlights?|compilations?|tokubetsu ?hen)\b", "g")
});
static REEDIT: LazyLock<Regex> = LazyLock::new(|| {
    rx(
        r"\b(?:directors? cut|final cut|(?:shin )?henshu ?ban|re ?edit(?:ed|ion)?|special edition|kanzenban|(?:\d+(?:st|nd|rd|th) anniversary )?remaster(?:ed)?(?: edition)?|\d+(?:st|nd|rd|th) anniversary edition)\b",
        "g",
    )
});
static THEATER: LazyLock<Regex> = LazyLock::new(|| rx(r"\b(?:gekijo ?ban|theatrical)\b", "g"));
const YEAR_SOURCE: &str = r"\b(19[5-9]\d|20[0-3]\d)\b";
static YEAR_RX: LazyLock<Regex> = LazyLock::new(|| rx(YEAR_SOURCE, ""));
static STRIP_YEAR: LazyLock<Regex> = LazyLock::new(|| rx(YEAR_SOURCE, "g"));
static ORDINAL_SEASON_RX: LazyLock<Regex> =
    LazyLock::new(|| rx(&format!(r"\b({}) (?:season|part)\b", *ORDINAL_PATTERN), ""));
static ROMAN_SEASON_RX: LazyLock<Regex> =
    LazyLock::new(|| rx(&format!(r"\b(?:part|season) ({})\b", *ROMAN_PATTERN), ""));
static ENDING_ROMAN_RX: LazyLock<Regex> =
    LazyLock::new(|| rx(&format!(r"^\S+ \S+.* ({})$", *MULTI_LETTER_ROMAN_PATTERN), ""));

static PUNCT_INNER_APOSTROPHE: LazyLock<Regex> = LazyLock::new(|| rx("'(?=[a-z0-9])", "g"));
static NUMBER_WORD: LazyLock<Regex> = LazyLock::new(|| rx(r"^\d{1,4}$", ""));
static OVA_PREFIX: LazyLock<Regex> = LazyLock::new(|| rx("^(ova|oad|oav)", ""));
static SPECIAL_PREFIX: LazyLock<Regex> = LazyLock::new(|| rx("^(special|sp$|extra|omake|bonus)", ""));
static NUMBERED_MOVIE: LazyLock<Regex> = LazyLock::new(|| rx(r"\b(\d{1,2}) the movie\b", ""));
static STRIP_NUMBERED_MOVIE: LazyLock<Regex> = LazyLock::new(|| rx(r"\b\d{1,2} the movie\b", ""));
static CHAPTER: LazyLock<Regex> = LazyLock::new(|| rx(r"\bchapter (\d{1,2})\b", ""));
static STRIP_CHAPTER: LazyLock<Regex> = LazyLock::new(|| rx(r"\bchapter \d{1,2}\b", ""));
static TRAILING_DIGIT: LazyLock<Regex> = LazyLock::new(|| rx(r" \d$", ""));
static TV_WORD: LazyLock<Regex> = LazyLock::new(|| rx(r"\btv\b", "g"));
static MOVIE_WORDS: LazyLock<Regex> = LazyLock::new(|| rx(r"\b(?:the )?(?:movie|film)\b", "g"));
static CENTURY_YEAR: LazyLock<Regex> = LazyLock::new(|| rx(r"\b(?:19|20)\d\d\b", "g"));
static PREFIX_SEPARATOR: LazyLock<Regex> = LazyLock::new(|| rx(r"[:\-]", ""));
static PREFIX_MOVIE_WORDS: LazyLock<Regex> = LazyLock::new(|| rx(r"\b(the )?(movie|film|gekijouban|gekijoban)\b", "g"));

static BARE_ORDINAL: LazyLock<Regex> =
    LazyLock::new(|| rx(r"(?<!\bthe )\b(\d{1,2})(?:st|nd|rd|th)\b(?! (?:part|cou?r))", ""));
static S_NUMBER: LazyLock<Regex> = LazyLock::new(|| rx(r"\bs ?\d{1,2}\b", ""));
static SEASON_NUMBER: LazyLock<Regex> = LazyLock::new(|| rx(r"\b(?:s|season|seasn)\s?(\d{1,2})\b", ""));
static NTH_SEASON: LazyLock<Regex> = LazyLock::new(|| rx(r"\b(\d{1,2})(?:st|nd|rd|th) season\b", ""));
static SEASON_BEFORE_PART: LazyLock<Regex> = LazyLock::new(|| rx(r"\b(\d{1,2}) (?:part|parte|cou?r) \d\b", ""));
static PART_NUMBER: LazyLock<Regex> = LazyLock::new(|| rx(r"\b(?:part|parte|cou?r)\s?(\d{1,2})\b", ""));
static NTH_PART: LazyLock<Regex> = LazyLock::new(|| rx(r"\b(\d{1,2})(?:st|nd|rd|th) (?:part|cou?r)\b", ""));
static X_NUMBER: LazyLock<Regex> = LazyLock::new(|| rx(r"\bx ?(\d)\b", ""));
static FINAL_RX: LazyLock<Regex> = LazyLock::new(|| rx(r"\bfinal (?:season|part|chapter)\b", ""));
static ENDING_DIGIT: LazyLock<Regex> = LazyLock::new(|| rx(r"(?<!\b\d{1,4}) (\d)$", ""));

static EPISODE_RANGE: LazyLock<Regex> = LazyLock::new(|| rx(r"(?:^|[\s(])(\d{1,4}) ?- ?\d{1,4}(?=[\s+)]|$)", ""));
static YEAR_WORD: LazyLock<Regex> = LazyLock::new(|| rx(r"^(19|20)\d\d$", ""));
static TRAILING_EPISODE: LazyLock<Regex> = LazyLock::new(|| rx(r"(?<!\bs) (\d{2,4})(?: v \d)?$", ""));

static REVISION: LazyLock<Regex> = LazyLock::new(|| rx(r"\b(\d{1,4})v\d\b", "gi"));
static PLUS_EXTRA: LazyLock<Regex> = LazyLock::new(|| rx(r"\+\s*(ovas?|oads?|movies?|films?|specials?|sp)\b", "gi"));
static DASH_EPISODE: LazyLock<Regex> = LazyLock::new(|| rx(r"\s-\s\d{1,4}(?:v\d)?(?=\s|$)", "g"));
static YEAR_RANGE: LazyLock<Regex> = LazyLock::new(|| rx(r"\b(19[5-9]\d|20[0-3]\d)(?: (?:19|20)\d\d)?\b", ""));
static P_NUMBER: LazyLock<Regex> = LazyLock::new(|| rx(r"\bp ?\d{1,2}\b", ""));
static RELEASE_REVISION: LazyLock<Regex> = LazyLock::new(|| rx(r"\bv \d{1,2}(?: \d)?\b", "g"));
static SEASON_PART: LazyLock<Regex> = LazyLock::new(|| rx(r"\b(\d{1,2}) (?:part|parte|cou?r) (\d)\b", ""));
static LONE_PART: LazyLock<Regex> =
    LazyLock::new(|| rx(r"\b(?:part|parte|cou?r) ?(\d)\b|\b(\d)(?:st|nd|rd|th) (?:part|cou?r)\b", ""));
static NUMBER_PAIR: LazyLock<Regex> = LazyLock::new(|| rx(r"\b\d{1,3} \d{1,3}\b", "g"));
static TVDB_SEASON: LazyLock<Regex> = LazyLock::new(|| rx(r"\b(?:s ?\d{1,2}|season ?\d{1,2})\b", ""));
static BARE_MOVIE: LazyLock<Regex> = LazyLock::new(|| rx(r"\b(?:movie|film)\b", ""));
static EXTRA_FORM: LazyLock<Regex> = LazyLock::new(|| rx(r"\b(movie|film|pilot)\b", ""));

static TITLE_SEPARATOR: LazyLock<Regex> = LazyLock::new(|| rx(r"\s+(?:\/|\||aka|a\.k\.a\.)\s+", "i"));
static BRACKETED: LazyLock<Regex> = LazyLock::new(|| rx(r"^(.*?)\s*\(([^()]+)\)\s*(.*)$", ""));
static FOUR_LETTERS: LazyLock<Regex> = LazyLock::new(|| rx(r"\p{L}{4}", "u"));
static KIND_ONLY: LazyLock<Regex> = LazyLock::new(|| rx("^(?:tv|ova|oad|movie|special|uncut|dub|sub)$", "i"));
static DOUBLE_SPACE: LazyLock<Regex> = LazyLock::new(|| rx(r"\s{2,}", ""));

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Marker {
    Recap,
    Reedit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum ExtraKind {
    #[serde(rename = "OVA")]
    Ova,
    #[serde(rename = "special")]
    Special,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_excessive_bools)]
pub struct ParsedQuery {
    pub full: String,
    pub text: String,
    pub base: String,
    pub punct_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub episode: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub season: Option<Season>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub part: Option<u32>,
    pub tvdb_season: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub movie: Option<u32>,
    pub bare_movie: bool,
    pub theater: bool,
    pub markers: Vec<Marker>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra_kind: Option<ExtraKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra_word: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra_number: Option<u32>,
    pub is_pack: bool,
    pub wants_extra: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Glued {
    pub words: Vec<String>,
    pub initials: IndexSet<String>,
}

fn group<'t>(text: &'t str, m: &fastjs_regex::Match, index: usize) -> Option<&'t str> {
    m.group(index).map(|range| &text[range])
}

fn number(digits: &str) -> u32 {
    digits.parse().unwrap_or_else(|error| panic!("not a number {digits}: {error}"))
}

fn first_number(rx: &Regex, text: &str) -> Option<u32> {
    rx.find(text).and_then(|m| group(text, &m, 1)).map(number)
}

// lowercase title keeping only the sequel punctuation ("hatarakusaibou!!", "okusamagaseitokaichou!+!", "gintama'",
// "working'!!", "kerorogunsou☆"; not the apostrophe of "kuroko's")
pub fn punct_key(text: &str) -> String {
    let plain = without_marks(text).replace('＋', "+").replace(['’', '′'], "'");
    let plain = PUNCT_INNER_APOSTROPHE.replace(&plain, "");
    plain.chars().filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || SEQUEL_MARKS.contains(c)).collect()
}

pub fn compact(text: &str) -> String {
    text.replace(' ', "")
}

pub fn numerals_of(text: &str) -> Vec<u32> {
    text.split(' ')
        .filter_map(|word| roman(word).or_else(|| NUMBER_WORD.is_match(word).then(|| number(word))))
        .collect()
}

pub fn roman_numeral(number: u32) -> Option<&'static str> {
    number.checked_sub(1).and_then(|index| ROMAN_WORDS.get(index as usize)).copied()
}

fn extra_kind_of(word: &str) -> ExtraKind {
    if OVA_PREFIX.is_match(word) { ExtraKind::Ova } else { ExtraKind::Special }
}

pub fn extra_family(word: &str) -> &str {
    if OVA_PREFIX.is_match(word) {
        "ova"
    } else if SPECIAL_PREFIX.is_match(word) {
        "special"
    } else {
        word
    }
}

pub fn ending_extra_word(title: &str) -> Option<&str> {
    ENDING_EXTRA_RX.find(title).and_then(|m| group(title, &m, 1))
}

pub fn has_ending_extra_word(title: &str) -> bool {
    ENDING_EXTRA_RX.is_match(title)
}

pub fn extra_number_of(title: &str) -> Option<u32> {
    EXTRA_RX.find(title).and_then(|m| group(title, &m, 2)).map(number)
}

pub fn markers_of(text: &str) -> Vec<Marker> {
    [(Marker::Recap, &*RECAP), (Marker::Reedit, &*REEDIT)]
        .into_iter()
        .filter(|(_, rx)| rx.search(text).is_some())
        .map(|(marker, _)| marker)
        .collect()
}

fn strip_markers(text: &str) -> String {
    let stripped = RECAP.replace(text, " ");
    let stripped = REEDIT.replace(&stripped, " ");
    THEATER.replace(&stripped, " ").into_owned()
}

fn movie_index_of(text: &str) -> Option<u32> {
    first_number(&MOVIE_RX, text)
}

pub fn title_movie_index_of(title: &str, is_movie: bool) -> Option<u32> {
    movie_index_of(title).or_else(|| {
        let numbered = NUMBERED_MOVIE.find(title).or_else(|| is_movie.then(|| CHAPTER.find(title))?);
        numbered.and_then(|m| group(title, &m, 1)).map(number)
    })
}

pub fn title_year_of(title: &str) -> Option<u32> {
    first_number(&YEAR_RX, title).filter(|year| *year != 0)
}

// season marker of a normalised title: number, FINAL_SEASON for "final season", or none
pub fn season_of(text: &str) -> Option<Season> {
    // a bare ordinal in the torrent's own title ("... 4th 2026 S2") beats the S<n> the scraper adds; "lupin the 3rd" is
    // a name
    if let Some(ordinal) = first_number(&BARE_ORDINAL, text)
        && S_NUMBER.is_match(text)
    {
        return Some(ordinal);
    }
    if let Some(season) = first_number(&SEASON_NUMBER, text) {
        return Some(season);
    }
    if let Some(season) = first_number(&NTH_SEASON, text) {
        return Some(season);
    }
    // a season before its part ("shingeki no kyojin 3 part 2"), not a name ("86 part 2")
    if let Some(season) = first_number(&SEASON_BEFORE_PART, text)
        && season as usize <= ORDINAL_WORDS.len()
    {
        return Some(season);
    }
    if let Some(season) = first_number(&PART_NUMBER, text) {
        return Some(season);
    }
    if let Some(season) = first_number(&NTH_PART, text) {
        return Some(season);
    }
    if let Some(m) = ORDINAL_SEASON_RX.find(text) {
        return group(text, &m, 1).and_then(ordinal);
    }
    if let Some(m) = ROMAN_SEASON_RX.find(text) {
        return group(text, &m, 1).and_then(roman);
    }
    // a roman numeral ending a title of three or more words ("... gakkou e kayou ii"), not "lupin iii" or "mk iv"
    if let Some(m) = ENDING_ROMAN_RX.find(text) {
        return group(text, &m, 1).and_then(roman);
    }
    if let Some(season) = first_number(&X_NUMBER, text) {
        return Some(season);
    }
    if FINAL_RX.is_match(text) {
        return Some(FINAL_SEASON);
    }
    // not the 2 of "ranma 1 2" (Ranma ½): a number right after another one
    first_number(&ENDING_DIGIT, text)
}

// what a Kitsu title is matched by: the normalised title without its season, year, movie number, extra and recap words
pub fn title_base_of(title: &str, is_movie: bool) -> String {
    let base = strip_markers(title);
    let base = STRIP_YEAR.replace(&base, " ");
    let base = STRIP_MOVIE.replace(&base, " ");
    let base = STRIP_NUMBERED_MOVIE.replace(&base, " ");
    let base = if is_movie { STRIP_CHAPTER.replace(&base, " ").into_owned() } else { base.into_owned() };
    let base = STRIP_SEASON.replace(&base, " ");
    let base = TRAILING_DIGIT.replace(&base, "");
    let base = STRIP_EXTRA.replace(&base, " ");
    squash(&TV_WORD.replace(&base, " "))
}

// for movie queries the words "movie" / "film" come off too ("Kakushigoto Movie")
pub fn without_movie_words(base: &str) -> String {
    squash(&MOVIE_WORDS.replace(base, " "))
}

pub fn seasonless_title(title: &str) -> String {
    let normalised = norm(title);
    let stripped = STRIP_SEASON.replace(&normalised, " ");
    let stripped = TRAILING_DIGIT.replace(&stripped, "");
    squash(&CENTURY_YEAR.replace(&stripped, " "))
}

// a movie franchise's prefix, for numbering its movies by release ("pokemon movie 20")
pub fn movie_prefix_of(title: &str) -> String {
    let prefix = PREFIX_SEPARATOR.split(title).into_iter().next().flatten().unwrap_or_default();
    let normalised = norm(prefix);
    let stripped = STRIP_MOVIE.replace(&normalised, " ");
    squash(&PREFIX_MOVIE_WORDS.replace(&stripped, " "))
}

fn not_a_year(digits: &str) -> Option<u32> {
    (!YEAR_WORD.is_match(digits)).then(|| number(digits))
}

// an episode range ("15-18") or a trailing absolute number ("boku no hero academia 138"), never a year; "dr stone - new
// world 16 v2", not the season of "S17" (normalised to "s 17")
fn episode_number_of(query: &str) -> Option<u32> {
    if let Some(m) = EPISODE_RANGE.find(query)
        && let Some(episode) = group(query, &m, 1).and_then(not_a_year)
    {
        return Some(episode);
    }
    let normalised = norm(query);
    TRAILING_EPISODE.find(&normalised).and_then(|m| group(&normalised, &m, 1)).and_then(not_a_year)
}

// A search query: `base` is what titles are matched by, the rest is what the query asks for besides the name.
pub fn parse_query(query: &str) -> ParsedQuery {
    // a season before a bracketed alternative title ("kuroko's basketball 2 (kuroko no baske)") goes after it, unless a
    // word ties the number to the name ("exogenesis symphony part 3 (redemption)")
    let query = SEASON_BEFORE_ALTERNATIVE_TITLE.replace(query, " $2 $1");
    let query = REVISION.replace(&query, "$1").into_owned();
    let is_pack = PACK_RX.is_match(&query);
    let cleaned = PLUS_EXTRA.replace(&query, " ");
    let mut text = norm(&DASH_EPISODE.replace(&cleaned, " "));
    let mut year = None;
    if let Some(m) = YEAR_RANGE.find(&text) {
        year = group(&text, &m, 1).map(number);
        let whole = text[m.range()].to_owned();
        text = text.replacen(&whole, " ", 1);
    }
    let movie = movie_index_of(&text);
    if movie.is_some() {
        text = squash(&STRIP_MOVIE.replace(&text, " "));
    }
    let markers = markers_of(&text);
    let theater = THEATER.search(&text).is_some();
    text = squash(&strip_markers(&text));
    // the extra word and its number ("ova 1", "special 01", "(p1)") come off before seasons are read
    let extra = if is_pack {
        None
    } else {
        EXTRA_RX.find(&text).map(|m| {
            let word = group(&text, &m, 1).unwrap_or_default().to_owned();
            (word, group(&text, &m, 2).map(number))
        })
    };
    if extra.is_some() {
        text = squash(&P_NUMBER.replace(&EXTRA_RX.replace(&text, " "), " "));
    }
    // a release revision ("v2", "v1.5") isn't a season, but stays in the text: titles hold such numbers ("Duel Masters
    // Victory V3")
    let season = if movie.is_some() { None } else { season_of(&squash(&RELEASE_REVISION.replace(&text, " "))) };
    // the part asked for: "shingeki no kyojin 3 part 2", or a part alone ("mushoku tensei cour 2"; the 86 of "86 part
    // 2" isn't a season); a cour beside the scraper's S<n> is the season's part ("bleach thousand-year blood war (cour
    // 2) 2023 S17")
    let season_part = SEASON_PART
        .find(&text)
        .map(|m| (group(&text, &m, 1).map(number), group(&text, &m, 2).map(number).unwrap_or_default()));
    let lone_number = LONE_PART
        .find(&text)
        .and_then(|m| group(&text, &m, 1).or_else(|| group(&text, &m, 2)))
        .map(number)
        .filter(|number| *number != 0);
    let part = match season_part {
        Some((first, second)) if first == season => Some(second),
        _ if lone_number.is_some() && lone_number == season => season,
        _ if lone_number.is_some() && S_NUMBER.is_match(&text) => lone_number,
        _ => None,
    };
    let stripped = STRIP_SEASON.replace(&text, " ");
    let stripped = NUMBER_PAIR.replace(&stripped, " ");
    let base = squash(&PACK_WORDS.replace(&stripped, " "));
    let wants_extra = movie.is_some()
        || (!is_pack && (extra.is_some() || !markers.is_empty() || theater || EXTRA_FORM.is_match(&text)));
    let (extra_word, extra_number) = extra.map_or((None, None), |(word, number)| (Some(word), number));
    ParsedQuery {
        full: norm(&query),
        base: if base.is_empty() { text.clone() } else { base },
        punct_key: punct_key(&query),
        episode: episode_number_of(&query),
        year,
        season,
        part,
        tvdb_season: season.is_some() && TVDB_SEASON.is_match(&text),
        movie,
        bare_movie: movie.is_none() && BARE_MOVIE.is_match(&text),
        theater,
        markers,
        extra_kind: extra_word.as_deref().map(extra_kind_of),
        extra_word,
        extra_number,
        is_pack,
        wants_extra,
        text,
    }
}

// several alternative titles in one name ("Doupo Cangqiong / Dou Po Cang Qiong / Battle Through The Heavens"); cleaned
// queries have lost the "/" and keep a double space, which "&" removal also leaves ("cloak  dagger"), so a double-space
// split needs two words in every part
pub fn split_titles(query: &str) -> Vec<String> {
    let separated: Vec<String> = TITLE_SEPARATOR
        .split(query)
        .into_iter()
        .flatten()
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect();
    if separated.len() > 1 {
        return separated;
    }
    // "Wu Ni (Martial Inverse)": a bracketed alternative title ("(TV)" and "(2019)" stay); an extra word ending the
    // name before the bracket asks for the bracketed title's extra too ("shingeki no kyojin ovas (attack on titan)")
    if let Some(m) = BRACKETED.find(query) {
        let [name, bracketed, rest] = [1, 2, 3].map(|index| group(query, &m, index).unwrap_or_default());
        if FOUR_LETTERS.is_match(bracketed) && !KIND_ONLY.is_match(bracketed.trim()) && !name.trim().is_empty() {
            let extra = ENDING_RAW_EXTRA_RX.find(name).and_then(|m| group(name, &m, 1)).unwrap_or_default();
            return vec![format!("{name} {rest}").trim().to_owned(), squash(&format!("{bracketed} {extra} {rest}"))];
        }
    }
    let spaced: Vec<&str> =
        DOUBLE_SPACE.split(query).into_iter().flatten().map(str::trim).filter(|part| !part.is_empty()).collect();
    if spaced.len() > 1 && spaced.iter().all(|part| part.split(' ').count() >= 2) {
        spaced.into_iter().map(str::to_owned).collect()
    } else {
        vec![query.to_owned()]
    }
}

// merge adjacent title words a query writes glued together ("shadow side" for "shadowside"), or as initials ("bw" for
// "best wishes" / "black and white"; Kitsu's own titles only: synonyms like "Clown Arc" make chance initials like "ca")
pub fn glue<S: AsRef<str>, Q: AsRef<str>>(title_words: &[S], query_words: &[Q], initials_ok: bool) -> Glued {
    let query: IndexSet<&str> = query_words.iter().map(AsRef::as_ref).collect();
    let title: Vec<&str> = title_words.iter().map(AsRef::as_ref).collect();
    let filler = |word: &str| INITIALS_FILLER.contains(&word);
    let mut out = Glued::default();
    let mut i = 0;
    'outer: while i < title.len() {
        let word = title[i];
        if i + 1 < title.len() && !query.contains(word) {
            let joined = format!("{word}{}", title[i + 1]);
            if query.contains(joined.as_str()) {
                out.words.push(joined);
                i += 2;
                continue;
            }
        }
        if initials_ok && !query.contains(word) && word.starts_with(|c: char| c.is_ascii_lowercase()) {
            // a leading "the" only in longer initials ("thoyw" for "the hidden one year war"), up to one word more
            let (min_words, max_words) = if filler(word) { (4, 5) } else { (2, 4) };
            let mut initials = String::new();
            let mut used = 0;
            let mut j = i;
            while j < title.len() && used < max_words {
                if filler(title[j]) && j > i {
                    j += 1;
                    continue;
                }
                initials.extend(title[j].chars().next());
                used += 1;
                if used >= min_words && query.contains(initials.as_str()) && !query.contains(title[j]) {
                    out.words.push(initials.clone());
                    out.initials.insert(initials);
                    i = j + 1;
                    continue 'outer;
                }
                j += 1;
            }
        }
        out.words.push(word.to_owned());
        i += 1;
    }
    out
}
