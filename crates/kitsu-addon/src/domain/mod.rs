pub mod genres;
pub mod manifest;
pub mod meta;

pub use genres::GENRES;
pub use manifest::{CatalogDefinition, Extra, Manifest, manifest};
pub use meta::{CatalogMeta, Link, Meta, Trailer, Video};
