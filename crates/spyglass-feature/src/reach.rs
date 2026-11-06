//! Where a feature can write, as boxes around the position it's called with: sound bounds from
//! its configuration, never too small (`docs/specs/features.md#neighbours`, ADR 0055). Taken
//! from each feature's bytecode in every supported version.

use serde_json::{Map, Value};
use spyglass_data::codec::{At, LoadError, field, full_id, int, object};
use spyglass_data::providers::IntProvider;
use spyglass_density::Resources;

/// Configured features, from 26.3.
const FEATURE: &str = "worldgen/feature";
/// Configured features, in 26.2.
const CONFIGURED_FEATURE: &str = "worldgen/configured_feature";
/// Placed features.
const PLACED_FEATURE: &str = "worldgen/placed_feature";

/// How far an unbounded or unknown feature is taken to reach: past its 3 × 3 region, which the
/// game doesn't write outside.
const ANYWHERE: i32 = 48;

/// What a feature's writes can change about the blocks they replace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Changes {
    /// Only which block it is: a solid, non-air block without fluid becomes another (ores, disks).
    Identity,
    /// Anything.
    Any,
}

/// A box of positions a feature can write, relative to where it's called.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteBox {
    /// The least offset.
    pub min: [i32; 3],
    /// The greatest offset.
    pub max: [i32; 3],
    /// Whether y is any y: the box is whole columns.
    pub column: bool,
    /// What its writes change.
    pub changes: Changes,
}

impl WriteBox {
    fn new(min: [i32; 3], max: [i32; 3]) -> Self {
        WriteBox {
            min,
            max,
            column: false,
            changes: Changes::Any,
        }
    }

    /// Whole columns from `-xz` to `xz` around the position.
    fn columns(xz: i32) -> Self {
        WriteBox {
            min: [-xz, 0, -xz],
            max: [xz, 0, xz],
            column: true,
            changes: Changes::Any,
        }
    }

    fn identity(mut self) -> Self {
        self.changes = Changes::Identity;
        self
    }

    /// Moved by every offset from `lo` to `hi`.
    fn shifted(self, lo: [i32; 3], hi: [i32; 3], column: bool) -> Self {
        let mut b = self;
        for i in 0..3 {
            b.min[i] = b.min[i].saturating_add(lo[i]);
            b.max[i] = b.max[i].saturating_add(hi[i]);
        }
        b.column |= column;
        b
    }
}

/// Where a placed or configured feature can write.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Reach(pub Vec<WriteBox>);

impl Reach {
    /// A placed feature's feature, called at its tries.
    pub fn of_placed(
        resources: &dyn Resources,
        json: &Value,
        at: &At<'_>,
    ) -> Result<Reach, LoadError> {
        let fields = object(json, at)?;
        feature(
            resources,
            field(fields, "feature", at)?,
            &at.child("feature"),
            0,
        )
    }
}

