//! Loot: the containers structures and features place in a chunk, each certain or a candidate,
//! with what their tables fill them with (`docs/specs/locate.md#loot`, `docs/specs/loot.md`).

use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap};
use std::ops::ControlFlow;
use std::sync::{Arc, Mutex, OnceLock, PoisonError, RwLock};

use rayon::prelude::*;
use spyglass_biome::{BiomeId, Lookup};
use spyglass_blocks::{Chunk, ChunkGenerator};
use spyglass_data::codec::full_id;
use spyglass_data::versions::Behavior;
use spyglass_feature::containers::{ContainerDoubt, StructureContainers, container_slots};
use spyglass_feature::order::PlacedRef;
use spyglass_feature::structures::{self, LayoutCache};
use spyglass_feature::{
    Blocks, Decorator, Doubt, FeatureSettings, NeighbourBounds, Outcome, Placer, Reading, Region,
    Wanted,
};
use spyglass_loot::{Loot, MapRequest, Stack};
use spyglass_structure::Starts;

use crate::{Error, Result, Session, World};

/// The block a dungeon's chests are.
const CHEST: &str = "minecraft:chest";

/// One decoration pass for a chunk's containers: the features of the steps before `step`,
/// placed, then the structures of `step` that place containers.
struct Pass {
    /// The step whose structures place after these features.
    step: usize,
    /// Whether any structure of the step places containers.
    structures: bool,
    /// The placed features of the steps before it that load, with their modifiers read in full.
    placing: Wanted,
    /// The placed features of those steps whose writes spyglass doesn't know: those it doesn't
    /// place, or whose modifiers it can't read.
    skipped: Vec<PlacedRef>,
}

/// A placed feature that puts down containers.
struct ContainerFeature {
    placed: PlacedRef,
    /// Its decoration step.
    step: usize,
    /// Its containers' loot table.
    table: &'static str,
    /// It on its own, for whether a chunk has tries of it.
    alone: Wanted,
}

/// A world's loot, seed-independent, loaded on first use: what places containers, and the
/// tables that fill them.
pub(crate) struct LootWorld {
    /// The placed and configured features, and each decoration step's order of them.
    settings: FeatureSettings,
    /// The blocks' behaviour, with the tags structures' processors test.
    blocks: Blocks,
    /// Every placed feature spyglass places.
    placer: Placer,
    /// The structures' processors, ruins and mineshaft blocks.
    containers: StructureContainers,
    /// Every placed feature, by their modifiers' shapes, for what a chunk's own could write.
    everything: Wanted,
    /// How far features' writes reach.
    bounds: NeighbourBounds,
    /// The passes, by step.
    passes: Vec<Pass>,
    /// The placed features that place containers.
    container_features: Vec<ContainerFeature>,
    /// The last step anything placing containers is in.
    last_step: usize,
    /// The tables loaded so far.
    tables: RwLock<Loot>,
    /// Whether a map that skips known structures first claims the one its container is in.
    claims_containing: bool,
}

