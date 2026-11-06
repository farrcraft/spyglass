//! The blocks features read and write: the generator's palette, extended with the states features
//! place, and each state's behaviour, tags and heightmaps (`docs/specs/features.md#the-region`).

use std::collections::HashSet;

use serde_json::{Map, Value};
use spyglass_blocks::behaviour::{Behaviour, BlockBehaviour, FluidKind};
use spyglass_blocks::state::BlockDefaults;
use spyglass_blocks::{Palette, State};
use spyglass_data::codec::{At, LoadError, LoadErrorKind, field, full_id, object};
use spyglass_density::Resources;

/// Block tags.
const BLOCK_TAGS: &str = "tags/block";

/// A block tag, by its index in [`Blocks`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tag(u8);

/// The level's heightmaps (`Heightmap.Types`), in their order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Heightmap {
    /// `WORLD_SURFACE_WG`: not air. Frozen from terrain on.
    WorldSurfaceWg,
    /// `WORLD_SURFACE`: not air.
    WorldSurface,
    /// `OCEAN_FLOOR_WG`: blocks motion. Frozen from terrain on.
    OceanFloorWg,
    /// `OCEAN_FLOOR`: blocks motion.
    OceanFloor,
    /// `MOTION_BLOCKING`: blocks motion, or holds a fluid.
    MotionBlocking,
    /// `MOTION_BLOCKING_NO_LEAVES`: as `MOTION_BLOCKING`, without leaves.
    MotionBlockingNoLeaves,
}

impl Heightmap {
    /// By the codec's name.
    pub fn parse(name: &str) -> Option<Heightmap> {
        Some(match name {
            "WORLD_SURFACE_WG" => Heightmap::WorldSurfaceWg,
            "WORLD_SURFACE" => Heightmap::WorldSurface,
            "OCEAN_FLOOR_WG" => Heightmap::OceanFloorWg,
            "OCEAN_FLOOR" => Heightmap::OceanFloor,
            "MOTION_BLOCKING" => Heightmap::MotionBlocking,
            "MOTION_BLOCKING_NO_LEAVES" => Heightmap::MotionBlockingNoLeaves,
            _ => return None,
        })
    }

    fn bit(self) -> u8 {
        1 << self as u8
    }
}

/// Per state: its behaviour, the tags it's in, and the heightmaps it counts for.
#[derive(Debug, Clone, Copy)]
struct Entry {
    behaviour: Behaviour,
    tags: u64,
    heightmaps: u8,
    /// The block has a `waterlogged` property.
    waterloggable: bool,
}

/// The palette features use, with what they need to know about each state.
#[derive(Debug, Clone)]
pub struct Blocks {
    palette: Palette,
    entries: Vec<Entry>,
    source: BlockBehaviour,
    defaults: BlockDefaults,
    /// Each registered tag's block ids.
    tags: Vec<HashSet<String>>,
    tag_names: Vec<String>,
    /// Switch `heightmap_blocks_motion`: 26.2's heightmaps test `blocksMotion()` and leaves, not
    /// tags.
    blocks_motion: bool,
    motion: Tag,
    motion_no_leaves: Tag,
}

impl Blocks {
    /// Starts from `palette`, the generator's, so its states keep their numbers.
    pub fn new(resources: &dyn Resources, palette: &Palette) -> Result<Self, LoadError> {
        let blocks_motion = resources.behavior().heightmap_blocks_motion;
        let mut blocks = Blocks {
            palette: Palette::new(),
            entries: Vec::new(),
            source: BlockBehaviour::load(resources)?,
            defaults: BlockDefaults::load(resources)?,
            tags: Vec::new(),
            tag_names: Vec::new(),
            blocks_motion,
            motion: Tag(0),
            motion_no_leaves: Tag(0),
        };
        if !blocks_motion {
            blocks.motion = blocks.tag(resources, "minecraft:blocks_motion_in_heightmap")?;
            blocks.motion_no_leaves =
                blocks.tag(resources, "minecraft:blocks_motion_in_heightmap_no_leaves")?;
        }
        for s in 0..palette.len() {
            let state = blocks.state(palette.name(s as State))?;
            debug_assert_eq!(usize::from(state), s);
        }
        Ok(blocks)
    }

    /// The tag `id`, loaded with its nested tags if it's new.
    pub fn tag(&mut self, resources: &dyn Resources, id: &str) -> Result<Tag, LoadError> {
        if let Some(i) = self.tag_names.iter().position(|t| t == id) {
            return Ok(Tag(i as u8));
        }
        let i = self.tags.len();
        if i >= 64 {
            return Err(LoadError::new(
                BLOCK_TAGS,
                id,
                LoadErrorKind::Invalid("more than 64 block tags"),
            ));
        }
        let members: HashSet<String> = spyglass_terrain::tag(resources, BLOCK_TAGS, id)?
            .into_iter()
            .collect();
        for (s, entry) in self.entries.iter_mut().enumerate() {
            if members.contains(self.palette.block_id(s as State)) {
                entry.tags |= 1 << i;
            }
        }
        self.tags.push(members);
        self.tag_names.push(id.to_string());
        let tag = Tag(i as u8);
        // The heightmaps read the motion tags, which may only now be known.
        for s in 0..self.entries.len() {
            self.entries[s].heightmaps = self.heightmaps(self.entries[s]);
        }
        Ok(tag)
    }

