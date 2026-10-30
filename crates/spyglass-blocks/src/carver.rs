//! Carvers: the caves and canyons that biomes' carvers cut into a chunk after the material rule,
//! as a carving mask from 26.3, and straight into the chunk in 26.2 (`docs/specs/carvers.md`).

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use serde_json::Value;
use spyglass_biome::{BiomeId, BiomeNames};
use spyglass_data::anchor::{AnchorHeights, VerticalAnchor};
use spyglass_data::codec::{At, LoadError, LoadErrorKind, field, float, full_id, int, object};
use spyglass_data::providers::{FloatProvider, HeightProvider, IntProvider};
use spyglass_density::Resources;
use spyglass_rng::{LegacyRandomSource, RandomSource};

/// The carvers registry, and 26.2's.
const CARVERS: [&str; 2] = ["worldgen/carver", "worldgen/configured_carver"];
/// Carver tags, and 26.2's.
const CARVER_TAGS: [&str; 2] = ["tags/worldgen/carver", "tags/worldgen/configured_carver"];

/// `WorldCarver.getRange()`: how many chunks a carver reaches, which sets its tunnels' length.
const RANGE: i32 = 4;

/// A carver (`WorldCarver`): `cave` or `canyon`.
#[derive(Debug, Clone)]
pub enum Carver {
    /// `CaveWorldCarver`.
    Cave(Box<Cave>),
    /// `CanyonWorldCarver`.
    Canyon(Box<Canyon>),
}

/// `CaveWorldCarver`'s fields.
#[derive(Debug, Clone)]
pub struct Cave {
    probability: f32,
    y: HeightProvider,
    count: IntProvider,
    thickness: FloatProvider,
    weird_thickness_bias: bool,
    room_vertical_radius_multiplier: FloatProvider,
    horizontal_radius_multiplier: FloatProvider,
    vertical_radius_multiplier: FloatProvider,
    start_vertical_radius_multiplier: FloatProvider,
    floor_level: FloatProvider,
}

/// `CanyonWorldCarver`'s fields.
#[derive(Debug, Clone)]
pub struct Canyon {
    probability: f32,
    y: HeightProvider,
    vertical_rotation: FloatProvider,
    distance_factor: FloatProvider,
    thickness: FloatProvider,
    width_smoothness: i32,
    horizontal_radius_factor: FloatProvider,
    vertical_radius_default_factor: f32,
    vertical_radius_center_factor: f32,
    y_scale: FloatProvider,
}

/// What a 26.2 carver's `CarverConfiguration` adds to its shape: which blocks it carves and
/// what it fills them with.
#[derive(Debug, Clone)]
pub struct Legacy {
    /// `replaceable`: the block ids it may carve.
    pub replaceable: HashSet<String>,
    /// `lava_level`: lava at and below it.
    pub lava_level: VerticalAnchor,
    /// `nether_cave`: lava up to 31 above the bottom, cave air above, and no aquifer.
    pub nether: bool,
}

/// Each biome's carvers, seed-independent.
#[derive(Debug, Clone, Default)]
pub struct CarverSettings {
    carvers: Vec<(Carver, Option<Legacy>)>,
    /// By biome id: indexes into `carvers`, in the biome's order.
    by_biome: Vec<Vec<usize>>,
}

