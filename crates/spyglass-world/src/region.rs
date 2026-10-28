//! Region files (`.mca`, `RegionFile`): which data version generated each chunk, so a save can
//! say how much of it older versions made.
//!
//! A region file starts with 1,024 four-byte locations, one per chunk: a 3-byte sector offset
//! and a 1-byte sector count (sectors are 4 KiB; zero means no chunk). At the offset: a
//! big-endian `u32` length, a compression byte (1 gzip, 2 zlib, 3 none, 4 LZ4; plus 128 when
//! the chunk is in a separate `c.<x>.<z>.mcc` file), then the NBT.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use flate2::read::{GzDecoder, ZlibDecoder};
use spyglass_data::nbt;

use crate::read_capped;

/// The most one chunk's NBT (or its `.mcc` file) may hold before or after decompression. Real
/// chunks are rarely over a megabyte; larger ones count as unreadable.
const MAX_CHUNK: u64 = 32 << 20;

/// How many chunks each data version generated, from one dimension's region files.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChunkVersions {
    /// Chunks by their `DataVersion`.
    pub by_version: BTreeMap<i32, usize>,
    /// Chunks with no `DataVersion`: saved before 1.9, so older than any versioned chunk.
    pub unversioned: usize,
    /// Chunks spyglass couldn't read: LZ4 or unknown compression, a missing `.mcc` file, or
    /// malformed data. Not errors: a save can still be used.
    pub unreadable: usize,
    /// Region files that couldn't be opened or read, whose chunks aren't counted at all.
    pub unreadable_files: usize,
}

impl ChunkVersions {
    /// Chunks older than `data_version`, unversioned ones included.
    pub fn older_than(&self, data_version: i32) -> usize {
        self.unversioned
            + self
                .by_version
                .range(..data_version)
                .map(|(_, n)| n)
                .sum::<usize>()
    }

    /// Every chunk counted, readable or not.
    pub fn total(&self) -> usize {
        self.by_version.values().sum::<usize>() + self.unversioned + self.unreadable
    }

    fn merge(&mut self, other: ChunkVersions) {
        for (v, n) in other.by_version {
            *self.by_version.entry(v).or_default() += n;
        }
        self.unversioned += other.unversioned;
        self.unreadable += other.unreadable;
        self.unreadable_files += other.unreadable_files;
    }
}

/// The region files (`r.<x>.<z>.mca`) in `dir`, sorted. Empty if `dir` doesn't exist.
pub fn region_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("r.") && n.ends_with(".mca"))
        })
        .collect();
    files.sort();
    files
}

/// Counts the chunks in every region file in `dir` by data version, reading files in parallel.
/// A file that can't be read is counted in [`ChunkVersions::unreadable_files`], not an error.
pub fn chunk_versions(dir: &Path) -> std::io::Result<ChunkVersions> {
    let files = region_files(dir);
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let chunk = files.len().div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        let handles: Vec<_> = files
            .chunks(chunk)
            .map(|part| {
                scope.spawn(move || -> std::io::Result<ChunkVersions> {
                    let mut counts = ChunkVersions::default();
                    for file in part {
                        match file_versions(file) {
                            Ok(file_counts) => counts.merge(file_counts),
                            Err(_) => counts.unreadable_files += 1,
                        }
                    }
                    Ok(counts)
                })
            })
            .collect();
        let mut total = ChunkVersions::default();
        for handle in handles {
            let counts = handle
                .join()
                .map_err(|_| std::io::Error::other("a region file reader panicked"))?;
            total.merge(counts?);
        }
        Ok(total)
    })
}

/// Counts one region file's chunks by data version. Each chunk is read on its own, at most its
/// 255 sectors (about 1 MiB), so a large file doesn't need the memory to hold it.
pub fn file_versions(path: &Path) -> std::io::Result<ChunkVersions> {
    let mut counts = ChunkVersions::default();
    let mut file = File::open(path)?;
    let mut header = Vec::with_capacity(4096);
    (&mut file).take(4096).read_to_end(&mut header)?;
    if header.len() < 4096 {
        return Ok(counts);
    }
    let mut buffer = Vec::new();
    for i in 0..1024 {
        let entry = u32::from_be_bytes(header[i * 4..i * 4 + 4].try_into().expect("4 bytes"));
        if entry == 0 {
            continue;
        }
        match chunk_version(path, &mut file, &mut buffer, entry, i) {
            Some(Some(v)) => *counts.by_version.entry(v).or_default() += 1,
            Some(None) => counts.unversioned += 1,
            None => counts.unreadable += 1,
        }
    }
    Ok(counts)
}

/// The `DataVersion` of the chunk at location `entry` (index `i` in the header), read into
/// `buffer`: `None` if the chunk can't be read, `Some(None)` if it has no data version.
fn chunk_version(
    path: &Path,
    file: &mut File,
    buffer: &mut Vec<u8>,
    entry: u32,
    i: usize,
) -> Option<Option<i32>> {
    // RegionFile.getChunkDataInputStream: the chunk's sectors, or what's left of the file.
    let (sector, sectors) = (u64::from(entry >> 8), u64::from(entry & 0xFF));
    buffer.clear();
    file.seek(SeekFrom::Start(sector * 4096)).ok()?;
    file.take(sectors * 4096).read_to_end(buffer).ok()?;
    // Then a length, counting the compression byte, and the compression byte.
    let header = buffer.get(..5)?;
    let length = i32::from_be_bytes(header[..4].try_into().ok()?);
    let compression = header[4];
    // A length of 0 is a chunk "allocated, but stream is missing", even an external one.
    if length == 0 {
        return None;
    }
    let external;
    let data = if compression & 128 != 0 {
        // RegionFile.getExternalChunkPath: c.<x>.<z>.mcc beside the region file, with the
        // chunk's absolute coordinates. The length is otherwise ignored.
        let (rx, rz) = region_coords(path)?;
        let (x, z) = (
            rx.wrapping_mul(32).wrapping_add((i % 32) as i32),
            rz.wrapping_mul(32).wrapping_add((i / 32) as i32),
        );
        external = read_capped(&path.with_file_name(format!("c.{x}.{z}.mcc")), MAX_CHUNK).ok()?;
        &external[..]
    } else {
        // Truncated if it runs past what was read; a negative length is malformed.
        let n = usize::try_from(length.wrapping_sub(1)).ok()?;
        buffer.get(5..5 + n)?
    };
    let mut raw = Vec::new();
    match compression & 127 {
        1 => GzDecoder::new(data)
            .take(MAX_CHUNK + 1)
            .read_to_end(&mut raw)
            .ok()?,
        2 => ZlibDecoder::new(data)
            .take(MAX_CHUNK + 1)
            .read_to_end(&mut raw)
            .ok()?,
        3 => {
            raw.extend_from_slice(data);
            raw.len()
        }
        _ => return None,
    };
    if raw.len() as u64 > MAX_CHUNK {
        return None;
    }
    nbt::read_root_int(&raw, "DataVersion").ok()
}

/// `(x, z)` from a file named `r.<x>.<z>.mca`.
fn region_coords(path: &Path) -> Option<(i32, i32)> {
    let name = path.file_name()?.to_str()?;
    let mut parts = name.strip_prefix("r.")?.strip_suffix(".mca")?.split('.');
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}
