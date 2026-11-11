//! A region file's chunk scan on any bytes, as `r.0.0.mca`, then every chunk it lists read again
//! whole, as a chunk and as an entity chunk. Every chunk it can't read is counted or skipped,
//! not an error; panics, aborts and hangs aren't fine.
#![no_main]

use std::path::PathBuf;
use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use spyglass_world::{chunk, region};

/// One file per fuzzing process, rewritten for each input.
fn path() -> &'static PathBuf {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    PATH.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("spyglass-fuzz-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("r.0.0.mca")
    })
}

fuzz_target!(|data: &[u8]| {
    std::fs::write(path(), data).unwrap();
    let Ok(index) = region::index_file(path()) else {
        return;
    };
    for (header_index, _) in &index.chunks {
        let pos = index.chunk_pos(*header_index).expect("r.0.0.mca parses");
        if let Some(raw) = region::read_chunk(path().parent().expect("in a folder"), pos) {
            let _ = chunk::read(&raw);
            let _ = chunk::read_entities(&raw);
        }
    }
});
