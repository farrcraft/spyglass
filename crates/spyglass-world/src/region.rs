//! Region files (`.mca`, `RegionFile`): which chunks a save has, and for each the data version
//! that saved it, its status and how long it was played, so a save can say how much of it older
//! versions made and show what's generated.
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
use spyglass_data::nbt::{self, RootField};

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
        for (version, count) in other.by_version {
            *self.by_version.entry(version).or_default() += count;
        }
        self.unversioned += other.unversioned;
        self.unreadable += other.unreadable;
        self.unreadable_files += other.unreadable_files;
    }
}

/// What a chunk's root holds that the scan keeps (`SerializableChunkData`), each `None` where
/// the chunk doesn't have it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChunkFields {
    /// `DataVersion`: the version that last saved it. Chunks saved before 1.9 have none.
    pub data_version: Option<i32>,
    /// `Status`, e.g. `minecraft:full`: how far generation got.
    pub status: Option<String>,
    /// `InhabitedTime`: ticks players spent nearby, summed over players.
    pub inhabited_time: Option<i64>,
}

impl ChunkFields {
    /// Whether generation finished: its status is `full`, namespaced or not (versions before
    /// 1.18 wrote it bare).
    pub fn is_full(&self) -> bool {
        matches!(self.status.as_deref(), Some("minecraft:full" | "full"))
    }
}

/// One chunk a region file's header lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChunkRecord {
    /// Read, with its root's fields.
    Read(ChunkFields),
    /// Listed but unreadable: LZ4 or unknown compression, a missing `.mcc` file, or malformed
    /// data.
    Unreadable,
}

/// One region file's chunks, by their place in its header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionIndex {
    /// The file.
    pub path: PathBuf,
    /// Its region coordinates, from its name; `None` if the name doesn't parse, so the game
    /// wouldn't read it.
    pub region: Option<[i32; 2]>,
    /// Each chunk it lists, by header index (`(x & 31) + (z & 31) · 32`), in order.
    pub chunks: Vec<(u16, ChunkRecord)>,
}

impl RegionIndex {
    /// The chunk position of header index `index`, for a file whose name parses.
    pub fn chunk_pos(&self, index: u16) -> Option<[i32; 2]> {
        let [region_x, region_z] = self.region?;
        Some(local_to_chunk(region_x, region_z, usize::from(index)))
    }

    fn versions(&self) -> ChunkVersions {
        let mut counts = ChunkVersions::default();
        for (_, record) in &self.chunks {
            match record {
                ChunkRecord::Read(ChunkFields {
                    data_version: Some(v),
                    ..
                }) => *counts.by_version.entry(*v).or_default() += 1,
                ChunkRecord::Read(_) => counts.unversioned += 1,
                ChunkRecord::Unreadable => counts.unreadable += 1,
            }
        }
        counts
    }
}

/// Every chunk in one dimension's region files, as [`index`] found them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChunkIndex {
    /// Each region file read, sorted by path.
    pub regions: Vec<RegionIndex>,
    /// Region files that couldn't be opened or read, whose chunks aren't listed.
    pub unreadable_files: usize,
    /// Where each region is in [`regions`](Self::regions), by its coordinates.
    by_region: BTreeMap<[i32; 2], usize>,
}

impl ChunkIndex {
    fn new(mut regions: Vec<RegionIndex>, unreadable_files: usize) -> Self {
        regions.sort_by(|a, b| a.path.cmp(&b.path));
        let by_region = regions
            .iter()
            .enumerate()
            .filter_map(|(i, r)| Some((r.region?, i)))
            .collect();
        ChunkIndex {
            regions,
            unreadable_files,
            by_region,
        }
    }

    /// The region file holding chunk `[x, z]`, if the dimension has it.
    pub fn region_of(&self, [x, z]: [i32; 2]) -> Option<&RegionIndex> {
        let i = *self.by_region.get(&[x >> 5, z >> 5])?;
        Some(&self.regions[i])
    }

    /// Chunk `[x, z]`'s record, if a region file lists it.
    pub fn chunk(&self, pos: [i32; 2]) -> Option<&ChunkRecord> {
        let region = self.region_of(pos)?;
        let index = header_index(pos) as u16;
        let at = region
            .chunks
            .binary_search_by_key(&index, |(i, _)| *i)
            .ok()?;
        Some(&region.chunks[at].1)
    }

    /// Every listed chunk in a region file whose name parses, with its position, region by
    /// region.
    pub fn chunks(&self) -> impl Iterator<Item = ([i32; 2], &ChunkRecord)> {
        self.regions.iter().flat_map(|region| {
            region
                .chunks
                .iter()
                .filter_map(|(index, record)| Some((region.chunk_pos(*index)?, record)))
        })
    }

    /// The chunks counted by data version, every region file included.
    pub fn versions(&self) -> ChunkVersions {
        let mut total = ChunkVersions {
            unreadable_files: self.unreadable_files,
            ..ChunkVersions::default()
        };
        for region in &self.regions {
            total.merge(region.versions());
        }
        total
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

/// Reads every region file in `dir`, in parallel, recording each chunk. A file that can't be
/// read is counted in [`ChunkIndex::unreadable_files`], not an error.
pub fn index(dir: &Path) -> std::io::Result<ChunkIndex> {
    let files = region_files(dir);
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let files_per_thread = files.len().div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        let handles: Vec<_> = files
            .chunks(files_per_thread)
            .map(|part| {
                scope.spawn(move || {
                    let mut regions = Vec::new();
                    let mut unreadable_files = 0;
                    for file in part {
                        match index_file(file) {
                            Ok(region) => regions.push(region),
                            Err(_) => unreadable_files += 1,
                        }
                    }
                    (regions, unreadable_files)
                })
            })
            .collect();
        let mut regions = Vec::new();
        let mut unreadable_files = 0;
        for handle in handles {
            let (part, unreadable) = handle
                .join()
                .map_err(|_| std::io::Error::other("a region file reader panicked"))?;
            regions.extend(part);
            unreadable_files += unreadable;
        }
        Ok(ChunkIndex::new(regions, unreadable_files))
    })
}

