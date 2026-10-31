//! `LargeDripstoneFeature`: a stalactite and a stalagmite facing each other across a cave,
//! perhaps leaning in the wind (`docs/specs/features.md#large-dripstone`). The same algorithm in
//! every supported version; 26.4's `large_speleothem` takes its block from `base_block`, where
//! earlier versions always use dripstone, and `Mth.sin` and `Mth.cos` follow switch
//! `rounded_trig`.

use serde_json::{Map, Value};
use spyglass_blocks::State;
use spyglass_blocks::carver::Trig;
use spyglass_data::codec::{At, LoadError, LoadErrorKind, field, float, full_id, int_field};
use spyglass_data::providers::{FloatProvider, IntProvider};
use spyglass_density::Resources;
use spyglass_rng::{RandomSource, WorldgenRandom};

use crate::blocks::{Blocks, Heightmap, Tag};
use crate::region::Region;

/// A large dripstone's configuration, with the codec's defaults.
#[derive(Debug, Clone)]
pub struct LargeDripstone {
    /// The default state of the base block's block: dripstone before 26.4.
    base: State,
    /// The base block's id, which `isBase` tests by block.
    base_id: String,
    replaceable: Tag,
    base_stone: Tag,
    floor_to_ceiling_search_range: i32,
    column_radius: IntProvider,
    height_scale: FloatProvider,
    max_column_radius_to_cave_height_ratio: f32,
    stalactite_bluntness: FloatProvider,
    stalagmite_bluntness: FloatProvider,
    wind_speed: FloatProvider,
    min_radius_for_wind: i32,
    min_bluntness_for_wind: f32,
    trig: Trig,
}

/// One of the pair (`LargeDripstoneFeature.LargeDripstone`).
struct Speleothem {
    root: [i32; 3],
    pointing_up: bool,
    radius: i32,
    bluntness: f64,
    scale: f64,
}

/// `WindOffsetter`: how far the wind pushes a block, by how far it is from the origin's y.
struct Wind {
    origin_y: i32,
    /// `windSpeed`'s x and z, or `None` for `noWind()`.
    speed: Option<(f64, f64)>,
    max_offset: i32,
}

impl Wind {
    /// `WindOffsetter.offset`.
    fn offset(&self, pos @ [x, y, z]: [i32; 3]) -> [i32; 3] {
        let Some((sx, sz)) = self.speed else {
            return pos;
        };
        let d = f64::from(self.origin_y.wrapping_sub(y));
        let clamp = |v: f64| floor(v).max(-self.max_offset).min(self.max_offset);
        [
            x.wrapping_add(clamp(sx * d)),
            y,
            z.wrapping_add(clamp(sz * d)),
        ]
    }
}

/// `Mth.floor(double)`.
fn floor(v: f64) -> i32 {
    v.floor() as i32
}

/// A float field in `floatRange(min, max)`.
fn float_in(
    o: &Map<String, Value>,
    key: &'static str,
    min: f32,
    max: f32,
    at: &At<'_>,
) -> Result<f32, LoadError> {
    let v = float(field(o, key, at)?, &at.child(key))?;
    if (min..=max).contains(&v) {
        Ok(v)
    } else {
        Err(at.child(key).error(LoadErrorKind::OutOfRange {
            value: v.to_string(),
            min: min.to_string(),
            max: max.to_string(),
        }))
    }
}

impl LargeDripstone {
    /// The one state it writes: the base block's.
    pub(crate) fn state(&self) -> State {
        self.base
    }

    /// From the configured feature's fields.
    pub fn load(
        o: &Map<String, Value>,
        at: &At<'_>,
        resources: &dyn Resources,
        blocks: &mut Blocks,
    ) -> Result<LargeDripstone, LoadError> {
        // `baseBlock.getBlock()`: the state's block is what counts, placed in its default state.
        let base_id = match o.get("base_block") {
            Some(json) => {
                let s = blocks.parse_state(json, &at.child("base_block"))?;
                blocks.palette().block_id(s).to_string()
            }
            None => "minecraft:dripstone_block".to_string(),
        };
        let base = blocks.state(&base_id)?;
        let replaceable = {
            let s = field(o, "replaceable_blocks", at)?
                .as_str()
                .and_then(|s| s.strip_prefix('#'))
                .ok_or_else(|| {
                    at.child("replaceable_blocks")
                        .error(LoadErrorKind::Shape("a #tag"))
                })?;
            blocks.tag(resources, &full_id(s))?
        };
        let int_provider =
            |key: &'static str| IntProvider::parse(field(o, key, at)?, &at.child(key));
        let float_provider =
            |key: &'static str| FloatProvider::parse(field(o, key, at)?, &at.child(key));
        Ok(LargeDripstone {
            base,
            base_id,
            replaceable,
            base_stone: blocks.tag(resources, "minecraft:base_stone_overworld")?,
            floor_to_ceiling_search_range: int_field(
                o,
                "floor_to_ceiling_search_range",
                (1, 512),
                Some(30),
                at,
            )?,
            column_radius: int_provider("column_radius")?,
            height_scale: float_provider("height_scale")?,
            max_column_radius_to_cave_height_ratio: float_in(
                o,
                "max_column_radius_to_cave_height_ratio",
                0.1,
                1.0,
                at,
            )?,
            stalactite_bluntness: float_provider("stalactite_bluntness")?,
            stalagmite_bluntness: float_provider("stalagmite_bluntness")?,
            wind_speed: float_provider("wind_speed")?,
            min_radius_for_wind: int_field(o, "min_radius_for_wind", (0, 100), None, at)?,
            min_bluntness_for_wind: float_in(o, "min_bluntness_for_wind", 0.0, 5.0, at)?,
            trig: if resources.behavior().rounded_trig {
                Trig::Rounded
            } else {
                Trig::Truncated
            },
        })
    }

