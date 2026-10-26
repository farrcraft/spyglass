//! `Aquifer.NoiseBasedAquifer`: which fluid, if any, fills each open block, from fluid levels
//! chosen per aquifer cell and barriers between them. Ported from the 26.3 bytecode; see
//! `docs/specs/terrain.md`.

use std::collections::HashMap;

use spyglass_density::{Sampler, Volume};
use spyglass_noise::mth;
use spyglass_rng::{PositionalRandomFactory, RandomSource, XoroshiroPositionalRandomFactory};

/// `DimensionType.WAY_BELOW_MIN_Y`: a fluid level below any world, so no block is under it.
pub const WAY_BELOW_MIN_Y: i32 = -32512;

/// A block of the generator's column: the noise settings' default block, or what the aquifer
/// fills an open block with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Block {
    /// The noise settings' `default_block` (`minecraft:stone` in the Overworld): what the
    /// aquifer's `null` stands for.
    Default,
    /// `minecraft:air`.
    Air,
    /// `minecraft:water`.
    Water,
    /// `minecraft:lava`.
    Lava,
}

/// `Aquifer.FluidStatus`: a fluid up to a level, and air from there up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FluidStatus {
    /// The first y without fluid.
    pub level: i32,
    /// The fluid below the level.
    pub fluid: Block,
}

impl FluidStatus {
    /// `at(int)`: the fluid below the level, air at and above it.
    pub fn at(&self, y: i32) -> Block {
        if y < self.level {
            self.fluid
        } else {
            Block::Air
        }
    }
}

/// `NoiseBasedChunkGenerator.createFluidPicker`: lava below `min(−54, sea level)`, else the
/// settings' default fluid up to the sea level. It doesn't depend on x or z.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FluidPicker {
    /// The settings' `sea_level`.
    pub sea_level: i32,
    /// The settings' `default_fluid`.
    pub default_fluid: Block,
}

impl FluidPicker {
    /// `computeFluid(x, y, z)`.
    pub fn status(&self, y: i32) -> FluidStatus {
        if y < (-54).min(self.sea_level) {
            FluidStatus {
                level: -54,
                fluid: Block::Lava,
            }
        } else {
            FluidStatus {
                level: self.sea_level,
                fluid: self.default_fluid,
            }
        }
    }
}

/// The noise settings' `aquifers` functions, bound to a seed.
#[derive(Debug, Clone)]
pub struct AquiferSamplers {
    /// `barrier`: how hard barriers between aquifers are.
    pub barrier: Sampler,
    /// `fluid_level_floodedness`: whether a cell is full, partly full or empty.
    pub fluid_level_floodedness: Sampler,
    /// `fluid_level_spread`: how a partly full cell's level moves.
    pub fluid_level_spread: Sampler,
    /// `lava`: where low aquifers hold lava.
    pub lava: Sampler,
    /// `exclusion`: where cells are empty.
    pub exclusion: Sampler,
    /// `surface_level`: the terrain's rough surface, which bounds fluid levels.
    pub surface_level: Sampler,
}

/// `Aquifer.NoiseBasedAquifer.SURFACE_SAMPLING_OFFSETS_IN_CHUNKS`, in the game's order.
const SURFACE_OFFSETS: [(i32, i32); 13] = [
    (0, 0),
    (-2, -1),
    (-1, -1),
    (0, -1),
    (1, -1),
    (-3, 0),
    (-2, 0),
    (-1, 0),
    (1, 0),
    (-2, 1),
    (-1, 1),
    (0, 1),
    (1, 1),
];

/// `NoiseBasedAquifer` for one region, as a `NoiseChunk` builds it. It remembers each cell's
/// centre and fluid status, and surface levels, so asking about one block changes nothing
/// another answer depends on: the answers don't depend on the order of questions.
pub struct Aquifer<'s> {
    samplers: &'s AquiferSamplers,
    random: XoroshiroPositionalRandomFactory,
    picker: FluidPicker,
    min_grid: [i32; 3],
    grid_size: [i32; 3],
    /// `aquiferLocationCache`: each cell's centre.
    centres: Vec<Option<[i32; 3]>>,
    /// `aquiferCache`: each cell's fluid status.
    statuses: Vec<Option<FluidStatus>>,
    /// `surfaceLevelCache`, by quart-aligned `(x, z)`.
    surface_levels: HashMap<(i32, i32), i32>,
    skip_sampling_above_y: i32,
}

