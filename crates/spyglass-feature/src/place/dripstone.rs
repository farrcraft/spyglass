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
    /// What the ceiling and floor may be, with the base block and lava.
    replaceable: Tag,
    /// What stops a column once it's placed a block (`base_stone_overworld`).
    base_stone: Tag,
    /// How far up and down the origin looks for the ceiling and floor.
    floor_to_ceiling_search_range: i32,
    /// Each column's radius, before the cave's height limits it.
    column_radius: IntProvider,
    /// How tall a column is for its radius: larger is taller.
    height_scale: FloatProvider,
    /// The largest radius as a share of the cave's height.
    max_column_radius_to_cave_height_ratio: f32,
    /// How flat the stalactite's tip is: larger is blunter.
    stalactite_bluntness: FloatProvider,
    /// How flat the stalagmite's tip is: larger is blunter.
    stalagmite_bluntness: FloatProvider,
    /// How far the wind leans the pair per block from the origin's y.
    wind_speed: FloatProvider,
    /// The least radius, of both, for the wind to blow.
    min_radius_for_wind: i32,
    /// The least bluntness, of both, for the wind to blow.
    min_bluntness_for_wind: f32,
    /// Which `Mth.sin` and `Mth.cos` the version uses.
    trig: Trig,
}

/// One of the pair (`LargeDripstoneFeature.LargeDripstone`).
struct Speleothem {
    /// Where its base is, on the ceiling or floor.
    root: [i32; 3],
    /// A stalagmite, growing up from the floor; else a stalactite, hanging down.
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
    /// The furthest it pushes a block, on each of x and z.
    max_offset: i32,
}

impl Wind {
    /// `WindOffsetter.offset`.
    fn offset(&self, pos @ [x, y, z]: [i32; 3]) -> [i32; 3] {
        let Some((speed_x, speed_z)) = self.speed else {
            return pos;
        };
        let below_origin = f64::from(self.origin_y.wrapping_sub(y));
        let clamp = |v: f64| floor(v).max(-self.max_offset).min(self.max_offset);
        [
            x.wrapping_add(clamp(speed_x * below_origin)),
            y,
            z.wrapping_add(clamp(speed_z * below_origin)),
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
                let state = blocks.parse_state(json, &at.child("base_block"))?;
                blocks.palette().block_id(state).to_string()
            }
            None => "minecraft:dripstone_block".to_string(),
        };
        let base = blocks.state(&base_id)?;
        let replaceable = {
            let tag = field(o, "replaceable_blocks", at)?
                .as_str()
                .and_then(|s| s.strip_prefix('#'))
                .ok_or_else(|| {
                    at.child("replaceable_blocks")
                        .error(LoadErrorKind::Shape("a #tag"))
                })?;
            blocks.tag(resources, &full_id(tag))?
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
    ///
    /// From an origin in an open cave, it finds the ceiling above and the floor below, and grows
    /// a stalactite down from one and a stalagmite up from the other, both of one radius (a
    /// share of the cave's height at most). Each is a cone of columns, tallest at its centre (see
    /// `speleothem_height`). Where both are wide and blunt enough, a wind leans both the same
    /// way, more the further a block is from the origin's y.
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
        let state = region.get(pos);
        let blocks = region.blocks();
        let id = blocks.palette().block_id(state);
        // SpeleothemUtils.isBaseOrLava.
        (id == self.base_id || blocks.is(state, self.replaceable) || id == "minecraft:lava")
            .then_some(pos[1])
    }

    /// `SpeleothemUtils.isEmptyOrWater`.
    fn empty_or_water(&self, region: &mut Region<'_>, pos: [i32; 3]) -> bool {
        let state = region.get(pos);
        let blocks = region.blocks();
        blocks.is_air(state) || blocks.palette().block_id(state) == "minecraft:water"
    }

    /// `SpeleothemUtils.isEmptyOrWaterOrLava`.
    fn empty_or_fluid(&self, region: &mut Region<'_>, pos: [i32; 3]) -> bool {
        let state = region.get(pos);
        let blocks = region.blocks();
        blocks.is_air(state) || {
            let id = blocks.palette().block_id(state);
            id == "minecraft:water" || id == "minecraft:lava"
        }
    }