impl CarverSettings {
    /// The carvers of each biome in `biomes`, from their `worldgen/biome` entries' `carvers`:
    /// 26.2's configured carvers if `v1_21`.
    pub fn load<R: Resources + ?Sized>(
        resources: &R,
        biomes: &BiomeNames,
        v1_21: bool,
    ) -> Result<Self, LoadError> {
        let family = usize::from(v1_21);
        let parse = |json: &Value, at: &At<'_>| -> Result<(Carver, Option<Legacy>), LoadError> {
            if v1_21 {
                let (carver, legacy) = Carver::parse_v1_21(resources, json, at)?;
                Ok((carver, Some(legacy)))
            } else {
                Ok((Carver::parse(json, at)?, None))
            }
        };
        let mut settings = CarverSettings::default();
        let mut by_id: HashMap<String, usize> = HashMap::new();
        for name in biomes.names() {
            let id = full_id(name);
            let root = At::root(&id);
            let json = resources
                .resource("worldgen/biome", &id)
                .map_err(|e| root.error(e.into()))?;
            let biome = object(&json, &root)?;
            let at = root.child("carvers");
            let mut list = Vec::new();
            let entries: Vec<(Value, At<'_>)> = match biome.get("carvers") {
                None => Vec::new(),
                Some(Value::Array(items)) => items
                    .iter()
                    .enumerate()
                    .map(|(i, v)| (v.clone(), at.child(&i.to_string())))
                    .collect(),
                Some(Value::String(s)) if s.starts_with('#') => {
                    spyglass_terrain::tag(resources, CARVER_TAGS[family], &s[1..])?
                        .into_iter()
                        .map(|id| (Value::String(id), at.clone()))
                        .collect()
                }
                Some(v) => vec![(v.clone(), at.clone())],
            };
            for (entry, at) in entries {
                let index = match &entry {
                    Value::String(id) => {
                        let id = full_id(id);
                        match by_id.get(&id) {
                            Some(&i) => i,
                            None => {
                                let root = At::root(&id);
                                let json = resources
                                    .resource(CARVERS[family], &id)
                                    .map_err(|e| root.error(e.into()))?;
                                settings.carvers.push(parse(&json, &root)?);
                                by_id.insert(id, settings.carvers.len() - 1);
                                settings.carvers.len() - 1
                            }
                        }
                    }
                    inline => {
                        settings.carvers.push(parse(inline, &at)?);
                        settings.carvers.len() - 1
                    }
                };
                list.push(index);
            }
            settings.by_biome.push(list);
        }
        Ok(settings)
    }

    /// The carvers of `biome`, in order, with 26.2's configuration.
    pub fn of(&self, biome: BiomeId) -> impl Iterator<Item = (&Carver, Option<&Legacy>)> {
        self.by_biome[usize::from(biome.0)].iter().map(|&i| {
            let (carver, legacy) = &self.carvers[i];
            (carver, legacy.as_ref())
        })
    }

    /// Whether two biomes have the same carvers, in the same order.
    pub fn same(&self, a: BiomeId, b: BiomeId) -> bool {
        self.by_biome[usize::from(a.0)] == self.by_biome[usize::from(b.0)]
    }
}

/// A block `HolderSet`: an id, a `#tag`, or a list of ids, as block ids.
fn block_set<R: Resources + ?Sized>(
    resources: &R,
    json: &Value,
    at: &At<'_>,
) -> Result<HashSet<String>, LoadError> {
    match json {
        Value::String(s) => match s.strip_prefix('#') {
            Some(tag) => Ok(spyglass_terrain::tag(resources, "tags/block", tag)?
                .into_iter()
                .collect()),
            None => Ok(HashSet::from([full_id(s)])),
        },
        Value::Array(items) => items
            .iter()
            .enumerate()
            .map(|(i, v)| {
                v.as_str().map(full_id).ok_or_else(|| {
                    at.child(&i.to_string())
                        .error(LoadErrorKind::Shape("a block id"))
                })
            })
            .collect(),
        _ => Err(at.error(LoadErrorKind::Shape("a block id, tag or list"))),
    }
}

/// `floatRange(0, 1)` for `probability`.
fn probability(o: &serde_json::Map<String, Value>, at: &At<'_>) -> Result<f32, LoadError> {
    let at = at.child("probability");
    let p = float(field(o, "probability", &at)?, &at)?;
    if !(0.0..=1.0).contains(&p) {
        return Err(at.error(LoadErrorKind::OutOfRange {
            value: p.to_string(),
            min: "0".to_string(),
            max: "1".to_string(),
        }));
    }
    Ok(p)
}

