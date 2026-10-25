use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::LazyLock,
};

use kitsu_search::records::read_records;

use super::mapping::Mapping;

pub fn data_path(file: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../static/data").join(file)
}

pub static REAL_MAPPING: LazyLock<Mapping> = LazyLock::new(|| {
    let records = read_records(File::open(data_path("search_titles.jsonl")).unwrap()).unwrap();
    Mapping::load(&data_path("imdb_mapping.json"), &records).unwrap()
});

pub fn fixture_mapping() -> Mapping {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mapping");
    let records = read_records(File::open(dir.join("search_titles.jsonl")).unwrap()).unwrap();
    Mapping::load(&dir.join("imdb_mapping.json"), &records).unwrap()
}
