use std::path::PathBuf;

pub fn golden(name: &str) -> Option<PathBuf> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../search_eval/golden").join(name);
    if path.exists() {
        return Some(path);
    }
    assert!(
        std::env::var("KITSU_GOLDEN_OPTIONAL").as_deref() == Ok("1"),
        "golden file {} is missing: run search_eval/golden/dump.js or set KITSU_GOLDEN_OPTIONAL=1",
        path.display()
    );
    eprintln!("skipping: golden file {} is missing", path.display());
    None
}