    /// `LargeDripstoneFeature.place`: whether it placed.
    pub fn place(
        &self,
        region: &mut Region<'_>,
        random: &mut WorldgenRandom,
        origin: [i32; 3],
    ) -> bool {
        if !self.empty_or_water(region, origin) {
            return false;
        }
        // Column.scan: up for the ceiling, then down for the floor.
        let ceiling = self.scan(region, origin, 1);
        let floor = self.scan(region, origin, -1);
        let (Some(ceiling), Some(floor)) = (ceiling, floor) else {
            return false;
        };
        let height = ceiling.wrapping_sub(floor).wrapping_sub(1);
        if height < 4 {
            return false;
        }
        let limit = (height as f32 * self.max_column_radius_to_cave_height_ratio) as i32;
        let (min, max) = (self.column_radius.min(), self.column_radius.max());
        let max_radius = limit.max(min).min(max);
        // Mth.randomBetweenInclusive.
        let radius = random
            .next_int_bounded(max_radius.wrapping_sub(min).wrapping_add(1))
            .wrapping_add(min);
        let [x, y, z] = origin;
        let make = |random: &mut WorldgenRandom, y: i32, up: bool, bluntness: &FloatProvider| {
            let bluntness = f64::from(bluntness.sample(random));
            let scale = f64::from(self.height_scale.sample(random));
            Speleothem {
                root: [x, y, z],
                pointing_up: up,
                radius,
                bluntness,
                scale,
            }
        };
        let mut stalactite = make(
            random,
            ceiling.wrapping_sub(1),
            false,
            &self.stalactite_bluntness,
        );
        let mut stalagmite = make(
            random,
            floor.wrapping_add(1),
            true,
            &self.stalagmite_bluntness,
        );
        let wind = if stalactite
            .suitable_for_wind(self.min_radius_for_wind, self.min_bluntness_for_wind)
            && stalagmite.suitable_for_wind(self.min_radius_for_wind, self.min_bluntness_for_wind)
        {
            let speed = self.wind_speed.sample(random);
            // Mth.randomBetween(random, 0, π).
            let angle = random.next_float() * (std::f32::consts::PI - 0.0) + 0.0;
            Wind {
                origin_y: y,
                speed: Some((
                    f64::from(self.trig.cos(f64::from(angle)) * speed),
                    f64::from(self.trig.sin(f64::from(angle)) * speed),
                )),
                max_offset: 16_i32.wrapping_sub(radius),
            }
        } else {
            Wind {
                origin_y: 0,
                speed: None,
                max_offset: 0,
            }
        };
        let stalactite_fits = self.move_back(region, &mut stalactite, &wind);
        let stalagmite_fits = self.move_back(region, &mut stalagmite, &wind);
        if stalactite_fits {
            self.place_blocks(region, random, &stalactite, &wind);
        }
        if stalagmite_fits {
            self.place_blocks(region, random, &stalagmite, &wind);
        }
        true
    }

    /// `Column.scanDirection` from `origin` by `dy`: the edge's y, if it's base or lava.
    fn scan(&self, region: &mut Region<'_>, origin: [i32; 3], dy: i32) -> Option<i32> {
        let mut pos = origin;
        let mut i = 1;
        while i < self.floor_to_ceiling_search_range && self.empty_or_water(region, pos) {
            pos[1] = pos[1].wrapping_add(dy);
            i += 1;
        }
        let s = region.get(pos);
        let blocks = region.blocks();
        let id = blocks.palette().block_id(s);
        // SpeleothemUtils.isBaseOrLava.
        (id == self.base_id || blocks.is(s, self.replaceable) || id == "minecraft:lava")
            .then_some(pos[1])
    }

    /// `SpeleothemUtils.isEmptyOrWater`.
    fn empty_or_water(&self, region: &mut Region<'_>, pos: [i32; 3]) -> bool {
        let s = region.get(pos);
        let blocks = region.blocks();
        blocks.is_air(s) || blocks.palette().block_id(s) == "minecraft:water"
    }

