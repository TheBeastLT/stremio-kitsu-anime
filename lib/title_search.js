// In-memory title search over the Kitsu catalog: matches a torrent-style query ("tokyo revengers 2023 S2",
// "hyouka + ova", "detective conan movie 3") against every entry's Kitsu titles plus anime-offline-database synonyms,
// then ranks the title matches with season, year, type and franchise rules. Tuned on search_eval/ (see its README).
const distance = require('jaro-winkler');
const {
  FINAL_SEASON, MARKERS, NO_MARKERS, EXTRA_RX, norm, punctKey, compact, numeralsOf, romanNumeral, extraFamily,
  endingExtraWord, hasEndingExtraWord, extraNumberOf, markersOf, titleMovieIndexOf, titleYearOf, seasonOf, titleBaseOf,
  withoutMovieWords, seasonlessTitle, moviePrefixOf, parseQuery, splitTitles, glue,
} = require('./title_query');

// Below this a top result is a guess (a partial or chance match, often a cartoon filed as anime): search returns
// nothing.
const MIN_CONFIDENCE = 0.65;
// Torrent titles stay well under this (the longest of 51k real queries is 188); longer ones only cost scoring time.
const MAX_QUERY_LENGTH = 250;
const SEARCHED_SUBTYPES = ['TV', 'OVA', 'ONA', 'movie', 'special'];
const SEARCHED_STATUSES = ['finished', 'current', 'upcoming'];
const SERIES_SUBTYPES = ['TV', 'ONA'];
const EXTRA_SUBTYPES = ['OVA', 'special'];
const SUBTYPE_RANK = { TV: 0, ONA: 1, OVA: 2, movie: 3, special: 4, music: 5 };
const FILLER_WORDS = new Set(['the', 'a', 'an', 'of', 'and', 'no', 'to', 'in', 'wa', 'ga', 'wo', 'ni']);
// synonyms numbered by other sites ("Episode 576.5", "Recap 7", "Film 14"): dropped
const NUMBERED_SYNONYM = /\b(?:episode|ep|recap|movie|film|pelicula|filme|part|vol|volume) \d/;

// How well a query matches an entry's titles, 0 to 1.
const TITLE_SCORE = {
  exact: 1,
  exactMapping: 0.985, // the imdb mapping's English title
  exactSynonym: 0.95, // an anime-offline-database synonym, a little below Kitsu's own titles
  synonymScale: 0.95, // a partial match on a synonym
  numberedMovie: 0.97, // the franchise name inside a movie title with the queried number ("dragon ball z movie 01")
  wordWeight: 0.6, // a partial match: idf-weighted word overlap, the rest jaro-winkler similarity
  coverageWeight: 0.35, // a query word missing from a title costs more than an extra title word (shortened titles)
  // a title holding the query's extra word inside it ("Fate/Extra: Last Encore") matching this well: a name, not an
  // extra request
  nameWithExtraWord: 0.8,
  initialsWeight: 0.5,
};
const CLOSE_WORD = 0.85; // jaro-winkler similarity of two spellings of one word
const CANDIDATES = {
  commonWordEntries: 400, // a word in more entries than this only fetches candidates when the query has no rarer word
  maxScored: 1000, // candidates sharing the most query words that get their titles scored
  max: 80, // best title matches the ranking rules look at
  // a name word in at most this many entries names the show ("igloo" in "mobile suit gundam igloo")
  distinctiveEntries: 10,
  nameWordLength: 3, // shorter words ("la", "z", "s") don't name a show
  maxTrigram: 200,
  minTrigramShare: 0.6, // of the query's trigrams an entry needs to be a trigram candidate
  minTrigramHits: 2,
};
// How far below the best title score a candidate still counts for a rule.
const BAND = {
  tie: 0.06, // about as good as the best: type, season and age tie-breaks
  reach: 0.1, // season, numeral and episode rules
  year: 0.15,
  earlierPart: 0.2,
  distinctive: 0.2, // a candidate holding the query's distinctive words
  sibling: 0.01, // an earlier season matching as well
  // an extra / movie of the requested kind holding the franchise name lands this far below the best, and up to
  // requestedKindSpread higher the closer its title is
  requestedKind: 0.04,
  requestedKindSpread: 0.03,
};
const BONUS = {
  seasonMatch: 0.08,
  seasonMismatch: -0.06, // a known season other than the queried one, when another candidate has it
  seasonWord: 0.03, // "S2" for a title saying "season 2" (not "part 2")
  laterSeasonExtra: -0.03, // "2nd Season Special" for an extra request without a season
  punctuatedSequel: 0.08,
  wholeQueryTitle: 0.03, // the whole query, extra words included, is a title
  distinctiveWords: 0.2, // BAND.distinctive
  missingWords: -0.3, // per share of the telling query words a near-equal match lacks, against the best covering one
  episodeInRange: 0.12,
  episodeOverflow: -0.05, // an episode number past the entry's episode count
  markerMatch: 0.2, // a recap / re-edit request and title
  markerUnasked: -0.04, // a recap / re-edit title for a plain query
  movieLean: 0.07, // a movie / compilation / theatrical request leans to movies
  subtypeStep: -0.02, // per SUBTYPE_RANK step below TV, among equal matches
  extraWordUnasked: -0.04, // a title ending in an extra word for a query not asking for one
  extra: 0.06, // an OVA / special for an extra request
  extraWordFamily: 0.02, // the title ends in the queried extra word's kind ("... Specials" for "specials")
  extraWordForm: 0.002, // and in its exact form ("special" vs "specials")
  extraFranchiseName: 0.02, // an extra titled exactly the franchise name ("Mazinkaiser", an OVA)
  extraNumber: 0.04, // "ova 2" for "OVA 2" (another number: the same against)
  movieForExtra: -0.04,
  movieSubtype: 0.04, // a movie for "movie N"
  movieNumber: 0.15, // "movie N" for the N-th movie (another number: the same against)
  year: 0.12,
  yearConfirmed: 0.15, // when a candidate close by started that year
  yearNear: 0.03, // a year off by one
  yearWithSeason: 0.03, // next to a season the year is usually the series premiere ("Show 2018 S03")
  yearNearWithSeason: 0.01,
  firstSeason: 0.02, // the leading franchise's first season (imdb mapping), for a plain query
  earlierSibling: -0.03, // a later season when an earlier one matches as well
  perYear: -0.0002, // per year since TIE_BREAK_YEAR: the earlier entry among equals
  perYearExtra: -0.0003,
};
const TIE_BREAK_YEAR = 1950;
const CONFIDENCE = {
  partialCoverage: 0.15, // taken off for the share of query words the top title lacks
  gluedCoverage: 0.85, // of query words across all the top entry's titles: several titles glued in one name
  gluedMinScore: 0.4,
  gluedBase: 0.6, // ... rising to 1 at full coverage
  splitAgreementScore: 0.9, // alternative titles in one name agreeing on an entry
  splitAgreement: 0.01,
};

