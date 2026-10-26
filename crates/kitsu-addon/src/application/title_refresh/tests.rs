use std::sync::Mutex;

use async_trait::async_trait;
use serde_json::{Value, json};

use super::*;
use crate::infrastructure::{KitsuTransport, NoUpstreamStats, http::TransportError, kitsu::KitsuResponse};

type Page = Result<Vec<Value>, String>;

struct FakeKitsu {
    pages: Box<dyn Fn(u64) -> Page + Send + Sync>,
    queries: Mutex<Vec<String>>,
}

impl FakeKitsu {
    fn new(pages: impl Fn(u64) -> Page + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self { pages: Box::new(pages), queries: Mutex::default() })
    }

    fn offsets(&self) -> Vec<u64> {
        self.queries.lock().unwrap().iter().map(|query| offset_of(query)).collect()
    }
}

fn offset_of(query: &str) -> u64 {
    let (_, rest) = query.split_once("page%5Boffset%5D=").unwrap();
    rest.split('&').next().unwrap().parse().unwrap()
}

#[async_trait]
impl KitsuTransport for FakeKitsu {
    async fn get(&self, _path: &str, query: &str) -> Result<KitsuResponse, TransportError> {
        self.queries.lock().unwrap().push(query.to_owned());
        let data = (self.pages)(offset_of(query)).map_err(TransportError)?;
        Ok(KitsuResponse {
            status: 200,
            body: json!({ "data": data, "meta": { "count": 1000 } }).to_string().into_bytes(),
        })
    }
}

fn entry(id: u64, title: &str) -> Value {
    json!({
        "id": id.to_string(), "type": "anime",
        "attributes": {
            "canonicalTitle": title, "titles": { "en": title }, "abbreviatedTitles": [], "subtype": "TV",
            "status": "current", "startDate": "2026-01-01", "episodeCount": 12,
        },
    })
}

fn numbered(ids: std::ops::Range<u64>) -> Vec<Value> {
    ids.map(|id| entry(id, &format!("Show{}", word(id)))).collect()
}

fn word(mut n: u64) -> String {
    let mut text = String::new();
    loop {
        text.push(char::from(b'a' + u8::try_from(n % 26).unwrap()));
        n /= 26;
        if n == 0 {
            return text;
        }
    }
}

fn known() -> Sources {
    let records = vec![record(1, "Cowboy Bebop"), record(2, "Trigun"), record(3, "Monster")];
    Sources { records, mapping: ImdbMappings::new(), aliases: Aliases::new() }
}

fn record(id: u64, title: &str) -> TitleRecord {
    let mut flat = entry(id, title)["attributes"].clone();
    flat["id"] = json!(id.to_string());
    TitleRecord::from_kitsu(&flat).unwrap()
}

struct Harness {
    refresh: TitleRefresh,
    search: Arc<SharedIndex>,
    kitsu: Arc<FakeKitsu>,
}

fn harness(pages: impl Fn(u64) -> Page + Send + Sync + 'static) -> Harness {
    let kitsu = FakeKitsu::new(pages);
    let sources = known();
    let search = Arc::new(SharedIndex::new(Index::build(&sources.records, &sources.mapping, &sources.aliases)));
    let api = KitsuApi::new(kitsu.clone(), Arc::new(NoUpstreamStats));
    Harness { refresh: TitleRefresh::new(api, Arc::clone(&search), sources), search, kitsu }
}

fn entries(search: &SharedIndex) -> usize {
    search.load().index().entries().len()
}

fn top(search: &SharedIndex, query: &str) -> Option<u32> {
    search.search(query).first().map(|hit| hit.id)
}

#[tokio::test]
async fn new_records_appear_in_the_index() {
    let mut harness = harness(|_| Ok(vec![entry(100, "Frieren Beyond Journeys End"), entry(1, "Cowboy Bebop")]));
    assert_eq!(top(&harness.search, "Frieren Beyond Journeys End"), None);
    assert_eq!(harness.refresh.refresh().await.unwrap(), 1);
    assert_eq!(top(&harness.search, "Frieren Beyond Journeys End"), Some(100));
    assert_eq!(top(&harness.search, "Cowboy Bebop"), Some(1));
    assert_eq!(entries(&harness.search), 4);
}

