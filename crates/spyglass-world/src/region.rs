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
use spyglass_data::nbt::{self, Tag};

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
            total.merge(handle.join().expect("region reader panicked")?);
        }
        Ok(total)
    })
}

/// Counts one region file's chunks by data version.
pub fn file_versions(path: &Path) -> std::io::Result<ChunkVersions> {
    let bytes = std::fs::read(path)?;
    let mut counts = ChunkVersions::default();
    if bytes.len() < 4096 {
        return Ok(counts);
    }
    for i in 0..1024 {
        let entry = u32::from_be_bytes(bytes[i * 4..i * 4 + 4].try_into().expect("4 bytes"));
        if entry == 0 {
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
    let compression = header[4];
    let external;
    let data = if compression & 128 != 0 {
        // RegionFile.getExternalChunkPath: c.<x>.<z>.mcc beside the region file, with the
        // chunk's absolute coordinates.
        let (rx, rz) = region_coords(path)?;
        let (x, z) = (rx * 32 + (i % 32) as i32, rz * 32 + (i / 32) as i32);
        external = std::fs::read(path.with_file_name(format!("c.{x}.{z}.mcc"))).ok()?;
        &external[..]
    } else {
        // The length counts the compression byte.
        bytes.get(start + 5..start + 4 + length)?
    };
    let mut raw = Vec::new();
    match compression & 127 {
        1 => GzDecoder::new(data).read_to_end(&mut raw).ok()?,
        2 => ZlibDecoder::new(data).read_to_end(&mut raw).ok()?,
        3 => {
            raw.extend_from_slice(data);
            raw.len()
        }
        _ => return None,
    };
    let (_, tag) = nbt::read(&raw).ok()?;
    Some(tag.get("DataVersion").and_then(Tag::as_int))
}

/// `(x, z)` from a file named `r.<x>.<z>.mca`.
fn region_coords(path: &Path) -> Option<(i32, i32)> {
    let name = path.file_name()?.to_str()?;
    let mut parts = name.strip_prefix("r.")?.strip_suffix(".mca")?.split('.');
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}