impl LootWorld {
    fn load(world: &World) -> Result<LootWorld> {
        let bundle = &world.bundle;
        let v1_21 = bundle
            .resource("worldgen/configured_feature", "minecraft:monster_room")
            .is_ok();
        let feature_settings = FeatureSettings::load(bundle, world.source.biome_names(), v1_21)?;
        let settings = &feature_settings;
        let mut blocks = Blocks::new(bundle, world.block_settings()?.palette())?;
        let all: Vec<PlacedRef> = settings.order().iter().flatten().copied().collect();
        // A placed feature whose feature spyglass can't load (the Nether's) is left out, and
        // its writes aren't known.
        let loads: Vec<PlacedRef> = all
            .iter()
            .copied()
            .filter(|&r| Placer::load(bundle, settings, &mut blocks, &[r]).is_ok())
            .collect();
        let placer = Placer::load(bundle, settings, &mut blocks, &loads)?;
        let pools = world.pools()?;
        let containers = StructureContainers::load(bundle, &mut blocks, pools, &world.sets)?;
        let step_of = |r: PlacedRef| {
            settings
                .order()
                .iter()
                .position(|step| step.contains(&r))
                .unwrap_or(0)
        };
        let container_features: Vec<(PlacedRef, usize, &'static str)> = all
            .iter()
            .copied()
            .filter_map(|r| Some((r, step_of(r), placer.container_table(r)?)))
            .collect();
        // A pass for each step with structures that place containers, and one after the last
        // step with features that do.
        let mut steps: Vec<(usize, bool)> = world
            .sets
            .structures()
            .iter()
            .filter(|s| structures::places_containers(s.kind))
            .map(|s| (s.step, true))
            .collect();
        if let Some(&(_, step, _)) = container_features.iter().max_by_key(|(_, step, _)| *step)
            && !steps.iter().any(|&(s, _)| s > step)
        {
            steps.push((step + 1, false));
        }
        steps.sort_unstable();
        steps.dedup_by(|b, a| {
            a.1 |= b.1;
            a.0 == b.0
        });
        let mut passes = Vec::new();
        for (step, has_structures) in steps {
            let mut loaded = Vec::new();
            let mut skipped = Vec::new();
            for &r in settings.order()[..step.min(settings.order().len())]
                .iter()
                .flatten()
            {
                let mut reading = Reading {
                    resources: bundle,
                    blocks: &mut blocks,
                };
                if placer.places(r) && Wanted::load(settings, &[r], Some(&mut reading)).is_ok() {
                    loaded.push(r);
                } else {
                    skipped.push(r);
                }
            }
            let mut reading = Reading {
                resources: bundle,
                blocks: &mut blocks,
            };
            passes.push(Pass {
                step,
                structures: has_structures,
                placing: Wanted::load(settings, &loaded, Some(&mut reading))?,
                skipped,
            });
        }
        let last_step = passes.last().map_or(0, |p| p.step);
        let container_features = container_features
            .into_iter()
            .map(|(placed, step, table)| {
                Ok(ContainerFeature {
                    placed,
                    step,
                    table,
                    alone: Wanted::load(settings, &[placed], None)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(LootWorld {
            everything: Wanted::load(settings, &all, None)?,
            bounds: NeighbourBounds::load(bundle, settings)?,
            settings: feature_settings,
            blocks,
            placer,
            containers,
            passes,
            container_features,
            last_step,
            tables: RwLock::new(Loot::new()),
            claims_containing: !Behavior::of(bundle.manifest().data_version).map_claims_nothing,
        })
    }
}

impl World {
    /// The world's loot, loaded the first time it's asked for.
    pub(crate) fn loot_world(&self) -> Result<&LootWorld> {
        if let Some(loot) = self.loot.get() {
            return Ok(loot);
        }
        let loot = LootWorld::load(self)?;
        Ok(self.loot.get_or_init(|| loot))
    }

    /// Whether any structure or feature in this world places loot containers.
    pub(crate) fn places_containers(&self) -> Result<bool> {
        Ok(!self.loot_world()?.passes.is_empty())
    }

    /// Loads `table` unless it's loaded.
    fn load_table(&self, loot: &LootWorld, table: &str) -> Result<()> {
        let loaded = loot
            .tables
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .has(table);
        if !loaded {
            loot.tables
                .write()
                .unwrap_or_else(PoisonError::into_inner)
                .load(&self.bundle, table)?;
        }
        Ok(())
    }

    /// Whether a container of loot table `table` could hold an item `query` names, whatever
    /// its seed: for a container whose contents aren't known (`Loot::possible_items`, a
    /// superset).
    pub fn table_could_hold(&self, query: &ItemQuery, table: &str) -> Result<bool> {
        if query.world != self.id {
            return Err(Error::OtherWorld(query.argument.clone()));
        }
        let loot = self.loot_world()?;
        self.load_table(loot, table)?;
        let tables = loot.tables.read().unwrap_or_else(PoisonError::into_inner);
        Ok(query.could_hold(&tables.possible_items(table)))
    }

    /// Resolves an item argument for [`Session::nearest_container`]: an item id, or an item
    /// `#tag`, with an enchantment the item must have (on it, or stored in a book) at
    /// `level` or higher if given (`docs/specs/locate.md#loot`).
    pub fn item_query(
        &self,
        argument: &str,
        enchantment: Option<(&str, Option<i32>)>,
    ) -> Result<ItemQuery> {
        let items = match argument.strip_prefix('#') {
            Some(tag) => {
                if !self.exists("tags/item", tag) {
                    return Err(Error::Unknown {
                        what: "item tag",
                        id: argument.to_string(),
                    });
                }
                spyglass_density::tag(&self.bundle, "tags/item", &full_id(tag))?
            }
            None => {
                let id = full_id(argument);
                let (namespace, path) = id.split_once(':').unwrap_or(("minecraft", &id));
                let report = format!("reports/{namespace}/components/item/{path}.json");
                if !self.bundle.manifest().files.contains_key(&report) {
                    return Err(Error::Unknown {
                        what: "item",
                        id: argument.to_string(),
                    });
                }
                vec![full_id(argument)]
            }
        };
        let enchantment = match enchantment {
            None => None,
            Some((id, level)) => {
                if !self.exists("enchantment", id) {
                    return Err(Error::Unknown {
                        what: "enchantment",
                        id: id.to_string(),
                    });
                }
                Some((full_id(id), level))
            }
        };
        let mut argument = argument.to_string();
        if let Some((id, level)) = &enchantment {
            argument.push_str(&format!(
                " with {id}{}",
                level.map_or(String::new(), |l| format!(" {l}"))
            ));
        }
        Ok(ItemQuery {
            world: self.id,
            argument,
            items: items.into_iter().collect(),
            enchantment,
        })
    }
}

/// An item argument resolved by [`World::item_query`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemQuery {
    /// The world it was resolved in, so it isn't asked of another.
    world: u64,
    /// The argument as given, with its enchantment.
    argument: String,
    /// The items it names.
    items: BTreeSet<String>,
    /// An enchantment the item must have, and its least level.
    enchantment: Option<(String, Option<i32>)>,
}

impl ItemQuery {
    /// The argument as given, e.g. `minecraft:enchanted_book with minecraft:mending`.
    pub fn argument(&self) -> &str {
        &self.argument
    }

    /// Whether `item` is one the query names.
    pub fn matches(&self, item: &Item) -> bool {
        self.items.contains(&item.id)
            && self.enchantment.as_ref().is_none_or(|(id, level)| {
                [&item.enchantments, &item.stored_enchantments]
                    .into_iter()
                    .flatten()
                    .any(|(e, l)| e == id && level.is_none_or(|least| *l >= least))
            })
    }

    /// Whether a container of `possible` items could hold one the query names.
    fn could_hold(&self, possible: &BTreeSet<String>) -> bool {
        self.items.iter().any(|i| possible.contains(i))
    }
}

/// What placed a container.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ContainerSource {
    /// A structure's pieces, by structure id, e.g. `minecraft:village_plains`.
    Structure(String),
    /// A feature, by placed feature id, e.g. `minecraft:monster_room_deep`.
    Feature(String),
}

/// A stack in a container: an item, its count, and the components its table gave it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Item {
    /// The item, e.g. `minecraft:golden_apple`.
    pub id: String,
    /// How many.
    pub count: i32,
    /// Damage taken, where the table set it.
    pub damage: Option<i32>,
    /// Its enchantments and their levels, by id.
    pub enchantments: Vec<(String, i32)>,
    /// An enchanted book's stored enchantments and their levels, by id.
    pub stored_enchantments: Vec<(String, i32)>,
    /// A potion, tipped arrow or the like's potion.
    pub potion: Option<String>,
    /// A suspicious stew's effects, each with its duration in ticks.
    pub stew_effects: Vec<(String, i32)>,
    /// A goat horn's instrument.
    pub instrument: Option<String>,
    /// A name the table gave it, as its text component's JSON.
    pub name: Option<String>,
    /// An ominous bottle's amplifier.
    pub ominous_bottle_amplifier: Option<i32>,
    /// Where an exploration map points.
    pub map: Option<MapDestination>,
}

impl Item {
    fn of(stack: &Stack) -> Item {
        Item {
            id: stack.item().to_string(),
            count: stack.count,
            damage: stack.damage,
            enchantments: stack
                .enchantments
                .iter()
                .map(|(e, l)| (e.clone(), *l))
                .collect(),
            stored_enchantments: stack
                .stored_enchantments
                .iter()
                .map(|(e, l)| (e.clone(), *l))
                .collect(),
            potion: stack.potion.clone(),
            stew_effects: stack.stew_effects.clone(),
            instrument: stack.instrument.clone(),
            name: stack.name.as_ref().map(|(_, text)| text.to_string()),
            ominous_bottle_amplifier: stack.ominous_bottle_amplifier,
            map: stack.map.as_ref().map(|m| MapDestination {
                target: m.target,
                decoration: m.decoration.clone(),
                zoom: m.zoom,
            }),
        }
    }
}

/// An exploration map's target and how it's drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct MapDestination {
    /// The structure's block x and z.
    pub target: [i32; 2],
    /// The marker's decoration type, e.g. `minecraft:red_x`.
    pub decoration: String,
    /// The map's scale.
    pub zoom: i32,
}

/// A filled slot of a container.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Slot {
    /// The slot, from 0.
    pub slot: usize,
    /// What's in it.
    pub item: Item,
}

