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

    let first = run("a");
    assert_eq!(first.manifest.version, "26.3");
    assert_eq!(first.manifest.data_version, 5023);
    for dir in write::JAR_DIRS {
        assert!(
            first.manifest.files.keys().any(|p| p.starts_with(dir)),
            "{dir}"
        );
    }
    for report in write::REPORTS {
        assert!(first.manifest.files.contains_key(*report), "{report}");
    }
    for dir in write::REPORT_DIRS {
        assert!(
            first.manifest.files.keys().any(|p| p.starts_with(dir)),
            "{dir}"
        );
    }
    assert_eq!(
        spyglass_data::bundle::verify(&first.dir).unwrap(),
        first.manifest
    );

    // A second extraction is identical, down to the bytes of bundle.json.
    let second = run("b");
    assert_eq!(second.manifest, first.manifest);
    assert_eq!(
        std::fs::read(first.dir.join(spyglass_data::bundle::MANIFEST)).unwrap(),
        std::fs::read(second.dir.join(spyglass_data::bundle::MANIFEST)).unwrap()
    );
}
