// Decides whether a resolved kitsu -> imdb mapping candidate is safe to merge without manual review.
// Pure: all remote data (Fribb link, Cinemeta meta, sibling episode counts) is passed in by the caller.

const SEASON_START_MONTH = { winter: 0, spring: 3, summer: 6, fall: 9 };
const DAY_MS = 24 * 60 * 60 * 1000;
const CONTINUATION_DATE_LEEWAY_DAYS = 3;
const SPECIAL_TYPES = ['OVA', 'ONA', 'special'];
const FEATURE_LENGTH_MINUTES = 45;

// MAL season like '2026/fall' -> date range covering it, padded a month early for pre-season premieres.
function seasonWindow(season) {
  const [year, name] = season.split('/');
  const startMonth = SEASON_START_MONTH[name];
  if (startMonth === undefined) {
    throw new Error(`Unknown MAL season: ${season}`);
  }
  return {
    from: new Date(Date.UTC(parseInt(year), startMonth - 1, 1)),
    to: new Date(Date.UTC(parseInt(year), startMonth + 3, 0, 23, 59, 59)),
  };
}

// candidate: { imdb_id, fromSeason } (fromSeason undefined for movies)
// linked:    the MAL <-> Kitsu link is confirmed (otherwise we may be mapping the wrong Kitsu entry)
// linkReason: why it is not, when known (e.g. a Kitsu id that points to another show)
// animeType: Kitsu subtype of the candidate
// kitsuYear: Kitsu release year of the candidate, if known
// kitsuEpisodes: Kitsu episode count of the candidate, if known
// kitsuFirstAired: air date of the candidate's Kitsu episode 1, if known
// cinemeta:  Cinemeta meta for imdb_id, or undefined when unknown
// siblings:  existing imdb_mapping rows for imdb_id, each with the Kitsu `episodeCount`
// window:    seasonWindow() of the imported MAL season
function verifyCandidate({
  candidate, linked, linkReason, animeType, kitsuYear, kitsuEpisodes, kitsuFirstAired, cinemeta, siblings, window
}) {
  if (!linked) {
    return fail(linkReason || 'mal-kitsu link not confirmed by Fribb');
  }
  const kind = verifyKind(candidate, animeType, kitsuEpisodes, cinemeta);
  if (!kind.ok) {
    return kind;
  }
  if (kind.isMovie) {
    return verifyMovie(candidate, kitsuYear, cinemeta, window);
  }
  if (candidate.fromEpisode > 1) {
    return verifyContinuation(candidate, cinemeta, siblings, window, kitsuFirstAired);
  }
  return verifySeason(candidate.fromSeason, cinemeta, siblings, window, kitsuFirstAired);
}

// A title Fribb maps to an imdb id nothing else uses yet. Fribb vouches for the link and the id, and the title may
// be older than the imported season (e.g. a 2011 OVA in a 2025 listing), so there is no season window. Cinemeta
// must still agree on what the title is: a movie for a movie, a series with S1E1 for a series. Otherwise every
// Kitsu episode points at IMDb episodes that don't exist (Next Shine!: 5 Kitsu episodes, an IMDb movie).
function verifyFreshTitle({ candidate, animeType, kitsuYear, kitsuEpisodes, cinemeta }) {
  const kind = verifyKind(candidate, animeType, kitsuEpisodes, cinemeta);
  if (!kind.ok) {
    return kind;
  }
  if (kind.isMovie) {
    if (candidate.fromSeason != null) {
      return fail('movie mapped onto a season');
    }
    return verifyKitsuYear(cinemeta, kitsuYear);
  }
  const season = candidate.fromSeason ?? 1;
  const first = (cinemeta.videos || []).find(video => video.season === season && video.episode === 1);
  return first ? ok() : fail(`imdb has no S${season}E1`);
}