/// A loot container a structure or feature placed, or might have.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FoundContainer {
    /// Where: the block, or a chest minecart's rail.
    pub position: [i32; 3],
    /// The chunk whose decoration placed it, which a dungeon's or a ruined portal's chest may
    /// be beside.
    pub chunk: [i32; 2],
    /// The container block or entity, e.g. `minecraft:chest`, `minecraft:barrel`,
    /// `minecraft:chest_minecart`.
    pub holder: String,
    /// What placed it.
    pub source: ContainerSource,
    /// Its loot table, e.g. `minecraft:chests/simple_dungeon`.
    pub table: String,
    /// Its loot seed, where it's known.
    pub seed: Option<i64>,
    /// Whether it's certain, for its chunk decorated before its neighbours: there, with this
    /// table and seed.
    pub certain: bool,
    /// Why it's a candidate, if it is.
    pub reason: Option<String>,
    /// What its table fills it with, from its seed, the slots filled in order; `None` where
    /// the seed isn't known.
    pub contents: Option<Vec<Slot>>,
    /// Why the contents may not be the game's, or aren't known, where the container itself is
    /// otherwise certain: a tied biome a condition reads, an exploration map's target that
    /// isn't certain, an unknown seed.
    pub contents_doubt: Option<String>,
}

impl FoundContainer {
    /// Whether its contents are certainly the game's.
    pub fn contents_certain(&self) -> bool {
        self.certain && self.contents.is_some() && self.contents_doubt.is_none()
    }

    /// Whether it holds an item `query` names.
    pub fn holds(&self, query: &ItemQuery) -> bool {
        self.contents
            .iter()
            .flatten()
            .any(|s| query.matches(&s.item))
    }
}

/// The answer to [`Session::nearest_container`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ContainerLocate {
    /// Every container that may be the nearest holding the item, nearest first by horizontal
    /// distance, ending at the first certain one; empty if none is within the radius.
    pub hits: Vec<FoundContainer>,
    /// Whether it ends at a certain one.
    pub complete: bool,
}

/// Each chunk's containers holding an item, or that might, by the item asked for: what one
/// seed's queries share, so a chunk is placed once for an item however many ask for it.
#[derive(Default)]
pub(crate) struct LootPlacings(HashMap<(String, [i32; 2]), Vec<FoundContainer>>);

