//! Jigsaw structures' settings, pools, templates and start piece (`JigsawStructure`,
//! `JigsawPlacement.addPieces` up to the start): where the start piece goes and where its biome
//! check samples. The rest of the assembly is in `assembly.rs`. See
//! `docs/specs/structure-placement.md`.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use serde_json::{Map, Value};
use spyglass_data::bundle::{Bundle, BundleError};
use spyglass_data::codec;
use spyglass_data::nbt::{self, Tag};
use spyglass_data::versions::Behavior;
use spyglass_rng::{LegacyRandomSource, RandomSource};

use spyglass_data::codec::{At, LoadError, LoadErrorKind, LoadResult, full_id};

const TEMPLATE_POOL: &str = "worldgen/template_pool";

/// `Pools.EMPTY`: the pool a jigsaw block names by default.
pub(crate) const EMPTY_POOL: &str = "minecraft:empty";

/// `Rotation`, in declaration order, which `Rotation.getRandom` indexes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rotation {
    /// `NONE`
    None,
    /// `CLOCKWISE_90`
    Clockwise90,
    /// `CLOCKWISE_180`
    Clockwise180,
    /// `COUNTERCLOCKWISE_90`
    Counterclockwise90,
}

impl Rotation {
    /// `Rotation.values()`.
    pub(crate) const ALL: [Rotation; 4] = [
        Rotation::None,
        Rotation::Clockwise90,
        Rotation::Clockwise180,
        Rotation::Counterclockwise90,
    ];

    /// `Rotation.getRandom`: `values()[nextInt(4)]`.
    pub fn random(random: &mut LegacyRandomSource) -> Self {
        Self::ALL[random.next_int_bounded(4) as usize]
    }

    /// `StructureTemplate.transform(pos, Mirror.NONE, rotation, pivot)` with pivot `(0, 0, 0)`.
    fn transform(self, [x, y, z]: [i32; 3]) -> [i32; 3] {
        match self {
            Rotation::None => [x, y, z],
            Rotation::Counterclockwise90 => [z, y, x.wrapping_neg()],
            Rotation::Clockwise90 => [z.wrapping_neg(), y, x],
            Rotation::Clockwise180 => [x.wrapping_neg(), y, z.wrapping_neg()],
        }
    }

    /// `Rotation.rotate(Direction)`: horizontal directions turn, up and down stay.
    fn rotate(self, d: Direction) -> Direction {
        use Direction::{East, North, South, West};
        if !d.is_horizontal() {
            return d;
        }
        match (self, d) {
            (Rotation::None, d) => d,
            (Rotation::Clockwise180, d) => d.opposite(),
            (Rotation::Clockwise90, North) | (Rotation::Counterclockwise90, South) => East,
            (Rotation::Clockwise90, East) | (Rotation::Counterclockwise90, West) => South,
            (Rotation::Clockwise90, South) | (Rotation::Counterclockwise90, North) => West,
            (Rotation::Clockwise90 | Rotation::Counterclockwise90, _) => North,
        }
    }
}

/// `Direction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Direction {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

impl Direction {
    fn named(name: &str) -> Option<Self> {
        Some(match name {
            "down" => Direction::Down,
            "up" => Direction::Up,
            "north" => Direction::North,
            "south" => Direction::South,
            "west" => Direction::West,
            "east" => Direction::East,
            _ => return None,
        })
    }

    /// The unit step towards this direction.
    pub(crate) fn step(self) -> [i32; 3] {
        match self {
            Direction::Down => [0, -1, 0],
            Direction::Up => [0, 1, 0],
            Direction::North => [0, 0, -1],
            Direction::South => [0, 0, 1],
            Direction::West => [-1, 0, 0],
            Direction::East => [1, 0, 0],
        }
    }

    pub(crate) fn opposite(self) -> Self {
        match self {
            Direction::Down => Direction::Up,
            Direction::Up => Direction::Down,
            Direction::North => Direction::South,
            Direction::South => Direction::North,
            Direction::West => Direction::East,
            Direction::East => Direction::West,
        }
    }

    fn is_horizontal(self) -> bool {
        !matches!(self, Direction::Down | Direction::Up)
    }
}

/// A block-aligned box, inclusive (`BoundingBox`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoundingBox {
    /// Minimum corner.
    pub min: [i32; 3],
    /// Maximum corner.
    pub max: [i32; 3],
}

impl BoundingBox {
    fn from_corners(a: [i32; 3], b: [i32; 3]) -> Self {
        BoundingBox {
            min: [a[0].min(b[0]), a[1].min(b[1]), a[2].min(b[2])],
            max: [a[0].max(b[0]), a[1].max(b[1]), a[2].max(b[2])],
        }
    }

    /// `moved(dx, dy, dz)`.
    pub(crate) fn moved(self, by: [i32; 3]) -> Self {
        let add = |p: [i32; 3]| [0, 1, 2].map(|i| p[i].wrapping_add(by[i]));
        BoundingBox {
            min: add(self.min),
            max: add(self.max),
        }
    }

    /// The box around both (`encapsulate`).
    pub(crate) fn encapsulate(self, other: BoundingBox) -> Self {
        BoundingBox {
            min: [0, 1, 2].map(|i| self.min[i].min(other.min[i])),
            max: [0, 1, 2].map(|i| self.max[i].max(other.max[i])),
        }
    }

    /// `inflatedBy(n)`: grown by `n` on every side.
    pub(crate) fn inflated(self, n: i32) -> Self {
        BoundingBox {
            min: self.min.map(|v| v.wrapping_sub(n)),
            max: self.max.map(|v| v.wrapping_add(n)),
        }
    }

    /// `isInside(pos)`.
    pub(crate) fn contains(&self, p: [i32; 3]) -> bool {
        (0..3).all(|i| self.min[i] <= p[i] && p[i] <= self.max[i])
    }

    /// Whether every block of `other` is in this box.
    pub(crate) fn contains_box(&self, other: &BoundingBox) -> bool {
        (0..3).all(|i| self.min[i] <= other.min[i] && other.max[i] <= self.max[i])
    }

    /// Whether the boxes share a block.
    pub(crate) fn intersects(&self, other: &BoundingBox) -> bool {
        (0..3).all(|i| self.min[i] <= other.max[i] && other.min[i] <= self.max[i])
    }

    /// `getYSpan`.
    pub(crate) fn y_span(&self) -> i32 {
        self.max[1].wrapping_sub(self.min[1]).wrapping_add(1)
    }
}

/// `JigsawBlockEntity.JointType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Joint {
    Rollable,
    Aligned,
}