#[tokio::test]
async fn pages_are_requested_newest_first() {
    let mut harness = harness(|_| Ok(vec![]));
    harness.refresh.refresh().await.unwrap();
    let queries = harness.kitsu.queries.lock().unwrap().clone();
    assert_eq!(queries.len(), 1);
    assert!(queries[0].contains("sort=-createdAt"), "{}", queries[0]);
    assert!(queries[0].contains("page%5Blimit%5D=20"), "{}", queries[0]);
}

#[tokio::test]
async fn a_known_entry_ends_the_paging() {
    let mut harness = harness(|offset| match offset {
        0 => Ok(numbered(100..120)),
        _ => Ok(numbered(120..139).into_iter().chain([entry(1, "Cowboy Bebop")]).collect()),
    });
    assert_eq!(harness.refresh.refresh().await.unwrap(), 39);
    assert_eq!(harness.kitsu.offsets(), vec![0, 20]);
    assert_eq!(entries(&harness.search), 42);
}

#[tokio::test]
async fn a_short_page_ends_the_paging() {
    let mut harness = harness(|_| Ok(numbered(100..119)));
    assert_eq!(harness.refresh.refresh().await.unwrap(), 19);
    assert_eq!(harness.kitsu.offsets(), vec![0]);
}

#[tokio::test]
async fn at_most_five_pages_are_read() {
    let mut harness = harness(|offset| Ok(numbered(1000 + offset..1020 + offset)));
    assert_eq!(harness.refresh.refresh().await.unwrap(), 100);
    assert_eq!(harness.kitsu.offsets(), vec![0, 20, 40, 60, 80]);
    assert_eq!(entries(&harness.search), 103);
}

#[tokio::test]
async fn an_entry_repeated_by_shifting_pages_is_added_once() {
    let mut harness = harness(|offset| match offset {
        0 => Ok(numbered(100..120)),
        1..=20 => Ok(numbered(119..139)),
        _ => Ok(vec![]),
    });
    assert_eq!(harness.refresh.refresh().await.unwrap(), 39);
    assert_eq!(entries(&harness.search), 42);
}

#[tokio::test]
async fn known_ids_are_not_added_again_by_the_next_refresh() {
    let mut harness = harness(|_| Ok(vec![entry(100, "Frieren Beyond Journeys End")]));
    assert_eq!(harness.refresh.refresh().await.unwrap(), 1);
    assert_eq!(harness.refresh.refresh().await.unwrap(), 0);
    assert_eq!(entries(&harness.search), 4);
}

#[tokio::test]
async fn nothing_new_keeps_the_index_untouched() {
    let mut harness = harness(|_| Ok(vec![entry(2, "Trigun")]));
    let before = harness.search.load();
    assert_eq!(harness.refresh.refresh().await.unwrap(), 0);
    assert!(Arc::ptr_eq(&before, &harness.search.load()));
}

