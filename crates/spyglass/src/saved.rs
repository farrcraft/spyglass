//! What a save already holds, for a session's answers: its generated chunks, and the structure
//! starts in them (ADR 0071; `docs/specs/world-saves.md`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use spyglass_data::codec::full_id;
use spyglass_structure::{StoredStarts, StructureSets};
use spyglass_world::chunk::{self, ChunkContents, SavedContainer, SavedStarts};
use spyglass_world::region::{self, ChunkIndex, ChunkRecord};

use crate::save::SaveReport;
use crate::{Dimension, InSave, Item, Slot};

/// How many chunks a [`ReadChunks`] keeps before it starts over: enough for any area a query
/// reads at once, so a long search across a large save doesn't grow without bound.
const MAX_READ_CHUNKS: usize = 16_384;

/// Chunks read so far, by position: `None` for one the save doesn't hold, or that can't be
/// read. Each chunk has its own cell, so the lock is held only to find it: chunks are read in
/// parallel, and a chunk two threads ask for at once is read once.
#[derive(Debug)]
struct ReadChunks<T> {
    cells: Mutex<HashMap<[i32; 2], Arc<ReadCell<T>>>>,
}

type ReadCell<T> = OnceLock<Option<Arc<T>>>;

impl<T> ReadChunks<T> {
    fn new() -> Self {
        ReadChunks {
            cells: Mutex::new(HashMap::new()),
        }
    }

    /// `chunk`, read by `read` the first time it's asked for. A `read` that panics leaves the
    /// chunk unread, for the next caller to try again.
    fn get(&self, chunk: [i32; 2], read: impl FnOnce() -> Option<T>) -> Option<Arc<T>> {
        let cell = {
            // The map is only ever left whole, so a panic elsewhere doesn't spoil it.
            let mut cells = self.cells.lock().unwrap_or_else(PoisonError::into_inner);
            if cells.len() >= MAX_READ_CHUNKS && !cells.contains_key(&chunk) {
                cells.clear();
            }
            Arc::clone(cells.entry(chunk).or_default())
        };
        cell.get_or_init(|| read().map(Arc::new)).clone()
    }
}

/// One dimension of a save: which chunks it has generated, each read whole when a query first
/// needs it. Made with [`SaveReport::chunks`] and given to a session with
/// [`Session::with_save`](crate::Session::with_save), whose answers then come from the save
/// wherever the game's would. The save is read as it was: a game running on it changes it
/// underneath.
#[derive(Debug)]
pub struct SaveChunks {
    /// Which dimension.
    dimension: Dimension,
    /// The save's data version: chunks saved by an older one are marked.
    data_version: Option<i32>,
    /// Its region files' folder, which chunks are read from directly.
    region: PathBuf,
    /// Its region files' chunks: [`Scan::Chunks`](crate::Scan::Chunks)'s, or read the first
    /// time they're asked for, since structure answers don't need them.
    index: OnceLock<Arc<ChunkIndex>>,
    /// Its entity region files' folder.
    entities: PathBuf,
    /// Chunks read so far: `None` for one that isn't held or can't be read.
    read: ReadChunks<ChunkContents>,
    /// Entity chunks read so far, their containers: `None` likewise.
    entities_read: ReadChunks<Vec<SavedContainer>>,
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
    /// The save's `dimension`, for [`Session::with_save`](crate::Session::with_save). Keeps
    /// the chunks [`Scan::Chunks`](crate::Scan::Chunks) found; after a quick scan, which chunks
    /// exist is read the first time it's asked for.
    pub fn chunks(&self, dimension: Dimension) -> SaveChunks {
        let folders = &self.dimensions[dimension.index()];
        let index = OnceLock::new();
        if let Some(scanned) = &folders.index {
            index.set(Arc::clone(scanned)).expect("a new cell is empty");
        }
        SaveChunks {
            dimension,
            data_version: self.save.data_version,
            region: folders.region.clone(),
            index,
            entities: folders.entities.clone(),
            read: ReadChunks::new(),
            entities_read: ReadChunks::new(),
        }
    }
}