impl Carver {
    /// `WorldCarver.DIRECT_CODEC`.
    pub fn parse(json: &Value, at: &At<'_>) -> Result<Carver, LoadError> {
        let o = object(json, at)?;
        let kind = field(o, "type", at)?
            .as_str()
            .map(full_id)
            .ok_or_else(|| at.child("type").error(LoadErrorKind::Shape("a type id")))?;
        let floats =
            |name: &'static str| FloatProvider::parse(field(o, name, at)?, &at.child(name));
        let y = HeightProvider::parse(field(o, "y", at)?, &at.child("y"))?;
        Ok(match kind.as_str() {
            "minecraft:cave" => {
                let count_at = at.child("count");
                let count = IntProvider::parse(field(o, "count", at)?, &count_at)?;
                if count.min() < 0 {
                    return Err(count_at.error(LoadErrorKind::Invalid("count can't be negative")));
                }
                let weird_thickness_bias = match o.get("weird_thickness_bias") {
                    None => false,
                    Some(v) => v.as_bool().ok_or_else(|| {
                        at.child("weird_thickness_bias")
                            .error(LoadErrorKind::Shape("true or false"))
                    })?,
                };
                let start_vertical_radius_multiplier =
                    match o.get("start_vertical_radius_multiplier") {
                        None => FloatProvider::Constant(1.0),
                        Some(v) => {
                            FloatProvider::parse(v, &at.child("start_vertical_radius_multiplier"))?
                        }
                    };
                Carver::Cave(Box::new(Cave {
                    probability: probability(o, at)?,
                    y,
                    count,
                    thickness: FloatProvider::parse_in(
                        field(o, "thickness", at)?,
                        0.0,
                        None,
                        &at.child("thickness"),
                    )?,
                    weird_thickness_bias,
                    room_vertical_radius_multiplier: floats("room_vertical_radius_multiplier")?,
                    horizontal_radius_multiplier: floats("horizontal_radius_multiplier")?,
                    vertical_radius_multiplier: floats("vertical_radius_multiplier")?,
                    start_vertical_radius_multiplier,
                    floor_level: FloatProvider::parse_in(
                        field(o, "floor_level", at)?,
                        -1.0,
                        Some(1.0),
                        &at.child("floor_level"),
                    )?,
                }))
            }
            "minecraft:canyon" => {
                let shape_at = at.child("shape");
                let shape = object(field(o, "shape", at)?, &shape_at)?;
                let shape_floats = |name: &'static str| {
                    FloatProvider::parse(field(shape, name, &shape_at)?, &shape_at.child(name))
                };
                let shape_float = |name: &'static str| {
                    float(field(shape, name, &shape_at)?, &shape_at.child(name))
                };
                let smoothness_at = shape_at.child("width_smoothness");
                let width_smoothness =
                    int(field(shape, "width_smoothness", &shape_at)?, &smoothness_at)?;
                if width_smoothness < 1 {
                    return Err(smoothness_at.error(LoadErrorKind::OutOfRange {
                        value: width_smoothness.to_string(),
                        min: "1".to_string(),
                        max: i32::MAX.to_string(),
                    }));
                }
                Carver::Canyon(Box::new(Canyon {
                    probability: probability(o, at)?,
                    y,
                    vertical_rotation: floats("vertical_rotation")?,
                    distance_factor: shape_floats("distance_factor")?,
                    thickness: shape_floats("thickness")?,
                    width_smoothness,
                    horizontal_radius_factor: shape_floats("horizontal_radius_factor")?,
                    vertical_radius_default_factor: shape_float("vertical_radius_default_factor")?,
                    vertical_radius_center_factor: shape_float("vertical_radius_center_factor")?,
                    y_scale: shape_floats("y_scale")?,
                }))
            }
            _ => {
                return Err(at.child("type").error(LoadErrorKind::Unsupported {
                    what: "carver type",
                    name: kind,
                }));
            }
        })
    }

    /// 26.2's `ConfiguredWorldCarver.CODEC`: `type` and `config`, as the 26.3 shape that carves
    /// the same, and what 26.2 adds. 26.2's caves draw their count as
    /// `nextInt(nextInt(nextInt(bound) + 1) + 1)` (bound 15, the Nether's 10), their thickness as
    /// `nextFloat() · 2 + nextFloat()` (the Nether's doubled), and scale their tunnels' vertical
    /// radius by 1 (the Nether's 5): exactly `very_biased_to_bottom` from 0, `trapezoid` from 0
    /// to 3 with plateau 1 (to 6 with plateau 2) and a constant start multiplier, with 26.3's
    /// draws and results.
    pub fn parse_v1_21<R: Resources + ?Sized>(
        resources: &R,
        json: &Value,
        at: &At<'_>,
    ) -> Result<(Carver, Legacy), LoadError> {
        let o = object(json, at)?;
        let kind = field(o, "type", at)?
            .as_str()
            .map(full_id)
            .ok_or_else(|| at.child("type").error(LoadErrorKind::Shape("a type id")))?;
        let at = at.child("config");
        let c = object(field(o, "config", &at)?, &at)?;
        let floats =
            |name: &'static str| FloatProvider::parse(field(c, name, &at)?, &at.child(name));
        let y = HeightProvider::parse(field(c, "y", &at)?, &at.child("y"))?;
        let lava_level =
            VerticalAnchor::parse(field(c, "lava_level", &at)?, &at.child("lava_level"))?;
        let replaceable = block_set(
            resources,
            field(c, "replaceable", &at)?,
            &at.child("replaceable"),
        )?;
        let y_scale = floats("yScale")?;
        let nether = kind == "minecraft:nether_cave";
        let carver = match kind.as_str() {
            "minecraft:cave" | "minecraft:nether_cave" => {
                let (bound, max, plateau, start) = if nether {
                    (10, 6.0, 2.0, 5.0)
                } else {
                    (15, 3.0, 1.0, 1.0)
                };
                Carver::Cave(Box::new(Cave {
                    probability: probability(c, &at)?,
                    y,
                    count: IntProvider::VeryBiasedToBottom(0, bound - 1),
                    thickness: FloatProvider::Trapezoid(0.0, max, plateau),
                    weird_thickness_bias: !nether,
                    room_vertical_radius_multiplier: y_scale,
                    horizontal_radius_multiplier: floats("horizontal_radius_multiplier")?,
                    vertical_radius_multiplier: floats("vertical_radius_multiplier")?,
                    start_vertical_radius_multiplier: FloatProvider::Constant(start),
                    floor_level: FloatProvider::parse_in(
                        field(c, "floor_level", &at)?,
                        -1.0,
                        Some(1.0),
                        &at.child("floor_level"),
                    )?,
                }))
            }
            "minecraft:canyon" => {
                let mut shape_json = field(c, "shape", &at)?.clone();
                if let Value::Object(shape) = &mut shape_json {
                    shape.insert("y_scale".to_string(), c["yScale"].clone());
                }
                let mut canyon = serde_json::Map::new();
                canyon.insert("type".to_string(), Value::String(kind.clone()));
                for name in ["probability", "y", "vertical_rotation"] {
                    canyon.insert(name.to_string(), field(c, name, &at)?.clone());
                }
                canyon.insert("shape".to_string(), shape_json);
                Carver::parse(&Value::Object(canyon), &at)?
            }
            _ => {
                return Err(at.child("type").error(LoadErrorKind::Unsupported {
                    what: "carver type",
                    name: kind,
                }));
            }
        };
        Ok((
            carver,
            Legacy {
                replaceable,
                lava_level,
                nether,
            },
        ))
    }

    /// `isStartChunk`: `nextFloat() ≤ probability`.
    pub fn is_start_chunk(&self, random: &mut LegacyRandomSource) -> bool {
        let p = match self {
            Carver::Cave(c) => c.probability,
            Carver::Canyon(c) => c.probability,
        };
        // fcmpg/ifgt: NaN never starts.
        random.next_float() <= p
    }

    /// `carve(context, random, chunk, start, mask)`: the carver started in chunk `start`, carving
    /// into `mask` for chunk `chunk`.
    pub fn carve(
        &self,
        anchors: AnchorHeights,
        random: &mut LegacyRandomSource,
        chunk: [i32; 2],
        start: [i32; 2],
        mask: &mut impl Sink,
    ) {
        match self {
            Carver::Cave(c) => c.carve(anchors, random, chunk, start, mask),
            Carver::Canyon(c) => c.carve(anchors, random, chunk, start, mask),
        }
    }
}