fn grid_x(x: i32) -> i32 {
    x >> 4
}

fn grid_y(y: i32) -> i32 {
    y.div_euclid(12)
}

fn grid_z(z: i32) -> i32 {
    z >> 4
}

fn from_grid_xz(g: i32, offset: i32) -> i32 {
    (g << 4) + offset
}

fn from_grid_y(g: i32, offset: i32) -> i32 {
    g * 12 + offset
}

/// Puts a centre at `distance` into the first slot whose distance is at least as far, pushing
/// the rest down; a centre beyond all three takes none. The game keeps a fourth slot, which only
/// decides when to schedule fluid updates.
fn take_slot(nearest: &mut [(i32, usize); 3], distance: i32, index: usize) {
    if let Some(slot) = nearest.iter().position(|&(d, _)| d >= distance) {
        nearest.copy_within(slot..2, slot + 1);
        nearest[slot] = (distance, index);
    }
}

/// `similarity(int, int)`: `1 − (b − a) / 25`.
fn similarity(a: i32, b: i32) -> f64 {
    1.0 - f64::from(b - a) / 25.0
}

impl<'s> Aquifer<'s> {
    /// The aquifer for the blocks of `region` (`NoiseBasedAquifer(...)`), with `random` the
    /// seed's `minecraft:aquifer` factory (`RandomState.getOrCreateRandomFactory`). Surface
    /// levels over the region are sampled up front as the game does, which also decides above
    /// which y the aquifer only consults the fluid picker.
    pub fn new(
        samplers: &'s AquiferSamplers,
        random: XoroshiroPositionalRandomFactory,
        region: &Volume,
        picker: FluidPicker,
    ) -> Self {
        let max = region.max_block();
        let min_grid = [
            grid_x(region.min[0] - 5),
            grid_y(region.min[1] + 1) - 1,
            grid_z(region.min[2] - 5),
        ];
        let max_grid = [
            grid_x(max[0] - 5) + 1,
            grid_y(max[1] + 1) + 1,
            grid_z(max[2] - 5) + 1,
        ];
        let grid_size: [i32; 3] = std::array::from_fn(|a| max_grid[a] - min_grid[a] + 1);
        let cells = grid_size.iter().map(|&s| s as usize).product();
        let mut aquifer = Aquifer {
            samplers,
            random,
            picker,
            min_grid,
            grid_size,
            centres: vec![None; cells],
            statuses: vec![None; cells],
            surface_levels: HashMap::new(),
            skip_sampling_above_y: 0,
        };
        let highest = aquifer.max_surface_level(
            from_grid_xz(min_grid[0], 0),
            from_grid_xz(min_grid[2], 0),
            from_grid_xz(max_grid[0], 9),
            from_grid_xz(max_grid[2], 9),
        );
        let top_cell = grid_y(highest + 8 + 12) + 1;
        aquifer.skip_sampling_above_y = from_grid_y(top_cell, 11) - 1;
        aquifer
    }

    /// `maxSurfaceLevel`: surface levels at every quart column of the box, sampled as one volume
    /// at y 0 and remembered, and the highest.
    fn max_surface_level(&mut self, x0: i32, z0: i32, x1: i32, z1: i32) -> i32 {
        let (qx0, qx1, qz0, qz1) = (x0 >> 2, x1 >> 2, z0 >> 2, z1 >> 2);
        let volume = Volume::new(
            [qx1 - qx0 + 1, 1, qz1 - qz0 + 1],
            [qx0 << 2, 0, qz0 << 2],
            [4, 1, 4],
        );
        let values = self.samplers.surface_level.sample_volume(&volume);
        let mut highest = i32::MIN;
        for iz in 0..volume.size[2] {
            for ix in 0..volume.size[0] {
                let level = mth::floor(f64::from(values[volume.index(ix, 0, iz)]));
                self.surface_levels
                    .insert((volume.block_x(ix), volume.block_z(iz)), level);
                highest = highest.max(level);
            }
        }
        highest
    }

    /// `surfaceLevel(int, int)`: at the quart column holding `(x, z)`, remembered, sampled at a
    /// point at y 0 if the region's volume didn't cover it.
    fn surface_level(&mut self, x: i32, z: i32) -> i32 {
        let (x, z) = ((x >> 2) << 2, (z >> 2) << 2);
        let surface_level = &self.samplers.surface_level;
        *self
            .surface_levels
            .entry((x, z))
            .or_insert_with(|| mth::floor(f64::from(surface_level.sample(x, 0, z))))
    }

