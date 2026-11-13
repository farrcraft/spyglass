//! A data pack zip on any bytes, read as `spyglass_world::open_pack` reads a save's pack, then
//! applied for a format and its every file looked up through the layer's filter. A zip it can't
//! read, or a `pack.mcmeta` the game would skip, is an error, not a panic; panics, aborts and
//! hangs aren't fine.
#![no_main]

use std::path::PathBuf;
use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use spyglass_data::pack::PackFormat;
use spyglass_world::open_pack;

/// One file per fuzzing process, rewritten for each input.
fn path() -> &'static PathBuf {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    PATH.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("spyglass-fuzz-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("pack.zip")
    })
}

fuzz_target!(|data: &[u8]| {
    std::fs::write(path(), data).unwrap();
    let Ok(pack) = open_pack(path()) else {
        return;
    };
    for format in [[107, 1], [121, 0], [127, 0]] {
        let layer = pack.layer(PackFormat::from(format));
        for file in layer.files.keys() {
            let _ = layer.hides(file);
        }
    }
    let _ = pack.worldgen_files();
});
