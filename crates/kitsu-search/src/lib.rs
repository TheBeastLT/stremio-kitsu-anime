pub mod index;
mod kitsu_record;
pub mod norm;
pub mod query;
pub mod rank;
pub mod records;
pub mod rules;
pub mod score;
pub mod search;
pub mod shared;

pub use rank::MIN_CONFIDENCE;
pub use search::{Hit, Ranking, Searcher};
pub use shared::{SharedIndex, merge_new_records, refresh_index};

fn rx(source: &str, flags: &str) -> fastjs_regex::Regex {
    fastjs_regex::Regex::new(source, flags).unwrap_or_else(|error| panic!("invalid pattern {source}: {error}"))
}
