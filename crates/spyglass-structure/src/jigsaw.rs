//! Jigsaw structures up to their start piece (`JigsawStructure.findGenerationPoint`,
//! `JigsawPlacement.addPieces`): where the start piece goes and where its biome check samples.
//! The rest of the jigsaw assembly happens after the check. See
//! `docs/specs/structure-placement.md`.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{Map, Value};
use spyglass_data::bundle::Bundle;
use spyglass_data::codec;
use spyglass_data::nbt::{self, Tag};
use spyglass_density::{LoadError, LoadErrorKind};
use spyglass_rng::{LegacyRandomSource, RandomSource};

use crate::load::{At, LoadResult, full_id};

const TEMPLATE_POOL: &str = "worldgen/template_pool";

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
    /// `Rotation.getRandom`: `values()[nextInt(4)]`.
    pub fn random(random: &mut LegacyRandomSource) -> Self {
        [
            Rotation::None,
            Rotation::Clockwise90,
            Rotation::Clockwise180,
            Rotation::Counterclockwise90,
        ][random.next_int_bounded(4) as usize]
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

    fn moved(self, by: [i32; 3]) -> Self {
        let add = |p: [i32; 3]| [0, 1, 2].map(|i| p[i].wrapping_add(by[i]));
        BoundingBox {
            min: add(self.min),
            max: add(self.max),
        }
    }

    fn encapsulate(self, other: BoundingBox) -> Self {
        BoundingBox {
            min: [0, 1, 2].map(|i| self.min[i].min(other.min[i])),
            max: [0, 1, 2].map(|i| self.max[i].max(other.max[i])),
        }
    }
}

/// A jigsaw block in a template (`StructureTemplate.JigsawBlockInfo`): the fields the start
/// needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JigsawBlock {
    /// Position in the template.
    pub pos: [i32; 3],
    /// `name`, default `minecraft:empty`.
    pub name: String,
    /// `selection_priority`, default 0.
    pub selection_priority: i32,
}

/// A structure template (`StructureTemplate`), reduced to its size and jigsaw blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    /// `size`.
    pub size: [i32; 3],
    /// Each palette's jigsaw blocks, in the template's block order: sorted by y, then x, then z.
    pub palettes: Vec<Vec<JigsawBlock>>,
}