impl SaveChunks {
    /// Which dimension it is.
    pub fn dimension(&self) -> Dimension {
        self.dimension
    }

    /// The dimension's chunks, read now if no scan found them. A failure to read the folder
    /// leaves it empty, as a missing folder does.
    fn index(&self) -> &ChunkIndex {
        self.index
            .get_or_init(|| Arc::new(region::index(&self.region).unwrap_or_default()))
    }

    /// How many chunks the save has in this dimension, readable or not.
    pub fn len(&self) -> usize {
        self.index().regions.iter().map(|r| r.chunks.len()).sum()
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
                let Some(region) = self.index().region_of([region_x << 5, region_z << 5]) else {
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
        self.read.get(chunk, || {
            chunk::read(&region::read_chunk(&self.region, chunk)?).ok()
        })
    }

    /// Entity chunk `chunk`'s containers, read once: `None` if the save has no entities saved
    /// there, or they can't be read.
    fn entity_containers(&self, chunk: [i32; 2]) -> Option<Arc<Vec<SavedContainer>>> {
        self.entities_read.get(chunk, || {
            region::read_chunk(&self.entities, chunk)
                .and_then(|raw| chunk::read_entities(&raw).ok())
        })
    }

    /// How the save holds a predicted container: `holder` at `position`, filled from `table`
    /// with `seed`. `None` where the save hasn't placed the features of the chunk holding it,
    /// or (for a minecart in a full chunk) saved no entities there. A minecart in a proto chunk
    /// is among the chunk's own entities.
    ///
    /// `block_entity` is a block holder's block-entity type, where the bundle knows it: a
    /// block entity of another type at `position` (a furnace where the chest was) means the
    /// container is gone. Without it, any block entity there is taken as the container.
    pub(crate) fn container(
        &self,
        position: [i32; 3],
        holder: &str,
        block_entity: Option<&str>,
        table: &str,
        seed: Option<i64>,
    ) -> Option<InSave> {
        let chunk = [position[0] >> 4, position[2] >> 4];
        let contents = self.contents(chunk);
        let entities;
        let found = if holder.ends_with("_minecart") {
            let held = match contents.as_deref() {
                Some(proto) if !proto.is_full() => {
                    if !proto.has_features() {
                        return None;
                    }
                    &proto.entities
                }
                _ => {
                    entities = self.entity_containers(chunk)?;
                    &*entities
                }
            };
            held.iter()
                .find(|c| c.position == position && c.id == holder)
        } else {
            let placed = contents.as_deref()?;
            if !placed.has_features() {
                return None;
            }
            placed
                .containers
                .as_ref()?
                .iter()
                .find(|c| c.position == position)
                .filter(|c| block_entity.is_none_or(|expected| full_id(&c.id) == expected))
        };
        Some(match found {
            None => InSave::Gone,
            Some(saved) => match &saved.loot_table {
                // A seed of 0 isn't saved: it's drawn when the container fills.
                Some(saved_table) => InSave::Unopened {
                    as_predicted: saved_table == table
                        && saved.loot_table_seed == seed.filter(|&s| s != 0),
                },
                None => InSave::Filled {
                    items: saved
                        .items
                        .iter()
                        .filter_map(|stack| {
                            Some(Slot {
                                slot: usize::try_from(stack.slot).ok()?,
                                item: Item::saved(stack),
                            })
                        })
                        .collect(),
                },
            },
        })
    }

    /// The structures starting in `chunk`, each as its key and its `id`, if the save holds it
    /// with starts the game reads: current-format `structures.starts` (ADR 0071).
    pub(crate) fn saved_starts(&self, chunk: [i32; 2]) -> Option<Vec<(String, String)>> {
        match &self.contents(chunk)?.starts {
            SavedStarts::Starts(starts) => Some(
                starts
                    .iter()
                    .map(|s| (s.structure.clone(), s.id.clone()))
                    .collect(),
            ),
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
        // A key or id this world doesn't have is dropped, as the game drops one its registry
        // lacks.
        let saved = self.chunks.saved_starts(chunk)?;
        Some(
            saved
                .iter()
                .filter(|(_, id)| self.sets.structure_index(id).is_some())
                .filter_map(|(key, _)| self.sets.structure_index(key))
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use super::*;

    #[test]
    fn chunks_are_read_once_and_in_parallel() {
        let chunks = ReadChunks::<usize>::new();
        let reads = AtomicUsize::new(0);
        let in_flight = AtomicUsize::new(0);
        let most_in_flight = AtomicUsize::new(0);
        let read = |value: usize| {
            reads.fetch_add(1, Ordering::SeqCst);
            let now = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            most_in_flight.fetch_max(now, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(50));
            in_flight.fetch_sub(1, Ordering::SeqCst);
            Some(value)
        };
        // Eight threads on one chunk: one read, the same answer for all.
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| assert_eq!(chunks.get([0, 0], || read(7)).as_deref(), Some(&7)));
            }
        });
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        // Eight chunks at once: read side by side, not one after another.
        std::thread::scope(|scope| {
            for i in 1..=8 {
                let (chunks, read) = (&chunks, &read);
                scope.spawn(move || chunks.get([i, 0], || read(i as usize)));
            }
        });
        assert_eq!(reads.load(Ordering::SeqCst), 9);
        assert!(most_in_flight.load(Ordering::SeqCst) > 1);
    }

