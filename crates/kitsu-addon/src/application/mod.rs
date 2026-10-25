pub mod catalog;
pub mod error;

pub use catalog::{catalog_exists, ensure_catalog};
pub use error::Error;