/// A jigsaw block's block-entity fields (`StructureTemplate.JigsawBlockInfo`), which don't
/// change with the piece's position or rotation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JigsawData {
    /// `name`, default `minecraft:empty`; none for a feature element's jigsaw, which any target
    /// matches.
    pub(crate) name: Option<String>,
    /// `pool`, default `minecraft:empty`.
    pub(crate) pool: String,
    /// `target`, default `minecraft:empty`.
    pub(crate) target: String,
    /// `joint`, default aligned for a horizontal front and rollable for a vertical one.
    pub(crate) joint: Joint,
    /// `placement_priority`, default 0.
    pub(crate) placement_priority: i32,
    /// `selection_priority`, default 0.
    pub(crate) selection_priority: i32,
}

/// `FeaturePoolElement.getShuffledJigsawBlocks`'s one jigsaw: facing down, top south, rollable,
/// no name, the empty pool and target.
static FEATURE_JIGSAW: LazyLock<JigsawData> = LazyLock::new(|| JigsawData {
    name: None,
    pool: EMPTY_POOL.to_string(),
    target: "minecraft:empty".to_string(),
    joint: Joint::Rollable,
    placement_priority: 0,
    selection_priority: 0,
});

/// [`FEATURE_JIGSAW`] as versions with the `feature_jigsaw_named_bottom` switch have it: named
/// `minecraft:bottom`, so only a parent targeting that attaches (ADR 0026).
static FEATURE_JIGSAW_NAMED_BOTTOM: LazyLock<JigsawData> = LazyLock::new(|| JigsawData {
    name: Some("minecraft:bottom".to_string()),
    ..FEATURE_JIGSAW.clone()
});

/// A jigsaw block in a template: its position and orientation (`JigsawBlock.ORIENTATION`, front
/// and top) in the template, and its fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TemplateJigsaw {
    pos: [i32; 3],
    front: Direction,
    top: Direction,
    data: JigsawData,
}

/// A jigsaw block placed with its piece: moved and rotated.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PlacedJigsaw<'a> {
    pub(crate) pos: [i32; 3],
    pub(crate) front: Direction,
    pub(crate) top: Direction,
    pub(crate) data: &'a JigsawData,
}

impl PlacedJigsaw<'_> {
    /// `JigsawBlock.canAttach(self, child)`: the child faces this one, their tops match unless
    /// this one's joint is rollable, and the child's name is this one's target.
    pub(crate) fn can_attach(&self, child: &PlacedJigsaw<'_>) -> bool {
        self.front == child.front.opposite()
            && (self.data.joint == Joint::Rollable || self.top == child.top)
            && child
                .data
                .name
                .as_ref()
                .is_none_or(|name| *name == self.data.target)
    }
}

/// A structure template (`StructureTemplate`), reduced to its size and jigsaw blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    /// `size`.
    pub size: [i32; 3],
    /// Each palette's jigsaw blocks, in the template's block order: sorted by y, then x, then z.
    palettes: Vec<Vec<TemplateJigsaw>>,
}

impl Template {
    /// `StructureTemplate.load`, keeping the size and the jigsaw blocks of each palette
    /// (`palettes`, or the single `palette`). Palette entries name their block by `id` and
    /// hold its state in `properties` (`NbtUtils.readBlockState`). The game data-fixes a template saved by an older version
    /// before loading it; spyglass doesn't, so `DataVersion` must be `data_version`.
    pub fn from_nbt(root: &Tag, id: &str, data_version: u32) -> LoadResult<Self> {
        let at = At::root(id);
        let saved = root.get("DataVersion").and_then(Tag::as_int);
        if saved.and_then(|v| u32::try_from(v).ok()) != Some(data_version) {
            return Err(at.child("DataVersion").error(LoadErrorKind::Unsupported {
                what: "template data version",
                name: format!("{saved:?}; only {data_version} is read without data fixing"),
            }));
        }
        let size = int3(root.get("size"), &at.child("size"))?;
        let blocks = root
            .get("blocks")
            .and_then(Tag::as_list)
            .ok_or_else(|| at.error(LoadErrorKind::Missing("blocks")))?;
        let palettes: Vec<&[Tag]> = match root.get("palettes").and_then(Tag::as_list) {
            Some(list) => list.iter().filter_map(Tag::as_list).collect(),
            None => vec![
                root.get("palette")
                    .and_then(Tag::as_list)
                    .ok_or_else(|| at.error(LoadErrorKind::Missing("palette")))?,
            ],
        };
        let palettes = palettes
            .into_iter()
            .map(|palette| {
                // Jigsaw blocks have block-entity NBT, so they're all in the template's list of
                // block entities, which is stably sorted by (y, x, z).
                let mut jigsaws = Vec::new();
                for (i, block) in blocks.iter().enumerate() {
                    let state = block.get("state").and_then(Tag::as_int).unwrap_or(0);
                    let entry = usize::try_from(state).ok().and_then(|s| palette.get(s));
                    // `NbtUtils.readBlockState`: `id` and `properties`, or in 26.2 `Name` and
                    // `Properties`; the template says which.
                    let name = entry
                        .and_then(|p| p.get("id").or_else(|| p.get("Name")))
                        .and_then(Tag::as_str);
                    if name.map(full_id).as_deref() != Some("minecraft:jigsaw") {
                        continue;
                    }
                    let blocks_at = at.child("blocks");
                    let at = blocks_at.child(&format!("[{i}]"));
                    let pos = int3(block.get("pos"), &at.child("pos"))?;
                    // The orientation property, `<front>_<top>`, in the entry's `properties`;
                    // the default state is north_up.
                    let (front, top) = entry
                        .and_then(|p| p.get("properties").or_else(|| p.get("Properties")))
                        .and_then(|p| p.get("orientation"))
                        .and_then(Tag::as_str)
                        .and_then(|o| o.split_once('_'))
                        .and_then(|(f, t)| Some((Direction::named(f)?, Direction::named(t)?)))
                        .unwrap_or((Direction::North, Direction::Up));
                    jigsaws.push(TemplateJigsaw {
                        pos,
                        front,
                        top,
                        data: jigsaw_data(block.get("nbt"), front),
                    });
                }
                jigsaws.sort_by_key(|j| (j.pos[1], j.pos[0], j.pos[2]));
                Ok(jigsaws)
            })
            .collect::<LoadResult<Vec<_>>>()?;
        Ok(Template { size, palettes })
    }

    /// `StructureTemplate.getBoundingBox(settings with rotation, pos)`: the box of the rotated
    /// corners `(0, 0, 0)` and `size - 1`, moved to `pos`.
    pub fn bounding_box(&self, pos: [i32; 3], rotation: Rotation) -> BoundingBox {
        let far = self.size.map(|s| s.wrapping_sub(1));
        BoundingBox::from_corners(rotation.transform([0; 3]), rotation.transform(far)).moved(pos)
    }

