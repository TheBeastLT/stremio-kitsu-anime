// Reading torrent-style titles for the title search (title_search.js): normalisation, and what a query or a Kitsu title
// says besides its name (season, year, movie number, extra, recap, pack).

const ORDINAL_WORDS = ['first', 'second', 'third', 'fourth', 'fifth', 'sixth', 'seventh', 'eighth', 'ninth', 'tenth',
  'eleventh', 'twelfth', 'thirteenth', 'fourteenth', 'fifteenth', 'sixteenth', 'seventeenth', 'eighteenth',
  'nineteenth', 'twentieth'];
const ROMAN_WORDS = ['i', 'ii', 'iii', 'iv', 'v', 'vi', 'vii', 'viii', 'ix', 'x', 'xi', 'xii', 'xiii', 'xiv', 'xv',
  'xvi', 'xvii', 'xviii', 'xix', 'xx'];
const longestFirst = words => [...words].sort((a, b) => b.length - a.length).join('|');
const ORDINALS = Object.fromEntries(ORDINAL_WORDS.map((word, index) => [word, index + 1]));
const ROMAN_NUMERALS = Object.fromEntries(ROMAN_WORDS.map((word, index) => [word, index + 1]));
const ORDINAL_PATTERN = ORDINAL_WORDS.join('|');
const ROMAN_PATTERN = longestFirst(ROMAN_WORDS);
// i, v and x alone are too often plain words or names ("Rockman X") to read as a season ending a title
const MULTI_LETTER_ROMAN_PATTERN = longestFirst(ROMAN_WORDS.filter(word => word.length > 1));
const FINAL_SEASON = 99; // "final season", whatever its number

const SEASON_BEFORE_ALTERNATIVE_TITLE = new RegExp('(?<!\\b(?:part|parte|season|cou?r|vol|volume|episode|ep|movie|film'
    + '|ova|special|sp))\\s(\\d{1,2})\\s*(\\([^()]*\\p{L}{4}[^()]*\\))\\s*$', 'iu');
const MOVIE_RX = /\b(?:the )?(?:movie|film|gekijouban|gekijoban)s? ?(\d{1,3})\b/;
const STRIP_MOVIE = new RegExp(MOVIE_RX.source, 'g');
const STRIP_SEASON = new RegExp('\\b(?:s ?\\d{1,2}|(?:season|part|cou?r) ?\\d{1,2}'
    + '|\\d{1,2}(?:st|nd|rd|th) (?:season|part|cou?r)'
    + `|(?:${ORDINAL_PATTERN}) (?:season|part)|final season|season)\\b`, 'g');
const PACK_WORDS = new RegExp('\\b(?:\\+? ?(?:ovas?|oads?|movies?|films?|specials?|sp|extras?)'
    + '|complete|batch|collection|bd|dvd|english|dubbed|dual audio|high quality|tv|series'
    // Portuguese / Spanish release words ("naruto classico" is Brazil's name for the original series)
    + '|dublado|legendado|latino|classico|(?:serie|temporada) completa|capitulos|episodios)\\b', 'g');
const EXTRA = '(?:ovas?|oads?|oavs?|specials?|sp|extras?|recaps?|picture dramas?|omake|bonus)';
// "hyouka + ova", "baccano (and specials)", "complete ... ova": the series with its extras, not the extra
const PACK_RX = new RegExp('(?:\\+|&|\\band\\b|\\bwith\\b|\\bincl\\w*|\\bplus\\b|,)\\s*'
    + `(?:the\\s+|all\\s+)?(?:\\d+\\s+)?(?:${EXTRA}|movies?|films?)\\b|\\bcomplete\\b.*\\b${EXTRA}\\b`, 'i');
