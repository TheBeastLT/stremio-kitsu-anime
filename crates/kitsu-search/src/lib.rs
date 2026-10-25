pub mod index;
pub mod jaro_winkler;
mod kitsu_record;
pub mod math;
pub mod norm;
pub mod query;
pub mod rank;
pub mod records;
pub mod rules;
pub mod score;
pub mod search;
pub mod shared;
pub mod unicode;

pub use rank::MIN_CONFIDENCE;
pub use search::{Hit, Ranking, Searcher};
pub use shared::{SharedIndex, merge_new_records, refresh_index};