    fn index(&self, [x, y, z]: [i32; 3]) -> usize {
        let [dx, dy, dz] = std::array::from_fn(|a| [x, y, z][a] - self.min_grid[a]);
        ((dy * self.grid_size[2] + dz) * self.grid_size[0] + dx) as usize
    }

    /// `computeSubstance(x, y, z, density)`: the default block (the game's `null`), or the fluid
    /// or air that fills the block.
    pub fn substance(&mut self, x: i32, y: i32, z: i32, density: f64) -> Block {
        if density > 0.0 {
            return Block::Default;
        }
        let global = self.picker.status(y);
        if y > self.skip_sampling_above_y {
            return global.at(y);
        }
        if global.at(y) == Block::Lava {
            return Block::Lava;
        }
        // The nearest three of the twelve candidate centres, x outermost, then y, then z. A
        // centre takes a slot at or beyond its distance, so on ties the later one wins.
        let (gx, gy, gz) = (grid_x(x - 5), grid_y(y + 1), grid_z(z - 5));
        let mut nearest = [(i32::MAX, 0usize); 3];
        for ox in 0..=1 {
            for oy in -1..=1 {
                for oz in 0..=1 {
                    let cell = [gx + ox, gy + oy, gz + oz];
                    let index = self.index(cell);
                    let [cx, cy, cz] = self.centre(index, cell);
                    let (dx, dy, dz) = (cx - x, cy - y, cz - z);
                    take_slot(&mut nearest, dx * dx + dy * dy + dz * dz, index);
                }
            }
        }
        let [(d1, i1), (d2, i2), (d3, i3)] = nearest;
        let s1 = self.status(i1);
        let sim12 = similarity(d1, d2);
        let state = s1.at(y);
        if sim12 <= 0.0 {
            return state;
        }
        if state == Block::Water && self.picker.status(y - 1).at(y - 1) == Block::Lava {
            return state;
        }
        // The barrier noise, sampled at most once, when a pressure first needs it.
        let mut barrier = f64::NAN;
        let s2 = self.status(i2);
        if density + sim12 * self.pressure(x, y, z, &mut barrier, s1, s2) > 0.0 {
            return Block::Default;
        }
        let s3 = self.status(i3);
        let sim13 = similarity(d1, d3);
        if sim13 > 0.0
            && density + sim12 * sim13 * self.pressure(x, y, z, &mut barrier, s1, s3) > 0.0
        {
            return Block::Default;
        }
        let sim23 = similarity(d2, d3);
        if sim23 > 0.0
            && density + sim12 * sim23 * self.pressure(x, y, z, &mut barrier, s2, s3) > 0.0
        {
            return Block::Default;
        }
        state
    }

    /// A cell's centre: `positionalRandomFactory.at(cell)`, then offsets of `nextInt(10)`,
    /// `nextInt(9)` and `nextInt(10)` along x, y and z.
    fn centre(&mut self, index: usize, [gx, gy, gz]: [i32; 3]) -> [i32; 3] {
        if let Some(c) = self.centres[index] {
            return c;
        }
        let mut r = self.random.at(gx, gy, gz);
        let ox = r.next_int_bounded(10);
        let oy = r.next_int_bounded(9);
        let oz = r.next_int_bounded(10);
        let c = [
            from_grid_xz(gx, ox),
            from_grid_y(gy, oy),
            from_grid_xz(gz, oz),
        ];
        self.centres[index] = Some(c);
        c
    }

    /// `getAquiferStatus`: a cell's fluid status, computed at its centre once.
    fn status(&mut self, index: usize) -> FluidStatus {
        if let Some(s) = self.statuses[index] {
            return s;
        }
        let [x, y, z] = self.centres[index].expect("a status's centre is found first");
        let s = self.compute_fluid(x, y, z);
        self.statuses[index] = Some(s);
        s
    }

