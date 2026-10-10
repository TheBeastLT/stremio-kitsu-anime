// Keep an imdb's entries together, with season-0 specials trailing at the end of the group. A regular
// season goes after the last non-special sibling; a special goes after the very last sibling. A
// brand-new imdb slots into kitsu-id order.
function insertMappingEntry(base, item) {
  const siblings = base.filter(e => e.imdb_id === item.imdb_id);
  if (!siblings.length) {
    insertInKitsuOrder(base, item);
    return;
  }
  const anchors = item.fromSeason === 0 ? siblings : siblings.filter(e => e.fromSeason !== 0);
  if (anchors.length) {
    base.splice(base.indexOf(anchors[anchors.length - 1]) + 1, 0, item);
  } else {
    base.splice(base.indexOf(siblings[0]), 0, item); // only specials exist -> put this before them
  }
}

function insertInKitsuOrder(base, item) {
  const idx = base.findIndex((e, i) =>
      e.kitsu_id > item.kitsu_id && base.slice(i).every(x => x.kitsu_id > item.kitsu_id));
  base.splice(idx === -1 ? base.length : idx, 0, item);
}

module.exports = { insertMappingEntry, insertInKitsuOrder };
