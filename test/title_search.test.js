const test = require('node:test');
const assert = require('node:assert');

const { createTitleIndex } = require('../lib/title_search');
const { parseQuery } = require('../lib/title_query');
const { searchTitles } = require('../lib/title_index');

const record = (id, canonicalTitle, subtype, startDate, extra = {}) => ({
  id, canonicalTitle, titles: { en_jp: canonicalTitle }, abbreviatedTitles: [], subtype, status: 'finished', startDate,
  episodeCount: 12, synonyms: [], ...extra,
});

const fixture = createTitleIndex([
  record(1, 'Durarara!!', 'TV', '2010-01-08'),
  record(2, 'Durarara!!x2 Shou', 'TV', '2015-01-10', { synonyms: ['Durarara!! 2nd Season'] }),
  record(3, 'Hyouka', 'TV', '2012-04-23'),
  record(4, 'Hyouka: Motsubeki Mono wa', 'OVA', '2012-07-08', { synonyms: ['Hyouka OVA'] }),
  record(5, 'Honzuki no Gekokujou', 'TV', '2019-10-03'),
  record(6, 'Honzuki no Gekokujou 2nd Season', 'TV', '2020-04-05', { synonyms: ['Honzuki no Gekokujou 2'] }),
  record(7, 'Kizumonogatari I: Tekketsu-hen', 'movie', '2016-01-08'),
  record(8, 'Kizumonogatari II: Nekketsu-hen', 'movie', '2016-08-19', { synonyms: ['Kizumonogatari Part 2'] }),
  record(9, 'Kizumonogatari III: Reiketsu-hen', 'movie', '2017-01-06', { synonyms: ['Kizumonogatari Part 3'] }),
  record(10, 'Shiki', 'TV', '2010-07-08'),
  record(11, 'Shiki Specials', 'special', '2011-01-26'),
  record(12, 'Some Music Video', 'music', '2020-01-01'),
]);
const topId = query => fixture.search(query)[0]?.id;

test('parseQuery reads seasons, years, movie numbers and extras', () => {
  assert.strictEqual(parseQuery('overlord S3').season, 3);
  assert.strictEqual(parseQuery('youkoso jitsuryoku shijou shugi no kyoushitsu e 4th 2026 S2').season, 4);
  assert.strictEqual(parseQuery('lupin the 3rd S4').season, 4);
  assert.strictEqual(parseQuery('nana 2006-2007').year, 2006);
  assert.strictEqual(parseQuery('detective conan movie 03').movie, 3);
  assert.strictEqual(parseQuery('isuca - ova 1').extraKind, 'OVA');
  assert.strictEqual(parseQuery('isuca - ova 1').season, undefined); // the extra's number is not a season
  assert.strictEqual(parseQuery('hyouka 13-14+ova').isPack, true);
  assert.strictEqual(parseQuery('blood blockade battlefront (kekkai sensen) v2').season, undefined); // a revision
  assert.strictEqual(parseQuery('dr stone - new world 16 v2').episode, 16);
  assert.strictEqual(parseQuery('kuroko\'s basketball 2 (kuroko no baske)').season, 2);
  assert.strictEqual(parseQuery('naruto classico').base, 'naruto');
});

test('a plain title gets the first season, a sequel its own name', () => {
  assert.strictEqual(topId('durarara!!'), 1);
  assert.strictEqual(topId('durarara!!x2 shou'), 2);
  assert.strictEqual(topId('honzuki no gekokujou'), 5); // not the 2nd season matching "Honzuki no Gekokujou 2" stripped
  assert.strictEqual(topId('honzuki no gekokujou S2'), 6);
});

test('a pack of the series with its extras gets the series, a named extra the extra', () => {
  assert.strictEqual(topId('hyouka 13-14+ova'), 3);
  assert.strictEqual(topId('hyouka ova'), 4);
  assert.strictEqual(topId('shiki + specials'), 10);
  assert.strictEqual(topId('shiki specials'), 11);
});

test('numbered parts follow the numeral, the plain name the first part', () => {
  assert.strictEqual(topId('kizumonogatari'), 7);
  assert.strictEqual(topId('kizumonogatari ii'), 8);
  assert.strictEqual(topId('kizumonogatari iii - reiketsu'), 9);
});

test('music entries are never searched', () => {
  assert.notStrictEqual(topId('some music video'), 12);
});