    /// `calculatePressure`: how strongly two neighbouring statuses push a barrier into the
    /// block between them, from their blocks at y and their levels.
    fn pressure(
        &self,
        x: i32,
        y: i32,
        z: i32,
        barrier: &mut f64,
        a: FluidStatus,
        b: FluidStatus,
    ) -> f64 {
        let (block_a, block_b) = (a.at(y), b.at(y));
        if (block_a == Block::Lava && block_b == Block::Water)
            || (block_a == Block::Water && block_b == Block::Lava)
        {
            return 2.0;
        }
        let difference = (a.level - b.level).abs();
        if difference == 0 {
            return 0.0;
        }
        let average = 0.5 * f64::from(a.level + b.level);
        let offset = f64::from(y) + 0.5 - average;
        let half = f64::from(difference) / 2.0;
        let o = half - offset.abs();
        let r = if offset > 0.0 {
            let q = 0.0 + o;
            if q > 0.0 { q / 1.5 } else { q / 2.5 }
        } else {
            let q = 3.0 + o;
            if q > 0.0 { q / 3.0 } else { q / 10.0 }
        };
        let s = if r < -2.0 || r > 2.0 {
            0.0
        } else {
            if barrier.is_nan() {
                *barrier = f64::from(self.samplers.barrier.sample(x, y, z));
            }
            *barrier
        };
        2.0 * (s + r)
    }

    /// `computeFluid`: a centre's fluid status, from the surface levels around it.
    fn compute_fluid(&mut self, x: i32, y: i32, z: i32) -> FluidStatus {
        let global = self.picker.status(y);
        let mut lowest_surface = i32::MAX;
        let (top, bottom) = (y + 12, y - 12);
        let mut flooded = false;
        for (ox, oz) in SURFACE_OFFSETS {
            let (sx, sz) = (x + (ox << 4), z + (oz << 4));
            let raw = self.surface_level(sx, sz);
            let adjusted = raw + 8;
            let centre = ox == 0 && oz == 0;
            if centre && bottom > adjusted {
                return global;
            }
            let reaches_surface = top > adjusted;
            if reaches_surface || centre {
                let surface = self.picker.status(adjusted);
                if surface.at(adjusted) != Block::Air {
                    if centre {
                        flooded = true;
                    }
                    if reaches_surface {
                        return surface;
                    }
                }
            }
            lowest_surface = lowest_surface.min(raw);
        }
        let level = self.compute_surface_level(x, y, z, global, lowest_surface, flooded);
        FluidStatus {
            level,
            fluid: self.compute_fluid_type(x, y, z, global, level),
        }
    }

    /// `computeSurfaceLevel`.
    fn compute_surface_level(
        &self,
        x: i32,
        y: i32,
        z: i32,
        global: FluidStatus,
        lowest_surface: i32,
        flooded: bool,
    ) -> i32 {
        let (partial, full) = if f64::from(self.samplers.exclusion.sample(x, y, z)) > 0.0 {
            (-1.0, -1.0)
        } else {
            let distance = (lowest_surface + 8) - y;
            let f = if flooded {
                clamped_map(f64::from(distance), 0.0, 64.0, 1.0, 0.0)
            } else {
                0.0
            };
            let floodedness = f64::from(self.samplers.fluid_level_floodedness.sample(x, y, z));
            let n = clamp(floodedness, -1.0, 1.0);
            let full_threshold = map(f, 1.0, 0.0, -0.3, 0.8);
            let partial_threshold = map(f, 1.0, 0.0, -0.8, 0.4);
            (n - partial_threshold, n - full_threshold)
        };
        if full > 0.0 {
            global.level
        } else if partial > 0.0 {
            self.randomized_level(x, y, z, lowest_surface)
        } else {
            WAY_BELOW_MIN_Y
        }
    }

    /// `computeRandomizedFluidSurfaceLevel`: a level in the 40-block band holding y, moved by
    /// the spread noise, sampled at the grid position, in steps of 3.
    fn randomized_level(&self, x: i32, y: i32, z: i32, lowest_surface: i32) -> i32 {
        let (gx, gy, gz) = (x.div_euclid(16), y.div_euclid(40), z.div_euclid(16));
        let middle = gy * 40 + 20;
        let spread = f64::from(self.samplers.fluid_level_spread.sample(gx, gy, gz) * 10.0);
        // Mth.quantize(spread, 3).
        let shift = mth::floor(spread / 3.0) * 3;
        lowest_surface.min(middle + shift)
    }