// The imdb id is a real title Cinemeta knows, it is animation, and movie vs series agrees with Kitsu.
function verifyKind(candidate, animeType, kitsuEpisodes, cinemeta) {
  if (!/^tt\d+$/.test(candidate.imdb_id || '')) {
    return fail('not a tt imdb id');
  }
  if (!cinemeta?.name) {
    return fail('imdb id unknown to cinemeta');
  }
  // Search-derived ids can hit a same-named live-action title (LOCA! -> "Vieja Loca", a horror film).
  if (!(cinemeta.genres || cinemeta.genre || []).includes('Animation')) {
    return fail('imdb title is not tagged Animation');
  }
  const isMovie = animeType === 'movie' || (cinemeta.type === 'movie' && isSingleEpisodeExtra(animeType, kitsuEpisodes));
  if (isMovie !== (cinemeta.type === 'movie')) {
    return fail(`type mismatch kitsu=${animeType} cinemeta=${cinemeta.type}`);
  }
  return { ok: true, isMovie };
}

// IMDb lists one-off ONAs/OVAs/specials (e.g. Chinese donghua movies released on streaming) as movies.
function isSingleEpisodeExtra(animeType, kitsuEpisodes) {
  return ['ONA', 'OVA', 'special'].includes(animeType) && kitsuEpisodes === 1;
}

function verifyMovie(candidate, kitsuYear, cinemeta, window) {
  if (candidate.fromSeason != null) {
    return fail('movie mapped onto a season');
  }
  const year = parseInt(cinemeta.year || cinemeta.releaseInfo);
  const seasonYear = window.to.getUTCFullYear();
  if (!(Math.abs(year - seasonYear) <= 1)) {
    return fail(`movie year ${year} far from season ${seasonYear}`);
  }
  return verifyKitsuYear(cinemeta, kitsuYear);
}

// A seasonal listing can resolve to an older Kitsu movie of the same name (1983 Doraemon film -> 2025 one).
function verifyKitsuYear(cinemeta, kitsuYear) {
  const year = parseInt(cinemeta.year || cinemeta.releaseInfo);
  const kitsuReleaseYear = parseInt(kitsuYear);
  if (kitsuReleaseYear && Math.abs(year - kitsuReleaseYear) > 1) {
    return fail(`movie year ${year} far from kitsu year ${kitsuReleaseYear}`);
  }
  return ok();
}

// The season must exist on IMDb from episode 1, and episode 1 of the new Kitsu entry must be S{N}E1 rather than an
// offset into a season IMDb keeps running (e.g. "Part 2" cours). Proven either by cleanly mapped earlier seasons or
// by the season before it being used up.
function verifySeason(season, cinemeta, siblings, window, kitsuFirstAired) {
  if (!(season >= 1)) {
    return fail('no season number');
  }
  const first = (cinemeta.videos || []).find(video => video.season === season && video.episode === 1);
  if (!first) {
    return fail(`imdb has no S${season}E1`);
  }
  const clean = verifyCleanSeasons(season, first, cinemeta, siblings, window);
  if (clean.ok || !followsUsedUpSeason(season, first, cinemeta, siblings, kitsuFirstAired)) {
    return clean;
  }
  return ok();
}

// The season premieres inside the MAL season, and every earlier season is covered by exactly one Kitsu entry with
// IMDb's episode count.
function verifyCleanSeasons(season, first, cinemeta, siblings, window) {
  const aired = new Date(first.released);
  if (!(aired >= window.from && aired <= window.to)) {
    return fail(`S${season}E1 aired ${first.released?.slice(0, 10)} outside season`);
  }
  const regular = siblings.filter(sibling => sibling.fromSeason > 0);
  const seasons = [...new Set(regular.map(sibling => sibling.fromSeason))].sort((a, b) => a - b);
  if (seasons.length !== season - 1 || seasons.some((s, i) => s !== i + 1)) {
    return fail(`existing seasons [${seasons}] not contiguous up to S${season - 1}`);
  }
  for (const s of seasons) {
    const rows = regular.filter(sibling => sibling.fromSeason === s);
    if (rows.length !== 1 || rows[0].fromEpisode !== 1) {
      return fail(`S${s} split across kitsu entries`);
    }
    const imdbCount = (cinemeta.videos || []).filter(video => video.season === s).length;
    if (rows[0].episodeCount !== imdbCount) {
      return fail(`S${s} kitsu eps ${rows[0].episodeCount} != imdb eps ${imdbCount}`);
    }
  }
  return ok();
}