/// Where a carver's ellipsoids carve: 26.3's mask, or 26.2's chunk itself.
pub trait Sink {
    /// The lowest y, which ellipsoids stop above.
    fn min_y(&self) -> i32;
    /// The highest y an ellipsoid carves.
    fn max_y(&self) -> i32;
    /// An ellipsoid starts a column (26.2's `MutableBoolean`, for carved grass).
    fn column(&mut self) {}
    /// Carves `(x, y, z)`, x and z within the chunk.
    fn carve(&mut self, x: i32, y: i32, z: i32);
}

/// `CarvingMask`: the positions carvers carve in one chunk, between `min_y` and `max_y`.
#[derive(Debug, Clone)]
pub struct CarvingMask {
    min_y: i32,
    max_y: i32,
    height: i32,
    /// By column (`(x << 4) + z`), then y from `min_y`.
    bits: Vec<u64>,
}

impl Sink for CarvingMask {
    fn min_y(&self) -> i32 {
        self.min_y
    }

    fn max_y(&self) -> i32 {
        self.max_y
    }

    fn carve(&mut self, x: i32, y: i32, z: i32) {
        CarvingMask::carve(self, x, y, z);
    }
}

impl CarvingMask {
    /// An empty mask over `min_y..=max_y`.
    pub fn new(min_y: i32, max_y: i32) -> Self {
        let height = max_y - min_y + 1;
        CarvingMask {
            min_y,
            max_y,
            height,
            bits: vec![0; (256 * height.max(0) as usize).div_ceil(64)],
        }
    }