// The committed titles file (static/data/search_titles.jsonl). Long-established titles only, so the weekly data update
// doesn't move them; a failure here blocks a deploy that would regress the search.
test('the title index finds the expected Kitsu entries', () => {
  const expected = {
    'Mob Psycho 100': 11578,
    'Tokyo Ghoul Root A': 9135,
    'Cardfight!! Vanguard Link Joker': 7283,
    'Land of the Lustrous': 13521,
    'Black Jack': 2004,
    'Hunter x Hunter': 115,
    'Re.Zero kara Hajimeru Isekai Seikatsu 2nd Season': 42198,
    'Attack on Titan Season 3 Part 2': 41982,
    'Houkago no Pleiades': 7713,
    'Overlord S3': 41174,
    'Detective Conan Movie 2': 697,
    'Gekijouban Sword Art Online Ordinal Scale': 11423,
    'LUPIN THE 3rd PART4': 9958,
    'Douluo Dalu - Soul Land Season 2': 42037,
    'When They Cry - Higurashi': 831,
    'My Hero Academia: Heroes Rising-Movie (2019)': 42205,
    'Hypnosis Mic -Division Rap Battle- Rhyme Anima': 42826,
    'Big Order': 11455,
    'Himitsukessha Taka no Tsume - Golden Spell': 43608,
    'Hataraku Saibou S2': 42202,
    'Hataraku Saibou': 14212,
    'Tensei Shitara Slime Datta Ken 2nd Season Part 2': 43361,
    'Tensei Shitara Slime Datta Ken 2nd Season': 42196,
    'Dr. Stone - Ryuusui': 45615,
    'Xian Wang de Richang Shenghuo 2': 43795,
    'kizumonogatari': 5649, // later parts matching only without their number give way to the first
    'flcl': 202, // the OVA series is the first season of the TV sequels ("FLCL 2" synonyms)
    'puchimas petit petit idolmaster': 8405, // not the earlier one-off OVA
    'rerise 2nd season': 43144, // the title names the season; Re:RISE 1 is only mapped to TVDB season 2
    'working\'!!': 6220, // the apostrophe marks the sequel, like "!!"
    'gintama\'': 5971,
    'baka to test to shoukanjuu matsuri': 5745, // its own title, not the special abbreviated "Matsuri Special"
    'ms gundam igloo 1 - part 1 (thoyw)': 1727, // "the hidden one year war" as initials
    'douluo dalu ii soul land 2': 42037, // "ii" is covered by the entry being season 2
    'ranma 1 2 2024': 48981, // the 2 of "1/2" isn't a season
    'yu gi oh duel monster 2000': 442, // "monster" is held by "Duel Monsters" too
    'higurashi when they cry': 831, // the 2006 series' synonym whole, over "Higurashi: When They Cry - Gou"
    'feng ling yu xiu 2': 45638, // its synonym is the whole query, the special's "... 2: Special" isn't
    'a record of a mortal s journey to immortality remastered': 47063, // "Remastered Edition": the marker takes "edition" too
    'Cardfight!! Vanguard 15th Anniversary Remastered': 5781, // an anniversary before a remaster is release noise
    'Akira 25th Anniversary Edition 1988': 29, // ... and before an edition
    'Ojamajo Carnival 25th Anniversary Version': 48678, // not before a version: Kitsu's own titles end in it
    'diamond no ace actii  second season ': 48878, // the whole query is its title glued, not "Diamond no Ace: Act II"
    // both halves name Steel Ball Run (imdb season 6), not the OVA holding both names
    'jojo`s bizarre adventure (jojo no kimyou na bouken) s6': 49847,
    'keroro gunsou☆': 50992, // a trailing ☆ marks the sequel, as "!!" does; plain "keroro gunsou" is the 2004 show
    'keroro gunsou': 475,
    'suikoden': 50988, // the series, not the 1993 OVA whose synonym "Suikoden" is the only exact match
    'fighting spirit 2000 S3': 7855, // Rising, by its imdb's English title "Fighting Spirit"; 2000 is the premiere year
    'the prince of tennis ii S5': 51007, // a release group's franchise-wide season: by its alias
    'bleach S17': 43078, // Thousand-Year Blood War, by its alias
    'bleach S1': 244, // still the 2004 series, though the alias gives TYBW an exact "Bleach" title
    'bleach thousand-year blood war (cour 2) 2023 S17': 46903, // the cour beside S17 is the season's part 2
    're zero shin henshuu ban': 42903, // the re-edit, below every entry titled just "Re:Zero"
    're zero director\'s cut': 42903,
    'shingeki no kyojin final S60': 42422, // "S60" is a season, not a trailing episode 60
    'shingeki no kyojin junior high': 11315, // romaji franchise, English subtitle: by its alias
    'white album S3': 7697, // White Album 2, the franchise's 3rd series to release groups: by its alias
    'mahou no shimai rurutto riryi': 49677, // a release group's transliteration of Lulutto Lilly: by its alias
    'mahou no shimai rurutto riryi part 2': 50996, // its Part 2: by its alias
    'battle through the heavens nf': 50105, // FSP's "NF" numbers the airing Nian Fan (4) from NF1 ep 1: by its alias
    'battle through the heavens': 14116, // the bare name keeps the 2017 series
    'white album S2': 4455,
    'sengoku basara S3': 8272, // no entry is season 3: the franchise's 3rd series, Judge End (not the 2nd, "Two")
    'horimiya S2': 47234, // its own imdb series: the 2nd series of the name
    'horimiya the missing pieces S2': 47234,
    'cardcaptor sakura S2': 12237,
    'hunter x hunter 2011 S4': 6448, // 3 imdb seasons: no season 4, and the OVA mapped at the 1999 series' S4 is no season
    'dragon ball z S3': 720, // Kai (2014) is a remake, not the next series
    'samurai jack S2': 7939,
    'baki S3': 42206, // imdb season 2 after a 26-episode season: release groups count two cours as two seasons
    'baki S2': 42206,
    'baki S1': 13140,
    'baki': 13140,
    'hanma baki S1': 43599,
    'uchuu senkan yamato space battleship yamato 2202': 12017, // no one title holds the whole name: half of it is enough
    'one piece 1001': 12, // "1001 Nights" holds the number but none of the name
    'inuyasha S2': 224, // its Kanketsu-hen is season 8 of the same imdb series: S2 lies inside the first entry
    'kyo kara maoh S2': 226,
    'black clover S3': 13209,
    'fairy tail S3': 4676, // an earlier series of 175 episodes spans several seasons: S3 lies inside it
    'dragon ball super S3': 10879,
    'dragon ball S5': 199, // five series share the name: their count says nothing about the season
    'steven universe S6': 42605, // "Season 5" names its own season: no shift to 6
    'uchuu senkan yamato 2205': 44694, // a 4-digit number that is a title's word is the name, not an episode
    'ghost in the shell-sac S2': 715, // Stand Alone Complex's 2nd GIG, not SAC_2045 2nd Season: by its alias
    'prince of tennis - another story': 4264, // the first part, not the "OVA Another Story II" synonym's
    'gate jieitai kanochi nite': 10085, // not Part 2, which has the bare name as an own title too
    'gate jieitai kanochi nite S2': 11376, // the alias restoring 10085's dropped synonym leaves the season to Part 2
    'oye tonbo!': 47427, // not the 2nd Season's Spanish title "¡Oye Tonbo!"
    'oye tonbo! S2': 49013,
    'mini link click': 45503, // the 2021 shorts, not the 2026 ones Kitsu titles so: by its alias
    'mini link click S2': 50691,
    'steven universe S1': 10735, // not Future, S1 of its own imdb but titled "Season 6"
    'kizumonogatari i - tekketsu': 5649,
    'kizumonogatari ii - nekketsu': 11406,
    'kizumonogatari iii - reiketsu': 11407,
    'bakemonogatari': 3919,
    'nisemonogatari': 6546,
    'nekomonogatari (black)': 7294,
    'nekomonogatari (white)': 7573,
    'kabukimonogatari': 7573,
    'otorimonogatari': 7573,
    'onimonogatari': 7573,
    'koimonogatari': 7573,
    'hanamonogatari': 8032,
    'tsukimonogatari': 9961,
    'owarimonogatari': 11182,
    'koyomimonogatari': 11613,
    'owarimonogatari S2': 13268,
    'zoku owarimonogatari': 42279,
    // scraper-style queries
    'durarara!!': 4696,
    'kuroko no basket': 6595,
    'hyouka 13-14+ova': 6686,
    'shiki specials': 6027,
    'dragon ball z movie 01': 794,
    'pocket monsters bw movie 1 - victini and the light hero - reshiram': 6327,
    'doupo cangqiong  dou po cang qiong  fights break sphere  battle through the heavens': 14116,
    'ojisan wa kawaii mono ga osuki uncle\'s obsession with cute things 2026': 51021,
    'kuroko\'s basketball 2 (kuroko no baske)': 7545,
    'Shingeki no Kyojin OVAs (Attack on Titan)': 7694, // the extra before the bracket goes to its title too
    'Shingeki no Kyojin (Attack on Titan)': 7442,
    'Koufu Collector': 50994, // an uploader's typo for Kyoufu: one letter off, the fallback below the cut-off
    'Hosizora Kiseki': 1097, // the misspelt word scores low: the query spelt as the title is ranked anew
    'blue excorcist': 5940, // ... a word no title holds; not a real word one letter off another ("song", "kiss")
    'Shingeki no Kyojn': 7442,
    'sword art online S2': 8174, // not "Alicization - War of Underworld 2nd Season", matched by the franchise name
    'jormungand S2': 6940, // filed by TVDB under season 1
    'jojo\'s bizarre adventure 2018 S5': 41410,
    'blood blockade battlefront (kekkai sensen) v2': 8646,
    'dance in the vampire bund v1 5': 4697,
    'keishicho tokumu-bu tokushu kyoaku-han taisaku-shitsu dai-nana-ka tokunana (special 7)': 42272,
    'dr stone - new world 16 v2': 47246, // Part 2 numbered on from Part 1's 11 episodes
    'shingeki no kyojin 21': 7442, // the first season holds episode 21
    'naruto clássico': 11,
    'mazinger z la serie completa 92 capitulos': 2041,
    'slam dunk ova 3': 2267,
    'naruto clássico S2': 11, // not "Naruto Movie 2"
    'date a live iii - 01v2-12v2': 14040,
    'stand by me doraemon 2': 44424, // a bare number can be a movie's
    // a franchise label the title lacks ("mobile suit") doesn't decide; the words telling the entries apart do
    'mobile suit sd gundam force': 2168,
    'mobile suit sd gundam world heroes': 46405,
    'mobile suit sd gundam sangokuden brave battle warriors': 5277,
    'mobile suit sd gundam gaiden': 1748,
    'mobile suit gundam build fighters try': 8637,
    'mobile suit gundam build fighters battlogue': 13711,
    'mobile suit gundam build divers': 40565,
    'mobile suit gundam build divers re rise': 42585,
    'mobile suit gundam build metaverse': 48065,
    'mobile suit gundam breaker battlogue': 44744,
    'mobile suit gundam igloo': 1726,
    'mobile suit gundam evolve': 2911,
    'mobile suit gundam': 59,
    'mobile suit sd gundam mk i': 2084, // the only exact title: no tie-break against it
    'mobile suit sd gundam mk ii': 2085,
    '86 - eighty six (part 2)': 44398, // "86 Part 2" is the second part, not season 86
    '86 - eighty six': 43066,
    '86': 43066, // a name without letters still finds the entry titled so
    '86 S2': 44398,
    '22/7': 42456,
    '663114': 7064,
    '009-1': 1423,
    'shingeki no kyojin 3 part 2': 41982, // the season's part, from the entry's own title
    'shingeki no kyojin season 3 part 1': 13569,
    'mahou tsukai no yome season 2 cour 2': 47649, // "cour" (folded to "cor" with the long vowels) is a part too
    'dr. stone - new world cour 2': 47246,
    'bleach thousand year blood war cour 3': 48015,
    'spy x family cour 2': 45619,
  };
  const wrong = Object.entries(expected)
      .map(([query, id]) => [query, id, searchTitles(query)[0]?.id])
      .filter(([, id, found]) => found !== id);
  assert.deepStrictEqual(wrong, []);
});

