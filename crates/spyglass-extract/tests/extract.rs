//! End to end against the real game: needs network (or a cached jar) and a matching Java runtime.
//! Run with `cargo test -p spyglass-extract -- --ignored`.

use spyglass_extract::cache::JarCache;
use spyglass_extract::extract::{self, Options};
use spyglass_extract::write;

#[test]
#[ignore = "needs network and Java 25"]
fn extracts_26_3_reproducibly() {
    let cache = JarCache::open_default().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let run = |out: &str| {
        let out = tmp.path().join(out);
        let options = Options {
            version: "26.3",
            cache: &cache,
            java: None,
            out: &out,
            replace: false,
        };
        extract::extract(&options, |_| {}).unwrap()
    };

    let a = run("a");
    assert_eq!(a.manifest.version, "26.3");
    assert_eq!(a.manifest.data_version, 5023);
    for dir in write::JAR_DIRS {
        assert!(a.manifest.files.keys().any(|p| p.starts_with(dir)), "{dir}");
    }
    for report in write::REPORTS {
        assert!(a.manifest.files.contains_key(*report), "{report}");
    }
    assert_eq!(spyglass_data::bundle::verify(&a.dir).unwrap(), a.manifest);

    // A second extraction is identical, down to the bytes of bundle.json.
    let b = run("b");
    assert_eq!(b.manifest, a.manifest);
    assert_eq!(
        std::fs::read(a.dir.join(spyglass_data::bundle::MANIFEST)).unwrap(),
        std::fs::read(b.dir.join(spyglass_data::bundle::MANIFEST)).unwrap()
    );
}