// The Kitsu entries of the last mapped season cover exactly IMDb's episodes of it, and S{N}E1 aired with Kitsu's
// episode 1. Holds where the clean check can't: earlier seasons split across cours (Re:Zero, Slime) or a season
// that premiered before the imported MAL season. Replayed against reviewed 2026 rows: 258 of 258 agree.
function followsUsedUpSeason(season, first, cinemeta, siblings, kitsuFirstAired) {
  const start = continuationStart(siblings);
  if (start?.fromSeason !== season - 1 || !kitsuFirstAired) {
    return false;
  }
  const previous = (cinemeta.videos || []).filter(video => video.season === start.fromSeason);
  return previous.length === start.fromEpisode - 1
      && previous.every(video => video.episode < start.fromEpisode)
      && airedNear(first, kitsuFirstAired);
}

// A continuation cour (e.g. "Part 2") that IMDb keeps in the running season: it starts right after the episodes
// the existing Kitsu entries cover in the last mapped season. IMDb must list that episode, airing this MAL season
// on (about) the same day as Kitsu's episode 1, and the episode before it must have aired clearly earlier, which
// proves it is the first episode of the new cour. The leeway absorbs broadcast vs streaming / timezone differences
// while staying under half the 7 day weekly gap; whole-part streaming drops (same date for every episode) can't
// pin down the boundary and are left for review.
function verifyContinuation(candidate, cinemeta, siblings, window, kitsuFirstAired) {
  const start = continuationStart(siblings);
  if (!start) {
    return fail('no fully counted season to continue');
  }
  if (candidate.fromSeason !== start.fromSeason || candidate.fromEpisode !== start.fromEpisode) {
    return fail(`continuation should start at S${start.fromSeason}E${start.fromEpisode}`);
  }
  const target = (cinemeta.videos || [])
      .find(video => video.season === start.fromSeason && video.episode === start.fromEpisode);
  if (!target?.released) {
    return fail(`imdb has no dated S${start.fromSeason}E${start.fromEpisode}`);
  }
  const aired = new Date(target.released);
  if (!(aired >= window.from && aired <= window.to)) {
    return fail(`S${start.fromSeason}E${start.fromEpisode} aired ${target.released.slice(0, 10)} outside season`);
  }
  const kitsuAired = kitsuFirstAired && new Date(kitsuFirstAired);
  if (!kitsuAired || !airedNear(target, kitsuAired)) {
    return fail(`S${start.fromSeason}E${start.fromEpisode} aired ${target.released.slice(0, 10)}, kitsu ep 1 ${kitsuAired ? kitsuAired.toISOString().slice(0, 10) : 'unknown'}`);
  }
  const previous = (cinemeta.videos || [])
      .find(video => video.season === start.fromSeason && video.episode === start.fromEpisode - 1);
  if (!previous?.released || !startsRun(cinemeta.videos, target, kitsuAired)) {
    return fail(`no clear cour boundary before S${start.fromSeason}E${start.fromEpisode}`);
  }
  return ok();
}