// An entry: one Kitsu anime, with
// - titles: its title variants (see toTitleVariant): Kitsu's own first, then synonyms (fromSynonym), then the imdb
//   mapping's English title (fromMapping); abbreviated: one of Kitsu's abbreviated titles; ownTitles: the normalised
//   texts of the ones that aren't synonyms
// - titleSeason: the season its own titles name; imdbSeason: the imdb mapping's season (TVDB-style numbering) of its
//   franchise (the imdb id); season: titleSeason, else imdbSeason when the mapping starts at its first episode
// A candidate: an entry matched to a query, with its title `score`, the `final` score after the ranking rules, and
// - matchedWithoutNumber: its best title matched only with its own season / part number removed ("Honzuki no
//   Gekokujou 2" for "honzuki no gekokujou")
// - wholeQueryIsTitle: the query, extra words included, is one of its titles
// - bestTitleMovie: the movie number of its best matching title

// records: [{ id, canonicalTitle, titles, abbreviatedTitles, subtype, status, startDate, episodeCount, synonyms }]
// imdbMapping(kitsuId): { imdb_id, title, fromSeason, fromEpisode } of the repo's imdb mapping, if any
function createTitleIndex(records, { imdbMapping = () => undefined } = {}) {
  const entries = records
      .filter(record => SEARCHED_SUBTYPES.includes(record.subtype) && SEARCHED_STATUSES.includes(record.status))
      .map(record => toEntry(record, imdbMapping(record.id)));
  numberUnnumberedMovies(entries);
  const seasonGroups = groupSeasons(entries);
  layOutEpisodes(entries);

  const entriesWithWord = new Map();
  entries.forEach((entry, index) => new Set(entry.titles.flatMap(variant => variant.title.split(' ')))
      .forEach(word => pushTo(entriesWithWord, word, index)));
  const entryCount = word => entriesWithWord.get(word)?.length || 0;
  const idf = word => Math.log(entries.length / (1 + entryCount(word)));
  let trigrams;

  // spelling similarity, from the first query word the title has: leading words it lacks ("mobile suit" before "sd
  // gundam force") would only cost the prefix-weighted similarity, and the word score already counts them missing
  function charSimilarity(queryWords, title) {
    const titleWords = title.split(' ');
    const inTitle = word => titleWords.some(other => other === word || distance(word, other) >= CLOSE_WORD);
    let start = 0;
    while (start < queryWords.length - 1 && !inTitle(queryWords[start])) start++;
    return distance(queryWords.slice(start).join(' '), title);
  }

  function tokenScore(queryWords, titleWords) {
    const title = new Set(titleWords);
    // a word matched as initials (glue) is a guess: worth less than the written word ("sd" is "SD Gundam" before
    // "Gundam SEED Destiny")
    const weight = word => idf(word) * (titleWords.initials?.has(word) ? TITLE_SCORE.initialsWeight : 1);
    let shared = 0, queryWeight = 0, titleWeight = 0;
    new Set(queryWords).forEach(word => {
      queryWeight += idf(word);
      if (title.has(word)) shared += weight(word);
    });
    title.forEach(word => { titleWeight += weight(word); });
    const dice = queryWeight + titleWeight ? 2 * shared / (queryWeight + titleWeight) : 0;
    const coverage = queryWeight ? shared / queryWeight : 0;
    return (1 - TITLE_SCORE.coverageWeight) * dice + TITLE_SCORE.coverageWeight * coverage;
  }

  // words the index has never seen ("rehamatora" for "Re: Hamatora"): candidates by character trigrams instead
  function trigramCandidates(base) {
    if (!trigrams) {
      trigrams = new Map();
      entries.forEach((entry, index) => {
        const grams = new Set();
        entry.titles.filter(variant => !variant.fromSynonym || /^[a-z0-9 ]+$/.test(variant.base))
            .forEach(variant => trigramsOf(compact(variant.base)).forEach(gram => grams.add(gram)));
        grams.forEach(gram => pushTo(trigrams, gram, index));
      });
    }
    const grams = trigramsOf(compact(base));
    const hits = new Map();
    grams.forEach(gram => (trigrams.get(gram) || []).forEach(index => hits.set(index, (hits.get(index) || 0) + 1)));
    const minHits = Math.max(CANDIDATES.minTrigramHits, CANDIDATES.minTrigramShare * grams.size);
    return [...hits].filter(([, count]) => count >= minHits)
        .sort((a, b) => b[1] - a[1]).slice(0, CANDIDATES.maxTrigram).map(([index]) => index);
  }

  // entries sharing the query's rarer words (all its words when every one is common); past CANDIDATES.maxScored, the
  // ones sharing the most
  function candidatesFor(parsed, queryWords) {
    const weights = new Map();
    if (queryWords.some(word => !entriesWithWord.has(word))) {
      trigramCandidates(parsed.base).forEach(index => weights.set(index, Infinity));
    }
    const rare = queryWords.filter(word => entryCount(word) < CANDIDATES.commonWordEntries);
    (rare.length ? rare : queryWords).forEach(word => (entriesWithWord.get(word) || [])
        .forEach(index => weights.set(index, (weights.get(index) || 0) + idf(word))));
    const indexes = [...weights.keys()];
    if (indexes.length <= CANDIDATES.maxScored) return indexes;
    return indexes.sort((a, b) => weights.get(b) - weights.get(a)).slice(0, CANDIDATES.maxScored);
  }

  // share of the query's words found in the entry's best title, and across all its titles together
  function coverageOf(parsed, entry) {
    const words = parsed.base.split(' ').filter(word => word && !FILLER_WORDS.has(word) && !/^\d+$/.test(word));
    if (!words.length) return { single: 1, all: 1 };
    // a roman numeral is covered by the entry being that season ("douluo dalu ii" for "Douluo Dalu 2")
    const season = entry.titleSeason ?? entry.season;
    const seasonWord = word => word.length > 1 && season !== undefined && numeralsOf(word)[0] === season;
    const glued = entry.titles.map(variant => glue(baseFor(variant, parsed).split(' '), words, !variant.fromSynonym))
        .map(title => [...title, ...words.filter(seasonWord)]);
    const single = Math.max(0, ...glued.map(title => {
      const set = new Set(title);
      return words.filter(word => set.has(word)).length / words.length;
    }));
    const all = new Set(glued.flat());
    return { single, all: words.filter(word => all.has(word)).length / words.length };
  }

  function scoreTitles(entry, parsed, queryWords) {
    let best = 0, matchedWithoutNumber = false, bestTitleMovie, matchedByMapping = false, seasonTitleScore = 0;
    // whether the best matching titles include a Kitsu main title without an ending extra word / an own title with one
    let plainMatched = false, extraMatched = false;
    for (const variant of entry.titles) {
      const base = baseFor(variant, parsed);
      const exact = base === parsed.base || variant.title === parsed.base || variant.title === parsed.text
          || compact(base) === compact(parsed.base)
          // the whole query equal to a title with spaces ignored ("x1999" for "X/1999"), before a year / season is
          // taken out
          || (!variant.fromSynonym && compact(variant.title) === compact(parsed.full));
      let score;
      if (exact) {
        score = variant.fromMapping ? TITLE_SCORE.exactMapping
            : variant.fromSynonym ? TITLE_SCORE.exactSynonym : TITLE_SCORE.exact;
      } else {
        score = TITLE_SCORE.wordWeight * tokenScore(queryWords, glue(base.split(' '), queryWords, !variant.fromSynonym))
            + (1 - TITLE_SCORE.wordWeight) * charSimilarity(queryWords, base);
        if (variant.fromSynonym) score *= TITLE_SCORE.synonymScale;
      }
      if (parsed.movie !== undefined && (entry.movie ?? entry.derivedMovie) === parsed.movie) {
        const words = new Set(base.split(' '));
        if (queryWords.every(word => words.has(word))) score = Math.max(score, TITLE_SCORE.numberedMovie);
      }
      if (!variant.fromMapping && parsed.season && variant.season === parsed.season) {
        seasonTitleScore = Math.max(seasonTitleScore, score);
      }
      const own = !variant.fromSynonym && !variant.fromMapping;
      const extra = own && hasEndingExtraWord(variant.title);
      // not an abbreviated title: those often hold the franchise name ("The Ancient Magus' Bride" for its Specials)
      const plain = own && !extra && !variant.abbreviated;
      if (score === best) {
        plainMatched ||= plain;
        extraMatched ||= extra;
      }
      if (score > best) {
        best = score;
        plainMatched = plain;
        extraMatched = extra;
        bestTitleMovie = variant.movie;
        matchedByMapping = !!variant.fromMapping;
        matchedWithoutNumber = exact && variant.title !== parsed.base && variant.title !== parsed.text
            && (variant.strippedSeason ?? variant.season) > 1;
      }
    }
    const wholeQueryIsTitle = entry.titles.some(variant => !variant.fromSynonym && variant.title !== parsed.base
        && (variant.title === parsed.text || variant.title === parsed.full));
    // matched as an extra: by a title ending in an extra word ("Matsuri Special"), or by a synonym while the entry's
    // main titles end in one; not when a main title without one matched ("Matsuri" next to its "... OVA" English title)
    const matchedAsExtra = !plainMatched && (extraMatched || entry.hasExtraWord);
    return { entry, score: best, matchedAsExtra, wholeQueryIsTitle, matchedWithoutNumber, bestTitleMovie,
      matchedByMapping, seasonTitleScore };
  }

  // an extra or movie request: an entry of that kind whose title holds the whole franchise name is a near-top match
  // ("isuca - ova 1": "Isuca: Gokuraku", "sailor moon s movie": "Sailor Moon S: Kaguya Hime no Koibito")
  function preferRequestedKind(scored, parsed, queryWords) {
    const movieRequest = (parsed.bareMovie || parsed.theater) && parsed.movie === undefined;
    const kind = parsed.extraKind ? 'extra' : movieRequest ? 'movie' : undefined;
    if (!kind) return;
    const top = scored[0].score;
    scored.forEach(candidate => {
      const { entry } = candidate;
      if (kind === 'extra' ? !isExtraEntry(entry) : entry.subtype !== 'movie') return;
      const holding = entry.titles.filter(variant => {
        const words = new Set(glue(variant.base.split(' '), queryWords));
        return queryWords.every(word => words.has(word));
      });
      if (holding.length) {
        const closest = Math.max(...holding.map(variant => tokenScore(queryWords, variant.base.split(' '))));
        candidate.score = Math.max(candidate.score, top - BAND.requestedKind + BAND.requestedKindSpread * closest);
      }
    });
    sortByScore(scored);
  }

  // a later season or part that matched only with its own number removed ("Honzuki no Gekokujou 2", "Kizumonogatari
  // Part 2" for the plain name) names its franchise: the first season / earliest part gets the same title score
  function preferFirstOfFranchise(scored, parsed) {
    if (parsed.season || parsed.wantsExtra || parsed.movie !== undefined) return;
    const top = scored[0].score;
    const stripped = within => scored.filter(candidate => candidate.matchedWithoutNumber && candidate.score >= within);
    stripped(top - BAND.tie).forEach(candidate => {
      const first = earliestOf(seasonGroups.get(candidate.entry.groupKey) || []);
      if (!first || first === candidate.entry) return;
      const present = scored.find(other => other.entry === first);
      if (present) present.score = Math.max(present.score, candidate.score);
      else scored.push({ entry: first, score: candidate.score, wholeQueryIsTitle: false, matchedWithoutNumber: false });
    });
    // numbered parts of any type, when the query is just the shared name (no numeral, no year) and the earlier part's
    // titles hold all of it (not "bleach thousand-year blood war - the calamity")
    if (!parsed.year && !numeralsOf(parsed.base).length) {
      const currentTop = scored[0].score;
      stripped(currentTop - BAND.tie).forEach(candidate => {
        const earlier = scored.filter(other => other !== candidate && other.entry.subtype === candidate.entry.subtype
            && other.score >= currentTop - BAND.earlierPart
            && (other.entry.titleSeason ?? 1) < (candidate.entry.titleSeason ?? 1)
            && coverageOf(parsed, other.entry).all >= 1);
        if (!earlier.length) return;
        const firstEntry = earliestOf(earlier.map(other => other.entry));
        const first = earlier.find(other => other.entry === firstEntry);
        first.score = Math.max(first.score, candidate.score);
        candidate.score -= BAND.sibling; // and gives way to it
      });
    }
    sortByScore(scored);
  }

  // "fate extra last encore": an extra word inside a close title is part of its name, not an extra request; with its
  // number, inside any of the entry's own titles ("tokunana (special 7)": "Special 7: Special Crime Investigation
  // Unit"), though not ending one ("Slam Dunk OVA 3")
  function extraWordInName(scored, parsed) {
    if (!parsed.extraKind) return false;
    const word = (parsed.full.match(EXTRA_RX) || [])[1];
    const ending = new RegExp(`\\b${word}(?: \\d{1,2})?$`);
    const numbered = parsed.extraNumber !== undefined && new RegExp(`\\b${word} 0?${parsed.extraNumber}\\b`);
    return scored.slice(0, 10).some(({ entry }) => !EXTRA_SUBTYPES.includes(entry.subtype) && !entry.hasExtraWord
        && entry.titles.some(variant => !variant.fromMapping && !variant.fromSynonym && (
          (numbered && numbered.test(variant.title) && !ending.test(variant.title))
          || (variant.title.split(' ').includes(word) && !ending.test(variant.title)
            && variant.base.split(' ').length > 1
            && tokenScore(parsed.full.split(' '), variant.title.split(' ')) >= TITLE_SCORE.nameWithExtraWord))));
  }

  // whether a candidate's titles hold a query word (split spellings joined: "pri chan" for "prichan")
  // (a plural spelling holds the word too: "Duel Monsters" for "duel monster")
  function holdsWord({ entry }, word, queryWords) {
    const forms = [word, `${word}s`, ...(word.length > 3 && word.endsWith('s') ? [word.slice(0, -1)] : [])];
    return entry.titles.some(variant => glue(variant.title.split(' '), queryWords, false)
        .some(titleWord => forms.includes(titleWord)));
  }

  // the query words that can name a show: not short, filler or numbers, nor a word the best match lacks while one of
  // its titles is all query words (noise around a name: "dragon ball - uncut", "bleach anime")
  function nameWords(scored, queryWords) {
    const query = new Set(queryWords);
    const topTitleInQuery = scored[0].entry.titles.some(variant => glue(variant.title.split(' '), queryWords, false)
        .every(word => query.has(word)));
    return [...query].filter(word => word.length >= CANDIDATES.nameWordLength && !FILLER_WORDS.has(word)
        && !/^\d+$/.test(word) && (!topTitleInQuery || holdsWord(scored[0], word, queryWords)));
  }

  // the rare name words (CANDIDATES.distinctiveEntries): the close candidates holding all of them get
  // distinctiveWordBonus
  function markDistinctiveWords(scored, queryWords, words) {
    const distinctive = words.filter(word => entryCount(word) > 0 && entryCount(word) <= CANDIDATES.distinctiveEntries);
    if (!distinctive.length) return;
    const top = scored[0].score;
    scored.forEach(candidate => {
      candidate.holdsDistinctiveWords = candidate.score >= top - BAND.distinctive
          && distinctive.every(word => holdsWord(candidate, word, queryWords));
    });
  }

  // near-equal matches (BAND.tie): the share of the name words telling them apart (held by some, at most half of
  // them) each one's titles hold, for coverageBonus ("gaiden" among the Mobile Suit SD Gundam entries)
  function markTellingCoverage(scored, queryWords, words) {
    const near = scored.filter(candidate => candidate.score >= scored[0].score - BAND.tie);
    const telling = words.filter(word => {
      const holders = near.filter(candidate => holdsWord(candidate, word, queryWords)).length;
      return holders > 0 && holders <= near.length / 2;
    });
    if (!telling.length) return;
    near.forEach(candidate => {
      candidate.coverage = telling.filter(word => holdsWord(candidate, word, queryWords)).length / telling.length;
    });
  }

  // the best entries for a query with their title scores, the top one with its confidence
  function rank(query, limit) {
    const parsed = parseQuery(query);
    const queryWords = parsed.base.split(' ');
    const scored = sortByScore(candidatesFor(parsed, queryWords)
        .map(index => scoreTitles(entries[index], parsed, queryWords))).slice(0, CANDIDATES.max);
    if (!scored.length) return [];
    preferRequestedKind(scored, parsed, queryWords);
    preferFirstOfFranchise(scored, parsed);
    const words = nameWords(scored, queryWords);
    markDistinctiveWords(scored, queryWords, words);
    markTellingCoverage(scored, queryWords, words);
    const asked = extraWordInName(scored, parsed) ? { ...parsed, extraKind: undefined, wantsExtra: false } : parsed;
    const results = applyRules(scored, asked).slice(0, limit);
    results[0].confidence = confidenceOf(results[0], coverageOf(parsed, results[0].entry));
    return results;
  }

  // [{ id, score, confidence (top result only) }] best first; empty when the top result's confidence is below
  // minConfidence (MIN_CONFIDENCE; the evaluation lowers it to see every ranking)
  function search(query, { limit = 20, minConfidence = MIN_CONFIDENCE } = {}) {
    const trimmed = query.length > MAX_QUERY_LENGTH ? query.slice(0, MAX_QUERY_LENGTH).replace(/\s+\S*$/, '') : query;
    let results = rank(trimmed, limit);
    const parts = results[0]?.confidence >= MIN_CONFIDENCE ? [] : splitTitles(trimmed);
    if (parts.length > 1) {
      // the whole name matched weakly: it may hold several alternative titles; a part only wins by matching better
      const runs = parts.map(part => rank(part, limit)).filter(run => run[0]);
      const agreeing = id => runs
          .filter(run => run[0].entry.id === id && run[0].score >= CONFIDENCE.splitAgreementScore).length;
      const support = run => run[0].confidence + CONFIDENCE.splitAgreement * agreeing(run[0].entry.id);
      const best = runs.sort((a, b) => support(b) - support(a))[0];
      if (best && best[0].confidence > (results[0]?.confidence ?? 0)) results = best;
    }
    if (!(results[0]?.confidence >= minConfidence)) return [];
    return results.map(({ entry, final, confidence }) => ({ id: entry.id, score: final, confidence }));
  }

  return { search, size: entries.length };
}

