//! The NBT reader on any bytes: plain, gzip-compressed with the limit world saves use, and the
//! root-int reader region scans use. Errors are fine; panics, aborts and hangs aren't.
#![no_main]

use libfuzzer_sys::fuzz_target;
use spyglass_data::nbt;

fuzz_target!(|data: &[u8]| {
    let _ = nbt::read(data);
    let _ = nbt::read_root_int(data, "DataVersion");
    let _ = nbt::read_compressed_limited(data, 1 << 20);
});