/// A container as placing finds it, before it's filled.
struct Placed {
    position: [i32; 3],
    chunk: [i32; 2],
    holder: String,
    source: ContainerSource,
    table: String,
    seed: Option<i64>,
    reason: Option<String>,
}

/// What a loot query keeps between chunks.
struct Cache<'s> {
    /// The seed's starts, confirmed, with the jigsaw pools.
    starts: Starts<'s>,
    /// Each start's pieces.
    layouts: LayoutCache,
    /// Carved chunks, by chunk coordinates: each is a neighbour of up to 9 chunks. A cell per
    /// chunk, so one carved by one thread is waited for, not carved again, by another.
    carved: Mutex<HashMap<[i32; 2], Arc<OnceLock<Chunk>>>>,
    /// Each chunk's stored biomes.
    biomes: Mutex<HashMap<[i32; 2], Vec<BiomeId>>>,
    /// Each table's possible items.
    possible: Mutex<HashMap<String, Arc<BTreeSet<String>>>>,
}

impl Cache<'_> {
    /// Forgets the chunks in rows before `cz`.
    fn keep_rows_from(&mut self, cz: i32) {
        let carved = self
            .carved
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner);
        carved.retain(|c, _| c[1] >= cz);
        let biomes = self
            .biomes
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner);
        biomes.retain(|c, _| c[1] >= cz);
    }

    /// Chunk `chunk` carved.
    fn carved(&self, generator: &ChunkGenerator<'_>, chunk: [i32; 2]) -> Chunk {
        let cell = {
            let mut carved = self.carved.lock().unwrap_or_else(PoisonError::into_inner);
            carved.entry(chunk).or_default().clone()
        };
        cell.get_or_init(|| generator.carved_chunk(chunk[0], chunk[1]))
            .clone()
    }

    /// Chunk `chunk`'s stored biomes.
    fn biomes(&self, generator: &ChunkGenerator<'_>, chunk: [i32; 2]) -> Vec<BiomeId> {
        let mut biomes = self.biomes.lock().unwrap_or_else(PoisonError::into_inner);
        biomes
            .entry(chunk)
            .or_insert_with(|| generator.chunk_biomes(chunk[0], chunk[1]))
            .clone()
    }
}

/// What filling needs of the session: biomes at blocks, and exploration maps' targets as a
/// fresh world's maps find them (`docs/specs/loot.md#maps`).
struct FillWorld<'a, 's> {
    session: &'a Session<'s>,
    generator: &'a ChunkGenerator<'s>,
    starts: &'a Starts<'a>,
    claims_containing: bool,
    /// Why the fill might not be the game's: a tie or a map's target.
    doubt: RefCell<Option<String>>,
    /// Whether a map search failed, so the fill stops.
    failed: Cell<bool>,
}

impl FillWorld<'_, '_> {
    fn doubt(&self, why: String) {
        self.doubt.borrow_mut().get_or_insert(why);
    }
}

impl spyglass_loot::World for FillWorld<'_, '_> {
    fn biome(&self, x: i32, y: i32, z: i32) -> Option<String> {
        let lookup = self.generator.block_biome([x, y, z]);
        if matches!(lookup, Lookup::Tied(_)) {
            self.doubt("a biome its table reads is tied".to_string());
        }
        Some(self.session.world.source.name(lookup.biome()).to_string())
    }

    fn map_target(&self, request: &MapRequest) -> Option<[i32; 2]> {
        let world = self.session.world;
        let mut targets = Vec::new();
        for destination in &request.destination {
            match world.sets.resolve(&world.bundle, destination) {
                Ok(found) => targets.extend(found),
                Err(_) => self.failed.set(true),
            }
        }
        let answer = self.starts.map_target(
            &targets,
            request.from,
            request.search_radius,
            request.skip_existing_chunks,
            self.claims_containing,
            &mut Vec::new(),
        );
        match answer {
            Ok(hit) => hit.map(|h| [h.position[0], h.position[2]]),
            // spyglass's best answer: the search's first hit.
            Err(search) => {
                self.doubt("an exploration map's target isn't certain".to_string());
                search
                    .and_then(|s| s.hits.first().cloned())
                    .map(|h| [h.position[0], h.position[2]])
            }
        }
    }
}

impl<'w> Session<'w> {
    /// Every loot container a structure or feature places, or might, in chunks `min..=max`
    /// (chunk coordinates, inclusive), chunk by chunk (z, then x), each in the order the chunk
    /// places them, with its contents where its seed is known. Places every chunk a container
    /// structure's pieces or a dungeon's tries reach, which takes a while each
    /// (`docs/specs/locate.md#loot`).
    pub fn containers_in(&self, min: [i32; 2], max: [i32; 2]) -> Result<Vec<FoundContainer>> {
        self.containers_where(min, max, None)
    }

    /// The containers in chunks `min..=max` that hold an item `query` names, or might, as
    /// [`nearest_container`](Self::nearest_container) counts them, in
    /// [`containers_in`](Self::containers_in)'s order. Faster than filtering `containers_in`:
    /// chunks are placed only for containers whose table could hold the item, so for an item
    /// dungeons can't have, only container structures' chunks are.
    pub fn containers_holding(
        &self,
        query: &ItemQuery,
        min: [i32; 2],
        max: [i32; 2],
    ) -> Result<Vec<FoundContainer>> {
        self.check_world(query.world, &query.argument)?;
        self.containers_where(min, max, Some(query))
    }