    fn index(&self, x: i32, y: i32, z: i32) -> usize {
        ((y - self.min_y) + (z + (x << 4)) * self.height) as usize
    }

    /// `carve(x, y, z)`, with x and z within the chunk.
    pub fn carve(&mut self, x: i32, y: i32, z: i32) {
        let i = self.index(x, y, z);
        self.bits[i / 64] |= 1 << (i % 64);
    }

    /// Whether `(x, y, z)` is carved, with x and z within the chunk.
    pub fn is_carved(&self, x: i32, y: i32, z: i32) -> bool {
        if !(self.min_y..=self.max_y).contains(&y) {
            return false;
        }
        let i = self.index(x, y, z);
        self.bits[i / 64] & (1 << (i % 64)) != 0
    }

    /// Whether nothing is carved.
    pub fn is_empty(&self) -> bool {
        self.bits.iter().all(|&b| b == 0)
    }

    /// `visit`: each column's runs of carved positions, columns by `(x << 4) + z`, runs bottom
    /// up, as `(x, z, min_y, max_y)`.
    pub fn runs(&self) -> Vec<(i32, i32, i32, i32)> {
        let mut runs = Vec::new();
        for column in 0..256 {
            let (x, z) = (column >> 4, column & 15);
            let mut y = self.min_y;
            while y <= self.max_y {
                if self.is_carved(x, y, z) {
                    let bottom = y;
                    while y < self.max_y && self.is_carved(x, y + 1, z) {
                        y += 1;
                    }
                    runs.push((x, z, bottom, y));
                }
                y += 1;
            }
        }
        runs
    }

    /// Whether `(x, y, z)` is carved, then marks it (26.2's `get`, then `set`).
    pub fn test_and_carve(&mut self, x: i32, y: i32, z: i32) -> bool {
        let carved = self.is_carved(x, y, z);
        self.carve(x, y, z);
        carved
    }

    /// Adds every position `other` carves.
    pub fn union(&mut self, other: &CarvingMask) {
        for (a, b) in self.bits.iter_mut().zip(&other.bits) {
            *a |= b;
        }
    }
}

/// 26.3's `Mth.SIN`: `(float) Math.sin(i / 10430.378350470453)` for `i` in `0..65536`.
fn sin_table() -> &'static [f32] {
    static TABLE: OnceLock<Vec<f32>> = OnceLock::new();
    TABLE.get_or_init(|| {
        (0..65_536)
            .map(|i| (f64::from(i) / 10_430.378_350_470_453).sin() as f32)
            .collect()
    })
}

/// 26.3's `Mth.sin(double)`: `SIN[(int) ((long) (x · 10430.378350470453) & 65535)]`.
fn sin(x: f64) -> f32 {
    sin_table()[((x * 10_430.378_350_470_453) as i64 & 65_535) as usize]
}

/// 26.3's `Mth.cos(double)`: `SIN[(int) ((long) (x · 10430.378350470453 + 16384) & 65535)]`.
fn cos(x: f64) -> f32 {
    sin_table()[((x * 10_430.378_350_470_453 + 16_384.0) as i64 & 65_535) as usize]
}

/// `Mth.floor(double)`.
fn floor(v: f64) -> i32 {
    let i = v as i32;
    if v < f64::from(i) { i - 1 } else { i }
}

/// `Mth.randomBetween(random, min, max)`.
fn random_between(random: &mut LegacyRandomSource, min: f32, max: f32) -> f32 {
    random.next_float() * (max - min) + min
}

