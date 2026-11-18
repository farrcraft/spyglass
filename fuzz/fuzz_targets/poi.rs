//! A points-of-interest folder's read on any bytes, as `poi/r.0.0.mca`, then every chunk it
//! lists read again on its own. Every chunk or section it can't read is counted or left out,
//! not an error; panics, aborts and hangs aren't fine.
#![no_main]

use std::path::PathBuf;
use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use spyglass_world::{poi, region};

/// One folder per fuzzing process, its file rewritten for each input.
fn path() -> &'static PathBuf {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    PATH.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("spyglass-fuzz-poi-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("r.0.0.mca")
    })
}

fuzz_target!(|data: &[u8]| {
    std::fs::write(path(), data).unwrap();
    // The Overworld's sections.
    let _ = poi::read(path().parent().expect("in a folder"), -4..=19);
    let _ = region::visit_chunks(path(), |_, raw| {
        if let Some(raw) = raw {
            let _ = poi::read_chunk(&raw, -4..=19);
        }
    });
});