    /// `StructureTemplate.getJigsaws(pos, rotation)`: one palette's jigsaw blocks, rotated and
    /// moved to `pos`. With several palettes, one is picked by a random seeded from `pos`
    /// (`StructurePlaceSettings.getRandomPalette`), not from the structure's random.
    fn jigsaws(&self, pos: [i32; 3], rotation: Rotation) -> Vec<PlacedJigsaw<'_>> {
        if self.palettes.is_empty() {
            return Vec::new();
        }
        let mut palette_random =
            LegacyRandomSource::new(spyglass_rng::block_seed(pos[0], pos[1], pos[2]));
        let palette =
            &self.palettes[palette_random.next_int_bounded(self.palettes.len() as i32) as usize];
        palette
            .iter()
            .map(|j| {
                let r = rotation.transform(j.pos);
                PlacedJigsaw {
                    pos: [0, 1, 2].map(|i| r[i].wrapping_add(pos[i])),
                    front: rotation.rotate(j.front),
                    top: rotation.rotate(j.top),
                    data: &j.data,
                }
            })
            .collect()
    }

    /// Every pool any of the template's jigsaw blocks names.
    fn pools(&self) -> impl Iterator<Item = &str> {
        self.palettes.iter().flatten().map(|j| j.data.pool.as_str())
    }
}

/// `JigsawBlockInfo.parse`: the block entity's fields, with their defaults. Names, pools and
/// targets are identifiers; the joint defaults by the front's axis
/// (`StructureTemplate.getDefaultJointType`).
fn jigsaw_data(entity: Option<&Tag>, front: Direction) -> JigsawData {
    let string = |key: &str| {
        entity
            .and_then(|n| n.get(key))
            .and_then(Tag::as_str)
            .map_or_else(|| "minecraft:empty".to_string(), full_id)
    };
    let int = |key: &str| {
        entity
            .and_then(|n| n.get(key))
            .and_then(Tag::as_int)
            .unwrap_or(0)
    };
    let joint = match entity.and_then(|n| n.get("joint")).and_then(Tag::as_str) {
        Some("rollable") => Joint::Rollable,
        Some("aligned") => Joint::Aligned,
        _ if front.is_horizontal() => Joint::Aligned,
        _ => Joint::Rollable,
    };
    JigsawData {
        name: Some(string("name")),
        pool: string("pool"),
        target: string("target"),
        joint,
        placement_priority: int("placement_priority"),
        selection_priority: int("selection_priority"),
    }
}

fn int3(tag: Option<&Tag>, at: &At<'_>) -> LoadResult<[i32; 3]> {
    let list = tag
        .and_then(Tag::as_list)
        .filter(|l| l.len() == 3)
        .ok_or_else(|| at.error(LoadErrorKind::Shape("a list of three ints")))?;
    let mut out = [0; 3];
    for (o, t) in out.iter_mut().zip(list) {
        *o = t
            .as_int()
            .ok_or_else(|| at.error(LoadErrorKind::Shape("a list of three ints")))?;
    }
    Ok(out)
}

/// `StructureTemplatePool.Projection`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Projection {
    /// `rigid`: placed against its parent piece.
    Rigid,
    /// `terrain_matching`: placed on the surface.
    TerrainMatching,
}

/// A pool element (`StructurePoolElement`).
#[derive(Debug, Clone)]
pub enum PoolElement {
    /// `single_pool_element` or `legacy_single_pool_element`: one template.
    Single(Arc<Template>, Projection),
    /// `list_pool_element`: several elements placed together.
    List(Vec<PoolElement>, Projection),
    /// `feature_pool_element`: a placed feature, with a 1 × 1 × 1 box. The flag is the
    /// `feature_jigsaw_named_bottom` switch: whether its jigsaw is named `minecraft:bottom`.
    Feature(Projection, bool),
    /// `empty_pool_element`.
    Empty,
}

impl PoolElement {
    /// `getProjection`. The empty element is never placed, so its projection doesn't matter.
    pub(crate) fn projection(&self) -> Projection {
        match self {
            PoolElement::Single(_, p) | PoolElement::List(_, p) | PoolElement::Feature(p, _) => *p,
            PoolElement::Empty => Projection::Rigid,
        }
    }

    /// `StructurePoolElement.getBoundingBox(manager, pos, rotation)`.
    pub(crate) fn bounding_box(&self, pos: [i32; 3], rotation: Rotation) -> Option<BoundingBox> {
        match self {
            PoolElement::Single(template, _) => Some(template.bounding_box(pos, rotation)),
            // ListPoolElement: the box around every non-empty element's box.
            PoolElement::List(elements, _) => elements
                .iter()
                .filter_map(|e| e.bounding_box(pos, rotation))
                .reduce(BoundingBox::encapsulate),
            PoolElement::Feature(..) => Some(BoundingBox { min: pos, max: pos }),
            PoolElement::Empty => None,
        }
    }

    /// `StructurePoolElement.getShuffledJigsawBlocks(manager, pos, rotation, random)`: shuffled
    /// with `Util.shuffle`, then stably sorted by selection priority, highest first. A list
    /// element uses its first element's; a feature element has one jigsaw, without drawing.
    pub(crate) fn shuffled_jigsaws(
        &self,
        pos: [i32; 3],
        rotation: Rotation,
        random: &mut LegacyRandomSource,
    ) -> Vec<PlacedJigsaw<'_>> {
        match self {
            PoolElement::Single(template, _) => {
                let mut jigsaws = template.jigsaws(pos, rotation);
                shuffle(&mut jigsaws, random);
                jigsaws.sort_by_key(|j| std::cmp::Reverse(j.data.selection_priority));
                jigsaws
            }
            PoolElement::List(elements, _) => elements
                .first()
                .map(|e| e.shuffled_jigsaws(pos, rotation, random))
                .unwrap_or_default(),
            PoolElement::Feature(_, named_bottom) => vec![PlacedJigsaw {
                pos,
                front: Direction::Down,
                top: Direction::South,
                data: if *named_bottom {
                    &FEATURE_JIGSAW_NAMED_BOTTOM
                } else {
                    &FEATURE_JIGSAW
                },
            }],
            PoolElement::Empty => Vec::new(),
        }
    }

    /// Every template the element places.
    fn templates(&self) -> Vec<&Template> {
        match self {
            PoolElement::Single(t, _) => vec![t],
            PoolElement::List(elements, _) => elements.iter().flat_map(|e| e.templates()).collect(),
            PoolElement::Feature(..) | PoolElement::Empty => Vec::new(),
        }
    }
}

/// `Util.shuffle(list, random)`: Fisher-Yates from the end, `nextInt(i)` for each `i` from the
/// length down to 2.
pub(crate) fn shuffle<T>(list: &mut [T], random: &mut LegacyRandomSource) {
    for i in (2..=list.len()).rev() {
        let j = random.next_int_bounded(i as i32) as usize;
        list.swap(i - 1, j);
    }
}