    /// [`containers_holding`](Self::containers_holding) over the chunks `wanted` keeps, each
    /// container handed to `visit` as it's found, until it breaks. Chunks already in `placings`
    /// for this item aren't placed again. Keeps the carved chunks of three rows at a time.
    pub(crate) fn each_container(
        &self,
        query: &ItemQuery,
        min: [i32; 2],
        max: [i32; 2],
        wanted: impl Fn([i32; 2]) -> bool,
        placings: &mut LootPlacings,
        mut visit: impl FnMut(FoundContainer) -> ControlFlow<()>,
    ) -> Result<()> {
        self.check_world(query.world, &query.argument)?;
        let loot = self.world.loot_world()?;
        let mut cache: Option<Cache<'_>> = None;
        let key = |chunk: [i32; 2]| (query.argument.clone(), chunk);
        for cz in min[1]..=max[1] {
            // The rows before this one's neighbours aren't read again.
            if let Some(cache) = &mut cache {
                cache.keep_rows_from(cz - 1);
            }
            let todo: Vec<i32> = (min[0]..=max[0])
                .filter(|&cx| wanted([cx, cz]) && !placings.0.contains_key(&key([cx, cz])))
                .collect();
            if !todo.is_empty() {
                if cache.is_none() {
                    cache = Some(self.loot_cache()?);
                }
                let shared = cache.as_ref().expect("made above");
                // The row's chunks placed at once, then visited in order below. A visit that
                // breaks early leaves the rest of the row placed but unread.
                let place = |&cx: &i32| self.chunk_holding(loot, [cx, cz], shared, Some(query));
                let placed: Vec<Vec<FoundContainer>> = if self.parallel {
                    todo.par_iter().map(place).collect::<Result<_>>()?
                } else {
                    todo.iter().map(place).collect::<Result<_>>()?
                };
                for (cx, found) in todo.into_iter().zip(placed) {
                    placings.0.insert(key([cx, cz]), found);
                }
            }
            for cx in (min[0]..=max[0]).filter(|&cx| wanted([cx, cz])) {
                for c in placings.0.get(&key([cx, cz])).into_iter().flatten() {
                    if visit(c.clone()).is_break() {
                        return Ok(());
                    }
                }
            }
        }
        Ok(())
    }

    /// The containers in chunks `min..=max`, all of them or those holding `query`'s item.
    fn containers_where(
        &self,
        min: [i32; 2],
        max: [i32; 2],
        query: Option<&ItemQuery>,
    ) -> Result<Vec<FoundContainer>> {
        let loot = self.world.loot_world()?;
        let mut cache = self.loot_cache()?;
        let mut out = Vec::new();
        for cz in min[1]..=max[1] {
            // The rows before this one's neighbours aren't read again.
            cache.keep_rows_from(cz - 1);
            // The row's chunks placed at once, then kept in order.
            let place = |&cx: &i32| self.chunk_holding(loot, [cx, cz], &cache, query);
            let row: Vec<i32> = (min[0]..=max[0]).collect();
            let placed: Vec<Vec<FoundContainer>> = if self.parallel {
                row.par_iter().map(place).collect::<Result<_>>()?
            } else {
                row.iter().map(place).collect::<Result<_>>()?
            };
            out.extend(placed.into_iter().flatten());
        }
        Ok(out)
    }

    /// One chunk's containers filled, all of them or those holding `query`'s item, or that
    /// might: a candidate whose contents aren't known but whose table could hold it.
    fn chunk_holding(
        &self,
        loot: &LootWorld,
        chunk: [i32; 2],
        cache: &Cache<'_>,
        query: Option<&ItemQuery>,
    ) -> Result<Vec<FoundContainer>> {
        // Dungeons only where their table could hold the item.
        let mut dungeons = Vec::with_capacity(loot.container_features.len());
        for f in &loot.container_features {
            dungeons.push(match query {
                None => true,
                Some(query) => self.could_hold(loot, cache, f.table, query)?,
            });
        }
        let mut out = Vec::new();
        for placed in self.chunk_containers(loot, chunk, cache, &dungeons)? {
            if let Some(query) = query
                && !self.could_hold(loot, cache, &placed.table, query)?
            {
                continue;
            }
            let container = self.fill(loot, placed, &cache.starts)?;
            if query.is_none_or(|q| container.contents.is_none() || container.holds(q)) {
                out.push(container);
            }
        }
        Ok(out)
    }

