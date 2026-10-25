//! End to end against the real game: needs network (or a cached jar) and a matching Java runtime.
//! Run with `cargo test -p spyglass-data -- --ignored`.

use spyglass_data::bundle;
use spyglass_data::cache::JarCache;
use spyglass_data::extract::{self, Options};

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
    for dir in bundle::JAR_DIRS {
        assert!(a.manifest.files.keys().any(|p| p.starts_with(dir)), "{dir}");
    }
    for report in bundle::REPORTS {
        assert!(a.manifest.files.contains_key(*report), "{report}");
    }
    assert_eq!(bundle::verify(&a.dir).unwrap(), a.manifest);

    // A second extraction is identical, down to the bytes of bundle.json.
    let b = run("b");
    assert_eq!(b.manifest, a.manifest);
    assert_eq!(
        std::fs::read(a.dir.join(bundle::MANIFEST)).unwrap(),
        std::fs::read(b.dir.join(bundle::MANIFEST)).unwrap()
    );
}