// A name holding several of the entry's titles glued together ("ojisan wa kawaii mono ga osuki uncle's obsession with
// cute things") covers its words across those titles: confident although no single title matches it all.
function confidenceOf(top, coverage) {
  const gluedShare = (coverage.all - CONFIDENCE.gluedCoverage) / (1 - CONFIDENCE.gluedCoverage);
  const glued = coverage.all >= CONFIDENCE.gluedCoverage && top.score >= CONFIDENCE.gluedMinScore
      ? CONFIDENCE.gluedBase + (1 - CONFIDENCE.gluedBase) * gluedShare
      : 0;
  return Math.max(top.score - CONFIDENCE.partialCoverage * (1 - coverage.single), glued);
}

// The ranking rules: preferences on top of the title score, for the candidates close to the best match.
function applyRules(scored, parsed) {
  const top = scored[0].score;
  const near = scored.filter(candidate => candidate.score >= top - BAND.reach);
  const context = {
    top,
    leader: scored[0].entry,
    scored,
    seasonAround: parsed.season
        && near.some(({ entry }) => [entry.titleSeason, entry.imdbSeason].includes(parsed.season)),
    imdbSeasonAround: parsed.season && near.some(({ entry }) => entry.imdbSeason === parsed.season),
    sequelByPunctuation: punctuatedSequel(near, parsed),
    bestCoverage: Math.max(0, ...scored.map(candidate => candidate.coverage ?? 0)),
    // a tie-break needs a tie: alone in the band, the best match keeps its lead ("mobile suit sd gundam mk i")
    tied: scored.filter(candidate => candidate.score >= top - BAND.tie).length > 1,
    yearAround: parsed.year && !parsed.season && scored.filter(candidate => candidate.score >= top - BAND.year)
        .some(({ entry }) => entry.year === parsed.year || entry.titleYears.includes(parsed.year)),
    // the episode fits an entry close to the best match counted from its own first episode
    episodeFromStart: parsed.episode && scored.some(({ entry, score }) => score >= top - BAND.reach
        && SERIES_SUBTYPES.includes(entry.subtype) && (entry.imdbFromEpisode ?? 1) === 1
        && entry.episodeCount >= parsed.episode),
    recapAround: scored
        .some(candidate => candidate.score >= top - BAND.tie && candidate.entry.markers.includes('recap')),
  };
  return scored.map(candidate => {
    const { entry } = candidate;
    const rule = {
      candidate,
      entry,
      parsed,
      context,
      inBand: candidate.score >= top - BAND.tie,
      inReach: candidate.score >= top - BAND.reach,
      seasonMatch: isSeasonMatch(candidate, parsed, context),
    };
    const bonus = RULES.reduce((sum, ruleBonus) => sum + ruleBonus(rule), 0);
    return { ...candidate, final: candidate.score + bonus };
  }).sort((a, b) => b.final - a.final);
}

