//! What a save already holds, for a session's answers: its generated chunks, and the structure
//! starts in them (ADR 0071; `docs/specs/world-saves.md`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use spyglass_structure::{StoredStarts, StructureSets};
use spyglass_world::chunk::{self, ChunkContents, SavedStarts};
use spyglass_world::region::{self, ChunkIndex, ChunkRecord};

use crate::save::SaveReport;
use crate::{Dimension, Error, Result};

/// One dimension of a save: which chunks it has generated, read whole on demand. Made with
/// [`SaveReport::chunks`] and given to a session with
/// [`Session::with_save`](crate::Session::with_save), whose answers then come from the save
/// wherever the game's would. The save is read as it was: a game running on it changes it
/// underneath.
#[derive(Debug)]
pub struct SaveChunks {
    /// Which dimension.
    dimension: Dimension,
    /// The save's data version: chunks saved by an older one are marked.
    data_version: Option<i32>,
    /// Its region files' chunks.
    index: Arc<ChunkIndex>,
    /// Its entity region files' folder.
    #[expect(
        dead_code,
        reason = "read for chest minecarts, with containers' save state"
    )]
    entities: PathBuf,
    /// Chunks read so far: `None` for one that isn't held or can't be read.
    read: Mutex<HashMap<[i32; 2], Option<Arc<ChunkContents>>>>,
}

/// A chunk a save has generated, for [`SaveChunks::generated_in`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct GeneratedChunk {
    /// The chunk.
    pub chunk: [i32; 2],
    /// Whether generation finished (status `full`); otherwise it's a proto chunk, generated
    /// partway, as the edges of explored land are.
    pub full: bool,
    /// Whether a version older than the save's last saved it, so its terrain and structures
    /// are that version's. Chunks without a data version (before 1.9) count.
    pub older: bool,
    /// Ticks players spent near it (`InhabitedTime`), summed over players, where saved.
    pub inhabited_time: Option<i64>,
    /// Whether it could be read at all; an unreadable chunk is neither full nor older.
    pub readable: bool,
}

impl SaveReport {
    /// The save's `dimension`, for [`Session::with_save`](crate::Session::with_save). Uses the
    /// chunks [`Scan::Chunks`](crate::Scan::Chunks) found; after a quick scan, reads the
    /// dimension's region files now.
    pub fn chunks(&self, dimension: Dimension) -> Result<SaveChunks> {
        let folders = &self.dimensions[dimension.index()];
        let index = match &folders.index {
            Some(index) => Arc::clone(index),
            None => Arc::new(region::index(&folders.region).map_err(|source| Error::Io {
                path: folders.region.clone(),
                source,
            })?),
        };
        Ok(SaveChunks {
            dimension,
            data_version: self.save.data_version,
            index,
            entities: folders.entities.clone(),
            read: Mutex::new(HashMap::new()),
        })
    }
}

impl SaveChunks {
    /// Which dimension it is.
    pub fn dimension(&self) -> Dimension {
        self.dimension
    }

    /// How many chunks the save has in this dimension, readable or not.
    pub fn len(&self) -> usize {
        self.index.regions.iter().map(|r| r.chunks.len()).sum()
    }

    /// Whether it has none.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Every chunk the save has in `min..=max` (chunk coordinates, inclusive), z outer and x
    /// inner.
    pub fn generated_in(&self, min: [i32; 2], max: [i32; 2]) -> Vec<GeneratedChunk> {
        let mut out = Vec::new();
        if max[0] < min[0] || max[1] < min[1] {
            return out;
        }
        // Region by region, so a large area costs what the save holds there, not its size.
        for region_z in (min[1] >> 5)..=(max[1] >> 5) {
            for region_x in (min[0] >> 5)..=(max[0] >> 5) {
                let Some(region) = self.index.region_of([region_x << 5, region_z << 5]) else {
                    continue;
                };
                for (index, record) in &region.chunks {
                    let chunk = region.chunk_pos(*index).expect("an indexed region parses");
                    if (min[0]..=max[0]).contains(&chunk[0])
                        && (min[1]..=max[1]).contains(&chunk[1])
                    {
                        out.push(self.generated(chunk, record));
                    }
                }
            }
        }
        out.sort_by_key(|c| [c.chunk[1], c.chunk[0]]);
        out
    }

    fn generated(&self, chunk: [i32; 2], record: &ChunkRecord) -> GeneratedChunk {
        match record {
            ChunkRecord::Read(fields) => GeneratedChunk {
                chunk,
                full: fields.is_full(),
                older: match (fields.data_version, self.data_version) {
                    (Some(chunk), Some(save)) => chunk < save,
                    (None, _) => true,
                    (Some(_), None) => false,
                },
                inhabited_time: fields.inhabited_time,
                readable: true,
            },
            ChunkRecord::Unreadable => GeneratedChunk {
                chunk,
                full: false,
                older: false,
                inhabited_time: None,
                readable: false,
            },
        }
    }

    /// Chunk `chunk` read whole, once: `None` if the save doesn't hold it, or it can't be read.
    pub(crate) fn contents(&self, chunk: [i32; 2]) -> Option<Arc<ChunkContents>> {
        let mut read = self.read.lock().expect("no reader panics holding the lock");
        if let Some(contents) = read.get(&chunk) {
            return contents.clone();
        }
        let contents = self.read_chunk(chunk).map(Arc::new);
        read.insert(chunk, contents.clone());
        contents
    }

    fn read_chunk(&self, chunk: [i32; 2]) -> Option<ChunkContents> {
        let ChunkRecord::Read(_) = self.index.chunk(chunk)? else {
            return None;
        };
        let raw = region::read_chunk(self.index.region_of(chunk)?, chunk)?;
        chunk::read(&raw).ok()
    }

    /// The structures starting in `chunk` by id, if the save holds it with starts the game
    /// reads: current-format `structures.starts` (ADR 0071).
    pub(crate) fn saved_starts(&self, chunk: [i32; 2]) -> Option<Vec<String>> {
        match &self.contents(chunk)?.starts {
            SavedStarts::Starts(starts) => {
                Some(starts.iter().map(|s| s.structure.clone()).collect())
            }
            SavedStarts::Missing | SavedStarts::Legacy => None,
        }
    }
}

/// A save's starts as a world's structure sets number them, for [`spyglass_structure::Starts`].
pub(crate) struct SessionSave<'w> {
    /// The save's chunks.
    pub(crate) chunks: Arc<SaveChunks>,
    /// The session's world's structures.
    pub(crate) sets: &'w StructureSets,
}

impl StoredStarts for SessionSave<'_> {
    fn starts_at(&self, chunk: [i32; 2]) -> Option<Vec<usize>> {
        // An id this world doesn't have is dropped, as the game drops one its registry lacks.
        let saved = self.chunks.saved_starts(chunk)?;
        Some(
            saved
                .iter()
                .filter_map(|id| self.sets.structure_index(id))
                .collect(),
        )
    }
}