// An OVA/ONA/special of an already-mapped franchise that IMDb lists among the franchise's specials (season 0).
// Every Kitsu episode must line up with consecutive S0 episodes aired on (about) the same day, that run must be the
// only one that fits, and none of its episodes may already belong to another Kitsu entry. A single feature-length
// special is left for review: IMDb often also lists those as standalone movies (Violet Evergarden: Recollections).
// kitsuAirDates: air date of each Kitsu episode, in order
// kitsuEpisodeLength: Kitsu episode length in minutes, if known
// siblings: existing imdb_mapping rows for the franchise imdb, S0 rows with the Kitsu `episodeCount`
// kitsuTitles: Kitsu canonical title and aliases, used to spot an S0 episode titled after the special
// siblings carry `episodeCount` for every row (regular seasons too, to know which regular episodes are mapped)
function verifySpecial({ animeType, kitsuAirDates, kitsuEpisodeLength, kitsuTitles = [], cinemeta, siblings, window }) {
  if (!SPECIAL_TYPES.includes(animeType)) {
    return fail(`kitsu type ${animeType} is not a special`);
  }
  if (!kitsuAirDates?.length || kitsuAirDates.some(date => !date || isNaN(new Date(date)))) {
    return fail('kitsu episodes undated');
  }
  if (kitsuAirDates.length === 1 && kitsuEpisodeLength >= FEATURE_LENGTH_MINUTES) {
    return fail(`single ${kitsuEpisodeLength} min special may be a standalone movie`);
  }
  if (!cinemeta?.name) {
    return fail('franchise imdb unknown to cinemeta');
  }
  if (cinemeta.type !== 'series') {
    return fail(`franchise imdb is a ${cinemeta.type}`);
  }
  const used = new Set();
  for (const row of siblings.filter(sibling => sibling.fromSeason === 0)) {
    if (!Number.isInteger(row.episodeCount) || row.episodeCount < 1) {
      return fail(`S0 span of kitsu ${row.kitsu_id} unknown`);
    }
    for (let i = 0; i < row.episodeCount; i++) {
      used.add(row.fromEpisode + i);
    }
  }
  const specials = (cinemeta.videos || []).filter(video => video.season === 0);
  if (!specials.length) {
    return fail('imdb has no S0');
  }
  const byEpisode = new Map(specials.map(video => [video.episode, video]));
  const starts = specials.map(video => video.episode).filter(start => kitsuAirDates.every((date, i) =>
      airedNear(byEpisode.get(start + i), date) && !used.has(start + i)));
  if (starts.length !== 1) {
    return fail(starts.length
        ? `${starts.length} S0 runs match the kitsu air dates (${starts.map(start => `S0E${start}`).join(', ')})`
        : 'no unused S0 episodes match the kitsu air dates');
  }
  // Any other free S0 episode aired within the same dates could be one of this entry's episodes just as well
  // (Elf-san: kitsu ep 2 on 10-19 fits IMDb E14 and E15), so the run is only proven when it is the only candidate.
  const kitsuTimes = kitsuAirDates.map(date => new Date(date).getTime());
  const spanFrom = Math.min(...kitsuTimes) - CONTINUATION_DATE_LEEWAY_DAYS * DAY_MS;
  const spanTo = Math.max(...kitsuTimes) + CONTINUATION_DATE_LEEWAY_DAYS * DAY_MS;
  const run = new Set(kitsuAirDates.map((_, i) => starts[0] + i));
  const rivals = specials.filter(video => !run.has(video.episode) && !used.has(video.episode) && video.released
      && new Date(video.released) >= spanFrom && new Date(video.released) <= spanTo);
  if (rivals.length) {
    return fail(`S0E${starts[0]} fits, but ${rivals.map(video => `S0E${video.episode}`).join(', ')} aired on the same dates`);
  }
  // Release dates can coincide by chance (a Blu-ray extra dated like another short): when another free S0 episode is
  // titled after this special and the matched run is not, IMDb most likely lists it there (Guilty Crown: Lost
  // Christmas is S0E2, not the 4-panel short S0E9 aired the same week).
  const subtitles = specialSubtitles(kitsuTitles);
  const titled = video => subtitles.some(subtitle => normalizeTitle(video.name || video.title).includes(subtitle));
  const titleRival = specials.find(video => !run.has(video.episode) && !used.has(video.episode) && titled(video));
  if (titleRival && ![...run].some(episode => titled(byEpisode.get(episode)))) {
    return fail(`S0E${starts[0]} fits the dates, but S0E${titleRival.episode} is titled "${titleRival.name || titleRival.title}"`);
  }
  // IMDb sometimes counts an OVA as an extra regular episode (Kamichu!: S1E13) instead of, or as well as, a special.
  // A regular episode that no Kitsu entry covers yet, aired on the same dates, makes the S0 placement uncertain.
  const covered = coveredRegularEpisodes(cinemeta, siblings);
  const regularRival = (cinemeta.videos || []).find(video => video.season > 0 && video.released
      && !covered.has(`${video.season}:${video.episode}`)
      && new Date(video.released) >= spanFrom && new Date(video.released) <= spanTo);
  if (regularRival) {
    return fail(`S0E${starts[0]} fits, but unmapped S${regularRival.season}E${regularRival.episode} aired on the same dates`);
  }
  const aired = new Date(byEpisode.get(starts[0]).released);
  if (!(aired >= window.from && aired <= window.to)) {
    return { ...fail(`S0E${starts[0]} aired ${aired.toISOString().slice(0, 10)} outside season`), fromEpisode: starts[0] };
  }
  return { ok: true, fromEpisode: starts[0] };
}