/// A template pool (`StructureTemplatePool`): its elements, each repeated `weight` times, in
/// file order.
#[derive(Debug, Clone)]
pub struct Pool {
    /// The weight-expanded list `getRandomTemplate` indexes.
    pub templates: Vec<PoolElement>,
    /// `fallback`, a pool id.
    pub(crate) fallback: String,
    /// `getMaxSize`: the tallest non-empty element, unrotated (`getYSpan`); 0 if none.
    pub(crate) max_size: i32,
}

impl Pool {
    /// `getRandomTemplate(random)`: `templates[nextInt(len)]`, or empty for an empty pool.
    fn random_template(&self, random: &mut LegacyRandomSource) -> &PoolElement {
        if self.templates.is_empty() {
            return &PoolElement::Empty;
        }
        &self.templates[random.next_int_bounded(self.templates.len() as i32) as usize]
    }
}

/// A vertical anchor (`VerticalAnchor`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerticalAnchor {
    /// `absolute`.
    Absolute(i32),
    /// `above_bottom`: offset from the generation context's minimum y.
    AboveBottom(i32),
    /// `below_top`: offset down from its top.
    BelowTop(i32),
}

impl VerticalAnchor {
    /// `resolveY(context)`.
    fn resolve(self, heights: &GenerationHeights) -> i32 {
        match self {
            VerticalAnchor::Absolute(y) => y,
            VerticalAnchor::AboveBottom(o) => heights.min_y.wrapping_add(o),
            VerticalAnchor::BelowTop(o) => heights
                .height
                .wrapping_sub(1)
                .wrapping_add(heights.min_y)
                .wrapping_sub(o),
        }
    }
}

/// A height provider (`HeightProvider`), the kinds jigsaw starts and nether fossils use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeightProvider {
    /// `constant`, or a bare anchor.
    Constant(VerticalAnchor),
    /// `uniform`: `Mth.randomBetweenInclusive(min, max)`; the minimum alone if the range is
    /// empty.
    Uniform(VerticalAnchor, VerticalAnchor),
}

impl HeightProvider {
    /// `sample(random, context)`.
    pub(crate) fn sample(
        self,
        random: &mut LegacyRandomSource,
        heights: &GenerationHeights,
    ) -> i32 {
        match self {
            HeightProvider::Constant(a) => a.resolve(heights),
            HeightProvider::Uniform(lo, hi) => {
                let (lo, hi) = (lo.resolve(heights), hi.resolve(heights));
                if lo > hi {
                    lo
                } else {
                    random
                        .next_int_bounded(hi.wrapping_sub(lo).wrapping_add(1))
                        .wrapping_add(lo)
                }
            }
        }
    }
}

/// A dimension's heights, as structures see them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GenerationHeights {
    /// `WorldGenerationContext.getMinGenY()`: the larger of the dimension type's and the noise
    /// settings' minimum y.
    pub min_y: i32,
    /// `getGenDepth()`: the smaller of the two heights.
    pub height: i32,
    /// The dimension type's (the level's) minimum y.
    pub level_min_y: i32,
    /// The dimension type's height.
    pub level_height: i32,
}

impl GenerationHeights {
    pub(crate) fn level_max_y(&self) -> i32 {
        self.level_min_y
            .wrapping_add(self.level_height)
            .wrapping_sub(1)
    }
}

/// A pool alias binding (`PoolAliasBinding`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AliasBinding {
    /// `direct`: `alias` is `target`.
    Direct { alias: String, target: String },
    /// `random`: `alias` is one of `targets`, by weight.
    Random {
        alias: String,
        targets: Vec<(String, i32)>,
    },
    /// `random_group`: one group of bindings, by weight, each resolved in order.
    RandomGroup {
        groups: Vec<(Vec<AliasBinding>, i32)>,
    },
}

/// A jigsaw structure's settings (`JigsawStructure`).
#[derive(Debug, Clone)]
pub struct Jigsaw {
    /// `start_pool`'s id.
    pub start_pool_id: String,
    /// `start_pool`, loaded.
    pub start_pool: Arc<Pool>,
    /// `start_jigsaw_name`.
    pub start_jigsaw_name: Option<String>,
    /// `size`: the maximum depth of the piece tree, 0 to 20.
    pub size: i32,
    /// `start_height`.
    pub start_height: HeightProvider,
    /// `use_expansion_hack`.
    pub use_expansion_hack: bool,
    /// `project_start_to_heightmap`: the heightmap's name, if projected.
    pub project_start_to_heightmap: Option<String>,
    /// `max_distance_from_center`: (horizontal, vertical).
    pub max_distance: (i32, i32),
    /// `pool_aliases`.
    pub(crate) pool_aliases: Vec<AliasBinding>,
    /// `dimension_padding`: (bottom, top). `None` when absent: the default,
    /// `DimensionPadding.ZERO`, which the game tests by identity and doesn't check. A value in
    /// the JSON, zeros included, is checked.
    pub dimension_padding: Option<(i32, i32)>,
    /// Whether the structure's `terrain_adaptation` isn't `none`, which grows its bounds.
    pub adapts_terrain: bool,
}

/// Where a jigsaw start's biome check samples.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JigsawCheck {
    /// No start: an empty start pool, a missing start jigsaw, or a start too close to the world
    /// height limits.
    NoStart,
    /// The check is at this block position.
    At([i32; 3]),
    /// The start is projected to a heightmap, so y needs terrain. The game first requires
    /// some biome in the structure's set in the level's column at `(x, z)`
    /// (`couldStructureExistInColumn`). Then the piece's y is `start_y` plus the heightmap's
    /// first free height at `(x, z)`, which must fit the padding
    /// ([`Jigsaw::fits_padding`]), and the check is at `(x, y + offset_y, z)`.
    Projected {
        /// Block x of the check.
        x: i32,
        /// Block z of the check.
        z: i32,
        /// The start height, before the heightmap is added.
        start_y: i32,
        /// The start jigsaw's offset in y, added for the check.
        offset_y: i32,
        /// The start piece's box in y, before it's moved to its height.
        box_y: [i32; 2],
        /// The heightmap, e.g. `WORLD_SURFACE_WG`.
        heightmap: String,
    },
}

/// The start piece, before it's moved to its height.
pub(crate) struct StartPiece<'a> {
    /// `(minX, start height, minZ)`.
    pub(crate) start: [i32; 3],
    pub(crate) rotation: Rotation,
    pub(crate) element: &'a PoolElement,
    /// The start jigsaw's position relative to `start`, or zero.
    pub(crate) offset: [i32; 3],
    /// The piece's position, `start - offset`.
    pub(crate) pos: [i32; 3],
    /// The element's box at `pos`.
    pub(crate) bbox: BoundingBox,
}

impl StartPiece<'_> {
    /// The check's x and z: the box's centre, with Java's truncating division.
    pub(crate) fn center(&self) -> [i32; 2] {
        [
            self.bbox.max[0].wrapping_add(self.bbox.min[0]) / 2,
            self.bbox.max[2].wrapping_add(self.bbox.min[2]) / 2,
        ]
    }
}