// The scraper writes any season as S<n>, whether the torrent said "2nd Season" (Kitsu's numbering) or S02 (TVDB's),
// so either counts; an S<n> / "season n" wants a series, not "Naruto Movie 2" for "naruto s2" (a bare number can be a
// movie's: "stand by me doraemon 2"). Matched by the mapping's franchise name next to an entry that is that TVDB
// season, an entry's own season counts only when a title naming it matches about as well or the queried year is its
// own ("sword art online s2" isn't "Alicization - War of Underworld 2nd Season"; "jojo's bizarre adventure 2018 s5" is
// Golden Wind).
function isSeasonMatch(candidate, parsed, context) {
  const { entry } = candidate;
  if (entry.subtype === 'movie' && parsed.tvdbSeason) return false;
  if (entry.imdbSeason === parsed.season) return true;
  if (entry.titleSeason === undefined && entry.imdbSeason === undefined) return parsed.season === 1;
  return entry.titleSeason === parsed.season && (!candidate.matchedByMapping || !context.imdbSeasonAround
      || candidate.seasonTitleScore >= candidate.score - BAND.reach || entry.year === parsed.year);
}

function seasonBonus({ entry, parsed, context, inReach, seasonMatch }) {
  let bonus = 0;
  // an extra request without a season wants the first season's extra, not "2nd Season Special"
  if (parsed.extraKind && !parsed.season && inReach && entry.titleSeason > 1
      && entry.ownTitles.some(title => /\b(?:\d{1,2}(?:st|nd|rd|th) season|season \d{1,2})\b/.test(title))) {
    bonus += BONUS.laterSeasonExtra;
  }
  if (parsed.extraKind && parsed.season && inReach) {
    // "spice and wolf ii specials": the season picks which extra
    const extraSeason = isExtraEntry(entry) && (seasonMatch || entry.ownTitles.some(title => title.split(' ')
        .some(word => word === String(parsed.season) || word === romanNumeral(parsed.season))));
    if (extraSeason) bonus += BONUS.seasonMatch;
  } else if (parsed.season && inReach) {
    const finalMatch = parsed.season === FINAL_SEASON && entry.ownTitles.some(title => /final/.test(title));
    if (seasonMatch || finalMatch) bonus += BONUS.seasonMatch;
    else if (context.seasonAround && (entry.titleSeason !== undefined || entry.imdbSeason !== undefined)) {
      bonus += BONUS.seasonMismatch;
    }
    // a season named with a word ("S2", "2nd season"): the entry whose own title names it, not one only mapped there
    // that season, or its part as TVDB counts split cours ("War of Underworld Part 2" for "S2"); not another season
    // ("Steven Universe Season 6", mapped as its own imdb's S1)
    const namesSeason = title => /\b(?:season \d|\d(?:st|nd|rd|th) season)\b/.test(title)
        && (seasonOf(title) === parsed.season || entry.partNumber === parsed.season);
    if ((parsed.tvdbSeason || /\b\d{1,2}(?:st|nd|rd|th) season\b/.test(parsed.text)) && seasonMatch
        && entry.ownTitles.some(namesSeason)) {
      bonus += BONUS.seasonWord;
    }
    // the season's part, from the entry's own titles (no part number: the first)
    if (parsed.part && seasonMatch && (entry.partNumber ?? 1) === parsed.part) bonus += BONUS.seasonWord;
    bonus += ageTieBreak(entry, BONUS.perYear);
  }
  return bonus;
}

