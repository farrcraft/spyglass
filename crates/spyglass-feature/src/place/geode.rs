//! `GeodeFeature`: a shell of layers around a few points, with a crack and buds
//! (`docs/specs/features.md#geode`). The same algorithm in every supported version; the noise
//! differs (switch `geode_noise_v1_21`).

use serde_json::{Map, Value};
use spyglass_blocks::State;
use spyglass_data::codec::{At, LoadError, LoadErrorKind, double, field, full_id, int, object};
use spyglass_data::providers::IntProvider;
use spyglass_density::Resources;
use spyglass_noise::{NoiseStack, NormalNoise, v1_21};
use spyglass_rng::{LegacyRandomSource, RandomSource, WorldgenRandom};

use crate::blocks::{Blocks, Tag};
use crate::region::Region;

/// `Direction.values()`: down, up, north, south, west, east.
const DIRECTIONS: [([i32; 3], &str); 6] = [
    ([0, -1, 0], "down"),
    ([0, 1, 0], "up"),
    ([0, 0, -1], "north"),
    ([0, 0, 1], "south"),
    ([-1, 0, 0], "west"),
    ([1, 0, 0], "east"),
];

/// A geode's configuration, with the codec's defaults.
#[derive(Debug, Clone)]
pub struct Geode {
    filling: State,
    inner: State,
    alternate_inner: State,
    middle: State,
    outer: State,
    /// Each inner placement's states, by direction (in [`DIRECTIONS`]' order), then
    /// waterlogged (false, true).
    placements: Vec<[[State; 2]; 6]>,
    cannot_replace: Tag,
    invalid: Tag,
    layers: [f64; 4],
    crack_chance: f64,
    crack_size: f64,
    crack_offset: i32,
    potential_chance: f64,
    alternate_chance: f64,
    require_alternate: bool,
    outer_wall: IntProvider,
    points: IntProvider,
    point_offset: IntProvider,
    gen_offsets: [i32; 2],
    noise_multiplier: f64,
    invalid_threshold: i32,
    /// The noise: 26.2's, or the parity noise from 26.3.
    noise: GeodeNoise,
}

#[derive(Debug, Clone)]
enum GeodeNoise {
    V1_21,
    Parity(NormalNoise),
}

/// The noise for one seed.
enum Seeded {
    V1_21(v1_21::NormalNoise),
    Parity(NoiseStack),
}

impl Seeded {
    fn get(&self, x: i32, y: i32, z: i32) -> f64 {
        let (x, y, z) = (f64::from(x), f64::from(y), f64::from(z));
        match self {
            Seeded::V1_21(n) => n.get_value(x, y, z),
            Seeded::Parity(n) => f64::from(n.get(x, y, z)),
        }
    }
}