impl Jigsaw {
    /// `findGenerationPoint` and `addPieces` up to the start piece, drawing from `random` (the
    /// structure's `setLargeFeatureSeed` random) in the game's order. `None` for no start.
    pub(crate) fn start_piece(
        &self,
        chunk: [i32; 2],
        heights: &GenerationHeights,
        random: &mut LegacyRandomSource,
    ) -> Option<StartPiece<'_>> {
        let start = [
            chunk[0].wrapping_shl(4),
            self.start_height.sample(random, heights),
            chunk[1].wrapping_shl(4),
        ];
        let rotation = Rotation::random(random);
        let element = self.start_pool.random_template(random);
        if matches!(element, PoolElement::Empty) {
            return None;
        }
        let offset = match &self.start_jigsaw_name {
            None => [0; 3],
            Some(name) => {
                let jigsaws = element.shuffled_jigsaws(start, rotation, random);
                let j = jigsaws
                    .iter()
                    .find(|j| j.data.name.as_ref() == Some(name))?;
                [0, 1, 2].map(|i| j.pos[i].wrapping_sub(start[i]))
            }
        };
        let pos = [0, 1, 2].map(|i| start[i].wrapping_sub(offset[i]));
        let bbox = element.bounding_box(pos, rotation)?;
        Some(StartPiece {
            start,
            rotation,
            element,
            offset,
            pos,
            bbox,
        })
    }

    /// `findGenerationPoint` up to the biome check, drawing from `random` (the structure's
    /// `setLargeFeatureSeed` random) in the game's order.
    pub fn check_position(
        &self,
        chunk: [i32; 2],
        heights: &GenerationHeights,
        random: &mut LegacyRandomSource,
    ) -> JigsawCheck {
        let Some(piece) = self.start_piece(chunk, heights, random) else {
            return JigsawCheck::NoStart;
        };
        let [x, z] = piece.center();
        let box_y = [piece.bbox.min[1], piece.bbox.max[1]];
        match &self.project_start_to_heightmap {
            Some(heightmap) => JigsawCheck::Projected {
                x,
                z,
                start_y: piece.start[1],
                offset_y: piece.offset[1],
                box_y,
                heightmap: heightmap.clone(),
            },
            None => {
                let y = piece.pos[1];
                if !self.fits_padding(y, box_y, heights) {
                    return JigsawCheck::NoStart;
                }
                JigsawCheck::At([x, y.wrapping_add(piece.offset[1]), z])
            }
        }
    }

    /// `isStartTooCloseToWorldHeightLimits`, negated: the start piece, whose box spans `box_y`,
    /// moves so its box's minimum y plus the ground level delta (1) is at `y`; with
    /// `dimension_padding`, it must then fit inside the padded level height.
    pub fn fits_padding(&self, y: i32, box_y: [i32; 2], heights: &GenerationHeights) -> bool {
        let Some((bottom, top)) = self.dimension_padding else {
            return true;
        };
        let dy = y.wrapping_sub(box_y[0].wrapping_add(1));
        let (min_y, max_y) = (box_y[0].wrapping_add(dy), box_y[1].wrapping_add(dy));
        min_y >= heights.level_min_y.wrapping_add(bottom)
            && max_y <= heights.level_max_y().wrapping_sub(top)
    }

    /// Every pool id the structure names directly: its start pool and its aliases' targets.
    pub(crate) fn named_pools(&self) -> Vec<String> {
        fn targets(b: &AliasBinding, out: &mut Vec<String>) {
            match b {
                AliasBinding::Direct { target, .. } => out.push(target.clone()),
                AliasBinding::Random { targets, .. } => {
                    out.extend(targets.iter().map(|(t, _)| t.clone()))
                }
                AliasBinding::RandomGroup { groups } => {
                    for (group, _) in groups {
                        for b in group {
                            targets(b, out);
                        }
                    }
                }
            }
        }
        let mut out = vec![self.start_pool_id.clone()];
        for b in &self.pool_aliases {
            targets(b, &mut out);
        }
        out
    }
}