function titleFormBonus({ candidate, context }) {
  return (candidate === context.sequelByPunctuation ? BONUS.punctuatedSequel : 0)
      + (candidate.wholeQueryIsTitle ? BONUS.wholeQueryTitle : 0);
}

// absolute episode numbers: the franchise's TV entries laid end to end; an episode past an entry's count rules it out
function episodeBonus({ entry, parsed, context, inReach }) {
  if (!parsed.episode || !inReach) return 0;
  let bonus = 0;
  const inRange = (from, count) => from > 1 && parsed.episode >= from && (!count || parsed.episode < from + count);
  // a later cour numbered on within its season ("dr stone new world 16": Part 2, 12-22), when no close match holds the
  // episode from its own start (not "shingeki no kyojin 21": the first season has 25)
  const laterCour = !context.episodeFromStart && entry.imdbSeason > 0
      && inRange(entry.imdbFromEpisode, entry.episodeCount);
  if (!parsed.season && (inRange(entry.absFrom, entry.absTo - entry.absFrom + 1) || laterCour)) {
    bonus += BONUS.episodeInRange;
  }
  if (entry.episodeCount && parsed.episode > entry.episodeCount + 1
      && !entry.titles.some(variant => variant.title.split(' ').includes(String(parsed.episode)))) {
    bonus += BONUS.episodeOverflow;
  }
  return bonus;
}