/// A configured feature, by id or inline.
fn feature(
    resources: &dyn Resources,
    json: &Value,
    at: &At<'_>,
    depth: u32,
) -> Result<Reach, LoadError> {
    if depth > 16 {
        return Ok(Reach(vec![WriteBox::columns(ANYWHERE)]));
    }
    let owned;
    let json = match json {
        Value::String(id) => {
            let id = full_id(id);
            owned = resources
                .resource(FEATURE, &id)
                .or_else(|_| resources.resource(CONFIGURED_FEATURE, &id))
                .map_err(|e| At::root(&id).error(e.into()))?;
            &owned
        }
        other => other,
    };
    let fields = object(json, at)?;
    let kind = fields
        .get("type")
        .and_then(Value::as_str)
        .map(full_id)
        .unwrap_or_default();
    // 26.2 nests the configuration under `config`.
    let config: &Map<String, Value> = match fields.get("config") {
        Some(Value::Object(config)) => config,
        _ => fields,
    };
    // A provider's largest value, or `default` when it's missing or unreadable.
    let max = |key: &str, default: i32| -> i32 {
        config
            .get(key)
            .and_then(|v| IntProvider::parse(v, &at.child(key)).ok())
            .map_or(default, |p| p.max())
    };
    let between = WriteBox::new;
    let one = |w: WriteBox| Ok(Reach(vec![w]));
    let name = kind.strip_prefix("minecraft:").unwrap_or(&kind);
    match name {
        "no_op" => Ok(Reach::default()),
        // A blob around ceil(3s/16) across and ceil(s/16) up (OreFeature); scattered within 7.
        "ore" => {
            let size = max("size", 64);
            let xz = (3 * size + 15) / 16;
            let y = (size + 15) / 16;
            one(between([-xz, -y - 2, -xz], [xz, y, xz]).identity())
        }
        "scattered_ore" => {
            let spread = (max("size", 64) - 1).clamp(0, 7);
            let y = (max("size", 64) + 15) / 16;
            one(between([-spread, -y - 2, -spread], [spread, y, spread]).identity())
        }
        "disk" => {
            let radius = max("radius", 8);
            let half_height = max("half_height", 4);
            one(between(
                [-radius, -half_height, -radius],
                [radius, half_height, radius],
            )
            .identity())
        }
        "spring_feature" | "vines" => one(between([0; 3], [0; 3])),
        "simple_block" => one(between([0, 0, 0], [0, 1, 0])),
        "block_column" => {
            let layers = config.get("layers").and_then(Value::as_array);
            let total: i32 = layers
                .map(|ls| {
                    ls.iter()
                        .map(|l| {
                            l.get("height")
                                .and_then(|h| IntProvider::parse(h, at).ok())
                                .map_or(ANYWHERE, |p| p.max())
                        })
                        .sum()
                })
                .unwrap_or(ANYWHERE);
            // How far past its first block the column reaches.
            let extent = total.max(1) - 1;
            match config.get("direction").and_then(Value::as_str) {
                Some("up") => one(between([0, 0, 0], [0, extent, 0])),
                Some("down") => one(between([0, -extent, 0], [0, 0, 0])),
                _ => one(between([0, -extent, 0], [0, extent, 0])),
            }
        }
        // Its own chunk's top layer, from the chunk's corner.
        "freeze_top_layer" => one(WriteBox {
            min: [0, 0, 0],
            max: [15, 0, 15],
            column: true,
            changes: Changes::Any,
        }),
        "blue_ice" => one(between([-2, -5, -2], [2, 4, 2])),
        "underwater_magma" => {
            let radius = max("placement_radius_around_floor", 7);
            let range = max("floor_search_range", 64);
            one(between(
                [-radius, -(range - 1) - radius, -radius],
                [radius, radius - 1, radius],
            ))
        }
        "multiface_growth" => one(between([-2; 3], [2; 3])),
        "bamboo" => {
            let stalk = between([0, 0, 0], [0, 16, 0]);
            Ok(Reach(vec![stalk, WriteBox::columns(4)]))
        }
        "sculk_patch" => one(between([-16, -65, -16], [16, 65, 16])),
        // 26.2's ocean plants follow the ocean floor's heightmap.
        "kelp" => one(WriteBox::columns(0)),
        "seagrass" | "sea_pickle" => one(WriteBox::columns(7)),
        "block_pile" => one(between([-3, 0, -3], [3, 1, 3])),
        "geode" => one(between([-16; 3], [16; 3])),
        "lake" => one(between([-8, -4, -8], [7, 3, 7])),
        "monster_room" => one(between([-4, -1, -4], [4, 3, 4])),
        // Its y follows the ocean floor; its footprint is at most 13 across.
        "fossil" => one(WriteBox::columns(7)),
        "block_blob" => one(WriteBox::columns(5)),
        "large_dripstone" | "large_speleothem" => one(WriteBox::columns(32)),
        // At sea level, and down a column (the ice spike's stalk).
        "iceberg" => one(WriteBox::columns(11)),
        "spike" => one(WriteBox::columns(3)),
        "speleothem_cluster" => {
            let radius = max("radius", 48);
            let search = max("floor_to_ceiling_search_range", 512);
            let thickness = max(
                "speleothem_block_layer_thickness",
                max("dripstone_block_layer_thickness", 128),
            );
            let height = max("height", 128);
            let lo = (1 - height).min(-search - thickness + 1);
            let hi = (search + thickness - 2).max(height - 1);
            one(between([-radius, lo, -radius], [radius, hi, radius]))
        }
        "crevasse" => {
            let width = max("max_width", 32);
            let length = max("max_length", 64);
            let xz = length / 2 + (width + 1) / 2 + 2;
            one(WriteBox::columns(xz))
        }
        // The roots, trunk, leaves and decorators, and the edge they update: every vanilla
        // config fits; a trunk taller than vanilla's widens it; pale moss hangs down a column.
        "tree" => {
            let trunk = config.get("trunk_placer").and_then(Value::as_object);
            let height = trunk.map_or(64, |t| {
                ["base_height", "height_rand_a", "height_rand_b"]
                    .iter()
                    .map(|k| t.get(*k).and_then(|v| int(v, at).ok()).unwrap_or(0))
                    .sum()
            });
            let moss = config
                .get("decorators")
                .and_then(Value::as_array)
                .is_some_and(|ds| {
                    ds.iter().any(|d| {
                        d.get("type")
                            .and_then(Value::as_str)
                            .map(full_id)
                            .as_deref()
                            == Some("minecraft:pale_moss")
                    })
                });
            if moss {
                return one(WriteBox::columns(10));
            }
            one(between([-10, -15, -10], [10, 32.max(height + 12), 10]))
        }
        "vegetation_patch" | "waterlogged_vegetation_patch" => {
            let radius = max("xz_radius", 64) + 1;
            let vertical_range = config
                .get("vertical_range")
                .and_then(|v| int(v, at).ok())
                .unwrap_or(256);
            let extra = config
                .get("extra_bottom_block_chance")
                .and_then(Value::as_f64)
                .is_some_and(|x| x > 0.0);
            let ground_depth = max("depth", 128) + i32::from(extra);
            let mut boxes = vec![between(
                [-radius, -vertical_range - ground_depth, -radius],
                [radius, vertical_range + ground_depth, radius],
            )];
            if let Some(vegetation) = config.get("vegetation_feature") {
                let inner = placed_inline(
                    resources,
                    vegetation,
                    &at.child("vegetation_feature"),
                    depth,
                )?;
                boxes.extend(inner.0.into_iter().map(|w| {
                    w.shifted(
                        [-radius, -vertical_range - 1, -radius],
                        [radius, vertical_range, radius],
                        false,
                    )
                }));
            }
            Ok(Reach(boxes))
        }
        "root_system" => {
            let radius = max("root_radius", 64).max(max("hanging_root_radius", 64)) - 1;
            let tree_space = config
                .get("required_vertical_space_for_tree")
                .and_then(|v| int(v, at).ok())
                .unwrap_or(64);
            let column_height = config
                .get("root_column_max_height")
                .and_then(|v| int(v, at).ok())
                .unwrap_or(64);
            let hang = max("hanging_roots_vertical_span", 16);
            let mut boxes = vec![between(
                [-radius, -(tree_space - 1) - hang, -radius],
                [radius, (column_height - 2).max(tree_space - 1), radius],
            )];
            if let Some(tree) = config.get("feature") {
                let inner = placed_inline(resources, tree, &at.child("feature"), depth)?;
                boxes.extend(
                    inner
                        .0
                        .into_iter()
                        .map(|w| w.shifted([0, 1, 0], [0, column_height, 0], false)),
                );
            }
            Ok(Reach(boxes))
        }
        "template" => {
            let templates = config.get("templates").and_then(Value::as_array);
            let mut boxes = Vec::new();
            for t in templates.into_iter().flatten() {
                let id = t
                    .pointer("/data/id")
                    .or_else(|| t.get("data"))
                    .and_then(Value::as_str)
                    .map(full_id);
                let size = id.and_then(|id| template_size(resources, &id));
                let [sx, sy, sz] = size.unwrap_or([ANYWHERE; 3]);
                // Centred on the origin and turned: half the larger side, either way.
                let xz = sx.max(sz);
                boxes.push(between([-xz, 0, -xz], [xz, sy, xz]));
            }
            Ok(Reach(boxes))
        }
        "random_selector"
        | "simple_random_selector"
        | "weighted_random_selector"
        | "random_boolean_selector"
        | "sequence"
        | "overlay" => {
            let mut boxes = Vec::new();
            let mut children: Vec<(&Value, At<'_>)> = Vec::new();
            for key in ["features", "default", "feature_true", "feature_false"] {
                match config.get(key) {
                    Some(Value::Array(items)) => {
                        for (i, item) in items.iter().enumerate() {
                            // random_selector's entries are `{feature, chance}`; weighted ones
                            // `{data, weight}`.
                            let item = item
                                .get("data")
                                .filter(|_| item.get("weight").is_some())
                                .or_else(|| {
                                    item.get("feature").filter(|_| item.get("chance").is_some())
                                })
                                .unwrap_or(item);
                            children.push((item, at.child(key).child(&i.to_string())));
                        }
                    }
                    Some(v) => children.push((v, at.child(key))),
                    None => {}
                }
            }
            for (child, at) in children {
                // A `#tag` of placed features is each of them.
                if let Some(tag) = child.as_str().and_then(|s| s.strip_prefix('#')) {
                    for id in spyglass_terrain::tag(resources, "tags/worldgen/placed_feature", tag)?
                    {
                        boxes.extend(placed_inline(resources, &Value::String(id), &at, depth)?.0);
                    }
                    continue;
                }
                boxes.extend(placed_inline(resources, child, &at, depth)?.0);
            }
            Ok(Reach(boxes))
        }
        _ => one(WriteBox::columns(ANYWHERE)),
    }
}

/// A placed feature nested in a configuration, by id or inline: its feature's boxes moved by
/// everything its modifiers can do.
fn placed_inline(
    resources: &dyn Resources,
    json: &Value,
    at: &At<'_>,
    depth: u32,
) -> Result<Reach, LoadError> {
    let owned;
    let json = match json {
        Value::String(id) => {
            let id = full_id(id);
            owned = resources
                .resource(PLACED_FEATURE, &id)
                .map_err(|e| At::root(&id).error(e.into()))?;
            &owned
        }
        other => other,
    };
    let fields = object(json, at)?;
    let inner = feature(
        resources,
        field(fields, "feature", at)?,
        &at.child("feature"),
        depth + 1,
    )?;
    let (lo, hi, column) = displacement(fields.get("placement"));
    Ok(Reach(
        inner
            .0
            .into_iter()
            .map(|w| w.shifted(lo, hi, column))
            .collect(),
    ))
}

/// The least and greatest offsets a list of modifiers can move a position by, and whether it
/// can end anywhere in its column.
fn displacement(modifiers: Option<&Value>) -> ([i32; 3], [i32; 3], bool) {
    let (mut lo, mut hi, mut column) = ([0; 3], [0; 3], false);
    let Some(Value::Array(list)) = modifiers else {
        return (lo, hi, column);
    };
    for modifier in list {
        let (l, h, c) = one_displacement(modifier);
        for i in 0..3 {
            lo[i] = lo[i].saturating_add(l[i]);
            hi[i] = hi[i].saturating_add(h[i]);
        }
        column |= c;
    }
    (lo, hi, column)
}

/// One modifier's least and greatest offsets, and whether it can move a position anywhere in its
/// column.
fn one_displacement(modifier: &Value) -> ([i32; 3], [i32; 3], bool) {
    let kind = modifier
        .get("type")
        .and_then(Value::as_str)
        .map(full_id)
        .unwrap_or_default();
    let range = |key: &str| {
        modifier
            .get(key)
            .and_then(|v| IntProvider::parse(v, &At::root("placement")).ok())
            .map_or((-ANYWHERE, ANYWHERE), |p| (p.min(), p.max()))
    };
    match kind.trim_start_matches("minecraft:") {
        "count"
        | "block_predicate_filter"
        | "random_chance"
        | "rarity_filter"
        | "biome"
        | "surface_relative_threshold_filter"
        | "surface_water_depth_filter"
        | "noise_based_count"
        | "noise_threshold_count" => ([0; 3], [0; 3], false),
        "in_square" => ([0; 3], [15, 0, 15], false),
        // Positions in the origin's chunk, at any height.
        "fixed_placement" => ([0; 3], [15, 0, 15], true),
        "offset" => {
            let (x, y, z) = (range("x"), range("y"), range("z"));
            ([x.0, y.0, z.0], [x.1, y.1, z.1], false)
        }
        "random_offset" => {
            let (xz, y) = (range("xz_spread"), range("y_spread"));
            ([xz.0, y.0, xz.0], [xz.1, y.1, xz.1], false)
        }
        "environment_scan" => {
            let steps = modifier
                .get("max_steps")
                .and_then(Value::as_i64)
                .unwrap_or(32) as i32;
            match modifier.get("direction_of_search").and_then(Value::as_str) {
                Some("up") => ([0; 3], [0, steps, 0], false),
                _ => ([0, -steps, 0], [0; 3], false),
            }
        }
        "randomly_selected" => {
            let (mut lo, mut hi) = ([i32::MAX; 3], [i32::MIN; 3]);
            let mut column = false;
            // Any one of the choices: the box around all of them.
            for p in modifier
                .get("placements")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let (l, h, c) = one_displacement(p);
                for i in 0..3 {
                    lo[i] = lo[i].min(l[i]);
                    hi[i] = hi[i].max(h[i]);
                }
                column |= c;
            }
            // No choices: it never moves the position.
            if lo[0] > hi[0] {
                return ([0; 3], [0; 3], false);
            }
            (lo, hi, column)
        }
        "cuboid" => {
            let (xz, y) = (range("xz_size").1, range("y_size").1);
            ([0; 3], [xz, y, xz], false)
        }
        // A height or heightmap: anywhere in the column; anything else, anywhere near.
        "height_range" | "heightmap" => ([0; 3], [0; 3], true),
        _ => ([-ANYWHERE, 0, -ANYWHERE], [ANYWHERE, 0, ANYWHERE], true),
    }
}

/// A structure template's size, from its NBT.
fn template_size(resources: &dyn Resources, id: &str) -> Option<[i32; 3]> {
    let bytes = resources.structure_template(id).ok()?;
    let (_, root) = spyglass_data::nbt::read_compressed(&bytes).ok()?;
    let list = root.get("size")?.as_list()?;
    let sides: Vec<i32> = list.iter().filter_map(|t| t.as_int()).collect();
    (sides.len() == 3).then(|| [sides[0], sides[1], sides[2]])
}