    /// `SpeleothemUtils.isCircleMostlyEmbeddedInStone`: the block and points around a circle of
    /// `radius` all solid, so a base there is held in stone.
    fn embedded(&self, region: &mut Region<'_>, [x, y, z]: [i32; 3], radius: i32) -> bool {
        if self.empty_or_fluid(region, [x, y, z]) {
            return false;
        }
        // Points about 6 blocks apart around the circle (the step is in radians), so about one
        // per block of radius: a sparse test, hence "mostly".
        let step = 6.0_f32 / radius as f32;
        let mut angle = 0.0_f32;
        while angle < 6.283_185_5_f32 {
            let dx = (self.trig.cos(f64::from(angle)) * radius as f32) as i32;
            let dz = (self.trig.sin(f64::from(angle)) * radius as f32) as i32;
            if self.empty_or_fluid(region, [x.wrapping_add(dx), y, z.wrapping_add(dz)]) {
                return false;
            }
            angle += step;
        }
        true
    }

    /// `moveBackUntilBaseIsInsideStoneAndShrinkRadiusIfNecessary`: whether it found a root.
    ///
    /// The root steps back into the ceiling or floor, up to 10 blocks, until its circle is in
    /// stone; failing that, the radius halves and it tries again from where it started.
    fn move_back(&self, region: &mut Region<'_>, speleothem: &mut Speleothem, wind: &Wind) -> bool {
        while speleothem.radius > 1 {
            let mut pos = speleothem.root;
            let tries = speleothem.height().min(10);
            for _ in 0..tries {
                let state = region.get(pos);
                if region.blocks().palette().block_id(state) == "minecraft:lava" {
                    return false;
                }
                if self.embedded(region, wind.offset(pos), speleothem.radius) {
                    speleothem.root = pos;
                    return true;
                }
                pos[1] = pos[1].wrapping_add(if speleothem.pointing_up { -1 } else { 1 });
            }
            speleothem.radius /= 2;
        }
        false
    }

    /// `LargeDripstone.placeBlocks`: a column at each block within the radius, as tall as
    /// [`Speleothem::height_at`] its distance from the centre, a fifth of them cut a little
    /// short. A column fills only air and fluid, and stops at base stone once it's started.
    fn place_blocks(
        &self,
        region: &mut Region<'_>,
        random: &mut WorldgenRandom,
        speleothem: &Speleothem,
        wind: &Wind,
    ) {
        let radius = speleothem.radius;
        let step = if speleothem.pointing_up { 1 } else { -1 };
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                // Mth.sqrt(float).
                let from_centre = f64::from((dx * dx + dz * dz) as f32).sqrt() as f32;
                if from_centre > radius as f32 {
                    continue;
                }
                let mut height = speleothem.height_at(from_centre);
                if height <= 0 {
                    continue;
                }
                if f64::from(random.next_float()) < 0.2 {
                    // Mth.randomBetween(random, 0.8, 1).
                    height = (height as f32 * (random.next_float() * (1.0_f32 - 0.8_f32) + 0.8_f32))
                        as i32;
                }
                let mut pos = [
                    speleothem.root[0].wrapping_add(dx),
                    speleothem.root[1],
                    speleothem.root[2].wrapping_add(dz),
                ];
                let mut placed = false;
                let max_y = if speleothem.pointing_up {
                    region.height(Heightmap::WorldSurfaceWg, pos[0], pos[2])
                } else {
                    i32::MAX
                };
                for _ in 0..height {
                    if pos[1] >= max_y {
                        break;
                    }
                    let blown = wind.offset(pos);
                    if self.empty_or_fluid(region, blown) {
                        placed = true;
                        region.set(blown, self.base);
                    } else if placed {
                        let state = region.get(blown);
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

    /// `getHeightAtRadius`: the column's height at `from_centre` blocks from the centre.
    fn height_at(&self, from_centre: f32) -> i32 {
        speleothem_height(
            f64::from(from_centre),
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

/// `SpeleothemUtils.getSpeleothemHeight`: a column's height by its distance from the centre.
///
/// The curve, in `x = distance / radius · 0.384`, is `¾x^(4/3) − x^(2/3) − ⅓ ln x`: large near
/// the centre, where `ln x` goes to minus infinity, and falling to 0 at the radius, so the
/// columns make a point. Distances within `bluntness` are taken as `bluntness`, which cuts the
/// point off flat.
#[expect(
    clippy::disallowed_methods,
    reason = "Math.pow and Math.log, unchecked in the last bit (features.md)"
)]
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
