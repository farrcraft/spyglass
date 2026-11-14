//! One chunk's NBT, read whole: the structure starts it holds and its block entities
//! (`SerializableChunkData`), and the containers in an entity chunk (`entities/`). See
//! `docs/specs/world-saves.md`, "Structure starts in saves" and "Containers in saves".

use spyglass_data::nbt::{self, NbtError, Tag};

/// The most memory one chunk's tree may take. A chunk is at most 32 MiB uncompressed
/// (`region`'s limit), and a tree takes several times its bytes.
const MAX_CHUNK_TREE: u64 = 256 << 20;

/// The data version from which starts are keyed by structure (`minecraft:village_plains`), as
/// the registry names them: `StructuresBecomeConfiguredFix`'s schema, 1.18.2's. Chunks saved
/// before it (1.18.0 and 1.18.1 among them) key starts by structure feature (`minecraft:village`,
/// `minecraft:ocean_ruin`), which that fix renames by the chunk's biomes. A fixed historical
/// version, the same for every game version.
const CONFIGURED_STRUCTURES: i32 = 2970;

/// The data version from which suspicious sand's block entity is `minecraft:brushable_block`:
/// the schema of `DataFixers`' "Rename Suspicious Sand to Brushable Block"
/// (`BlockEntityRenameFix`), 1.20's. Chunks saved before it, by 1.19.4 with its 1.20 features
/// on, name it `minecraft:suspicious_sand`. The only block entity renamed since 1.18 (26.3's
/// `DataFixers`).
const BRUSHABLE_BLOCK: i32 = 3438;

/// What a chunk says about the structures starting in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SavedStarts {
    /// No `structures.starts`: the game predicts this chunk's starts from the seed.
    Missing,
    /// The chunk's starts, by structure, `INVALID` ones left out. Empty if none start here.
    Starts(Vec<SavedStart>),
    /// Starts the game data-fixes to read, and spyglass doesn't: saved before 1.18
    /// (`Level.Structures.Starts`), or before 1.18.2 renamed them (data version 2970, or none
    /// saved). The game's answer comes from the fixed starts, so spyglass predicts them.
    Legacy,
}

/// A structure start a chunk holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedStart {
    /// The structure, e.g. `minecraft:village_plains`: the key in `starts`.
    pub structure: String,
    /// `id`: the structure the start loads as, normally the key. The game keeps the start only
    /// if both name a structure its registry has (`SerializableChunkData.unpackStructureStart`,
    /// `StructureStart.loadStaticStart`).
    pub id: String,
    /// `ChunkX` and `ChunkZ`: its start chunk, 0 where missing, as the game reads them.
    pub chunk: [i32; 2],
    /// `references`: how many exploration maps have pointed at it.
    pub references: i32,
    /// `Children`: the pieces placed.
    pub pieces: Vec<SavedPiece>,
}

/// One piece of a saved structure start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedPiece {
    /// The piece type, e.g. `minecraft:jigsaw`.
    pub id: String,
    /// `BB`: its box, min x, y, z then max x, y, z, corners included.
    pub bounds: [i32; 6],
}

/// A block entity or entity with an inventory or a loot table.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedContainer {
    /// The block entity's or entity's id, e.g. `minecraft:chest`, `minecraft:chest_minecart`.
    pub id: String,
    /// Its block (`x`, `y`, `z`), or an entity's `Pos`, rounded down.
    pub position: [i32; 3],
    /// `LootTable`: present until the container is first filled.
    pub loot_table: Option<String>,
    /// `LootTableSeed`, absent when 0 (a seed drawn when it's filled).
    pub loot_table_seed: Option<i64>,
    /// `Items`: what it holds now.
    pub items: Vec<SavedStack>,
}

/// A stack in a saved container.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedStack {
    /// `Slot`.
    pub slot: i32,
    /// The item, e.g. `minecraft:diamond`.
    pub id: String,
    /// `count`, or `Count` before 1.20.5; 1 if neither.
    pub count: i32,
    /// `components`, or `tag` before 1.20.5, as saved.
    pub components: Option<Tag>,
}

/// What [`read`] finds in a chunk.
#[derive(Debug, Clone, PartialEq)]
pub struct ChunkContents {
    /// `DataVersion`.
    pub data_version: Option<i32>,
    /// `Status`, e.g. `minecraft:full`.
    pub status: Option<String>,
    /// Its structure starts.
    pub starts: SavedStarts,
    /// Its block entities, each as a container whether it has an inventory or not, so one
    /// emptied or never filled is still found; `None` for a chunk saved before 1.18,
    /// whose block entities are in `Level.TileEntities`, in older formats.
    pub containers: Option<Vec<SavedContainer>>,
    /// The containers among a proto chunk's own `entities`, such as chest minecarts: a chunk
    /// keeps its entities until it's full, and then they move to `entities/`
    /// (`SerializableChunkData`). Empty for a full chunk.
    pub entities: Vec<SavedContainer>,
}

impl ChunkContents {
    /// Whether its features have been placed, structures' pieces and their containers among
    /// them: a status of `features` or later (`ChunkStatus`), namespaced or not. 1.18 to 1.19
    /// end with `light`, `spawn`, `heightmaps`, `full`; 1.20 dropped `heightmaps` and added
    /// `initialize_light` before `light`.
    pub fn has_features(&self) -> bool {
        let status = self.status.as_deref().unwrap_or_default();
        matches!(
            status.strip_prefix("minecraft:").unwrap_or(status),
            "features" | "initialize_light" | "light" | "spawn" | "heightmaps" | "full"
        )
    }