    /// `SpeleothemUtils.isEmptyOrWaterOrLava`.
    fn empty_or_fluid(&self, region: &mut Region<'_>, pos: [i32; 3]) -> bool {
        let s = region.get(pos);
        let blocks = region.blocks();
        blocks.is_air(s) || {
            let id = blocks.palette().block_id(s);
            id == "minecraft:water" || id == "minecraft:lava"
        }
    }

    /// `SpeleothemUtils.isCircleMostlyEmbeddedInStone`.
    fn embedded(&self, region: &mut Region<'_>, [x, y, z]: [i32; 3], radius: i32) -> bool {
        if self.empty_or_fluid(region, [x, y, z]) {
            return false;
        }
        let step = 6.0_f32 / radius as f32;
        let mut f = 0.0_f32;
        while f < 6.283_185_5_f32 {
            let dx = (self.trig.cos(f64::from(f)) * radius as f32) as i32;
            let dz = (self.trig.sin(f64::from(f)) * radius as f32) as i32;
            if self.empty_or_fluid(region, [x.wrapping_add(dx), y, z.wrapping_add(dz)]) {
                return false;
            }
            f += step;
        }
        true
    }

    /// `moveBackUntilBaseIsInsideStoneAndShrinkRadiusIfNecessary`: whether it found a root.
    fn move_back(&self, region: &mut Region<'_>, s: &mut Speleothem, wind: &Wind) -> bool {
        while s.radius > 1 {
            let mut pos = s.root;
            let n = s.height().min(10);
            for _ in 0..n {
                let state = region.get(pos);
                if region.blocks().palette().block_id(state) == "minecraft:lava" {
                    return false;
                }
                if self.embedded(region, wind.offset(pos), s.radius) {
                    s.root = pos;
                    return true;
                }
                pos[1] = pos[1].wrapping_add(if s.pointing_up { -1 } else { 1 });
            }
            s.radius /= 2;
        }
        false
    }

    /// `LargeDripstone.placeBlocks`.
    fn place_blocks(
        &self,
        region: &mut Region<'_>,
        random: &mut WorldgenRandom,
        s: &Speleothem,
        wind: &Wind,
    ) {
        let r = s.radius;
        let step = if s.pointing_up { 1 } else { -1 };
        for dx in -r..=r {
            for dz in -r..=r {
                // Mth.sqrt(float).
                let d = f64::from((dx * dx + dz * dz) as f32).sqrt() as f32;
                if d > r as f32 {
                    continue;
                }
                let mut h = s.height_at(d);
                if h <= 0 {
                    continue;
                }
                if f64::from(random.next_float()) < 0.2 {
                    // Mth.randomBetween(random, 0.8, 1).
                    h = (h as f32 * (random.next_float() * (1.0_f32 - 0.8_f32) + 0.8_f32)) as i32;
                }
                let mut pos = [
                    s.root[0].wrapping_add(dx),
                    s.root[1],
                    s.root[2].wrapping_add(dz),
                ];
                let mut placed = false;
                let max_y = if s.pointing_up {
                    region.height(Heightmap::WorldSurfaceWg, pos[0], pos[2])
                } else {
                    i32::MAX
                };
                for _ in 0..h {
                    if pos[1] >= max_y {
                        break;
                    }
                    let p = wind.offset(pos);
                    if self.empty_or_fluid(region, p) {
                        placed = true;
                        region.set(p, self.base);
                    } else if placed {
                        let state = region.get(p);
                        if region.blocks().is(state, self.base_stone) {
                            break;
                        }
                    }
                    pos[1] = pos[1].wrapping_add(step);
                }
            }
        }
    }
}

impl Speleothem {
    /// `getHeight`.
    fn height(&self) -> i32 {
        self.height_at(0.0)
    }

    /// `getHeightAtRadius`.
    fn height_at(&self, d: f32) -> i32 {
        speleothem_height(
            f64::from(d),
            f64::from(self.radius),
            self.scale,
            self.bluntness,
        ) as i32
    }

    /// `isSuitableForWind`.
    fn suitable_for_wind(&self, min_radius: i32, min_bluntness: f32) -> bool {
        self.radius >= min_radius && self.bluntness >= f64::from(min_bluntness)
    }
}

/// `SpeleothemUtils.getSpeleothemHeight`.
fn speleothem_height(from_center: f64, radius: f64, scale: f64, bluntness: f64) -> f64 {
    let from_center = if from_center < bluntness {
        bluntness
    } else {
        from_center
    };
    let x = from_center / radius * 0.384;
    let a = 0.75 * x.powf(1.333_333_333_333_333_3);
    let b = x.powf(0.666_666_666_666_666_6);
    let c = 0.333_333_333_333_333_3 * x.ln();
    let h = scale * (a - b - c);
    // Math.max: NaN stays NaN.
    let h = if h.is_nan() { h } else { h.max(0.0) };
    h / 0.384 * radius
}