    #[test]
    fn a_panicking_read_leaves_the_chunk_for_the_next() {
        let chunks = ReadChunks::<i32>::new();
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            chunks.get([1, 2], || panic!("a reader failing"))
        }));
        assert!(panicked.is_err());
        assert_eq!(chunks.get([1, 2], || Some(5)).as_deref(), Some(&5));
        assert_eq!(chunks.get([3, 4], || None), None);
    }

    #[test]
    fn a_block_entity_of_another_type_is_gone() {
        let block_entity = |id: &str, x: i32| SavedContainer {
            id: id.to_string(),
            position: [x, 64, 0],
            loot_table: Some("minecraft:chests/simple_dungeon".to_string()),
            loot_table_seed: Some(5),
            items: Vec::new(),
        };
        let save = SaveChunks {
            dimension: Dimension::Overworld,
            data_version: None,
            region: PathBuf::new(),
            index: OnceLock::new(),
            entities: PathBuf::new(),
            read: ReadChunks::new(),
            entities_read: ReadChunks::new(),
        };
        save.read.get([0, 0], || {
            Some(ChunkContents {
                data_version: Some(4671),
                status: Some("minecraft:full".to_string()),
                starts: SavedStarts::Missing,
                containers: Some(vec![
                    block_entity("minecraft:chest", 1),
                    // A player's furnace where a chest was predicted.
                    block_entity("furnace", 2),
                ]),
                entities: Vec::new(),
            })
        });
        let table = "minecraft:chests/simple_dungeon";
        let at = |x: i32, block_entity: Option<&str>| {
            save.container([x, 64, 0], "minecraft:chest", block_entity, table, Some(5))
        };
        let unopened = Some(InSave::Unopened { as_predicted: true });
        assert_eq!(at(1, Some("minecraft:chest")), unopened);
        assert_eq!(at(2, Some("minecraft:chest")), Some(InSave::Gone));
        // A bundle whose block report doesn't say: any block entity there is taken.
        assert_eq!(at(2, None), unopened);
        assert_eq!(at(3, Some("minecraft:chest")), Some(InSave::Gone));
    }

    #[test]
    fn the_cache_starts_over_when_full() {
        let chunks = ReadChunks::<i32>::new();
        for x in 0..MAX_READ_CHUNKS as i32 {
            chunks.get([x, 0], || Some(x));
        }
        assert_eq!(chunks.cells.lock().unwrap().len(), MAX_READ_CHUNKS);
        chunks.get([-1, 0], || Some(-1));
        assert_eq!(chunks.cells.lock().unwrap().len(), 1);
    }
}