/// Every `alias` string anywhere in a `pool_aliases` value.
fn alias_names(json: &Value) -> Vec<&str> {
    match json {
        Value::Array(items) => items.iter().flat_map(alias_names).collect(),
        Value::Object(obj) => obj
            .iter()
            .flat_map(|(k, v)| match (k.as_str(), v) {
                ("alias", Value::String(s)) => vec![s.as_str()],
                _ => alias_names(v),
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Loads template pools and templates, each once.
pub(crate) struct JigsawLoader<'a> {
    bundle: &'a Bundle,
    /// The `feature_jigsaw_named_bottom` switch for the bundle's version.
    feature_jigsaw_named_bottom: bool,
    pools: HashMap<String, Arc<Pool>>,
    templates: HashMap<String, Arc<Template>>,
}

impl<'a> JigsawLoader<'a> {
    pub(crate) fn new(bundle: &'a Bundle) -> Self {
        JigsawLoader {
            bundle,
            feature_jigsaw_named_bottom: Behavior::of(bundle.manifest().data_version)
                .feature_jigsaw_named_bottom,
            pools: HashMap::new(),
            templates: HashMap::new(),
        }
    }

    /// `JigsawStructure.CODEC`'s fields, and whether the structure adapts terrain.
    pub(crate) fn jigsaw(&mut self, obj: &Map<String, Value>, at: &At<'_>) -> LoadResult<Jigsaw> {
        let pool_id = full_id(
            obj.get("start_pool")
                .and_then(Value::as_str)
                .ok_or_else(|| at.error(LoadErrorKind::Missing("start_pool")))?,
        );
        // Pool aliases rename pools by their `alias` fields; renaming the start pool isn't
        // supported, so the start pool is loaded with the structure.
        let pool_aliases = match obj.get("pool_aliases") {
            None => Vec::new(),
            Some(aliases) => {
                if alias_names(aliases).iter().any(|a| full_id(a) == pool_id) {
                    return Err(at.child("pool_aliases").error(LoadErrorKind::Unsupported {
                        what: "pool alias of the start pool",
                        name: pool_id,
                    }));
                }
                let at = at.child("pool_aliases");
                aliases
                    .as_array()
                    .ok_or_else(|| at.error(LoadErrorKind::Shape("a list")))?
                    .iter()
                    .enumerate()
                    .map(|(i, b)| alias_binding(b, &at.child(&format!("[{i}]"))))
                    .collect::<LoadResult<_>>()?
            }
        };
        let start_height = height_provider(
            obj.get("start_height")
                .ok_or_else(|| at.error(LoadErrorKind::Missing("start_height")))?,
            &at.child("start_height"),
        )?;
        let size = obj
            .get("size")
            .ok_or_else(|| at.error(LoadErrorKind::Missing("size")))?;
        let size = codec::int_of(size)
            .filter(|s| (0..=20).contains(s))
            .ok_or_else(|| {
                at.child("size")
                    .error(LoadErrorKind::Shape("an int in 0..=20"))
            })?;
        let use_expansion_hack = obj
            .get("use_expansion_hack")
            .ok_or_else(|| at.error(LoadErrorKind::Missing("use_expansion_hack")))?
            .as_bool()
            .ok_or_else(|| {
                at.child("use_expansion_hack")
                    .error(LoadErrorKind::Shape("a boolean"))
            })?;
        let project_start_to_heightmap = match obj.get("project_start_to_heightmap") {
            None => None,
            Some(v) => Some(
                v.as_str()
                    .ok_or_else(|| {
                        at.child("project_start_to_heightmap")
                            .error(LoadErrorKind::Shape("a heightmap name"))
                    })?
                    .to_string(),
            ),
        };
        let max_distance = max_distance(
            obj.get("max_distance_from_center")
                .ok_or_else(|| at.error(LoadErrorKind::Missing("max_distance_from_center")))?,
            &at.child("max_distance_from_center"),
        )?;
        // DimensionPadding.CODEC: an int for both, or {bottom, top}, each defaulting to 0.
        // Decoding makes a new padding, never DimensionPadding.ZERO.
        let dimension_padding = match obj.get("dimension_padding") {
            None => None,
            Some(v) => {
                let at = at.child("dimension_padding");
                let (bottom, top) = match v {
                    Value::Object(p) => (
                        p.get("bottom").and_then(codec::int_of).unwrap_or(0),
                        p.get("top").and_then(codec::int_of).unwrap_or(0),
                    ),
                    _ => {
                        let n = codec::int_of(v).ok_or_else(|| {
                            at.error(LoadErrorKind::Shape("an int or {bottom, top}"))
                        })?;
                        (n, n)
                    }
                };
                Some((bottom, top))
            }
        };
        let start_jigsaw_name = obj
            .get("start_jigsaw_name")
            .and_then(Value::as_str)
            .map(full_id);
        // Structure.StructureSettings: terrain_adaptation defaults to none.
        let adapts_terrain = obj
            .get("terrain_adaptation")
            .and_then(Value::as_str)
            .is_some_and(|t| t != "none");
        Ok(Jigsaw {
            start_pool: self.pool(&pool_id)?,
            start_pool_id: pool_id,
            start_jigsaw_name,
            size,
            start_height,
            use_expansion_hack,
            project_start_to_heightmap,
            max_distance,
            pool_aliases,
            dimension_padding,
            adapts_terrain,
        })
    }

    /// A pool by id, loaded once.
    pub(crate) fn pool(&mut self, id: &str) -> LoadResult<Arc<Pool>> {
        self.optional_pool(id)?.ok_or_else(|| {
            At::root(id).error(LoadErrorKind::Missing("template pool in the bundle"))
        })
    }

    /// `StructureTemplatePool.DIRECT_CODEC`: `fallback`, and `elements`, each `{weight,
    /// element}` with weight in 1..=150. `None` if the bundle has no such pool.
    pub(crate) fn optional_pool(&mut self, id: &str) -> LoadResult<Option<Arc<Pool>>> {
        if let Some(pool) = self.pools.get(id) {
            return Ok(Some(pool.clone()));
        }
        let json = match self.bundle.resource(TEMPLATE_POOL, id) {
            Ok(json) => json,
            Err(BundleError::NotFound(_)) => return Ok(None),
            Err(e) => return Err(At::root(id).error(e.into())),
        };
        let at = At::root(id);
        let fallback = full_id(
            json.get("fallback")
                .and_then(Value::as_str)
                .ok_or_else(|| at.error(LoadErrorKind::Missing("fallback")))?,
        );
        let elements_at = at.child("elements");
        let elements = json
            .get("elements")
            .and_then(Value::as_array)
            .ok_or_else(|| at.error(LoadErrorKind::Missing("elements")))?;
        let mut templates = Vec::new();
        for (i, entry) in elements.iter().enumerate() {
            let at = elements_at.child(&format!("[{i}]"));
            let weight = entry
                .get("weight")
                .and_then(codec::int_of)
                .filter(|w| (1..=150).contains(w))
                .ok_or_else(|| {
                    at.child("weight")
                        .error(LoadErrorKind::Shape("an int in 1..=150"))
                })?;
            let element = self.element(
                entry
                    .get("element")
                    .ok_or_else(|| at.error(LoadErrorKind::Missing("element")))?,
                &at.child("element"),
            )?;
            templates.extend(std::iter::repeat_n(element, weight as usize));
        }
        let max_size = templates
            .iter()
            .filter_map(|e| e.bounding_box([0; 3], Rotation::None))
            .map(|b| b.y_span())
            .max()
            .unwrap_or(0);
        let pool = Arc::new(Pool {
            templates,
            fallback,
            max_size,
        });
        self.pools.insert(id.to_string(), pool.clone());
        Ok(Some(pool))
    }

    /// The pools a loaded pool leads to: its fallback, and every pool its elements' jigsaw
    /// blocks name.
    pub(crate) fn pools_named_by(pool: &Pool) -> Vec<String> {
        let mut out = vec![pool.fallback.clone()];
        for element in &pool.templates {
            for template in element.templates() {
                out.extend(template.pools().map(str::to_string));
            }
            if matches!(element, PoolElement::Feature(..)) {
                out.push(FEATURE_JIGSAW.pool.clone());
            }
        }
        out
    }

    fn element(&mut self, json: &Value, at: &At<'_>) -> LoadResult<PoolElement> {
        let kind = json
            .get("element_type")
            .and_then(Value::as_str)
            .ok_or_else(|| at.error(LoadErrorKind::Missing("element_type")))?;
        let kind = full_id(kind);
        if kind == "minecraft:empty_pool_element" {
            return Ok(PoolElement::Empty);
        }
        let projection = match json.get("projection").and_then(Value::as_str) {
            Some("rigid") => Projection::Rigid,
            Some("terrain_matching") => Projection::TerrainMatching,
            _ => {
                return Err(at
                    .child("projection")
                    .error(LoadErrorKind::Shape("\"rigid\" or \"terrain_matching\"")));
            }
        };
        Ok(match kind.as_str() {
            "minecraft:single_pool_element" | "minecraft:legacy_single_pool_element" => {
                let location = json
                    .get("location")
                    .and_then(Value::as_str)
                    .ok_or_else(|| at.error(LoadErrorKind::Missing("location")))?;
                PoolElement::Single(self.template(&full_id(location))?, projection)
            }
            "minecraft:list_pool_element" => {
                let list_at = at.child("elements");
                let items = json
                    .get("elements")
                    .and_then(Value::as_array)
                    .ok_or_else(|| at.error(LoadErrorKind::Missing("elements")))?;
                let mut elements = Vec::with_capacity(items.len());
                for (i, item) in items.iter().enumerate() {
                    elements.push(self.element(item, &list_at.child(&format!("[{i}]")))?);
                }
                PoolElement::List(elements, projection)
            }
            "minecraft:feature_pool_element" => {
                PoolElement::Feature(projection, self.feature_jigsaw_named_bottom)
            }
            _ => {
                return Err(at.child("element_type").error(LoadErrorKind::Unsupported {
                    what: "pool element type",
                    name: kind,
                }));
            }
        })
    }

    /// A structure template by id, loaded once. A template the bundle doesn't have is empty
    /// (`StructureTemplateManager.getOrCreate`): size zero and no palettes, as the game makes
    /// it. 26.3's own pools name one (`ancient_city/walls/intact_horizontal_wall_stairs_5`).
    pub(crate) fn template(&mut self, id: &str) -> LoadResult<Arc<Template>> {
        if let Some(t) = self.templates.get(id) {
            return Ok(t.clone());
        }
        let error = |kind: LoadErrorKind| LoadError::new(id, "", kind);
        let template = match self.bundle.structure_template(id) {
            Ok(bytes) => {
                let (_, root) = nbt::read_compressed(&bytes)
                    .map_err(|e| error(LoadErrorKind::Invalid(nbt_error(&e))))?;
                let data_version = self.bundle.manifest().data_version;
                Template::from_nbt(&root, id, data_version)?
            }
            Err(BundleError::NotFound(_)) => Template {
                size: [0; 3],
                palettes: Vec::new(),
            },
            Err(e) => return Err(error(e.into())),
        };
        let template = Arc::new(template);
        self.templates.insert(id.to_string(), template.clone());
        Ok(template)
    }
}

fn nbt_error(e: &nbt::NbtError) -> &'static str {
    match e {
        nbt::NbtError::Gzip(_) => "not gzip-compressed NBT",
        _ => "malformed NBT",
    }
}

/// `JigsawStructure.MaxDistance.CODEC`: an int in 1..=128 for both, or `{horizontal, vertical}`.
fn max_distance(json: &Value, at: &At<'_>) -> LoadResult<(i32, i32)> {
    let shape = || {
        at.error(LoadErrorKind::Shape(
            "an int in 1..=128 or {horizontal, vertical}",
        ))
    };
    match json {
        Value::Object(obj) => {
            let field = |name: &'static str| {
                obj.get(name)
                    .and_then(codec::int_of)
                    .ok_or_else(|| at.error(LoadErrorKind::Missing(name)))
            };
            let horizontal = field("horizontal")?;
            if !(1..=128).contains(&horizontal) {
                return Err(shape());
            }
            Ok((horizontal, field("vertical")?))
        }
        _ => {
            let n = codec::int_of(json)
                .filter(|n| (1..=128).contains(n))
                .ok_or_else(shape)?;
            Ok((n, n))
        }
    }
}

/// `PoolAliasBinding.CODEC`, dispatched on `type`: `direct` `{alias, target}`, `random`
/// `{alias, targets}` and `random_group` `{groups}`, the lists weighted (`{data, weight}`).
fn alias_binding(json: &Value, at: &At<'_>) -> LoadResult<AliasBinding> {
    let obj = json
        .as_object()
        .ok_or_else(|| at.error(LoadErrorKind::Shape("a pool alias binding")))?;
    let string = |name: &'static str| {
        obj.get(name)
            .and_then(Value::as_str)
            .map(full_id)
            .ok_or_else(|| at.error(LoadErrorKind::Missing(name)))
    };
    let weighted = |name: &'static str| -> LoadResult<Vec<(&Value, i32)>> {
        let at = at.child(name);
        obj.get(name)
            .and_then(Value::as_array)
            .ok_or_else(|| at.error(LoadErrorKind::Shape("a weighted list")))?
            .iter()
            .map(|e| {
                let data = e
                    .get("data")
                    .ok_or_else(|| at.error(LoadErrorKind::Missing("data")))?;
                let weight = e
                    .get("weight")
                    .and_then(codec::int_of)
                    .filter(|&w| w > 0)
                    .ok_or_else(|| at.error(LoadErrorKind::Shape("a positive weight")))?;
                Ok((data, weight))
            })
            .collect()
    };
    let kind = string("type")?;
    match kind.as_str() {
        "minecraft:direct" => Ok(AliasBinding::Direct {
            alias: string("alias")?,
            target: string("target")?,
        }),
        "minecraft:random" => Ok(AliasBinding::Random {
            alias: string("alias")?,
            targets: weighted("targets")?
                .into_iter()
                .map(|(d, w)| {
                    d.as_str()
                        .map(|t| (full_id(t), w))
                        .ok_or_else(|| at.child("targets").error(LoadErrorKind::Shape("a pool id")))
                })
                .collect::<LoadResult<_>>()?,
        }),
        "minecraft:random_group" => Ok(AliasBinding::RandomGroup {
            groups: weighted("groups")?
                .into_iter()
                .map(|(d, w)| {
                    let at = at.child("groups");
                    let bindings = d
                        .as_array()
                        .ok_or_else(|| at.error(LoadErrorKind::Shape("a list of bindings")))?
                        .iter()
                        .map(|b| alias_binding(b, &at))
                        .collect::<LoadResult<_>>()?;
                    Ok((bindings, w))
                })
                .collect::<LoadResult<_>>()?,
        }),
        _ => Err(at.child("type").error(LoadErrorKind::Unsupported {
            what: "pool alias binding type",
            name: kind,
        })),
    }
}