// An already mapped row (sequel, continuation cour or special) re-checked against fresh Cinemeta data: IMDb keeps
// reshaping running shows (a "Part 2" cour moved into its own season, specials renumbered), which silently shifts
// every episode of the row. It still holds when its start episode SxEy aired on (about) the day Kitsu's entry
// started. Otherwise `suggestions` are the IMDb episodes starting a run that day (specials for a season-0 row,
// regular seasons otherwise); `unknown` marks rows that can't be checked.
// row: imdb_mapping row; kitsuStart: Kitsu start date of its entry; cinemeta: Cinemeta meta of its imdb
function verifyOffset({ row, kitsuStart, cinemeta }) {
  if (!cinemeta?.name) {
    return { ...fail('imdb unknown to cinemeta'), unknown: true };
  }
  const videos = cinemeta.videos || [];
  const start = new Date(kitsuStart);
  const position = `S${row.fromSeason}E${row.fromEpisode}`;
  const target = videos.find(video => video.season === row.fromSeason && video.episode === row.fromEpisode);
  if (target && !target.released) {
    return { ...fail(`imdb ${position} has no air date`), unknown: true };
  }
  if (target && airedNear(target, start)) {
    return ok();
  }
  const suggestions = videos
      .filter(video => (video.season === 0) === (row.fromSeason === 0) && airedNear(video, start))
      .filter(video => startsRun(videos, video, start))
      .map(video => ({ fromSeason: video.season, fromEpisode: video.episode }));
  const reason = target
      ? `${position} aired ${target.released.slice(0, 10)}, kitsu started ${kitsuStart}`
      : `imdb has no ${position}`;
  return { ...fail(reason), suggestions };
}

// A season-0 row hands out Kitsu episodes to every IMDb special from its start until the next S0 row, with no cap
// (enrichImdbEpisodes), so a run longer than the Kitsu entry gives the extra specials episodes Kitsu doesn't have.
// row and siblings carry the Kitsu `episodeCount`.
function verifySpecialRun({ row, siblings, cinemeta }) {
  if (!cinemeta?.name) {
    return { ...fail('imdb unknown to cinemeta'), unknown: true };
  }
  if (!Number.isInteger(row.episodeCount) || row.episodeCount < 1) {
    return { ...fail('kitsu episode count unknown'), unknown: true };
  }
  const nextStart = Math.min(...siblings
      .filter(sibling => sibling.fromSeason === 0 && sibling.fromEpisode > row.fromEpisode)
      .map(sibling => sibling.fromEpisode));
  const run = (cinemeta.videos || [])
      .filter(video => video.season === 0 && video.episode >= row.fromEpisode && video.episode < nextStart)
      .sort((a, b) => a.episode - b.episode);
  const extra = run.slice(row.episodeCount);
  if (!extra.length) {
    return ok();
  }
  const range = `S0E${extra[0].episode}${extra.length > 1 ? `-E${extra[extra.length - 1].episode}` : ''}`;
  return fail(`also covers ${range}, kitsu has ${row.episodeCount} episode${row.episodeCount > 1 ? 's' : ''}`);
}

