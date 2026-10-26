//! Region files (`.mca`, `RegionFile`): which data version generated each chunk, so a save can
//! say how much of it older versions made.
//!
//! A region file starts with 1,024 four-byte locations, one per chunk: a 3-byte sector offset
//! and a 1-byte sector count (sectors are 4 KiB; zero means no chunk). At the offset: a
//! big-endian `u32` length, a compression byte (1 gzip, 2 zlib, 3 none, 4 LZ4; plus 128 when
//! the chunk is in a separate `c.<x>.<z>.mcc` file), then the NBT.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use flate2::read::{GzDecoder, ZlibDecoder};
use spyglass_data::nbt;

use crate::read_capped;

/// The largest region file the format can address in its chunk sectors: the header and 1,024
/// chunks of at most 255 sectors. Larger files are corrupt or crafted.
const MAX_REGION_FILE: u64 = (2 + 1024 * 255) * 4096;
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
                        counts.merge(file_versions(file)?);
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

/// Counts one region file's chunks by data version. Every chunk of a file too large for the
/// format is unreadable.
pub fn file_versions(path: &Path) -> std::io::Result<ChunkVersions> {
    let mut counts = ChunkVersions::default();
    let (bytes, oversized) = match read_capped(path, MAX_REGION_FILE) {
        Ok(bytes) => (bytes, false),
        Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
            let mut header = vec![0; 4096];
            std::fs::File::open(path)?.read_exact(&mut header)?;
            (header, true)
        }
        Err(e) => return Err(e),
    };
    if bytes.len() < 4096 {
        return Ok(counts);
    }
    for i in 0..1024 {
        let entry = u32::from_be_bytes(bytes[i * 4..i * 4 + 4].try_into().expect("4 bytes"));
        if entry == 0 {
            continue;
        }
        if oversized {
            counts.unreadable += 1;
            continue;
        }
        match chunk_version(path, &bytes, entry, i) {
            Some(Some(v)) => *counts.by_version.entry(v).or_default() += 1,
            Some(None) => counts.unversioned += 1,
            None => counts.unreadable += 1,
        }
    }
    Ok(counts)
}

/// The `DataVersion` of the chunk at location `entry` (index `i` in the header): `None` if the
/// chunk can't be read, `Some(None)` if it has no data version.
fn chunk_version(path: &Path, bytes: &[u8], entry: u32, i: usize) -> Option<Option<i32>> {
    let start = (entry >> 8) as usize * 4096;
    let header = bytes.get(start..start + 5)?;
    let length = u32::from_be_bytes(header[..4].try_into().ok()?) as usize;
    // The data must fit in the sectors the location gives it.
    if length > (entry & 0xFF) as usize * 4096 {
        return None;
    }
    let compression = header[4];
    let external;
    let data = if compression & 128 != 0 {
        // RegionFile.getExternalChunkPath: c.<x>.<z>.mcc beside the region file, with the
        // chunk's absolute coordinates.
        let (rx, rz) = region_coords(path)?;
        let (x, z) = (
            rx.wrapping_mul(32).wrapping_add((i % 32) as i32),
            rz.wrapping_mul(32).wrapping_add((i / 32) as i32),
        );
        external = read_capped(&path.with_file_name(format!("c.{x}.{z}.mcc")), MAX_CHUNK).ok()?;
        &external[..]
    } else {
        // The length counts the compression byte.
        bytes.get(start + 5..start + 4 + length)?
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