/// Counts the chunks in every region file in `dir` by data version: [`index`]'s counts.
pub fn chunk_versions(dir: &Path) -> std::io::Result<ChunkVersions> {
    Ok(index(dir)?.versions())
}

/// Counts one region file's chunks by data version: [`index_file`]'s counts.
pub fn file_versions(path: &Path) -> std::io::Result<ChunkVersions> {
    Ok(index_file(path)?.versions())
}

/// Reads one region file's chunks. Each chunk is read on its own, at most its 255 sectors
/// (about 1 MiB), so a large file doesn't need the memory to hold it.
pub fn index_file(path: &Path) -> std::io::Result<RegionIndex> {
    let mut region = RegionIndex {
        path: path.to_path_buf(),
        region: region_coords(path),
        chunks: Vec::new(),
    };
    let mut file = File::open(path)?;
    let mut header = Vec::with_capacity(4096);
    (&mut file).take(4096).read_to_end(&mut header)?;
    if header.len() < 4096 {
        return Ok(region);
    }
    let mut buffer = Vec::new();
    for index in 0..1024 {
        let entry = location(&header, index);
        if entry == 0 {
            continue;
        }
        let record = chunk_data(path, &mut file, &mut buffer, entry, index)
            .and_then(|raw| chunk_fields(&raw))
            .map_or(ChunkRecord::Unreadable, ChunkRecord::Read);
        region.chunks.push((index as u16, record));
    }
    Ok(region)
}

/// Chunk `pos`'s NBT, uncompressed, read again from `region`'s file: `None` if the file no
/// longer lists it or it can't be read, under the scan's rules.
pub fn read_chunk(region: &RegionIndex, pos: [i32; 2]) -> Option<Vec<u8>> {
    let index = header_index(pos);
    let mut file = File::open(&region.path).ok()?;
    let mut header = vec![0; 4096];
    file.read_exact(&mut header).ok()?;
    let entry = location(&header, index);
    if entry == 0 {
        return None;
    }
    chunk_data(&region.path, &mut file, &mut Vec::new(), entry, index)
}

/// Chunk `[x, z]`'s place in its region file's header, x fastest.
fn header_index([x, z]: [i32; 2]) -> usize {
    ((x & 31) + (z & 31) * 32) as usize
}

/// Location `index` of a region file's 4 KiB header.
fn location(header: &[u8], index: usize) -> u32 {
    u32::from_be_bytes(
        header[index * 4..index * 4 + 4]
            .try_into()
            .expect("4 bytes"),
    )
}

/// The chunk at header index `index` of region `(region_x, region_z)`.
fn local_to_chunk(region_x: i32, region_z: i32, index: usize) -> [i32; 2] {
    [
        region_x.wrapping_mul(32).wrapping_add((index % 32) as i32),
        region_z.wrapping_mul(32).wrapping_add((index / 32) as i32),
    ]
}

/// The fields the scan keeps from a chunk's uncompressed NBT: `None` if it's malformed.
fn chunk_fields(raw: &[u8]) -> Option<ChunkFields> {
    let [data_version, status, inhabited_time] =
        nbt::read_root_fields(raw, ["DataVersion", "Status", "InhabitedTime"]).ok()?;
    Some(ChunkFields {
        data_version: match data_version {
            Some(RootField::Int(v)) => Some(v),
            _ => None,
        },
        status: match status {
            Some(RootField::String(s)) => Some(s),
            _ => None,
        },
        inhabited_time: match inhabited_time {
            Some(RootField::Long(t)) => Some(t),
            _ => None,
        },
    })
}

/// The uncompressed NBT of the chunk at location `entry` (`index` in the header, x fastest),
/// read through `buffer`: `None` if the chunk can't be read.
fn chunk_data(
    path: &Path,
    file: &mut File,
    buffer: &mut Vec<u8>,
    entry: u32,
    index: usize,
) -> Option<Vec<u8>> {
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
        let [region_x, region_z] = region_coords(path)?;
        let [x, z] = local_to_chunk(region_x, region_z, index);
        external = read_capped(&path.with_file_name(format!("c.{x}.{z}.mcc")), MAX_CHUNK).ok()?;
        &external[..]
    } else {
        // Truncated if it runs past what was read; a negative length is malformed.
        let payload_len = usize::try_from(length.wrapping_sub(1)).ok()?;
        buffer.get(5..5 + payload_len)?
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
    Some(raw)
}

/// `[x, z]` from a file named `r.<x>.<z>.mca`.
fn region_coords(path: &Path) -> Option<[i32; 2]> {
    let name = path.file_name()?.to_str()?;
    let mut parts = name.strip_prefix("r.")?.strip_suffix(".mca")?.split('.');
    Some([parts.next()?.parse().ok()?, parts.next()?.parse().ok()?])
}