impl Geode {
    /// From the configured feature's fields.
    pub fn load(
        o: &Map<String, Value>,
        at: &At<'_>,
        resources: &dyn Resources,
        blocks: &mut Blocks,
    ) -> Result<Geode, LoadError> {
        let b_at = at.child("blocks");
        let b = object(field(o, "blocks", at)?, &b_at)?;
        let provider = |key: &'static str, blocks: &mut Blocks| {
            blocks.parse_simple_provider(field(b, key, &b_at)?, &b_at.child(key))
        };
        let filling = provider("filling_provider", blocks)?;
        let inner = provider("inner_layer_provider", blocks)?;
        let alternate_inner = provider("alternate_inner_layer_provider", blocks)?;
        let middle = provider("middle_layer_provider", blocks)?;
        let outer = provider("outer_layer_provider", blocks)?;
        let p_at = b_at.child("inner_placements");
        let Value::Array(items) = field(b, "inner_placements", &b_at)? else {
            return Err(p_at.error(LoadErrorKind::Shape("a list of block states")));
        };
        if items.is_empty() {
            return Err(p_at.error(LoadErrorKind::Invalid("no inner placements")));
        }
        let mut placements = Vec::new();
        for (i, item) in items.iter().enumerate() {
            let at = p_at.child(&i.to_string());
            let s = blocks.parse_state(item, &at)?;
            let id = blocks.palette().block_id(s).to_string();
            let mut by_direction = [[0; 2]; 6];
            for (d, (_, facing)) in DIRECTIONS.iter().enumerate() {
                for (w, waterlogged) in ["false", "true"].iter().enumerate() {
                    let mut props = Map::new();
                    props.insert("facing".into(), Value::from(*facing));
                    props.insert("waterlogged".into(), Value::from(*waterlogged));
                    by_direction[d][w] = blocks
                        .parse_state(&serde_json::json!({"Name": id, "Properties": props}), &at)?;
                }
            }
            placements.push(by_direction);
        }
        let tag = |key: &'static str, blocks: &mut Blocks| -> Result<Tag, LoadError> {
            let s = field(b, key, &b_at)?
                .as_str()
                .and_then(|s| s.strip_prefix('#'))
                .ok_or_else(|| b_at.child(key).error(LoadErrorKind::Shape("a #tag")))?;
            blocks.tag(resources, &full_id(s))
        };
        let cannot_replace = tag("cannot_replace", blocks)?;
        let invalid = tag("invalid_blocks", blocks)?;
        let section = |key: &str| -> Result<Map<String, Value>, LoadError> {
            match o.get(key) {
                None => Ok(Map::new()),
                Some(v) => Ok(object(v, &at.child(key))?.clone()),
            }
        };
        let num = |m: &Map<String, Value>, key: &str, default: f64, at: &At<'_>| match m.get(key) {
            None => Ok(default),
            Some(v) => double(v, &at.child(key)),
        };
        let l = section("layers")?;
        let l_at = at.child("layers");
        let layers = [
            num(&l, "filling", 1.7, &l_at)?,
            num(&l, "inner_layer", 2.2, &l_at)?,
            num(&l, "middle_layer", 3.2, &l_at)?,
            num(&l, "outer_layer", 4.2, &l_at)?,
        ];
        let c = section("crack")?;
        let c_at = at.child("crack");
        let crack_offset = match c.get("crack_point_offset") {
            None => 2,
            Some(v) => int(v, &c_at.child("crack_point_offset"))?,
        };
        let int_provider = |key: &str, default: IntProvider| match o.get(key) {
            None => Ok(default),
            Some(v) => IntProvider::parse(v, &at.child(key)),
        };
        let plain_int = |key: &str, default: i32| match o.get(key) {
            None => Ok(default),
            Some(v) => int(v, &at.child(key)),
        };
        let require_alternate = match o.get("placements_require_layer0_alternate") {
            None => true,
            Some(v) => v.as_bool().ok_or_else(|| {
                at.child("placements_require_layer0_alternate")
                    .error(LoadErrorKind::Shape("a boolean"))
            })?,
        };
        let invalid_threshold = int(
            field(o, "invalid_blocks_threshold", at)?,
            &at.child("invalid_blocks_threshold"),
        )?;
        let noise = if resources.behavior().geode_noise_v1_21 {
            GeodeNoise::V1_21
        } else {
            GeodeNoise::Parity(
                NormalNoise::create_parity(-4, &[1.0])
                    .map_err(|e| at.error(LoadErrorKind::Invalid(e.reason())))?,
            )
        };
        Ok(Geode {
            filling,
            inner,
            alternate_inner,
            middle,
            outer,
            placements,
            cannot_replace,
            invalid,
            layers,
            crack_chance: num(&c, "generate_crack_chance", 1.0, &c_at)?,
            crack_size: num(&c, "base_crack_size", 2.0, &c_at)?,
            crack_offset,
            potential_chance: num(o, "use_potential_placements_chance", 0.35, at)?,
            alternate_chance: num(o, "use_alternate_layer0_chance", 0.0, at)?,
            require_alternate,
            outer_wall: int_provider("outer_wall_distance", IntProvider::Uniform(4, 5))?,
            points: int_provider("distribution_points", IntProvider::Uniform(3, 4))?,
            point_offset: int_provider("point_offset", IntProvider::Uniform(1, 2))?,
            gen_offsets: [
                plain_int("min_gen_offset", -16)?,
                plain_int("max_gen_offset", 16)?,
            ],
            noise_multiplier: num(o, "noise_multiplier", 0.05, at)?,
            invalid_threshold,
            noise,
        })
    }

    /// `safeSetBlock`: writes unless the block there is in `cannot_replace`.
    fn safe_set(&self, region: &mut Region<'_>, pos: [i32; 3], s: State) {
        let here = region.get(pos);
        if !region.blocks().is(here, self.cannot_replace) {
            region.set(pos, s);
        }
    }

    /// `GeodeFeature.place`: whether it placed.
    pub fn place(
        &self,
        region: &mut Region<'_>,
        random: &mut WorldgenRandom,
        origin: [i32; 3],
        seed: i64,
    ) -> bool {
        let blocks = region.blocks();
        let n = self.points.sample(random);
        let noise = match &self.noise {
            GeodeNoise::V1_21 => Seeded::V1_21(v1_21::NormalNoise::create(
                &mut LegacyRandomSource::new(seed),
                &v1_21::NoiseParameters::new(-4, vec![1.0]),
            )),
            GeodeNoise::Parity(p) => Seeded::Parity(p.create(&mut LegacyRandomSource::new(seed))),
        };
        let d9 = f64::from(n) / f64::from(self.outer_wall.max());
        let filling_d = 1.0 / self.layers[0].sqrt();
        let inner_d = 1.0 / (self.layers[1] + d9).sqrt();
        let middle_d = 1.0 / (self.layers[2] + d9).sqrt();
        let outer_d = 1.0 / (self.layers[3] + d9).sqrt();
        let crack_size = 1.0
            / (self.crack_size + random.next_double() / 2.0 + if n > 3 { d9 } else { 0.0 }).sqrt();
        let crack = f64::from(random.next_float()) < self.crack_chance;
        let mut invalid = 0;
        let mut points: Vec<([i32; 3], i32)> = Vec::new();
        for _ in 0..n {
            let dx = self.outer_wall.sample(random);
            let dy = self.outer_wall.sample(random);
            let dz = self.outer_wall.sample(random);
            let p = add(origin, [dx, dy, dz]);
            let s = region.get(p);
            if blocks.is_air(s) || blocks.is(s, self.invalid) {
                invalid += 1;
                if invalid > self.invalid_threshold {
                    return false;
                }
            }
            points.push((p, self.point_offset.sample(random)));
        }
        let mut cracks: Vec<[i32; 3]> = Vec::new();
        if crack {
            let r = random.next_int_bounded(4);
            let len = n * 2 + 1;
            let (cx, cz) = match r {
                0 => (len, 0),
                1 => (0, len),
                2 => (len, len),
                _ => (0, 0),
            };
            for y in [7, 5, 1] {
                cracks.push(add(origin, [cx, y, cz]));
            }
        }
        let mut potential: Vec<[i32; 3]> = Vec::new();
        let [lo, hi] = self.gen_offsets;
        // BlockPos.betweenClosed: x fastest, then y, then z.
        for z in lo..=hi {
            for y in lo..=hi {
                for x in lo..=hi {
                    let pos = add(origin, [x, y, z]);
                    let nz = noise.get(pos[0], pos[1], pos[2]) * self.noise_multiplier;
                    let mut s = 0.0;
                    for &(p, offset) in &points {
                        s += inv_sqrt(dist_sqr(pos, p) + f64::from(offset)) + nz;
                    }
                    if s < outer_d {
                        continue;
                    }
                    if s >= filling_d {
                        self.safe_set(region, pos, self.filling);
                        continue;
                    }
                    let mut t = 0.0;
                    for &c in &cracks {
                        t += inv_sqrt(dist_sqr(pos, c) + f64::from(self.crack_offset)) + nz;
                    }
                    if crack && t >= crack_size {
                        self.safe_set(region, pos, spyglass_blocks::state::AIR);
                        // The fluid ticks it schedules beside the crack change no blocks.
                    } else if s >= inner_d {
                        let alternate = f64::from(random.next_float()) < self.alternate_chance;
                        let block = if alternate {
                            self.alternate_inner
                        } else {
                            self.inner
                        };
                        self.safe_set(region, pos, block);
                        if (!self.require_alternate || alternate)
                            && f64::from(random.next_float()) < self.potential_chance
                        {
                            potential.push(pos);
                        }
                    } else if s >= middle_d {
                        self.safe_set(region, pos, self.middle);
                    } else if s >= outer_d {
                        self.safe_set(region, pos, self.outer);
                    }
                }
            }
        }
        for p in potential {
            let placement =
                &self.placements[random.next_int_bounded(self.placements.len() as i32) as usize];
            for (d, (offset, _)) in DIRECTIONS.iter().enumerate() {
                let q = add(p, *offset);
                let ns = region.get(q);
                let source = blocks.behaviour(ns).fluid.is_some_and(|f| f.source);
                let state = placement[d][usize::from(source)];
                // BuddingAmethystBlock.canClusterGrowAtState: air, or a full water block.
                let grows = blocks.is_air(ns)
                    || (blocks.palette().block_id(ns) == "minecraft:water"
                        && blocks.behaviour(ns).fluid.is_some_and(|f| f.amount == 8));
                if grows {
                    self.safe_set(region, q, state);
                    break;
                }
            }
        }
        true
    }
}

fn add(a: [i32; 3], b: [i32; 3]) -> [i32; 3] {
    [
        a[0].wrapping_add(b[0]),
        a[1].wrapping_add(b[1]),
        a[2].wrapping_add(b[2]),
    ]
}

/// `Vec3i.distSqr`: in `double`, `(dx·dx + dy·dy) + dz·dz`.
fn dist_sqr(a: [i32; 3], b: [i32; 3]) -> f64 {
    let dx = f64::from(a[0]) - f64::from(b[0]);
    let dy = f64::from(a[1]) - f64::from(b[1]);
    let dz = f64::from(a[2]) - f64::from(b[2]);
    dx * dx + dy * dy + dz * dz
}

/// `Mth.invSqrt`: joml's `1.0 / Math.sqrt(x)`.
fn inv_sqrt(x: f64) -> f64 {
    1.0 / x.sqrt()
}