function markerBonus({ entry, parsed, context, inBand }) {
  if (!inBand) return 0;
  let bonus = 0;
  for (const marker of Object.keys(MARKERS)) {
    if (parsed.markers.includes(marker)) bonus += entry.markers.includes(marker) ? BONUS.markerMatch : 0;
    else if (entry.markers.includes(marker)) bonus += BONUS.markerUnasked;
  }
  // compilation movies rarely say so: with no recap entry around, a recap request leans to movies
  const recapRequest = parsed.markers.includes('recap') && !context.recapAround;
  if ((recapRequest || parsed.theater || parsed.bareMovie) && !parsed.extraKind) {
    bonus += entry.subtype === 'movie' ? BONUS.movieLean : 0;
  }
  return bonus;
}

// among near-equal matches, the query words telling them apart weigh more than the type and age tie-breaks ("mobile
// suit sd gundam gaiden": the OVA SD Gundam Gaiden, not the series Gundam SEED Destiny)
function coverageBonus({ candidate, context, inBand }) {
  return inBand && candidate.coverage !== undefined
      ? BONUS.missingWords * (context.bestCoverage - candidate.coverage) : 0;
}

// the words naming the show ("igloo" in "mobile suit gundam igloo") over a closer title without them; the same for
// every candidate holding them, so their own order stays
function distinctiveWordBonus({ candidate }) {
  return candidate.holdsDistinctiveWords ? BONUS.distinctiveWords : 0;
}

// a season query wants the series itself: TV over other types, not a title ending in an extra word
function seasonQueryTypeBonus({ entry, parsed, context, inBand }) {
  if (!inBand || !context.tied || !parsed.season || parsed.wantsExtra) return 0;
  return BONUS.subtypeStep * subtypeRank(entry) + (entry.hasExtraWord ? BONUS.extraWordUnasked : 0);
}