#[tokio::test]
async fn a_failing_page_keeps_the_old_index_and_a_later_refresh_still_works() {
    let failing = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let flag = Arc::clone(&failing);
    let mut harness = harness(move |_| {
        if flag.load(std::sync::atomic::Ordering::SeqCst) {
            Err("connect timeout".to_owned())
        } else {
            Ok(vec![entry(100, "Frieren Beyond Journeys End")])
        }
    });
    let error = harness.refresh.refresh().await.unwrap_err();
    assert_eq!(error.to_string(), "connect timeout");
    assert_eq!(entries(&harness.search), 3);
    assert_eq!(top(&harness.search, "Trigun"), Some(2));
    failing.store(false, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(harness.refresh.refresh().await.unwrap(), 1);
    assert_eq!(top(&harness.search, "Frieren Beyond Journeys End"), Some(100));
}

#[tokio::test]
async fn a_later_page_failing_adds_nothing() {
    let mut harness = harness(|offset| if offset == 0 { Ok(numbered(100..120)) } else { Err("boom".to_owned()) });
    assert!(harness.refresh.refresh().await.is_err());
    assert_eq!(entries(&harness.search), 3);
}

#[tokio::test]
async fn a_restart_searches_entries_created_since_the_title_file_without_waiting_an_interval() {
    let harness = harness(|_| Ok(vec![entry(100, "Zyxwvut Freshly Created")]));
    let task = harness.refresh.spawn(REFRESH_INTERVAL);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while top(&harness.search, "zyxwvut freshly created").is_none() && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(top(&harness.search, "zyxwvut freshly created"), Some(100));
    task.abort();
}

#[tokio::test(start_paused = true)]
async fn the_loop_runs_at_start_then_every_interval_and_survives_errors() {
    let harness = harness(|_| Err("down".to_owned()));
    let kitsu = Arc::clone(&harness.kitsu);
    let task = harness.refresh.spawn(REFRESH_INTERVAL);
    tokio::task::yield_now().await;
    assert_eq!(kitsu.offsets().len(), 1);
    tokio::time::advance(REFRESH_INTERVAL.checked_sub(Duration::from_secs(1)).unwrap()).await;
    tokio::task::yield_now().await;
    assert_eq!(kitsu.offsets().len(), 1);
    tokio::time::advance(Duration::from_secs(2)).await;
    tokio::task::yield_now().await;
    assert_eq!(kitsu.offsets().len(), 2);
    tokio::time::advance(REFRESH_INTERVAL).await;
    tokio::task::yield_now().await;
    assert_eq!(kitsu.offsets().len(), 3);
    assert!(!task.is_finished());
    task.abort();
}

fn broken_rebuild(
    _records: &mut Vec<TitleRecord>,
    _fresh: Vec<TitleRecord>,
    _mapping: &ImdbMappings,
    _aliases: &Aliases,
) -> Option<(usize, Index)> {
    panic!("rebuild broken")
}

fn half_done_rebuild(
    records: &mut Vec<TitleRecord>,
    fresh: Vec<TitleRecord>,
    _mapping: &ImdbMappings,
    _aliases: &Aliases,
) -> Option<(usize, Index)> {
    records.extend(fresh);
    panic!("rebuild broken after the merge")
}

fn rebuild_on_its_own_thread(
    records: &mut Vec<TitleRecord>,
    fresh: Vec<TitleRecord>,
    mapping: &ImdbMappings,
    aliases: &Aliases,
) -> Option<(usize, Index)> {
    assert_eq!(std::thread::current().name(), Some(REBUILD_THREAD));
    refresh_index(records, fresh, mapping, aliases)
}

#[tokio::test]
async fn a_panicking_rebuild_keeps_the_records_and_the_next_refresh_works() {
    let mut harness = harness(|_| Ok(vec![entry(100, "Frieren Beyond Journeys End")]));
    harness.refresh.rebuild = broken_rebuild;
    let error = harness.refresh.refresh().await.unwrap_err();
    assert_eq!(error.to_string(), "index rebuild failed: rebuild broken");
    assert_eq!(entries(&harness.search), 3);
    harness.refresh.rebuild = refresh_index;
    assert_eq!(harness.refresh.refresh().await.unwrap(), 1);
    assert_eq!(top(&harness.search, "Frieren Beyond Journeys End"), Some(100));
}

#[tokio::test]
async fn a_rebuild_panicking_after_the_merge_drops_the_merged_records() {
    let mut harness = harness(|_| Ok(vec![entry(100, "Frieren Beyond Journeys End")]));
    harness.refresh.rebuild = half_done_rebuild;
    assert!(harness.refresh.refresh().await.is_err());
    assert_eq!(locked(&harness.refresh.sources).records.len(), 3);
    harness.refresh.rebuild = refresh_index;
    assert_eq!(harness.refresh.refresh().await.unwrap(), 1);
    assert_eq!(top(&harness.search, "Frieren Beyond Journeys End"), Some(100));
}

#[tokio::test]
async fn the_rebuild_runs_on_a_dedicated_thread() {
    let mut harness = harness(|_| Ok(vec![entry(100, "Frieren Beyond Journeys End")]));
    harness.refresh.rebuild = rebuild_on_its_own_thread;
    assert_eq!(harness.refresh.refresh().await.unwrap(), 1);
}
