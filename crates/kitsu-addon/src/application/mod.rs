pub mod addon;
pub mod enrich;
pub mod error;
pub mod extras;
pub mod jsvalue;
pub mod mapping;
pub mod metadata;
pub mod search_gate;
#[cfg(test)]
mod testing;
pub mod title_refresh;

pub use addon::{Addon, Answer, IndexSearch, Parts, TitleSearch, Upstreams};
pub use enrich::{enrich_imdb, enrich_kitsu};
pub use error::Error;
pub use extras::{ExtraValue, Extras};
pub use jsvalue::{JsObject, JsValue, TypeError};
pub use mapping::{Mapping, MappingRow};
pub use metadata::{EntryOptions, GenreUrl, ImageLookup, Metadata, NoImages, catalog_fields};
pub use search_gate::SearchGate;
pub use title_refresh::{REFRESH_INTERVAL, TitleRefresh};