    /// The state named `name` (an id, then its non-default properties in brackets), added if
    /// it's new. Unknown to the block report, an error.
    pub fn state(&mut self, name: &str) -> Result<State, LoadError> {
        if let Some(s) = self.palette.get(name)
            && usize::from(s) < self.entries.len()
        {
            return Ok(s);
        }
        let behaviour = self.source.of(name).ok_or_else(|| {
            LoadError::new(
                "reports/block_behaviour.json",
                "states",
                LoadErrorKind::Unsupported {
                    what: "block state",
                    name: name.to_string(),
                },
            )
        })?;
        let s = self.palette.state(name);
        let id = self.palette.block_id(s).to_string();
        let mut tags = 0u64;
        for (i, members) in self.tags.iter().enumerate() {
            if members.contains(&id) {
                tags |= 1 << i;
            }
        }
        let mut entry = Entry {
            behaviour,
            tags,
            heightmaps: 0,
            waterloggable: self.defaults.has_property(&id, "waterlogged"),
        };
        entry.heightmaps = self.heightmaps(entry);
        self.entries.push(entry);
        Ok(s)
    }

    /// A block state in JSON: 26.2's `{"Name", "Properties"}` or 26.3's `{"id", "properties"}`,
    /// or a block id alone for its default state.
    pub fn parse_state(&mut self, json: &Value, at: &At<'_>) -> Result<State, LoadError> {
        let empty = Map::new();
        let (id, properties) = match json {
            Value::String(id) => (full_id(id), &empty),
            Value::Object(o) => {
                let (id_key, props_key) = if o.contains_key("Name") {
                    ("Name", "Properties")
                } else {
                    ("id", "properties")
                };
                let id = field(o, id_key, at)?
                    .as_str()
                    .map(full_id)
                    .ok_or_else(|| at.child(id_key).error(LoadErrorKind::Shape("a block id")))?;
                let properties = match o.get(props_key) {
                    None => &empty,
                    Some(p) => object(p, &at.child(props_key))?,
                };
                (id, properties)
            }
            _ => return Err(at.error(LoadErrorKind::Shape("a block state"))),
        };
        let name = self.defaults.name(&id, properties, at)?;
        self.state(&name)
    }

    /// A block state provider that's one state: 26.2's `simple_state_provider`, or 26.3's bare
    /// state. Others aren't supported yet.
    pub fn parse_simple_provider(&mut self, json: &Value, at: &At<'_>) -> Result<State, LoadError> {
        let o = object(json, at)?;
        match o.get("type").and_then(Value::as_str).map(full_id) {
            None => self.parse_state(json, at),
            Some(t) if t == "minecraft:simple_state_provider" => {
                self.parse_state(field(o, "state", at)?, &at.child("state"))
            }
            Some(t) => Err(at.child("type").error(LoadErrorKind::Unsupported {
                what: "block state provider",
                name: t,
            })),
        }
    }

    /// The heightmaps a state counts for (`Heightmap.Types`' predicates).
    fn heightmaps(&self, e: Entry) -> u8 {
        let b = e.behaviour;
        let fluid = b.fluid.is_some();
        let (motion, no_leaves) = if self.blocks_motion {
            (b.blocks_motion, (b.blocks_motion || fluid) && !b.leaves)
        } else {
            let has = |t: Tag| e.tags & (1 << t.0) != 0 && !self.tags.is_empty();
            (has(self.motion), has(self.motion_no_leaves) || fluid)
        };
        let mut bits = 0;
        if !b.air {
            bits |= Heightmap::WorldSurfaceWg.bit() | Heightmap::WorldSurface.bit();
        }
        if motion {
            bits |= Heightmap::OceanFloorWg.bit() | Heightmap::OceanFloor.bit();
        }
        if motion || fluid {
            bits |= Heightmap::MotionBlocking.bit();
        }
        if no_leaves {
            bits |= Heightmap::MotionBlockingNoLeaves.bit();
        }
        bits
    }

    /// The palette.
    pub fn palette(&self) -> &Palette {
        &self.palette
    }

    /// A state's behaviour.
    pub fn behaviour(&self, s: State) -> Behaviour {
        self.entries[usize::from(s)].behaviour
    }

    /// Whether a state is in `tag`.
    pub fn is(&self, s: State, tag: Tag) -> bool {
        self.entries[usize::from(s)].tags & (1 << tag.0) != 0
    }

    /// Whether block `id` (`minecraft:chest`) is in `tag`.
    pub fn block_in(&self, id: &str, tag: Tag) -> bool {
        self.tags[usize::from(tag.0)].contains(id)
    }

    /// Whether block `id`'s block entity takes a loot table and seed; `None` where the bundle's
    /// block report predates the flag, or doesn't know the block.
    pub fn loot_container(&self, id: &str) -> Option<bool> {
        if !self.source.knows_loot_containers() {
            return None;
        }
        self.source.of(id).map(|b| b.loot_container)
    }

    /// Whether the bundle's block report says which faces support (format 3 on).
    pub fn knows_support(&self) -> bool {
        self.source.knows_support()
    }

    /// The heightmaps a state counts for, a bit per [`Heightmap`] by its order.
    pub fn heightmap_bits(&self, s: State) -> u8 {
        self.entries[usize::from(s)].heightmaps
    }

    /// Whether a state counts for heightmap `h`.
    pub fn counts_for(&self, s: State, h: Heightmap) -> bool {
        self.entries[usize::from(s)].heightmaps & h.bit() != 0
    }

    /// Whether a state's block can hold water (has a `waterlogged` property).
    pub fn waterloggable(&self, s: State) -> bool {
        self.entries[usize::from(s)].waterloggable
    }

    /// `isAir()`.
    pub fn is_air(&self, s: State) -> bool {
        self.behaviour(s).air
    }

    /// Whether a state's fluid is `kind`.
    pub fn fluid_is(&self, s: State, kind: FluidKind) -> bool {
        self.behaviour(s).fluid.is_some_and(|f| f.kind == kind)
    }
}