    /// The container nearest to `from`, horizontally, within `radius` chunks, whose contents
    /// hold an item `query` names: every candidate nearer than the nearest certain one, then
    /// that one. A container whose seed isn't known is a candidate if its table could hold the
    /// item.
    pub fn nearest_container(
        &self,
        query: &ItemQuery,
        from: [i32; 3],
        radius: i32,
    ) -> Result<ContainerLocate> {
        self.check_world(query.world, &query.argument)?;
        let loot = self.world.loot_world()?;
        let cache = self.loot_cache()?;
        let centre = [from[0] >> 4, from[2] >> 4];
        let distance_sqr = |p: [i32; 3]| {
            let (dx, dz) = (i64::from(p[0] - from[0]), i64::from(p[2] - from[2]));
            dx * dx + dz * dz
        };
        // Square rings of chunks outwards, as `nearest_feature` searches.
        let mut found: Vec<FoundContainer> = Vec::new();
        let mut nearest_certain: Option<i64> = None;
        for ring in 0..=radius {
            if let Some(nearest) = nearest_certain {
                let ring_near = i64::from((ring - 1).max(0) * 16);
                if ring_near * ring_near > nearest {
                    break;
                }
            }
            // The ring's chunks, z then x, each placed whole before the stopping test.
            let chunks: Vec<[i32; 2]> = (-ring..=ring)
                .flat_map(|dz| (-ring..=ring).map(move |dx| [dx, dz]))
                .filter(|[dx, dz]| dx.abs().max(dz.abs()) == ring)
                .map(|[dx, dz]| [centre[0] + dx, centre[1] + dz])
                .collect();
            let place = |&chunk: &[i32; 2]| self.chunk_holding(loot, chunk, &cache, Some(query));
            let placed: Vec<Vec<FoundContainer>> = if self.parallel {
                chunks.par_iter().map(place).collect::<Result<_>>()?
            } else {
                chunks.iter().map(place).collect::<Result<_>>()?
            };
            for container in placed.into_iter().flatten() {
                if container.contents_certain() {
                    let d = distance_sqr(container.position);
                    nearest_certain = Some(nearest_certain.map_or(d, |n| n.min(d)));
                }
                found.push(container);
            }
        }
        found.sort_by_key(|c| distance_sqr(c.position));
        let complete = nearest_certain.is_some();
        if let Some(nearest) = nearest_certain {
            let end = found
                .iter()
                .position(|c| c.contents_certain() && distance_sqr(c.position) == nearest)
                .map_or(found.len(), |i| i + 1);
            found.truncate(end);
        }
        Ok(ContainerLocate {
            hits: found,
            complete,
        })
    }