// The IMDb episode aired within the leeway of `date`.
function airedNear(video, date) {
  return !!video?.released && Math.abs(new Date(video.released) - new Date(date)) / DAY_MS <= CONTINUATION_DATE_LEEWAY_DAYS;
}

// The IMDb episode is the first of its season aired around `date`: the one before it did not (so for a streaming
// drop with one date for every episode, only the first of the drop).
function startsRun(videos = [], video, date) {
  const previous = videos.find(other => other.season === video.season && other.episode === video.episode - 1);
  return !airedNear(previous, date);
}

// "Guilty Crown: Lost Christmas" -> ['lost christmas']: the part after the franchise name, long enough to be telling.
function specialSubtitles(titles) {
  return [...new Set(titles
      .map(title => (title || '').split(/:| - | ~/).slice(1).join(' '))
      .map(normalizeTitle)
      .filter(subtitle => subtitle.length >= 6))];
}

function normalizeTitle(title) {
  return (title || '').toLowerCase().normalize('NFKD').replace(/[^a-z0-9]+/g, ' ').trim();
}

// "season:episode" keys of the regular IMDb episodes existing rows map, rolling over into later seasons like the
// runtime does. A row with an unknown episode count covers nothing, which keeps the regular-episode check strict.
function coveredRegularEpisodes(cinemeta, siblings) {
  const regular = (cinemeta.videos || []).filter(video => video.season > 0)
      .sort((a, b) => a.season - b.season || a.episode - b.episode);
  const covered = new Set();
  for (const row of siblings.filter(sibling => sibling.fromSeason > 0 && Number.isInteger(sibling.episodeCount))) {
    const start = regular.findIndex(video => video.season === row.fromSeason && video.episode === row.fromEpisode);
    if (start === -1) {
      continue;
    }
    regular.slice(start, start + row.episodeCount).forEach(video => covered.add(`${video.season}:${video.episode}`));
  }
  return covered;
}

// Where a continuation would start: one past the episodes covered in the last mapped regular season, or
// undefined when any Kitsu entry in that season has an unknown episode count.
function continuationStart(siblings) {
  const regular = siblings.filter(sibling => sibling.fromSeason > 0);
  if (!regular.length) {
    return undefined;
  }
  const lastSeason = Math.max(...regular.map(sibling => sibling.fromSeason));
  const rows = regular.filter(sibling => sibling.fromSeason === lastSeason);
  if (rows.some(row => !Number.isInteger(row.episodeCount) || row.episodeCount < 1)) {
    return undefined;
  }
  const covered = Math.max(...rows.map(row => row.fromEpisode - 1 + row.episodeCount));
  return { fromSeason: lastSeason, fromEpisode: covered + 1 };
}

// Next season number for a sequel of an already-mapped imdb: one past the existing regular seasons.
function nextSeason(siblings) {
  return new Set(siblings.filter(sibling => sibling.fromSeason > 0).map(sibling => sibling.fromSeason)).size + 1;
}

function ok() {
  return { ok: true };
}

function fail(reason) {
  return { ok: false, reason };
}

module.exports = {
  seasonWindow, verifyCandidate, verifyFreshTitle, verifySpecial, verifyOffset, verifySpecialRun, nextSeason,
  continuationStart,
  CONTINUATION_DATE_LEEWAY_DAYS, SPECIAL_TYPES,
};