const EXTRA_RX = new RegExp(`\\b(${EXTRA})\\b(?: ?(\\d{1,2})\\b)?`);
const ENDING_EXTRA_RX = new RegExp(`\\b(${EXTRA})(?: \\d{1,2})?$`);
const STRIP_EXTRA = new RegExp(`\\b${EXTRA}\\b`, 'g');
// Recap and re-edit markers, with the Japanese terms Kitsu's romanised titles use for them, spelled as norm()
// leaves them (long vowels folded: "soushuuhen" -> "soshuhen"): soushuuhen 総集編 recap / compilation, tokubetsu-hen
// 特別編 special edition, (shin) henshuu-ban (新)編集版 (newly) edited version, kanzenban 完全版 complete version;
// gekijouban 劇場版 (THEATER) is the theatrical version.
const MARKERS = {
  recap: /\b(?:recaps?|recapitulation|soshu ?hen|digest|summary|highlights?|compilations?|tokubetsu ?hen)\b/g,
  reedit: new RegExp('\\b(?:directors? cut|final cut|(?:shin )?henshu ?ban|re ?edit(?:ed|ion)?|special edition'
      + '|kanzenban|remaster(?:ed)?)\\b', 'g'),
};
const NO_MARKERS = Object.freeze([]);
const THEATER = /\b(?:gekijo ?ban|theatrical)\b/g;
const YEAR_RX = /\b(19[5-9]\d|20[0-3]\d)\b/;
const STRIP_YEAR = new RegExp(YEAR_RX.source, 'g');
const ORDINAL_SEASON_RX = new RegExp(`\\b(${ORDINAL_PATTERN}) (?:season|part)\\b`);
const ROMAN_SEASON_RX = new RegExp(`\\b(?:part|season) (${ROMAN_PATTERN})\\b`);
const ENDING_ROMAN_RX = new RegExp(`^\\S+ \\S+.* (${MULTI_LETTER_ROMAN_PATTERN})$`);
const INITIALS_FILLER = new Set(['and', 'the', 'no', 'of', 'to']);

function norm(text) {
  return text.normalize('NFKD').replace(/[̀-ͯ]/g, '').toLowerCase()
      .replace(/×/g, ' x ').replace(/&/g, ' and ')
      .replace(/(\w)'s\b/g, '$1s')
      .replace(/[^\p{L}\p{N}]+/gu, ' ')
      .replace(/(\p{L})(\d)/gu, '$1 $2').replace(/(\d)(?!(?:st|nd|rd|th|d)\b)(\p{L})/gu, '$1 $2')
      .replace(/(^| )x(?= \D| ?$)/g, ' ') // a standalone × / x between words is a separator (SPY×FAMILY); x2 stays
      .replace(/ou/g, 'o').replace(/uu/g, 'u') // macron spellings: ō → o / ou / oo
      .trim().replace(/\s+/g, ' ');
}

// lowercase title keeping only the sequel punctuation ("hatarakusaibou!!", "okusamagaseitokaichou!+!", "gintama'",
// "working'!!"; not the apostrophe of "kuroko's")
const punctKey = text => text.normalize('NFKD').replace(/[̀-ͯ]/g, '').toLowerCase()
    .replace(/[＋]/g, '+').replace(/[’′]/g, "'").replace(/'(?=[a-z0-9])/g, '').replace(/[^a-z0-9!+◎']/g, '');
const squash = text => text.replace(/\s+/g, ' ').trim();
const compact = text => text.replace(/ /g, '');
const numeralsOf = text => text.split(' ')
    .map(word => ROMAN_NUMERALS[word] ?? (/^\d{1,4}$/.test(word) ? +word : undefined))
    .filter(value => value !== undefined);
const romanNumeral = number => ROMAN_WORDS[number - 1];
const extraKindOf = word => /^(ova|oad|oav)/.test(word) ? 'OVA' : 'special';
const extraFamily = word => {
  if (/^(ova|oad|oav)/.test(word)) return 'ova';
  return /^(special|sp$|extra|omake|bonus)/.test(word) ? 'special' : word;
};
const endingExtraWord = title => (title.match(ENDING_EXTRA_RX) || [])[1];
const hasEndingExtraWord = title => ENDING_EXTRA_RX.test(title);
const extraNumberOf = title => (title.match(EXTRA_RX) || [])[2];
const markersOf = text => {
  const found = Object.keys(MARKERS).filter(key => text.search(MARKERS[key]) >= 0);
  return found.length ? found : NO_MARKERS;
};
const stripMarkers = text => Object.values(MARKERS).reduce((acc, rx) => acc.replace(rx, ' '), text)
    .replace(THEATER, ' ');