/// `HeightProvider.CODEC` for the kinds jigsaw starts use: a bare anchor (constant),
/// `{"type": "constant", "value": anchor}` or `{"type": "uniform", "min_inclusive",
/// "max_inclusive"}`.
pub(crate) fn height_provider(json: &Value, at: &At<'_>) -> LoadResult<HeightProvider> {
    let obj = json
        .as_object()
        .ok_or_else(|| at.error(LoadErrorKind::Shape("a height provider")))?;
    let Some(kind) = obj.get("type").and_then(Value::as_str) else {
        return Ok(HeightProvider::Constant(anchor(json, at)?));
    };
    let field = |name: &'static str| {
        obj.get(name)
            .ok_or_else(|| at.error(LoadErrorKind::Missing(name)))
            .and_then(|v| anchor(v, &at.child(name)))
    };
    match full_id(kind).as_str() {
        "minecraft:constant" => Ok(HeightProvider::Constant(field("value")?)),
        "minecraft:uniform" => Ok(HeightProvider::Uniform(
            field("min_inclusive")?,
            field("max_inclusive")?,
        )),
        _ => Err(at.child("type").error(LoadErrorKind::Unsupported {
            what: "height provider type",
            name: kind.to_string(),
        })),
    }
}

/// `VerticalAnchor.CODEC`: exactly one of `absolute`, `above_bottom` or `below_top`.
fn anchor(json: &Value, at: &At<'_>) -> LoadResult<VerticalAnchor> {
    let obj = json
        .as_object()
        .filter(|o| o.len() == 1)
        .ok_or_else(|| at.error(LoadErrorKind::Shape("a vertical anchor")))?;
    let (key, value) = obj.iter().next().expect("one entry");
    let n = codec::int_of(value)
        .ok_or_else(|| at.child(key).error(LoadErrorKind::Shape("an integer")))?;
    match key.as_str() {
        "absolute" => Ok(VerticalAnchor::Absolute(n)),
        "above_bottom" => Ok(VerticalAnchor::AboveBottom(n)),
        "below_top" => Ok(VerticalAnchor::BelowTop(n)),
        _ => Err(at.error(LoadErrorKind::Shape("a vertical anchor"))),
    }
}