    fn loot_cache(&self) -> Result<Cache<'_>> {
        Ok(Cache {
            starts: self.starts(true).with_pools(self.world.pools()?),
            layouts: LayoutCache::default(),
            carved: Mutex::default(),
            biomes: Mutex::default(),
            possible: Mutex::default(),
        })
    }

    /// Loads `table` unless it's loaded.
    fn load_table(&self, loot: &LootWorld, table: &str) -> Result<()> {
        self.world.load_table(loot, table)
    }

    /// Whether a container of `table` could hold an item `query` names.
    fn could_hold(
        &self,
        loot: &LootWorld,
        cache: &Cache<'_>,
        table: &str,
        query: &ItemQuery,
    ) -> Result<bool> {
        let known = {
            let possible = cache
                .possible
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            possible.get(table).cloned()
        };
        let possible = match known {
            Some(possible) => possible,
            // Worked out without the lock: two threads may both, and get the same set.
            None => {
                self.load_table(loot, table)?;
                let tables = loot.tables.read().unwrap_or_else(PoisonError::into_inner);
                let possible = Arc::new(tables.possible_items(table));
                let mut cached = cache
                    .possible
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                cached.insert(table.to_string(), possible.clone());
                possible
            }
        };
        Ok(query.could_hold(&possible))
    }

    /// A placed container with its contents.
    fn fill(
        &self,
        loot: &LootWorld,
        placed: Placed,
        starts: &Starts<'_>,
    ) -> Result<FoundContainer> {
        let Placed {
            position,
            chunk,
            holder,
            source,
            table,
            seed,
            reason,
        } = placed;
        let (contents, contents_doubt) = match (seed, container_slots(&loot.blocks, &holder)) {
            (None, _) => (None, Some("its loot seed isn't known".to_string())),
            (Some(0), _) => (
                None,
                Some("a loot seed of 0 fills from the world's random sequence".to_string()),
            ),
            (_, None) => (
                None,
                Some(format!("{holder}'s slots aren't in the block report")),
            ),
            (Some(seed), Some(slots)) => {
                self.load_table(loot, &table)?;
                let world = FillWorld {
                    session: self,
                    generator: self.chunks()?,
                    starts,
                    claims_containing: loot.claims_containing,
                    doubt: RefCell::new(None),
                    failed: Cell::new(false),
                };
                let tables = loot.tables.read().unwrap_or_else(PoisonError::into_inner);
                match tables.fill(&table, seed, position, slots, &world) {
                    Ok(stacks) => {
                        let slots = stacks
                            .iter()
                            .enumerate()
                            .filter_map(|(slot, s)| {
                                s.as_ref().map(|s| Slot {
                                    slot,
                                    item: Item::of(s),
                                })
                            })
                            .collect();
                        let mut doubt = world.doubt.into_inner();
                        if world.failed.get() {
                            doubt.get_or_insert("a map's destination isn't known".to_string());
                        }
                        (Some(slots), doubt)
                    }
                    Err(e) => (None, Some(e.to_string())),
                }
            }
        };
        Ok(FoundContainer {
            position,
            chunk,
            holder,
            source,
            table,
            seed,
            certain: reason.is_none(),
            reason,
            contents,
            contents_doubt,
        })
    }

    /// One chunk's containers, in the order it places them.
    ///
    /// A chunk that neither a container structure's pieces nor a container feature's tries
    /// reach has none. Otherwise, for each step with container structures (and one after the
    /// container features' step), the chunk is decorated with the features of the steps before
    /// it, on its blocks and its neighbours', and that step's structures place their
    /// containers on what the features left (`docs/specs/loot.md#containers`). Only the
    /// container features `dungeons` keeps (by index in the loot world's) are answered.
    fn chunk_containers(
        &self,
        loot: &LootWorld,
        chunk: [i32; 2],
        cache: &Cache<'_>,
        dungeons: &[bool],
    ) -> Result<Vec<Placed>> {
        let world = self.world;
        let settings = &loot.settings;
        let generator = self.chunks()?;
        let terrain = &world.terrain;
        let (anchors, min_y) = (terrain.anchors(), terrain.level_min_y);
        let (level_min, level_top) = generator.level();
        let sets = &world.sets;
        let Cache {
            starts, layouts, ..
        } = cache;
        let pieces = structures::pieces_cached(
            starts,
            sets,
            chunk,
            loot.last_step,
            level_min,
            level_top - 1,
            layouts,
        );
        let (x0, z0) = (chunk[0] * 16, chunk[1] * 16);
        let structure_reaches = pieces.iter().any(|p| {
            structures::places_containers(sets.structures()[p.structure].kind)
                && p.min[0] <= x0 + 15
                && x0 <= p.max[0]
                && p.min[2] <= z0 + 15
                && z0 <= p.max[2]
        });
        let wanted: Vec<&ContainerFeature> = loot
            .container_features
            .iter()
            .zip(dungeons)
            .filter_map(|(f, &keep)| keep.then_some(f))
            .collect();
        let feature_reaches = wanted.iter().any(|f| {
            let decorator = Decorator::of(settings, generator, self.seed, anchors, min_y, &f.alone);
            !decorator
                .tries(chunk, &mut |_, _| spyglass_feature::Drawn::Unknown)
                .is_empty()
        });
        if !structure_reaches && !feature_reaches {
            return Ok(Vec::new());
        }
        let mut biomes = |chunk: [i32; 2]| cache.biomes(generator, chunk);
        let mut chunks = Vec::with_capacity(9);
        for dz in -1..=1 {
            for dx in -1..=1 {
                chunks.push(cache.carved(generator, [chunk[0] + dx, chunk[1] + dz]));
            }
        }
        let region = Region::new(&loot.blocks, chunk, chunks);
        let everything = Decorator::of(
            settings,
            generator,
            self.seed,
            anchors,
            min_y,
            &loot.everything,
        );
        let own = loot
            .bounds
            .own_writes(&everything, settings, &region, &mut biomes);
        let feature_name = |r: PlacedRef| settings.placed(r).id.clone().unwrap_or_default();
        let structure_name = |i: usize| sets.structures()[i].id.clone();
        let doubt_reason = |doubt: Doubt| match doubt {
            Doubt::Tie => "a block it reads depends on a tied biome".to_string(),
            Doubt::OutsideRegion => "it reads past its chunk's neighbours".to_string(),
            Doubt::ShapeUpdate => "a block beside it could change shape".to_string(),
            Doubt::Waterlogging => "a block it places could fill with water".to_string(),
            Doubt::OtherFeature(r) => format!("{} could change what it reads", feature_name(r)),
            Doubt::Neighbours(r) => format!("a neighbour's {}, decorated first", feature_name(r)),
            Doubt::Structure(i) => format!("{}'s pieces", structure_name(i)),
        };
        let container_reason = |doubt: &ContainerDoubt| match doubt {
            ContainerDoubt::Start => "its structure's start might not be there".to_string(),
            ContainerDoubt::StartOrder => {
                "several starts reach its chunk, in an order spyglass doesn't know".to_string()
            }
            ContainerDoubt::FeatureElement(f) => format!(
                "after {}, which draws from the same random",
                f.as_deref().unwrap_or("an inline feature")
            ),
            ContainerDoubt::Processor(p) => format!("a {p} processor spyglass can't follow"),
            ContainerDoubt::Read(d) => doubt_reason(*d),
        };
        let mut out = Vec::new();
        let mut features_done = false;
        for pass in &loot.passes {
            let wants_features =
                !features_done && feature_reaches && wanted.iter().any(|f| f.step < pass.step);
            if !wants_features && !(pass.structures && structure_reaches) {
                continue;
            }
            let mut region = region.clone();
            let decorator = Decorator::of(
                settings,
                generator,
                self.seed,
                anchors,
                min_y,
                &pass.placing,
            );
            let mut unsure = Vec::new();
            let placements = loot.placer.decorate_noting(
                &decorator,
                &mut region,
                self.seed,
                &[],
                &own,
                &pieces,
                &mut unsure,
            );
            if wants_features {
                features_done = true;
                for p in placements {
                    if !wanted.iter().any(|f| f.placed == p.at.placed) {
                        continue;
                    }
                    let reason = match p.outcome {
                        Outcome::Placed => None,
                        Outcome::Candidate { placed: true, why } => Some(doubt_reason(why)),
                        Outcome::Declined | Outcome::Candidate { placed: false, .. } => continue,
                    };
                    for c in p.containers {
                        out.push(Placed {
                            position: c.pos,
                            chunk,
                            holder: CHEST.to_string(),
                            source: ContainerSource::Feature(feature_name(p.at.placed)),
                            table: c.table,
                            seed: Some(c.seed),
                            reason: reason.clone(),
                        });
                    }
                }
            }
            if !(pass.structures && structure_reaches) {
                continue;
            }
            // A feature of an earlier step whose writes aren't known, or another structure's
            // piece placed before, could have written in the box a structure reads.
            let reads = |min: [i32; 3], max: [i32; 3], structure: usize| {
                let info = &sets.structures()[structure];
                let meets =
                    |lo: [i32; 3], hi: [i32; 3]| (0..3).all(|i| lo[i] <= max[i] && min[i] <= hi[i]);
                own.iter()
                    .filter(|f| f.order.0 < info.step && meets(f.min, f.max))
                    .find_map(|f| {
                        (pass.skipped.contains(&f.placed)
                            || unsure.iter().any(|(r, _)| *r == f.placed))
                        .then_some(Doubt::OtherFeature(f.placed))
                    })
                    .or_else(|| {
                        pieces
                            .iter()
                            .find(|p| {
                                p.structure != structure
                                    && (p.step, sets.structures()[p.structure].step_index)
                                        < (info.step, info.step_index)
                                    && meets(p.min, p.max)
                            })
                            .map(|p| Doubt::Structure(p.structure))
                    })
            };
            let found = structures::containers(
                starts,
                sets,
                &loot.containers,
                &loot.blocks,
                generator,
                &mut region,
                self.seed,
                pass.step,
                &reads,
            );
            for c in found {
                out.push(Placed {
                    position: c.pos,
                    chunk,
                    holder: full_id(&c.block),
                    source: ContainerSource::Structure(structure_name(c.structure)),
                    table: c.table,
                    seed: c.seed,
                    reason: c.doubt.as_ref().map(container_reason),
                });
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, stored: &[(&str, i32)]) -> Item {
        Item {
            id: id.to_string(),
            count: 1,
            damage: None,
            enchantments: Vec::new(),
            stored_enchantments: stored.iter().map(|(e, l)| (e.to_string(), *l)).collect(),
            potion: None,
            stew_effects: Vec::new(),
            instrument: None,
            name: None,
            ominous_bottle_amplifier: None,
            map: None,
        }
    }

    fn query(items: &[&str], enchantment: Option<(&str, Option<i32>)>) -> ItemQuery {
        ItemQuery {
            world: 0,
            argument: String::new(),
            items: items.iter().map(|i| i.to_string()).collect(),
            enchantment: enchantment.map(|(e, l)| (e.to_string(), l)),
        }
    }

    #[test]
    fn an_item_query_matches_its_items_and_enchantment() {
        let book = item("minecraft:enchanted_book", &[("minecraft:mending", 1)]);
        assert!(query(&["minecraft:enchanted_book"], None).matches(&book));
        assert!(!query(&["minecraft:book"], None).matches(&book));
        let mending = Some(("minecraft:mending", None));
        assert!(query(&["minecraft:enchanted_book"], mending).matches(&book));
        let sharp = Some(("minecraft:sharpness", None));
        assert!(!query(&["minecraft:enchanted_book"], sharp).matches(&book));
        let mending_2 = Some(("minecraft:mending", Some(2)));
        assert!(!query(&["minecraft:enchanted_book"], mending_2).matches(&book));
    }

    #[test]
    fn could_hold_reads_the_possible_items() {
        let possible: BTreeSet<String> = ["minecraft:golden_apple".to_string()].into();
        assert!(query(&["minecraft:golden_apple", "minecraft:apple"], None).could_hold(&possible));
        assert!(!query(&["minecraft:enchanted_golden_apple"], None).could_hold(&possible));
    }

    /// A seed's containers near the origin, sequentially as a search's checks run them, and in
    /// parallel on one thread and on several: the same answers in the same order.
    #[test]
    #[ignore = "needs bundles/<version> (SPYGLASS_ORACLE_VERSION, else the newest release)"]
    fn containers_are_the_same_on_any_number_of_threads() {
        let version = spyglass_oracle::version();
        let world = World::open(spyglass_oracle::bundle_dir())
            .unwrap_or_else(|e| panic!("{e}; run spyglass-extract {version}"));
        let saddle = world.item_query("minecraft:saddle", None).unwrap();
        for seed in [0, 12_345] {
            let answers = |session: &Session<'_>| {
                let area = session.containers_in([-4, -4], [3, 3]).unwrap();
                let holding = session
                    .containers_holding(&saddle, [-4, -4], [3, 3])
                    .unwrap();
                let nearest = session.nearest_container(&saddle, [40, 0, -40], 8).unwrap();
                (area, holding, nearest)
            };
            let alone = answers(&world.search_session(seed).unwrap());
            assert!(!alone.0.is_empty(), "seed {seed}: no containers to compare");
            for threads in [1, 8] {
                let pool = rayon::ThreadPoolBuilder::new()
                    .num_threads(threads)
                    .build()
                    .unwrap();
                let parallel = pool.install(|| answers(&world.session(seed).unwrap()));
                assert_eq!(parallel, alone, "seed {seed}, {threads} threads");
            }
        }
    }
}
