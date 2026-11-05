//! Loot: a container's contents from its loot table and loot seed, slot by slot, as the game
//! fills it when the container is first opened (`LootTable.fill`). See `docs/specs/loot.md` and
//! ADR 0060.
//!
//! [`Loot`] loads tables, with the items, enchantments and tags they name, from a bundle;
//! [`Loot::fill`] rolls one. Filling needs nothing of worldgen but two answers about the
//! container's world, which its caller gives through [`World`]: the biome at a block, for
//! `location_check`, and an exploration map's target.

mod enchant;
mod fill;
mod load;
mod model;
mod random;
mod stack;

use std::collections::HashMap;

use spyglass_data::codec::LoadError;
use spyglass_density::Resources;

pub use stack::{MapMark, NameTarget, Stack};

use model::Table;

/// Loot tables loaded from a bundle, with everything they name.
#[derive(Default)]
pub struct Loot {
    tables: HashMap<String, Table>,
    loader: load::Cache,
}

impl Loot {
    /// No tables yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Loads table `id` (`minecraft:chests/simple_dungeon`) and every table it nests, unless
    /// they're loaded already.
    pub fn load<R: Resources + ?Sized>(
        &mut self,
        resources: &R,
        id: &str,
    ) -> Result<(), LoadError> {
        load::Loader::new(resources, &mut self.loader, &mut self.tables).table(id)
    }

    /// Whether table `id` is loaded.
    pub fn has(&self, id: &str) -> bool {
        self.tables.contains_key(&spyglass_data::codec::full_id(id))
    }

    /// The contents of a container of `slots` empty slots at block `at`, filled from table `id`
    /// with loot seed `seed` ([loot.md](../../docs/specs/loot.md#filling)): each slot's stack,
    /// or `None` where it stays empty.
    pub fn fill(
        &self,
        id: &str,
        seed: i64,
        at: [i32; 3],
        slots: usize,
        world: &dyn World,
    ) -> Result<Vec<Option<Stack>>, FillError> {
        let id = spyglass_data::codec::full_id(id);
        if !self.tables.contains_key(&id) {
            return Err(FillError::NotLoaded(id));
        }
        if seed == 0 {
            return Err(FillError::NoSeed);
        }
        Ok(fill::fill(self, &id, seed, at, slots, world))
    }
}

/// Why a container couldn't be filled.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FillError {
    /// [`Loot::load`] wasn't called for the table.
    #[error("the loot table {0} isn't loaded")]
    NotLoaded(String),
    /// A loot seed of 0 takes the world's random sequence instead of a seeded random, which
    /// depends on the world's history, not its seed.
    #[error("a loot seed of 0 fills from the world's random sequence, which spyglass doesn't know")]
    NoSeed,
}

/// What filling needs to know about the container's world.
pub trait World {
    /// The biome at block `(x, y, z)` as `BiomeManager.getBiome` gives it (the zoomed lookup),
    /// for `location_check`, or `None` where the block's chunk isn't loaded.
    fn biome(&self, x: i32, y: i32, z: i32) -> Option<String>;

    /// Where an exploration map from the container points: the block x and z of the structure
    /// `ServerLevel.findNearestMapStructure` finds, or `None` if it finds none. The map's id,
    /// from the world's counter, isn't part of the answer.
    fn map_target(&self, request: &MapRequest) -> Option<[i32; 2]>;
}

/// An exploration map's search ([loot.md](../../docs/specs/loot.md#maps)).
#[derive(Debug, Clone, PartialEq)]
pub struct MapRequest {
    /// The container's block, which the search starts from.
    pub from: [i32; 3],
    /// The structures it looks for: a structure tag (`#minecraft:on_treasure_maps`), or ids.
    pub destination: Vec<String>,
    /// How far it looks, in placement cells.
    pub search_radius: i32,
    /// Whether it skips structures already referenced by a generated chunk (and, from 26.3,
    /// first claims the destination structure the container stands in).
    pub skip_existing_chunks: bool,
}
