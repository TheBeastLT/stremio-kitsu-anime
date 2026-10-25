use std::collections::HashSet;
use std::sync::Arc;

use arc_swap::ArcSwap;

use crate::index::Index;
use crate::records::{Aliases, ImdbMappings, TitleRecord};
use crate::search::{Hit, Searcher};

pub struct SharedIndex {
    searcher: ArcSwap<Searcher>,
}

impl SharedIndex {
    pub fn new(index: Index) -> Self {
        index.trigrams();
        Self { searcher: ArcSwap::from_pointee(Searcher::new(index)) }
    }

    pub fn load(&self) -> Arc<Searcher> {
        self.searcher.load_full()
    }

    pub fn swap(&self, index: Index) -> Arc<Searcher> {
        index.trigrams();
        self.searcher.swap(Arc::new(Searcher::new(index)))
    }

    pub fn search(&self, query: &str) -> Vec<Hit> {
        self.load().search(query)
    }
}

pub fn merge_new_records(records: &mut Vec<TitleRecord>, fresh: Vec<TitleRecord>) -> usize {
    let mut known: HashSet<u64> = records.iter().map(|record| record.id).collect();
    let before = records.len();
    let mut added: Vec<TitleRecord> = Vec::new();
    for record in fresh {
        if known.insert(record.id) {
            added.push(record);
        } else if let Some(previous) = added.iter_mut().find(|previous| previous.id == record.id) {
            *previous = record;
        }
    }
    records.extend(added);
    records.len() - before
}

pub fn refresh_index(
    records: &mut Vec<TitleRecord>,
    fresh: Vec<TitleRecord>,
    mapping: &ImdbMappings,
    aliases: &Aliases,
) -> Option<(usize, Index)> {
    let added = merge_new_records(records, fresh);
    (added > 0).then(|| (added, Index::build(records, mapping, aliases)))
}