const movieIndexOf = text => {
  const match = text.match(MOVIE_RX);
  return match ? +match[1] : undefined;
};
const titleMovieIndexOf = (title, subtype) => movieIndexOf(title)
    ?? (title.match(/\b(\d{1,2}) the movie\b/) || (subtype === 'movie' && title.match(/\bchapter (\d{1,2})\b/))
        || [])[1];
const titleYearOf = title => +(title.match(YEAR_RX) || [])[1] || undefined;

// season marker of a normalised title: number, FINAL_SEASON for "final season", or undefined
function seasonOf(text) {
  let match;
  // a bare ordinal in the torrent's own title ("... 4th 2026 S2") beats the S<n> the scraper adds; "lupin the 3rd" is a
  // name
  const bareOrdinal = text.match(/(?<!\bthe )\b(\d{1,2})(?:st|nd|rd|th)\b(?! (?:part|cou?r))/);
  if (bareOrdinal && /\bs ?\d{1,2}\b/.test(text)) return +bareOrdinal[1];
  if ((match = text.match(/\b(?:s|season|seasn)\s?(\d{1,2})\b/))) return +match[1];
  if ((match = text.match(/\b(\d{1,2})(?:st|nd|rd|th) season\b/))) return +match[1];
  // a season before its part ("shingeki no kyojin 3 part 2"), not a name ("86 part 2")
  match = text.match(/\b(\d{1,2}) (?:part|parte|cou?r) \d\b/);
  if (match && +match[1] <= ORDINAL_WORDS.length) return +match[1];
  if ((match = text.match(/\b(?:part|parte|cou?r)\s?(\d{1,2})\b/))) return +match[1];
  if ((match = text.match(/\b(\d{1,2})(?:st|nd|rd|th) (?:part|cou?r)\b/))) return +match[1];
  if ((match = text.match(ORDINAL_SEASON_RX))) return ORDINALS[match[1]];
  if ((match = text.match(ROMAN_SEASON_RX))) return ROMAN_NUMERALS[match[1]];
  // a roman numeral ending a title of three or more words ("... gakkou e kayou ii"), not "lupin iii" or "mk iv"
  if ((match = text.match(ENDING_ROMAN_RX))) return ROMAN_NUMERALS[match[1]];
  if ((match = text.match(/\bx ?(\d)\b/))) return +match[1];
  if (/\bfinal (?:season|part|chapter)\b/.test(text)) return FINAL_SEASON;
  if ((match = text.match(/ (\d)$/))) return +match[1];
  return undefined;
}

// what a Kitsu title is matched by: the normalised title without its season, year, movie number, extra and recap words
function titleBaseOf(title, subtype) {
  return squash(stripMarkers(title).replace(STRIP_YEAR, ' ').replace(STRIP_MOVIE, ' ')
      .replace(/\b\d{1,2} the movie\b/, ' ').replace(subtype === 'movie' ? /\bchapter \d{1,2}\b/ : /$^/, ' ')
      .replace(STRIP_SEASON, ' ').replace(/ \d$/, '').replace(STRIP_EXTRA, ' ').replace(/\btv\b/g, ' '));
}

// for movie queries the words "movie" / "film" come off too ("Kakushigoto Movie")
const withoutMovieWords = base => squash(base.replace(/\b(?:the )?(?:movie|film)\b/g, ' '));

// TV/ONA seasons sharing a canonical title apart from their season markers
const seasonlessTitle = title => squash(norm(title).replace(STRIP_SEASON, ' ').replace(/ \d$/, '')
    .replace(/\b(?:19|20)\d\d\b/g, ' '));

