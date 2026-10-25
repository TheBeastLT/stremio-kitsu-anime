use super::Error;
use crate::domain::manifest;

pub fn catalog_exists(id: &str) -> bool {
    manifest().has_catalog(id)
}

pub fn ensure_catalog(id: &str) -> Result<(), Error> {
    if catalog_exists(id) { Ok(()) } else { Err(Error::NotFound(format!("Unknown catalog: {id}"))) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_catalogs_pass() {
        assert!(ensure_catalog("kitsu-anime-trending").is_ok());
    }

    #[test]
    fn unknown_catalog_is_not_found() {
        let error = ensure_catalog("nope").unwrap_err();
        assert!(matches!(&error, Error::NotFound(reason) if reason == "Unknown catalog: nope"));
    }
}