#[cfg(test)]
impl Template {
    /// A hand-written template with one palette: jigsaw blocks as `(pos, front, top, data)`.
    pub(crate) fn for_test(
        size: [i32; 3],
        jigsaws: Vec<([i32; 3], Direction, Direction, JigsawData)>,
    ) -> Self {
        let mut palette: Vec<TemplateJigsaw> = jigsaws
            .into_iter()
            .map(|(pos, front, top, data)| TemplateJigsaw {
                pos,
                front,
                top,
                data,
            })
            .collect();
        palette.sort_by_key(|j| (j.pos[1], j.pos[0], j.pos[2]));
        Template {
            size,
            palettes: vec![palette],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn template(size: [i32; 3]) -> Template {
        Template {
            size,
            palettes: vec![vec![]],
        }
    }

    #[test]
    fn rotation_matches_the_game_switch() {
        // StructureTemplate.transform with pivot 0: CW90 (x, z) -> (-z, x), CCW90 -> (z, -x).
        assert_eq!(Rotation::Clockwise90.transform([1, 5, 2]), [-2, 5, 1]);
        assert_eq!(
            Rotation::Counterclockwise90.transform([1, 5, 2]),
            [2, 5, -1]
        );
        assert_eq!(Rotation::Clockwise180.transform([1, 5, 2]), [-1, 5, -2]);
    }

    #[test]
    fn directions_rotate_as_positions_do() {
        // A direction's step, rotated as a position, is the rotated direction's step.
        let all = [
            Direction::Down,
            Direction::Up,
            Direction::North,
            Direction::South,
            Direction::West,
            Direction::East,
        ];
        for r in Rotation::ALL {
            for d in all {
                assert_eq!(r.transform(d.step()), r.rotate(d).step(), "{r:?} {d:?}");
            }
        }
        assert_eq!(
            Rotation::Clockwise90.rotate(Direction::North),
            Direction::East
        );
    }

    #[test]
    fn bounding_box_rotates_about_the_origin() {
        let t = template([3, 4, 5]);
        assert_eq!(
            t.bounding_box([100, 64, 200], Rotation::None),
            BoundingBox {
                min: [100, 64, 200],
                max: [102, 67, 204]
            }
        );
        // CW90: x -> -z: x spans -4..=0, z spans 0..=2.
        assert_eq!(
            t.bounding_box([100, 64, 200], Rotation::Clockwise90),
            BoundingBox {
                min: [96, 64, 200],
                max: [100, 67, 202]
            }
        );
    }

    #[test]
    fn shuffle_is_fisher_yates_from_the_end() {
        let mut list = [0, 1, 2, 3, 4];
        let mut random = LegacyRandomSource::new(7);
        let mut expected = list;
        let mut check = LegacyRandomSource::new(7);
        for i in (2..=5).rev() {
            let j = check.next_int_bounded(i) as usize;
            expected.swap(i as usize - 1, j);
        }
        shuffle(&mut list, &mut random);
        assert_eq!(list, expected);
    }

    #[test]
    fn uniform_height_draws_between_inclusive() {
        let heights = GenerationHeights {
            min_y: -64,
            height: 384,
            level_min_y: -64,
            level_height: 384,
        };
        let h =
            HeightProvider::Uniform(VerticalAnchor::Absolute(-40), VerticalAnchor::Absolute(-20));
        let mut a = LegacyRandomSource::new(3);
        let mut b = LegacyRandomSource::new(3);
        assert_eq!(h.sample(&mut a, &heights), b.next_int_bounded(21) - 40);
        assert_eq!(VerticalAnchor::BelowTop(2).resolve(&heights), 317);
        assert_eq!(VerticalAnchor::AboveBottom(8).resolve(&heights), -56);
    }

    fn jigsaw(name: &str, target: &str, joint: Joint) -> JigsawData {
        JigsawData {
            name: Some(name.to_string()),
            pool: EMPTY_POOL.to_string(),
            target: target.to_string(),
            joint,
            placement_priority: 0,
            selection_priority: 0,
        }
    }

    #[test]
    fn jigsaws_attach_face_to_face_by_target() {
        use Direction::{Down, East, North, South, Up, West};
        let at = |front, top, data| PlacedJigsaw {
            pos: [0; 3],
            front,
            top,
            data,
        };
        let aligned = jigsaw("a:p", "a:c", Joint::Aligned);
        let rollable = jigsaw("a:p", "a:c", Joint::Rollable);
        let child = jigsaw("a:c", "a:p", Joint::Aligned);
        let parent = at(East, Up, &aligned);
        assert!(parent.can_attach(&at(West, Up, &child)));
        // Not facing each other.
        assert!(!parent.can_attach(&at(East, Up, &child)));
        // Tops differ: only a rollable joint allows it.
        assert!(!parent.can_attach(&at(West, North, &child)));
        assert!(at(East, Up, &rollable).can_attach(&at(West, North, &child)));
        // The child's name must be the target.
        let other = jigsaw("a:x", "a:p", Joint::Aligned);
        assert!(!parent.can_attach(&at(West, Up, &other)));
        // A feature's jigsaw has no name, so any target matches; its top is south.
        let feature = at(Down, South, &FEATURE_JIGSAW);
        assert!(!at(Up, Up, &aligned).can_attach(&feature));
        assert!(at(Up, Up, &rollable).can_attach(&feature));
        // With feature_jigsaw_named_bottom (26.2), only a parent targeting minecraft:bottom.
        let named = at(Down, South, &FEATURE_JIGSAW_NAMED_BOTTOM);
        assert!(!at(Up, Up, &rollable).can_attach(&named));
        let bottom = jigsaw("a:p", "minecraft:bottom", Joint::Rollable);
        assert!(at(Up, Up, &bottom).can_attach(&named));
    }

    #[test]
    fn default_joint_follows_the_front() {
        assert_eq!(jigsaw_data(None, Direction::North).joint, Joint::Aligned);
        assert_eq!(jigsaw_data(None, Direction::Up).joint, Joint::Rollable);
        assert_eq!(
            jigsaw_data(None, Direction::Up).pool,
            "minecraft:empty".to_string()
        );
    }
}
