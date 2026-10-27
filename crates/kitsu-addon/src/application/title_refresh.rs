use std::{
    collections::HashSet,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};

use indexmap::IndexMap;
use kitsu_search::{
    SharedIndex,
    index::Index,
    records::{self, Aliases, ImdbMappings, Sources, TitleRecord},
    refresh_index,
};
use tokio::task::JoinHandle;

use super::Error;
use crate::infrastructure::{KitsuApi, TitlePage, kitsu::PAGE_SIZE};

pub const REFRESH_INTERVAL: Duration = Duration::from_secs(60 * 60);
pub const MAX_PAGES: u64 = 5;
const NEWEST_FIRST: &str = "-createdAt";
const REBUILD_THREAD: &str = "title-index-rebuild";

pub type LoadSources = Arc<dyn Fn() -> Result<Sources, records::Error> + Send + Sync>;

struct Added {
    known: HashSet<u64>,
    records: Vec<TitleRecord>,
}

fn locked(added: &Mutex<Added>) -> MutexGuard<'_, Added> {
    added.lock().unwrap_or_else(PoisonError::into_inner)
}

fn rebuild_index(
    added: &Mutex<Added>,
    fresh: Vec<TitleRecord>,
    search: &SharedIndex,
    load: &LoadSources,
    rebuild: Rebuild,
) -> Result<usize, String> {
    let mut added = locked(added);
    let Sources { mut records, mapping, aliases } =
        load().map_err(|error| format!("reading the title data: {error}"))?;
    let from_file = records.len();
    records.extend(added.records.iter().cloned());
    let outcome = catch_unwind(AssertUnwindSafe(|| rebuild(&mut records, fresh, &mapping, &aliases)));
    match outcome {
        Ok(Some((count, index))) => {
            let new = records.split_off(from_file);
            drop(records);
            added.known.extend(new.iter().map(|record| record.id));
            added.records = new;
            search.swap(index);
            Ok(count)
        }
        Ok(None) => Ok(0),
        Err(panic) => Err(match panic.downcast::<String>() {
            Ok(message) => *message,
            Err(panic) => panic.downcast::<&str>().map(|message| (*message).to_owned()).unwrap_or_default(),
        }),
    }
}

async fn fetch_new_records(kitsu: &KitsuApi, known: &HashSet<u64>) -> Result<Vec<TitleRecord>, Error> {
    let mut fresh: IndexMap<u64, TitleRecord> = IndexMap::new();
    for page in 0..MAX_PAGES {
        let result =
            kitsu.anime_title_page(&TitlePage { offset: page * PAGE_SIZE, sort: NEWEST_FIRST.to_owned() }).await?;
        let entries = result.entries.len();
        let mut unknown = 0;
        for record in result.entries.iter().filter_map(TitleRecord::from_kitsu) {
            if !known.contains(&record.id) {
                unknown += 1;
                fresh.insert(record.id, record);
            }
        }
        if unknown < entries || entries < PAGE_SIZE as usize {
            break;
        }
    }
    Ok(fresh.into_values().collect())
}

type Rebuild = fn(&mut Vec<TitleRecord>, Vec<TitleRecord>, &ImdbMappings, &Aliases) -> Option<(usize, Index)>;

pub struct TitleRefresh {
    kitsu: KitsuApi,
    search: Arc<SharedIndex>,
    added: Arc<Mutex<Added>>,
    load: LoadSources,
    rebuild: Rebuild,
}

impl TitleRefresh {
    pub fn new(kitsu: KitsuApi, search: Arc<SharedIndex>, known: HashSet<u64>, load: LoadSources) -> Self {
        let added = Arc::new(Mutex::new(Added { known, records: Vec::new() }));
        Self { kitsu, search, added, load, rebuild: refresh_index }
    }

    pub async fn refresh(&mut self) -> Result<usize, Error> {
        let known = locked(&self.added).known.clone();
        let fresh = fetch_new_records(&self.kitsu, &known).await?;
        if fresh.is_empty() {
            return Ok(0);
        }
        let (added, search, rebuild) = (Arc::clone(&self.added), Arc::clone(&self.search), self.rebuild);
        let load = Arc::clone(&self.load);
        let (sender, receiver) = tokio::sync::oneshot::channel();
        std::thread::Builder::new()
            .name(REBUILD_THREAD.to_owned())
            .spawn(move || {
                let _ = sender.send(rebuild_index(&added, fresh, &search, &load, rebuild));
            })
            .map_err(|error| Error::failed(anyhow::anyhow!("index rebuild not started: {error}")))?;
        receiver
            .await
            .map_err(|_| Error::failed(anyhow::anyhow!("index rebuild ended without a result")))?
            .map_err(|message| Error::failed(anyhow::anyhow!("index rebuild failed: {message}")))
    }

    pub fn spawn(mut self, interval: Duration) -> JoinHandle<()> {
        tokio::spawn(async move {
            // at startup too: a restart would otherwise search without the entries created since the file's rebuild
            // for an interval
            let mut ticks = tokio::time::interval(interval);
            ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                ticks.tick().await;
                match self.refresh().await {
                    Ok(0) => {}
                    Ok(added) => tracing::info!("Title index: added {added} new Kitsu entries"),
                    Err(error) => tracing::warn!("Title index refresh failed: {error}"),
                }
            }
        })
    }
}

#[cfg(test)]
mod tests;