    /// `computeFluidType`: lava instead of the global fluid in low aquifers where the lava
    /// noise, sampled at the grid position, is strong.
    fn compute_fluid_type(&self, x: i32, y: i32, z: i32, global: FluidStatus, level: i32) -> Block {
        if level <= -10 && level != WAY_BELOW_MIN_Y && global.fluid != Block::Lava {
            let (gx, gy, gz) = (x.div_euclid(64), y.div_euclid(40), z.div_euclid(64));
            let lava = f64::from(self.samplers.lava.sample(gx, gy, gz));
            if lava.abs() > 0.3 {
                return Block::Lava;
            }
        }
        global.fluid
    }
}

/// `Mth.clamp(double, double, double)`: `min` if below it, else `Math.min(value, max)`.
fn clamp(value: f64, min: f64, max: f64) -> f64 {
    if value < min {
        min
    } else {
        java_min(value, max)
    }
}

/// `Math.min(double, double)`: NaN if either is, and −0 below +0.
fn java_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == 0.0 && b == 0.0 {
        if a.is_sign_negative() { a } else { b }
    } else if a < b {
        a
    } else {
        b
    }
}

/// `Mth.inverseLerp(double, ...)`, `Mth.lerp(double, ...)` and `Mth.map`.
fn map(v: f64, a: f64, b: f64, c: f64, d: f64) -> f64 {
    let t = (v - a) / (b - a);
    c + t * (d - c)
}