impl Template {
    /// `StructureTemplate.load`, keeping the size and the jigsaw blocks of each palette
    /// (`palettes`, or the single `palette`). Palette entries name their block by `id`
    /// (`NbtUtils.readBlockState`). The game data-fixes a template saved by an older version
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
                    let name = usize::try_from(state)
                        .ok()
                        .and_then(|s| palette.get(s))
                        .and_then(|p| p.get("id"))
                        .and_then(Tag::as_str);
                    if name.map(full_id).as_deref() != Some("minecraft:jigsaw") {
                        continue;
                    }
                    let blocks_at = at.child("blocks");
                    let at = blocks_at.child(&format!("[{i}]"));
                    let pos = int3(block.get("pos"), &at.child("pos"))?;
                    let entity = block.get("nbt");
                    jigsaws.push(JigsawBlock {
                        pos,
                        name: entity
                            .and_then(|n| n.get("name"))
                            .and_then(Tag::as_str)
                            .map_or_else(|| "minecraft:empty".to_string(), full_id),
                        selection_priority: entity
                            .and_then(|n| n.get("selection_priority"))
                            .and_then(Tag::as_int)
                            .unwrap_or(0),
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
    fn jigsaws(&self, pos: [i32; 3], rotation: Rotation) -> Vec<JigsawBlock> {
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
                JigsawBlock {
                    pos: [0, 1, 2].map(|i| r[i].wrapping_add(pos[i])),
                    ..j.clone()
                }
            })
            .collect()
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

/// A pool element (`StructurePoolElement`), as far as the start piece needs it.
#[derive(Debug, Clone)]
pub enum PoolElement {
    /// `single_pool_element` or `legacy_single_pool_element`: one template.
    Single(Arc<Template>),
    /// `list_pool_element`: several elements placed together.
    List(Vec<PoolElement>),
    /// `feature_pool_element`: a placed feature, with a 1 × 1 × 1 box.
    Feature,
    /// `empty_pool_element`.
    Empty,
}

impl PoolElement {
    /// `StructurePoolElement.getBoundingBox(manager, pos, rotation)`.
    fn bounding_box(&self, pos: [i32; 3], rotation: Rotation) -> Option<BoundingBox> {
        match self {
            PoolElement::Single(template) => Some(template.bounding_box(pos, rotation)),
            // ListPoolElement: the box around every non-empty element's box.
            PoolElement::List(elements) => elements
                .iter()
                .filter_map(|e| e.bounding_box(pos, rotation))
                .reduce(BoundingBox::encapsulate),
            PoolElement::Feature => Some(BoundingBox { min: pos, max: pos }),
            PoolElement::Empty => None,
        }
    }

    /// `StructurePoolElement.getShuffledJigsawBlocks(manager, pos, rotation, random)`: shuffled
    /// with `Util.shuffle`, then stably sorted by selection priority, highest first. A list
    /// element uses its first element's. `None` where spyglass doesn't support the element.
    fn shuffled_jigsaws(
        &self,
        pos: [i32; 3],
        rotation: Rotation,
        random: &mut LegacyRandomSource,
    ) -> Option<Vec<JigsawBlock>> {
        match self {
            PoolElement::Single(template) => {
                let mut jigsaws = template.jigsaws(pos, rotation);
                shuffle(&mut jigsaws, random);
                jigsaws.sort_by_key(|j| std::cmp::Reverse(j.selection_priority));
                Some(jigsaws)
            }
            PoolElement::List(elements) => {
                elements.first()?.shuffled_jigsaws(pos, rotation, random)
            }
            PoolElement::Feature | PoolElement::Empty => None,
        }
    }
}

/// `Util.shuffle(list, random)`: Fisher-Yates from the end, `nextInt(i)` for each `i` from the
/// length down to 2.
fn shuffle<T>(list: &mut [T], random: &mut LegacyRandomSource) {
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

/// A height provider (`HeightProvider`), the kinds jigsaw starts use.
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
    fn sample(self, random: &mut LegacyRandomSource, heights: &GenerationHeights) -> i32 {
        match self {
            HeightProvider::Constant(a) => a.resolve(heights),
            HeightProvider::Uniform(lo, hi) => {
                let (lo, hi) = (lo.resolve(heights), hi.resolve(heights));
                if lo > hi {
                    lo
                } else {
                    random.next_int_bounded(hi - lo + 1) + lo
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
    fn level_max_y(&self) -> i32 {
        self.level_min_y
            .wrapping_add(self.level_height)
            .wrapping_sub(1)
    }
}

/// A jigsaw structure's start settings (`JigsawStructure`).
#[derive(Debug, Clone)]
pub struct Jigsaw {
    /// `start_pool`, loaded.
    pub start_pool: Arc<Pool>,
    /// `start_jigsaw_name`.
    pub start_jigsaw_name: Option<String>,
    /// `start_height`.
    pub start_height: HeightProvider,
    /// `project_start_to_heightmap`: the heightmap's name, if projected.
    pub project_start_to_heightmap: Option<String>,
    /// `dimension_padding`: (bottom, top); `None` for `DimensionPadding.ZERO`.
    pub dimension_padding: Option<(i32, i32)>,
}

/// Where a jigsaw start's biome check samples.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JigsawCheck {
    /// No start: an empty start pool, a missing start jigsaw, or a start too close to the world
    /// height limits.
    NoStart,
    /// The check is at this block position.
    At([i32; 3]),
    /// The start is projected to a heightmap, so y needs terrain. The check is at
    /// `(x, heightmap y + y_offset, z)`, and the game first requires some biome in the
    /// structure's set in the column `min_y..=max_y` at `(x, z)`
    /// (`couldStructureExistInColumn`).
    Projected {
        /// Block x of the check.
        x: i32,
        /// Block z of the check.
        z: i32,
        /// Added to the heightmap's first free height for the check's y.
        y_offset: i32,
        /// The heightmap, e.g. `WORLD_SURFACE_WG`.
        heightmap: String,
    },
}

impl Jigsaw {
    /// `findGenerationPoint` up to the biome check, drawing from `random` (the structure's
    /// `setLargeFeatureSeed` random) in the game's order.
    pub fn check_position(
        &self,
        chunk: [i32; 2],
        heights: &GenerationHeights,
        random: &mut LegacyRandomSource,
    ) -> JigsawCheck {
        let start = [
            chunk[0].wrapping_shl(4),
            self.start_height.sample(random, heights),
            chunk[1].wrapping_shl(4),
        ];
        let rotation = Rotation::random(random);
        let element = self.start_pool.random_template(random);
        if matches!(element, PoolElement::Empty) {
            return JigsawCheck::NoStart;
        }
        let offset = match &self.start_jigsaw_name {
            None => [0; 3],
            Some(name) => {
                let jigsaws = element
                    .shuffled_jigsaws(start, rotation, random)
                    .unwrap_or_default();
                match jigsaws.iter().find(|j| &j.name == name) {
                    Some(j) => [0, 1, 2].map(|i| j.pos[i].wrapping_sub(start[i])),
                    None => return JigsawCheck::NoStart,
                }
            }
        };
        let piece = [0, 1, 2].map(|i| start[i].wrapping_sub(offset[i]));
        let Some(bbox) = element.bounding_box(piece, rotation) else {
            return JigsawCheck::NoStart;
        };
        let x = bbox.max[0].wrapping_add(bbox.min[0]) / 2;
        let z = bbox.max[2].wrapping_add(bbox.min[2]) / 2;
        match &self.project_start_to_heightmap {
            Some(heightmap) => JigsawCheck::Projected {
                x,
                z,
                y_offset: start[1].wrapping_add(offset[1]),
                heightmap: heightmap.clone(),
            },
            None => {
                let y = piece[1];
                if let Some((bottom, top)) = self.dimension_padding {
                    // The piece moves so its box's minimum y plus the ground level delta (1)
                    // is at y; then it must fit inside the padded level height.
                    let dy = y.wrapping_sub(bbox.min[1].wrapping_add(1));
                    let (min_y, max_y) =
                        (bbox.min[1].wrapping_add(dy), bbox.max[1].wrapping_add(dy));
                    if min_y < heights.level_min_y.wrapping_add(bottom)
                        || max_y > heights.level_max_y().wrapping_sub(top)
                    {
                        return JigsawCheck::NoStart;
                    }
                }
                JigsawCheck::At([x, y.wrapping_add(offset[1]), z])
            }
        }
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
    pools: HashMap<String, Arc<Pool>>,
    templates: HashMap<String, Arc<Template>>,
}

impl<'a> JigsawLoader<'a> {
    pub(crate) fn new(bundle: &'a Bundle) -> Self {
        JigsawLoader {
            bundle,
            pools: HashMap::new(),
            templates: HashMap::new(),
        }
    }

    /// `JigsawStructure.CODEC`'s start fields.
    pub(crate) fn jigsaw(&mut self, obj: &Map<String, Value>, at: &At<'_>) -> LoadResult<Jigsaw> {
        let pool_id = full_id(
            obj.get("start_pool")
                .and_then(Value::as_str)
                .ok_or_else(|| at.error(LoadErrorKind::Missing("start_pool")))?,
        );
        // Pool aliases (PoolAliasBinding) rename pools by their `alias` fields; renaming the
        // start pool isn't supported, since it would need PoolAliasLookup's own random.
        if let Some(aliases) = obj.get("pool_aliases")
            && alias_names(aliases).iter().any(|a| full_id(a) == pool_id)
        {
            return Err(at.child("pool_aliases").error(LoadErrorKind::Unsupported {
                what: "pool alias of the start pool",
                name: pool_id,
            }));
        }
        let start_height = height_provider(
            obj.get("start_height")
                .ok_or_else(|| at.error(LoadErrorKind::Missing("start_height")))?,
            &at.child("start_height"),
        )?;
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
        // DimensionPadding.CODEC: an int for both, or {bottom, top}, each defaulting to 0.
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
                (bottom != 0 || top != 0).then_some((bottom, top))
            }
        };
        let start_jigsaw_name = obj
            .get("start_jigsaw_name")
            .and_then(Value::as_str)
            .map(full_id);
        Ok(Jigsaw {
            start_pool: self.pool(&pool_id)?,
            start_jigsaw_name,
            start_height,
            project_start_to_heightmap,
            dimension_padding,
        })
    }

    /// `StructureTemplatePool.DIRECT_CODEC`: `elements`, each `{weight, element}` with weight in
    /// 1..=150.
    fn pool(&mut self, id: &str) -> LoadResult<Arc<Pool>> {
        if let Some(pool) = self.pools.get(id) {
            return Ok(pool.clone());
        }
        let json = self
            .bundle
            .resource(TEMPLATE_POOL, id)
            .map_err(|e| At::root(id).error(e.into()))?;
        let at = At::root(id);
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
        let pool = Arc::new(Pool { templates });
        self.pools.insert(id.to_string(), pool.clone());
        Ok(pool)
    }

    fn element(&mut self, json: &Value, at: &At<'_>) -> LoadResult<PoolElement> {
        let kind = json
            .get("element_type")
            .and_then(Value::as_str)
            .ok_or_else(|| at.error(LoadErrorKind::Missing("element_type")))?;
        Ok(match full_id(kind).as_str() {
            "minecraft:single_pool_element" | "minecraft:legacy_single_pool_element" => {
                let location = json
                    .get("location")
                    .and_then(Value::as_str)
                    .ok_or_else(|| at.error(LoadErrorKind::Missing("location")))?;
                PoolElement::Single(self.template(&full_id(location))?)
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
                PoolElement::List(elements)
            }
            "minecraft:feature_pool_element" => PoolElement::Feature,
            "minecraft:empty_pool_element" => PoolElement::Empty,
            _ => {
                return Err(at.child("element_type").error(LoadErrorKind::Unsupported {
                    what: "pool element type",
                    name: kind.to_string(),
                }));
            }
        })
    }

    fn template(&mut self, id: &str) -> LoadResult<Arc<Template>> {
        if let Some(t) = self.templates.get(id) {
            return Ok(t.clone());
        }
        let error = |kind: LoadErrorKind| LoadError {
            resource: id.to_string(),
            path: String::new(),
            kind: Box::new(kind),
        };
        let bytes = self
            .bundle
            .structure_template(id)
            .map_err(|e| error(e.into()))?;
        let (_, root) = nbt::read_compressed(&bytes)
            .map_err(|e| error(LoadErrorKind::Invalid(nbt_error(&e))))?;
        let data_version = self.bundle.manifest().data_version;
        let template = Arc::new(Template::from_nbt(&root, id, data_version)?);
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

/// `HeightProvider.CODEC` for the kinds jigsaw starts use: a bare anchor (constant),
/// `{"type": "constant", "value": anchor}` or `{"type": "uniform", "min_inclusive",
/// "max_inclusive"}`.
fn height_provider(json: &Value, at: &At<'_>) -> LoadResult<HeightProvider> {
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
}