// a movie franchise's prefix, for numbering its movies by release ("pokemon movie 20")
const moviePrefixOf = title => squash(norm(title.split(/[:\-]/)[0]).replace(STRIP_MOVIE, ' ')
    .replace(/\b(the )?(movie|film|gekijouban|gekijoban)\b/g, ' '));

function episodeNumberOf(query) {
  // an episode range ("15-18") or a trailing absolute number ("boku no hero academia 138"), never a year
  const range = query.match(/(?:^|[\s(])(\d{1,4}) ?- ?\d{1,4}(?=[\s+)]|$)/);
  if (range && !/^(19|20)\d\d$/.test(range[1])) return +range[1];
  const trailing = norm(query).match(/ (\d{2,4})(?: v \d)?$/); // "dr stone - new world 16 v2"
  return trailing && !/^(19|20)\d\d$/.test(trailing[1]) ? +trailing[1] : undefined;
}

// A search query: `base` is what titles are matched by, the rest is what the query asks for besides the name.
function parseQuery(query) {
  // a season before a bracketed alternative title ("kuroko's basketball 2 (kuroko no baske)") goes after it, unless a
  // word ties the number to the name ("exogenesis symphony part 3 (redemption)")
  query = query.replace(SEASON_BEFORE_ALTERNATIVE_TITLE, ' $2 $1')
      .replace(/\b(\d{1,4})v\d\b/gi, '$1'); // an episode's revision ("01v2-12v2")
  const isPack = PACK_RX.test(query);
  let text = norm(query.replace(/\+\s*(ovas?|oads?|movies?|films?|specials?|sp)\b/gi, ' ')
      .replace(/\s-\s\d{1,4}(?:v\d)?(?=\s|$)/g, ' ')); // " - 09" episode numbers
  let year;
  const yearMatch = text.match(/\b(19[5-9]\d|20[0-3]\d)(?: (?:19|20)\d\d)?\b/); // the start year of a range
  if (yearMatch) {
    year = +yearMatch[1];
    text = text.replace(yearMatch[0], ' ');
  }
  const movie = movieIndexOf(text);
  if (movie !== undefined) text = squash(text.replace(STRIP_MOVIE, ' '));
  const markers = markersOf(text);
  const theater = text.search(THEATER) >= 0;
  text = squash(stripMarkers(text));
  // the extra word and its number ("ova 1", "special 01", "(p1)") come off before seasons are read
  const extraMatch = !isPack && text.match(EXTRA_RX);
  if (extraMatch) text = squash(text.replace(EXTRA_RX, ' ').replace(/\bp ?\d{1,2}\b/, ' '));
  // a release revision ("v2", "v1.5") isn't a season, but stays in the text: titles hold such numbers ("Duel Masters
  // Victory V3")
  const season = movie !== undefined ? undefined : seasonOf(squash(text.replace(/\bv \d{1,2}(?: \d)?\b/g, ' ')));
  // the part asked for: "shingeki no kyojin 3 part 2", or a part alone ("mushoku tensei cour 2"; the 86 of "86 part 2"
  // isn't a season)
  const seasonPart = text.match(/\b(\d{1,2}) (?:part|parte|cou?r) (\d)\b/);
  const lonePart = text.match(/\b(?:part|parte|cou?r) ?(\d)\b|\b(\d)(?:st|nd|rd|th) (?:part|cou?r)\b/);
  const part = seasonPart && +seasonPart[1] === season ? +seasonPart[2]
      : lonePart && +(lonePart[1] ?? lonePart[2]) === season ? season : undefined;
  const base = squash(text.replace(STRIP_SEASON, ' ').replace(/\b\d{1,3} \d{1,3}\b/g, ' ').replace(PACK_WORDS, ' '));
  return {
    full: norm(query), // the whole query, normalised
    text, // without year, movie number, recap and extra words
    base: base || text, // and without season and pack words: what titles are matched by
    punctKey: punctKey(query),
    episode: episodeNumberOf(query),
    year,
    season,
    part,
    tvdbSeason: season !== undefined && /\b(?:s ?\d{1,2}|season ?\d{1,2})\b/.test(text), // "S2" / "season 2"
    movie, // "movie N"
    bareMovie: movie === undefined && /\b(?:movie|film)\b/.test(text), // "movie" without a number
    theater,
    markers, // recap / re-edit
    extraKind: extraMatch ? extraKindOf(extraMatch[1]) : undefined, // 'OVA' or 'special'
    extraWord: extraMatch ? extraMatch[1] : undefined,
    extraNumber: extraMatch && extraMatch[2] ? +extraMatch[2] : undefined,
    isPack,
    wantsExtra: movie !== undefined
        || (!isPack && (!!extraMatch || markers.length > 0 || theater || /\b(movie|film|pilot)\b/.test(text))),
  };
}

