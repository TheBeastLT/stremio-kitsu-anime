pub mod catalog;
pub mod enrich;
pub mod error;
pub mod jsvalue;
pub mod mapping;
pub mod metadata;
#[cfg(test)]
mod testing;

pub use catalog::{catalog_exists, ensure_catalog};
pub use enrich::{enrich_imdb, enrich_kitsu};
pub use error::Error;
pub use jsvalue::{JsObject, JsValue, TypeError};
pub use mapping::{Mapping, MappingRow};
pub use metadata::{EntryOptions, GenreUrl, ImageLookup, Metadata, NoImages, catalog_fields};