test('the title index returns nothing for titles that aren\'t anime on Kitsu', () => {
  const found = ['hello carbot 2018 S6', 'raba in maseu larva in mars 2024', 'jyulagi cops 4 jurassic cops 2026',
    'american dad S19', 'hawaii five 0 2010 S7', 'lingwu continent', 'a good day to ascend', 'mk iv']
      .map(query => [query, searchTitles(query)[0]?.id])
      .filter(([, id]) => id !== undefined);
  assert.deepStrictEqual(found, []);
});

test('a query that is only an extra word names no show', () => {
  assert.deepStrictEqual(searchTitles('OVA', 5), []);
  assert.deepStrictEqual(searchTitles('specials', 5), []);
});

test('a chance match on a word or two is below the cut-off', () => {
  // "Office Work" shares one of the two words
  assert.deepStrictEqual(searchTitles('office girls'), []);
  // the typo fallback takes one letter off, not two ("kyoto" for "kyofu")
  assert.deepStrictEqual(searchTitles('Kyoto Collector'), []);
  // a name without letters ("乱马 1/2 2024年版" with its CJK stripped) only names an entry titled so, not "2 koi"
  assert.deepStrictEqual(searchTitles('2 2024'), []);
  assert.deepStrictEqual(searchTitles('2'), []);
  assert.deepStrictEqual(searchTitles('12'), []);
});

test('every search alias names a Kitsu entry and finds it first', () => {
  const { readRecords, ALIASES_PATH } = require('../lib/title_records');
  const aliases = JSON.parse(require('fs').readFileSync(ALIASES_PATH, 'utf8'));
  const ids = new Set(readRecords().map(record => `${record.id}`));
  const wrong = Object.entries(aliases).flatMap(([id, titles]) => titles
      .filter(title => !ids.has(id) || searchTitles(title, 1)[0]?.id !== +id)
      .map(title => [id, title, searchTitles(title, 1)[0]?.id]));
  assert.deepStrictEqual(wrong, []);
});