/// The chunk's minimum block x and z.
fn min_block([x, z]: [i32; 2]) -> [i32; 2] {
    [x.wrapping_shl(4), z.wrapping_shl(4)]
}

/// `WorldCarver.canReach`: whether a tunnel at `(x, z)` with `count − step` steps left could
/// still reach chunk `chunk`.
fn can_reach(chunk: [i32; 2], x: f64, z: f64, step: i32, count: i32, thickness: f32) -> bool {
    let [mx, mz] = min_block(chunk).map(|v| f64::from(v.wrapping_add(8)));
    let (dx, dz) = (x - mx, z - mz);
    let left = f64::from(count.wrapping_sub(step));
    let reach = f64::from(thickness + 2.0 + 16.0);
    dx * dx + dz * dz - left * left <= reach * reach
}

/// `WorldCarver.carveEllipsoid`: every position of the ellipsoid at `(x, y, z)` with radii
/// `horizontal` and `vertical` that's within the chunk and the mask, and that `skip` doesn't
/// skip.
#[allow(clippy::too_many_arguments)]
fn carve_ellipsoid(
    chunk: [i32; 2],
    x: f64,
    y: f64,
    z: f64,
    horizontal: f64,
    vertical: f64,
    sink: &mut impl Sink,
    skip: impl Fn(f64, f64, f64, i32) -> bool,
) {
    let [min_x, min_z] = min_block(chunk);
    let (mx, mz) = (f64::from(min_x + 8), f64::from(min_z + 8));
    let reach = 16.0 + horizontal * 2.0;
    // dcmpl/ifgt, dcmpl/ifle: NaN goes on.
    if (x - mx).abs() > reach || (z - mz).abs() > reach {
        return;
    }
    let x0 = (floor(x - horizontal) - min_x - 1).max(0);
    let x1 = (floor(x + horizontal) - min_x).min(15);
    let y0 = (floor(y - vertical) - 1).max(sink.min_y());
    let y1 = (floor(y + vertical) + 1).min(sink.max_y());
    let z0 = (floor(z - horizontal) - min_z - 1).max(0);
    let z1 = (floor(z + horizontal) - min_z).min(15);
    for lx in x0..=x1 {
        let fx = (f64::from(min_x + lx) + 0.5 - x) / horizontal;
        for lz in z0..=z1 {
            let fz = (f64::from(min_z + lz) + 0.5 - z) / horizontal;
            // dcmpl/iflt: NaN carves.
            if fx * fx + fz * fz >= 1.0 {
                continue;
            }
            sink.column();
            let mut by = y1;
            while by > y0 {
                let fy = (f64::from(by) - 0.5 - y) / vertical;
                if !skip(fx, fy, fz, by) {
                    sink.carve(lx, by, lz);
                }
                by -= 1;
            }
        }
    }
}

/// `CaveWorldCarver.shouldSkip`: at or below the floor, or outside the unit sphere.
fn cave_skip(fx: f64, fy: f64, fz: f64, floor_level: f64) -> bool {
    // dcmpg/ifgt: NaN isn't below the floor; dcmpl/iflt: NaN is inside.
    fy <= floor_level || fx * fx + fy * fy + fz * fz >= 1.0
}

impl Cave {
    /// `CaveWorldCarver.carve`: `count` caves, each a room or not and some tunnels.
    fn carve(
        &self,
        anchors: AnchorHeights,
        random: &mut LegacyRandomSource,
        chunk: [i32; 2],
        start: [i32; 2],
        mask: &mut impl Sink,
    ) {
        let range = (RANGE * 2 - 1) << 4;
        let [sx, sz] = min_block(start);
        let count = self.count.sample(random);
        for _ in 0..count {
            let x = f64::from(sx.wrapping_add(random.next_int_bounded(16)));
            let y = f64::from(self.y.sample(random, anchors));
            let z = f64::from(sz.wrapping_add(random.next_int_bounded(16)));
            let horizontal = f64::from(self.horizontal_radius_multiplier.sample(random));
            let vertical = f64::from(self.vertical_radius_multiplier.sample(random));
            let start_vertical = f64::from(self.start_vertical_radius_multiplier.sample(random));
            let floor_level = f64::from(self.floor_level.sample(random));
            let skip = |fx, fy, fz, _y| cave_skip(fx, fy, fz, floor_level);
            let mut tunnels = 1;
            if random.next_int_bounded(4) == 0 {
                let room_vertical = f64::from(self.room_vertical_radius_multiplier.sample(random));
                let thickness = 1.0 + random.next_float() * 6.0;
                // createRoom.
                let h = 1.5 + f64::from(sin(1.570_796_370_506_286_6) * thickness);
                carve_ellipsoid(chunk, x + 1.0, y, z, h, h * room_vertical, mask, skip);
                tunnels += random.next_int_bounded(4);
            }
            for _ in 0..tunnels {
                let yaw = random.next_float() * 6.283_185_5;
                let pitch = (random.next_float() - 0.5) / 4.0;
                let thickness = self.thickness(random);
                let steps = range - random.next_int_bounded(range / 4);
                let tunnel = Tunnel {
                    chunk,
                    horizontal,
                    vertical,
                    floor_level,
                };
                tunnel.create(
                    random.next_long(),
                    [x, y, z],
                    thickness,
                    yaw,
                    pitch,
                    0,
                    steps,
                    start_vertical,
                    mask,
                );
            }
        }
    }