    /// Whether generation finished (status `full`), so its entities are in `entities/`.
    pub fn is_full(&self) -> bool {
        let status = self.status.as_deref().unwrap_or_default();
        status.strip_prefix("minecraft:").unwrap_or(status) == "full"
    }
}

/// Reads a chunk's uncompressed NBT. Malformed fields are left out, not errors: the game would
/// skip them too, or fail to load the chunk.
pub fn read(raw: &[u8]) -> Result<ChunkContents, NbtError> {
    let (_, root) = nbt::read_limited(raw, MAX_CHUNK_TREE)?;
    let data_version = int(&root, "DataVersion");
    let legacy = root.get("Level").is_some();
    // The game reads a missing data version as -1, so every fix applies.
    let feature_keys = data_version.is_none_or(|v| v < CONFIGURED_STRUCTURES);
    let starts = if legacy || feature_keys {
        SavedStarts::Legacy
    } else {
        match root.get("structures").and_then(|s| s.get("starts")) {
            Some(Tag::Compound(starts)) => SavedStarts::Starts(
                starts
                    .iter()
                    .filter_map(|(structure, start)| saved_start(structure, start))
                    .collect(),
            ),
            _ => SavedStarts::Missing,
        }
    };
    let rename_sand = data_version.is_none_or(|v| v < BRUSHABLE_BLOCK);
    let containers = (!legacy).then(|| {
        list(&root, "block_entities")
            .iter()
            .filter_map(|e| {
                let position = [int(e, "x")?, int(e, "y")?, int(e, "z")?];
                let mut found = container(e, position, true)?;
                if rename_sand && found.id == "minecraft:suspicious_sand" {
                    found.id = "minecraft:brushable_block".to_string();
                }
                Some(found)
            })
            .collect()
    });
    Ok(ChunkContents {
        data_version,
        status: root.get("Status").and_then(Tag::as_str).map(str::to_string),
        starts,
        containers,
        entities: entity_containers(list(&root, "entities")),
    })
}

/// Reads an entity chunk's uncompressed NBT (`entities/`, since 1.17): its entities with a loot
/// table or items, such as chest minecarts.
pub fn read_entities(raw: &[u8]) -> Result<Vec<SavedContainer>, NbtError> {
    let (_, root) = nbt::read_limited(raw, MAX_CHUNK_TREE)?;
    Ok(entity_containers(list(&root, "Entities")))
}

/// The entities among `entities` with a loot table or items, each at its `Pos` rounded down.
fn entity_containers(entities: &[Tag]) -> Vec<SavedContainer> {
    entities
        .iter()
        .filter_map(|e| {
            let pos = list(e, "Pos");
            let coord = |i: usize| pos.get(i)?.as_double().map(|v| v.floor() as i32);
            container(e, [coord(0)?, coord(1)?, coord(2)?], false)
        })
        .collect()
}

/// A start as `StructureStart.loadStaticStart` reads it, unless it's `INVALID` (no structure
/// here) or has no `id` (read as `""`, which names no structure, so the game skips it too).
fn saved_start(structure: &str, start: &Tag) -> Option<SavedStart> {
    let id = start.get("id")?.as_str()?;
    if id == "INVALID" {
        return None;
    }
    Some(SavedStart {
        structure: structure.to_string(),
        id: id.to_string(),
        chunk: [
            int(start, "ChunkX").unwrap_or(0),
            int(start, "ChunkZ").unwrap_or(0),
        ],
        references: int(start, "references").unwrap_or(0),
        pieces: list(start, "Children")
            .iter()
            .filter_map(|piece| {
                let bounds = match piece.get("BB")? {
                    Tag::IntArray(b) => <[i32; 6]>::try_from(b.as_slice()).ok()?,
                    _ => return None,
                };
                Some(SavedPiece {
                    id: piece.get("id")?.as_str()?.to_string(),
                    bounds,
                })
            })
            .collect(),
    })
}

/// `entity` at `position` as a container: any block entity (`any`), or an entity with a loot
/// table or items. A decorated pot holds one stack, as `item`.
fn container(entity: &Tag, position: [i32; 3], any: bool) -> Option<SavedContainer> {
    let loot_table = entity
        .get("LootTable")
        .and_then(Tag::as_str)
        .map(str::to_string);
    let items = entity.get("Items").and_then(Tag::as_list);
    let pot = entity.get("item").and_then(|item| {
        Some(SavedStack {
            slot: 0,
            id: item.get("id")?.as_str()?.to_string(),
            count: int(item, "count").unwrap_or(1),
            components: item.get("components").cloned(),
        })
    });
    if !any && loot_table.is_none() && items.is_none() {
        return None;
    }
    Some(SavedContainer {
        id: entity.get("id")?.as_str()?.to_string(),
        position,
        loot_table,
        loot_table_seed: entity
            .get("LootTableSeed")
            .and_then(Tag::as_long)
            .filter(|&s| s != 0),
        items: items
            .unwrap_or_default()
            .iter()
            .filter_map(stack)
            .chain(pot)
            .collect(),
    })
}

fn stack(item: &Tag) -> Option<SavedStack> {
    Some(SavedStack {
        slot: int(item, "Slot")?,
        id: item.get("id")?.as_str()?.to_string(),
        count: int(item, "count")
            .or_else(|| int(item, "Count"))
            .unwrap_or(1),
        components: item.get("components").or_else(|| item.get("tag")).cloned(),
    })
}

fn int(tag: &Tag, name: &str) -> Option<i32> {
    tag.get(name)?.as_int()
}

fn list<'t>(tag: &'t Tag, name: &str) -> &'t [Tag] {
    tag.get(name).and_then(Tag::as_list).unwrap_or_default()
}