/// `Mth.clampedMap`: as [`map`], with `t` clamped to `[0, 1]` (`clampedLerp`).
fn clamped_map(v: f64, a: f64, b: f64, c: f64, d: f64) -> f64 {
    let t = (v - a) / (b - a);
    if t < 0.0 {
        c
    } else if t > 1.0 {
        d
    } else {
        c + t * (d - c)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde_json::{Value, json};
    use spyglass_density::{Binder, Loader};
    use spyglass_rng::{RandomSource, XoroshiroRandomSource};

    use super::*;

    /// Aquifer functions from inline JSON, with one noise `minecraft:n` to use, bound for seed 1.
    fn samplers(functions: [Value; 6]) -> AquiferSamplers {
        let mut map: HashMap<String, Value> = HashMap::new();
        map.insert(
            "worldgen/noise/minecraft:n".into(),
            json!({"base_octave": -5, "octave_count": 3}),
        );
        for (i, f) in functions.into_iter().enumerate() {
            map.insert(format!("worldgen/density_function/minecraft:f{i}"), f);
        }
        let mut loader = Loader::new(&map);
        let functions: Vec<_> = (0..6)
            .map(|i| loader.function(&format!("minecraft:f{i}")).unwrap())
            .collect();
        let mut binder = Binder::new(loader.noises(), 1);
        let [a, b, c, d, e, f] = [0, 1, 2, 3, 4, 5].map(|i| binder.bind(&functions[i]).unwrap());
        AquiferSamplers {
            barrier: a,
            fluid_level_floodedness: b,
            fluid_level_spread: c,
            lava: d,
            exclusion: e,
            surface_level: f,
        }
    }

    fn noise(xz: f64, y: f64) -> Value {
        json!({"type": "noise", "noise": "n", "xz_scale": xz, "y_scale": y})
    }

    /// Functions that vary, like the Overworld's: noisy floodedness, spread and lava, no
    /// exclusion, and a surface level that wanders around 60.
    fn varied() -> AquiferSamplers {
        samplers([
            noise(1.0, 0.5),
            noise(1.0, 0.67),
            noise(1.0, 0.7),
            noise(1.0, 1.0),
            json!(-1),
            json!({"type": "add", "left": 60, "right": {"type": "mul", "left": 30,
                "right": noise(0.25, 0.0)}}),
        ])
    }

    fn random() -> XoroshiroPositionalRandomFactory {
        XoroshiroRandomSource::new(1).fork_positional()
    }

    const PICKER: FluidPicker = FluidPicker {
        sea_level: 63,
        default_fluid: Block::Water,
    };

    fn water(level: i32) -> FluidStatus {
        FluidStatus {
            level,
            fluid: Block::Water,
        }
    }

    #[test]
    fn the_fluid_picker_puts_lava_at_the_bottom() {
        assert_eq!(PICKER.status(-55).at(-55), Block::Lava);
        assert_eq!(PICKER.status(-55).level, -54);
        assert_eq!(PICKER.status(-54).at(-54), Block::Water);
        assert_eq!(PICKER.status(62).at(62), Block::Water);
        assert_eq!(PICKER.status(63).at(63), Block::Air);
        // A sea below −54 moves the lava down with it.
        let low = FluidPicker {
            sea_level: -60,
            default_fluid: Block::Water,
        };
        assert_eq!(low.status(-60), water(-60));
        assert_eq!(low.status(-61).fluid, Block::Lava);
    }

    #[test]
    fn ties_go_to_the_later_centre() {
        let mut nearest = [(i32::MAX, 0); 3];
        for (distance, index) in [(50, 1), (20, 2), (50, 3), (20, 4), (90, 5), (10, 6)] {
            take_slot(&mut nearest, distance, index);
        }
        // 10 first; of the two 20s the later is nearer; the 50s and 90 are pushed out.
        assert_eq!(nearest, [(10, 6), (20, 4), (20, 2)]);
    }

    #[test]
    fn pressure_cases() {
        let s = samplers([
            json!(0.25),
            json!(0),
            json!(0),
            json!(0),
            json!(0),
            json!(60),
        ]);
        let region = Volume::blocks([1, 16, 1], [0, 0, 0]);
        let aquifer = Aquifer::new(&s, random(), &region, PICKER);
        let lava = FluidStatus {
            level: 10,
            fluid: Block::Lava,
        };
        let mut barrier = f64::NAN;
        // Lava and water meeting at y: the strongest barrier, either way round.
        assert_eq!(
            aquifer.pressure(0, 5, 0, &mut barrier, lava, water(20)),
            2.0
        );
        assert_eq!(
            aquifer.pressure(0, 5, 0, &mut barrier, water(20), lava),
            2.0
        );
        // Not at y 15, where the lava's status is air.
        assert_ne!(
            aquifer.pressure(0, 15, 0, &mut barrier, lava, water(20)),
            2.0
        );
        barrier = f64::NAN;
        // Equal levels: none.
        assert_eq!(
            aquifer.pressure(0, 5, 0, &mut barrier, water(20), water(20)),
            0.0
        );
        // Far above both levels: r = (5 − 25.5) / 2.5 is outside ±2, so no barrier term, and
        // the barrier noise isn't sampled.
        let r = (5.0 - 25.5) / 2.5;
        assert_eq!(
            aquifer.pressure(0, 40, 0, &mut barrier, water(10), water(20)),
            2.0 * r
        );
        assert!(barrier.is_nan());
        // Between them: r = (3 + (5 − 2.5)) / 3, with the barrier noise added.
        let r = (3.0 + (5.0 - 2.5)) / 3.0;
        let got = aquifer.pressure(0, 12, 0, &mut barrier, water(10), water(20));
        assert_eq!(got, 2.0 * (0.25 + r));
        assert_eq!(barrier, 0.25);
    }

    #[test]
    fn skipping_gives_the_full_path_answer() {
        let s = varied();
        for (x, z) in [(0, 0), (37, -1003), (-400, 77)] {
            let region = Volume::blocks([1, 384, 1], [x, -64, z]);
            let mut skipping = Aquifer::new(&s, random(), &region, PICKER);
            let mut full = Aquifer::new(&s, random(), &region, PICKER);
            assert!(skipping.skip_sampling_above_y < 319);
            full.skip_sampling_above_y = i32::MAX;
            for y in (-64..320).rev() {
                for density in [-1.0, -0.05, 0.0] {
                    assert_eq!(
                        skipping.substance(x, y, z, density),
                        full.substance(x, y, z, density),
                        "({x}, {y}, {z}) at density {density}"
                    );
                }
            }
        }
    }

    #[test]
    fn answers_dont_depend_on_the_order_of_questions() {
        let s = varied();
        let region = Volume::blocks([1, 384, 1], [21, -64, -5]);
        let mut down = Aquifer::new(&s, random(), &region, PICKER);
        let mut up = Aquifer::new(&s, random(), &region, PICKER);
        let answers: Vec<_> = (-64..320)
            .rev()
            .map(|y| down.substance(21, y, -5, -0.02))
            .collect();
        let mut upwards: Vec<_> = (-64..320).map(|y| up.substance(21, y, -5, -0.02)).collect();
        upwards.reverse();
        assert_eq!(answers, upwards);
        // The column isn't trivial: air, water and at least one barrier of stone.
        for kind in [Block::Air, Block::Water, Block::Default] {
            assert!(answers.contains(&kind), "no {kind:?} in {answers:?}");
        }
        // A solid block is the default block, whatever the aquifer would hold there.
        assert_eq!(down.substance(21, 0, -5, 0.01), Block::Default);
    }
}