    /// `getThickness`: the thickness, sometimes much thicker with `weird_thickness_bias`.
    fn thickness(&self, random: &mut LegacyRandomSource) -> f32 {
        let mut t = self.thickness.sample(random);
        if self.weird_thickness_bias && random.next_int_bounded(10) == 0 {
            let a = random.next_float();
            t *= a * random.next_float() * 3.0 + 1.0;
        }
        t
    }
}

/// What a cave's tunnels share.
struct Tunnel {
    chunk: [i32; 2],
    horizontal: f64,
    vertical: f64,
    floor_level: f64,
}

impl Tunnel {
    /// `CaveWorldCarver.createTunnel`: a tunnel from `step` to `count` steps, which may split in
    /// two.
    #[allow(clippy::too_many_arguments)]
    fn create(
        &self,
        seed: i64,
        [mut x, mut y, mut z]: [f64; 3],
        thickness: f32,
        mut yaw: f32,
        mut pitch: f32,
        step: i32,
        count: i32,
        vertical_scale: f64,
        mask: &mut impl Sink,
    ) {
        let mut random = LegacyRandomSource::new(seed);
        let split = random.next_int_bounded(count / 2) + count / 4;
        let steep = random.next_int_bounded(6) == 0;
        let mut yaw_change = 0.0_f32;
        let mut pitch_change = 0.0_f32;
        for i in step..count {
            let h = 1.5
                + f64::from(
                    sin(f64::from(std::f32::consts::PI * i as f32 / count as f32)) * thickness,
                );
            let v = h * vertical_scale;
            let cos_pitch = cos(f64::from(pitch));
            x += f64::from(cos(f64::from(yaw)) * cos_pitch);
            y += f64::from(sin(f64::from(pitch)));
            z += f64::from(sin(f64::from(yaw)) * cos_pitch);
            pitch *= if steep { 0.92 } else { 0.7 };
            pitch += pitch_change * 0.1;
            yaw += yaw_change * 0.1;
            pitch_change *= 0.9;
            yaw_change *= 0.75;
            let (a, b) = (random.next_float(), random.next_float());
            pitch_change += (a - b) * random.next_float() * 2.0;
            let (a, b) = (random.next_float(), random.next_float());
            yaw_change += (a - b) * random.next_float() * 4.0;
            if i == split && thickness > 1.0 {
                for turn in [-1.570_796_4_f32, 1.570_796_4] {
                    let seed = random.next_long();
                    let branch = random.next_float() * 0.5 + 0.5;
                    self.create(
                        seed,
                        [x, y, z],
                        branch,
                        yaw + turn,
                        pitch / 3.0,
                        i,
                        count,
                        1.0,
                        mask,
                    );
                }
                return;
            }
            if random.next_int_bounded(4) != 0 {
                if !can_reach(self.chunk, x, z, i, count, thickness) {
                    return;
                }
                let floor_level = self.floor_level;
                carve_ellipsoid(
                    self.chunk,
                    x,
                    y,
                    z,
                    h * self.horizontal,
                    v * self.vertical,
                    mask,
                    |fx, fy, fz, _y| cave_skip(fx, fy, fz, floor_level),
                );
            }
        }
    }
}