// several alternative titles in one name ("Doupo Cangqiong / Dou Po Cang Qiong / Battle Through The Heavens"); cleaned
// queries have lost the "/" and keep a double space, which "&" removal also leaves ("cloak  dagger"), so a double-space
// split needs two words in every part
function splitTitles(query) {
  const separated = query.split(/\s+(?:\/|\||aka|a\.k\.a\.)\s+/i).map(part => part.trim()).filter(Boolean);
  if (separated.length > 1) return separated;
  // "Wu Ni (Martial Inverse)": a bracketed alternative title ("(TV)" and "(2019)" stay)
  const paren = query.match(/^(.*?)\s*\(([^()]+)\)\s*(.*)$/);
  if (paren && /\p{L}{4}/u.test(paren[2]) && !/^(?:tv|ova|oad|movie|special|uncut|dub|sub)$/i.test(paren[2].trim())
      && paren[1].trim()) {
    return [`${paren[1]} ${paren[3]}`.trim(), `${paren[2]} ${paren[3]}`.trim()];
  }
  const spaced = query.split(/\s{2,}/).map(part => part.trim()).filter(Boolean);
  return spaced.length > 1 && spaced.every(part => part.split(' ').length >= 2) ? spaced : [query];
}

// merge adjacent title words a query writes glued together ("shadow side" for "shadowside"), or as initials ("bw" for
// "best wishes" / "black and white"; Kitsu's own titles only: synonyms like "Clown Arc" make chance initials like "ca")
function glue(titleWords, queryWords, initialsOk = true) {
  const query = new Set(queryWords);
  const out = [];
  out.initials = new Set(); // the query words matched as initials
  outer: for (let i = 0; i < titleWords.length; i++) {
    if (i + 1 < titleWords.length && !query.has(titleWords[i]) && query.has(titleWords[i] + titleWords[i + 1])) {
      out.push(titleWords[i] + titleWords[i + 1]);
      i++;
      continue;
    }
    const word = titleWords[i];
    if (initialsOk && !query.has(word) && /^[a-z]/.test(word)) {
      // a leading "the" only in longer initials ("thoyw" for "the hidden one year war"), up to one word more
      const leadingFiller = INITIALS_FILLER.has(word);
      const [minWords, maxWords] = leadingFiller ? [4, 5] : [2, 4];
      let initials = '';
      let used = 0;
      for (let j = i; j < titleWords.length && used < maxWords; j++) {
        if (INITIALS_FILLER.has(titleWords[j]) && j > i) continue;
        initials += titleWords[j][0];
        used++;
        if (used >= minWords && query.has(initials) && !query.has(titleWords[j])) {
          out.push(initials);
          out.initials.add(initials);
          i = j;
          continue outer;
        }
      }
    }
    out.push(titleWords[i]);
  }
  return out;
}

module.exports = {
  FINAL_SEASON, MARKERS, NO_MARKERS, EXTRA_RX,
  norm, punctKey, squash, compact, numeralsOf, romanNumeral, extraFamily, endingExtraWord, hasEndingExtraWord,
  extraNumberOf, markersOf, titleMovieIndexOf, titleYearOf, seasonOf, titleBaseOf, withoutMovieWords, seasonlessTitle,
  moviePrefixOf, parseQuery, splitTitles, glue,
};