// the title's own extra word decides between extras; Kitsu's OVA/special subtype is too inconsistent for it
function extraBonus({ entry, parsed, inBand }) {
  if (!parsed.extraKind || !inBand) return 0;
  let bonus = 0;
  if (isExtraEntry(entry)) {
    const endings = entry.ownTitles.map(endingExtraWord).filter(Boolean);
    if (parsed.extraWord && endings.some(word => extraFamily(word) === extraFamily(parsed.extraWord))) {
      bonus += BONUS.extraWordFamily;
    }
    if (parsed.extraWord && endings.includes(parsed.extraWord)) bonus += BONUS.extraWordForm;
    if (compact(norm(entry.canonicalTitle)) === compact(parsed.base)) bonus += BONUS.extraFranchiseName;
    bonus += BONUS.extra + ageTieBreak(entry, BONUS.perYearExtra);
  }
  if (parsed.extraNumber !== undefined && entry.titles.some(variant => variant.extraNumber !== undefined)) {
    const sameNumber = entry.titles.some(variant => +variant.extraNumber === parsed.extraNumber);
    bonus += sameNumber ? BONUS.extraNumber : -BONUS.extraNumber;
  }
  if (entry.subtype === 'movie') bonus += BONUS.movieForExtra;
  return bonus;
}

// franchise-wide numbering ("Pokemon Movie 14") only counts against an entry when it is its best title match
function movieBonus({ candidate, entry, parsed, context }) {
  if (parsed.movie === undefined) return 0;
  const index = entry.movie ?? entry.derivedMovie;
  const numberedByBestTitle = candidate.bestTitleMovie !== undefined || candidate.score < context.top - BAND.reach;
  let bonus = entry.subtype === 'movie' ? BONUS.movieSubtype : 0;
  if (index === parsed.movie) bonus += BONUS.movieNumber;
  else if (index !== undefined && numberedByBestTitle) bonus -= BONUS.movieNumber;
  return bonus;
}

function yearBonus({ candidate, entry, parsed, context }) {
  if (!parsed.year || !(entry.year || entry.titleYears.length) || candidate.score < context.top - BAND.year) return 0;
  if (entry.year === parsed.year || entry.titleYears.includes(parsed.year)) {
    return parsed.season ? BONUS.yearWithSeason : (context.yearAround ? BONUS.yearConfirmed : BONUS.year);
  }
  return Math.abs(entry.year - parsed.year) <= 1 ? (parsed.season ? BONUS.yearNearWithSeason : BONUS.yearNear) : 0;
}

// a plain title: the first season, TV over other types, the earlier entry among equals
function plainQueryBonus({ candidate, entry, parsed, context, inBand }) {
  if (!inBand || !context.tied || parsed.season || parsed.wantsExtra) return 0;
  let bonus = entry.imdbSeason === 1 && entry.franchise === context.leader.franchise ? BONUS.firstSeason : 0;
  // an OVA series counts when it came first ("FLCL" before "FLCL Progressive"), not a one-off or a later side story
  const earlierSibling = entry.season > 1 && context.scored.some(other => other !== candidate
      && other.score >= candidate.score - BAND.sibling && !(other.entry.season > 1)
      && (SERIES_SUBTYPES.includes(other.entry.subtype)
          || other.entry.subtype === 'OVA' && other.entry.episodeCount > 1
          && (other.entry.startDate || '9999') < (entry.startDate || '')));
  bonus += (earlierSibling ? BONUS.earlierSibling : 0) + BONUS.subtypeStep * subtypeRank(entry);
  if (!parsed.extraKind && candidate.matchedAsExtra) bonus += BONUS.extraWordUnasked;
  return bonus + ageTieBreak(entry, BONUS.perYear);
}

const RULES = [seasonBonus, titleFormBonus, episodeBonus, markerBonus, seasonQueryTypeBonus, extraBonus, movieBonus,
  yearBonus, plainQueryBonus, distinctiveWordBonus, coverageBonus];