impl Canyon {
    /// `CanyonWorldCarver.carve`: one canyon.
    fn carve(
        &self,
        anchors: AnchorHeights,
        random: &mut LegacyRandomSource,
        chunk: [i32; 2],
        start: [i32; 2],
        mask: &mut impl Sink,
    ) {
        let range = (RANGE * 2 - 1) * 16;
        let [sx, sz] = min_block(start);
        let x = f64::from(sx.wrapping_add(random.next_int_bounded(16)));
        let y = self.y.sample(random, anchors);
        let z = f64::from(sz.wrapping_add(random.next_int_bounded(16)));
        let yaw = random.next_float() * 6.283_185_5;
        let pitch = self.vertical_rotation.sample(random);
        let y_scale = f64::from(self.y_scale.sample(random));
        let thickness = self.thickness.sample(random);
        let count = (range as f32 * self.distance_factor.sample(random)) as i32;
        let seed = random.next_long();
        self.carve_from(
            anchors,
            chunk,
            seed,
            [x, f64::from(y), z],
            thickness,
            yaw,
            pitch,
            count,
            y_scale,
            mask,
        );
    }

    /// `doCarve`.
    #[allow(clippy::too_many_arguments)]
    fn carve_from(
        &self,
        anchors: AnchorHeights,
        chunk: [i32; 2],
        seed: i64,
        [mut x, mut y, mut z]: [f64; 3],
        thickness: f32,
        mut yaw: f32,
        mut pitch: f32,
        count: i32,
        y_scale: f64,
        mask: &mut impl Sink,
    ) {
        let mut random = LegacyRandomSource::new(seed);
        let widths = self.width_factors(anchors, &mut random);
        let mut yaw_change = 0.0_f32;
        let mut pitch_change = 0.0_f32;
        for i in 0..count {
            let h = 1.5
                + f64::from(
                    sin(f64::from(std::f32::consts::PI * i as f32 / count as f32)) * thickness,
                );
            let v = h * y_scale;
            let h = h * f64::from(self.horizontal_radius_factor.sample(&mut random));
            let v = self.vertical_radius(&mut random, v, count as f32, i as f32);
            let (cos_pitch, sin_pitch) = (cos(f64::from(pitch)), sin(f64::from(pitch)));
            x += f64::from(cos(f64::from(yaw)) * cos_pitch);
            y += f64::from(sin_pitch);
            z += f64::from(sin(f64::from(yaw)) * cos_pitch);
            pitch *= 0.7;
            pitch += pitch_change * 0.05;
            yaw += yaw_change * 0.05;
            pitch_change *= 0.8;
            yaw_change *= 0.5;
            let (a, b) = (random.next_float(), random.next_float());
            pitch_change += (a - b) * random.next_float() * 2.0;
            let (a, b) = (random.next_float(), random.next_float());
            yaw_change += (a - b) * random.next_float() * 4.0;
            if random.next_int_bounded(4) != 0 {
                if !can_reach(chunk, x, z, i, count, thickness) {
                    return;
                }
                let min_y = anchors.min_y;
                carve_ellipsoid(chunk, x, y, z, h, v, mask, |fx, fy, fz, by| {
                    // dcmpl/iflt: NaN is inside.
                    let width = f64::from(widths[(by - min_y - 1) as usize]);
                    (fx * fx + fz * fz) * width + fy * fy / 6.0 >= 1.0
                });
            }
        }
    }

    /// `initWidthFactors`: a factor per y of the generation depth, changed at random.
    fn width_factors(&self, anchors: AnchorHeights, random: &mut LegacyRandomSource) -> Vec<f32> {
        let mut f = 1.0_f32;
        (0..anchors.height.max(0))
            .map(|i| {
                if i == 0 || random.next_int_bounded(self.width_smoothness) == 0 {
                    let a = random.next_float();
                    f = 1.0 + a * random.next_float();
                }
                f * f
            })
            .collect()
    }

    /// `updateVerticalRadius`: wider at the canyon's middle, times a random 0.75 to 1.
    fn vertical_radius(&self, random: &mut LegacyRandomSource, v: f64, count: f32, i: f32) -> f64 {
        let k = 1.0 - (0.5 - i / count).abs() * 2.0;
        let factor = self.vertical_radius_default_factor + self.vertical_radius_center_factor * k;
        f64::from(factor) * v * f64::from(random_between(random, 0.75, 1.0))
    }
}