// a sequel marked only by punctuation ("Hataraku Saibou!!" next to "Hataraku Saibou", "Gintama'" next to "Gintama"):
// the longest one in the query
function punctuatedSequel(near, parsed) {
  if (parsed.wantsExtra) return undefined;
  let winner, length = 0;
  near.forEach(candidate => candidate.entry.sequelKeys.forEach(key => {
    const bare = key.replace(/[!+◎']+$/, '');
    const lessPunctuated = otherKey => otherKey.length < key.length && otherKey.startsWith(bare)
        && /^[!+◎']*$/.test(otherKey.slice(bare.length));
    const sibling = near.some(other => other !== candidate
        && other.entry.mainTitles.map(punctKey).some(lessPunctuated));
    if (sibling && parsed.punctKey.includes(key) && key.length > length) {
      winner = candidate;
      length = key.length;
    }
  }));
  return winner;
}

function toTitleVariant(title, subtype) {
  const movie = titleMovieIndexOf(title, subtype);
  const stripped = titleBaseOf(title, subtype);
  const base = stripped === title ? title : stripped; // one string when nothing was stripped
  const movieBase = withoutMovieWords(base);
  return {
    title, // normalised
    base, // what queries are matched against
    movieBase: movieBase === base ? undefined : movieBase, // ... by a movie query, when it differs
    titleYear: titleYearOf(title),
    movie: movie === undefined ? undefined : +movie,
    season: movie === undefined ? seasonOf(title) : undefined,
    markers: markersOf(title),
    extraNumber: extraNumberOf(title),
  };
}

function toEntry(record, mapping) {
  const { subtype } = record;
  const mainTitles = [record.canonicalTitle, ...Object.values(record.titles || {})].filter(Boolean);
  const own = [...new Set([...mainTitles, ...(record.abbreviatedTitles || [])].map(norm).filter(Boolean))];
  const mainSet = new Set(mainTitles.map(norm));
  const titles = own.map(title => ({ ...toTitleVariant(title, subtype), abbreviated: !mainSet.has(title) }));
  const ownSet = new Set(own);
  // synonyms add title words only, never season / movie / extra numbers
  [...new Set((record.synonyms || []).map(norm).filter(Boolean))]
      .filter(title => !ownSet.has(title) && !NUMBERED_SYNONYM.test(title))
      .forEach(title => titles.push({
        ...toTitleVariant(title, subtype),
        fromSynonym: true,
        strippedSeason: seasonOf(title),
        season: undefined,
        movie: undefined,
        extraNumber: undefined,
        markers: NO_MARKERS,
      }));
  // the imdb mapping's English title, for TV/ONA only (it names every OVA and sequel after the franchise)
  if (mapping?.title && mapping.imdb_id && SERIES_SUBTYPES.includes(subtype)) {
    titles.push({ ...toTitleVariant(norm(mapping.title), subtype), fromMapping: true });
  }
  const notSynonyms = titles.filter(variant => !variant.fromSynonym);
  const kitsuTitles = notSynonyms.filter(variant => !variant.fromMapping);
  const titleSeason = kitsuTitles.map(variant => variant.season).find(Boolean);
  return {
    id: +record.id,
    canonicalTitle: record.canonicalTitle || '',
    mainTitles,
    subtype,
    startDate: record.startDate,
    year: record.startDate && +record.startDate.slice(0, 4),
    episodeCount: record.episodeCount,
    titles,
    ownTitles: notSynonyms.map(variant => variant.title),
    sequelKeys: mainTitles.map(title => punctKey(title.replace(/\([^)]*\)/g, ' ')))
        .filter(key => /[!+◎']$/.test(key) && key.length > 4),
    // an extra word ending a main title marks an extra ("... Lungmen Special"); inside one it is a name ("Fate/Extra")
    hasExtraWord: [record.canonicalTitle, record.titles?.en, record.titles?.en_jp].filter(Boolean)
        .some(title => hasEndingExtraWord(norm(title))),
    titleSeason,
    partNumber: notSynonyms.map(variant => +(variant.title.match(/\b(?:part|cou?r) (\d)\b/) || [])[1]).find(Boolean),
    season: titleSeason ?? ((mapping?.fromSeason && mapping.fromEpisode === 1) ? mapping.fromSeason : undefined),
    titleYears: notSynonyms.map(variant => variant.titleYear).filter(Boolean),
    movie: titles.map(variant => variant.movie).find(index => index !== undefined),
    markers: [...new Set(kitsuTitles.flatMap(variant => variant.markers))],
    franchise: mapping?.imdb_id,
    imdbSeason: mapping?.fromSeason,
    imdbFromEpisode: mapping?.fromEpisode, // where it starts in that season: a later cour numbered on (Part 2 from 12)
  };
}

const pushTo = (map, key, value) => (map.get(key) || map.set(key, []).get(key)).push(value);
const sortByScore = candidates => candidates.sort((a, b) => b.score - a.score);
const baseFor = (variant, parsed) => (parsed.bareMovie || parsed.theater) && variant.movieBase !== undefined
    ? variant.movieBase : variant.base;
const subtypeRank = entry => SUBTYPE_RANK[entry.subtype] ?? SUBTYPE_RANK.movie;
const ageTieBreak = (entry, perYear) => entry.year ? (entry.year - TIE_BREAK_YEAR) * perYear : 0;
const isExtraEntry = entry => EXTRA_SUBTYPES.includes(entry.subtype) || entry.hasExtraWord;
const trigramsOf = key => {
  const grams = new Set();
  for (let i = 0; i + 3 <= key.length; i++) grams.add(key.slice(i, i + 3));
  return grams;
};
const seasonOrder = entry => [entry.titleSeason ?? entry.imdbSeason ?? 1, entry.startDate || '9999'];
const earliestOf = list => list.slice().sort((a, b) => seasonOrder(a)[0] - seasonOrder(b)[0]
    || seasonOrder(a)[1].localeCompare(seasonOrder(b)[1]))[0];

// movies of a franchise without numbers in their titles, numbered by release ("pokemon movie 20")
function numberUnnumberedMovies(entries) {
  const groups = new Map();
  entries.filter(entry => entry.subtype === 'movie')
      .forEach(entry => pushTo(groups, moviePrefixOf(entry.canonicalTitle), entry));
  groups.forEach(list => {
    if (list.some(entry => entry.movie !== undefined)) return; // explicit numbering wins where a franchise has it
    list.filter(entry => entry.startDate).sort((a, b) => a.startDate.localeCompare(b.startDate))
        .forEach((entry, index) => { entry.derivedMovie = index + 1; });
  });
}

// TV/ONA seasons sharing a canonical title apart from season markers (imdb franchises are too broad: all of Pokemon)
function groupSeasons(entries) {
  const groups = new Map();
  entries.filter(entry => SERIES_SUBTYPES.includes(entry.subtype) && entry.canonicalTitle).forEach(entry => {
    entry.groupKey = seasonlessTitle(entry.canonicalTitle);
    pushTo(groups, entry.groupKey, entry);
  });
  return groups;
}

// absolute episode numbers: a franchise's TV entries (by imdb mapping) laid end to end by episode count
function layOutEpisodes(entries) {
  const franchises = new Map();
  entries.filter(entry => entry.franchise && SERIES_SUBTYPES.includes(entry.subtype) && entry.startDate)
      .forEach(entry => pushTo(franchises, entry.franchise, entry));
  franchises.forEach(list => {
    list.sort((a, b) => (a.imdbSeason ?? 99) - (b.imdbSeason ?? 99) || a.startDate.localeCompare(b.startDate));
    let from = 1;
    for (const entry of list) {
      entry.absFrom = from;
      if (!entry.episodeCount) {
        entry.absTo = Infinity;
        break;
      }
      entry.absTo = from + entry.episodeCount - 1;
      from = entry.absTo + 1;
    }
  });
}

module.exports = { createTitleIndex, MIN_CONFIDENCE, SEARCHED_SUBTYPES };
