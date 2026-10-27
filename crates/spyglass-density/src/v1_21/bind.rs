//! 26.2's density functions bound to a seed and evaluated at a point, all in `f64`
//! (`DensityFunction.compute`). See `docs/specs/density-functions.md#262-evaluation`.
//!
//! The game builds each function once (`TwoArgumentSimpleFunction.create`, `Mapped.create`),
//! choosing a constant-operand form and computing `minValue()`/`maxValue()` from the operands,
//! and rebuilds it each time a visitor maps it (`mapAll`). Three visitors matter, and they see
//! different graphs, so binding takes the [`Wiring`] it follows.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use spyglass_data::codec::{LoadError, LoadErrorKind};
use spyglass_noise::NormalNoiseParameters;
use spyglass_noise::mth;
use spyglass_noise::v1_21::{BlendedNoise, NormalNoise, SimplexNoise};
use spyglass_rng::{
    LegacyPositionalRandomFactory, LegacyRandomSource, PositionalRandomFactory, RandomSource,
    XoroshiroPositionalRandomFactory, XoroshiroRandomSource,
};

use super::chunk::NoiseChunk;
use super::function::{DensityFunction, Df, Mapped, Marker, Spline, TwoArgument};
use crate::java;
use crate::sampler::sample_multipoint;

/// The visitor a graph was mapped with, which decides what's a constant operand, which markers
/// remain and what the ranges are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Wiring {
    /// `RandomState`'s noise wiring (`RandomState$1NoiseWiringHelper`), as `RandomState.router()`
    /// holds it: references and markers stay, and markers are transparent.
    Plain,
    /// A `NoiseChunk`'s `wrap`: references and `blend_density` are stripped, and the other
    /// markers become the chunk's caches and interpolators.
    Chunk,
    /// `RandomState`'s climate sampler (`RandomState$1`): references and every marker stripped.
    Climate,
}

/// A 26.2 density function bound to a seed's noises, with its range.
#[derive(Debug, Clone)]
pub struct Function {
    pub(crate) node: Arc<Node>,
    /// `minValue()`.
    min: f64,
    /// `maxValue()`.
    max: f64,
}

#[derive(Debug)]
pub(crate) enum Node {
    Constant(f64),
    /// `Ap2` add: `argument1 + argument2`.
    Add(Function, Function),
    /// `Ap2` mul: `+0.0` if `argument1` is ±0, else `argument1 · argument2`.
    Mul(Function, Function),
    /// `Ap2` min: `argument1` if below `argument2`'s range, else `Math.min`.
    Min(Function, Function),
    /// `Ap2` max: `argument1` if above `argument2`'s range, else `Math.max`.
    Max(Function, Function),
    /// `MulOrAdd` add: `input + c`.
    AddConstant(Function, f64),
    /// `MulOrAdd` mul: `input · c`, no zero test.
    MulConstant(Function, f64),
    Mapped(Mapped, Function),
    Clamp(Function, f64, f64),
    Noise {
        noise: Arc<NormalNoise>,
        xz_scale: f64,
        y_scale: f64,
    },
    ShiftedNoise {
        noise: Arc<NormalNoise>,
        xz_scale: f64,
        y_scale: f64,
        shift: [Function; 3],
    },
    ShiftA(Arc<NormalNoise>),
    ShiftB(Arc<NormalNoise>),
    Shift(Arc<NormalNoise>),
    YClampedGradient {
        from_y: f64,
        to_y: f64,
        from_value: f64,
        to_value: f64,
    },
    RangeChoice {
        input: Function,
        min_inclusive: f64,
        max_exclusive: f64,
        when_in_range: Function,
        when_out_of_range: Function,
    },
    IntervalSelect {
        input: Function,
        thresholds: Vec<f64>,
        functions: Vec<Function>,
    },
    Spline(BoundSpline),
    FindTopSurface {
        density: Function,
        upper_bound: Function,
        lower_bound: i32,
        cell_height: i32,
    },
    EndIslands(Arc<SimplexNoise>),
    BlendedNoise(Arc<BlendedNoise>),
    /// A marker that remains: transparent in [`Wiring::Plain`], the `NoiseChunk`'s in
    /// [`Wiring::Chunk`]. `slot` numbers the chunk's tables (see [`NoiseChunk`]).
    Marker {
        marker: Marker,
        input: Function,
        binder: u64,
        slot: u32,
    },
}

#[derive(Debug)]
pub(crate) enum BoundSpline {
    Constant(f32),
    Multipoint {
        coordinate: Function,
        locations: Vec<f32>,
        values: Vec<BoundSpline>,
        derivatives: Vec<f32>,
        min: f32,
        max: f32,
    },
}

/// Where a function is evaluated: a block, and the `NoiseChunk` (if any) whose wrapped graph is
/// evaluated, with what the context is to it.
#[derive(Clone, Copy)]
pub(crate) struct Ctx<'c> {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) z: i32,
    pub(crate) chunk: Option<&'c NoiseChunk>,
    pub(crate) own: Own,
}

/// What a context is to the `NoiseChunk` evaluating it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Own {
    /// Another context, a `SinglePointContext` (or there's no chunk).
    Foreign,
    /// The chunk itself, filling its interpolators' corners (`fillSlice`): the corner, relative
    /// to the chunk's first cell.
    Corner { cx: usize, cy: usize, cz: usize },
    /// The chunk itself, filling a cell's final density (`fillingCell`): the cell, relative to
    /// the first, and the block's offsets in it. Interpolators answer with `lerp3`.
    Cell {
        cx: usize,
        cy: usize,
        cz: usize,
        ix: i32,
        iy: i32,
        iz: i32,
    },
    /// The chunk itself, positioned by `updateForY`, `updateForX` and `updateForZ` outside a
    /// fill (as the ore veinifier reads it): interpolators answer from their y, x, then z lerps.
    Incremental {
        cx: usize,
        cy: usize,
        cz: usize,
        ix: i32,
        iy: i32,
        iz: i32,
    },
}

impl<'c> Ctx<'c> {
    /// `SinglePointContext(x, y, z)` with no chunk.
    pub(crate) fn point(x: i32, y: i32, z: i32) -> Self {
        Ctx {
            x,
            y,
            z,
            chunk: None,
            own: Own::Foreign,
        }
    }

    /// `SinglePointContext(x, y, z)` evaluated by `chunk`'s graph.
    fn foreign(&self, x: i32, y: i32, z: i32) -> Self {
        Ctx {
            x,
            y,
            z,
            chunk: self.chunk,
            own: Own::Foreign,
        }
    }
}

impl Function {
    fn new(node: Node, min: f64, max: f64) -> Self {
        Function {
            node: Arc::new(node),
            min,
            max,
        }
    }

    fn constant(v: f64) -> Self {
        Function::new(Node::Constant(v), v, v)
    }

    /// `add(function, beardifier)` with no structures: an `Ap2` add of 0 (a `MulOrAdd` if
    /// `function` is a constant, which adds in the other order to the same value).
    pub(crate) fn add_zero(function: &Function) -> Self {
        let zero = Function::constant(0.0);
        let (min, max) = (function.min + zero.min, function.max + zero.max);
        Function::new(Node::Add(function.clone(), zero), min, max)
    }

    /// `minValue()`.
    pub fn min_value(&self) -> f64 {
        self.min
    }

    /// `maxValue()`.
    pub fn max_value(&self) -> f64 {
        self.max
    }

    /// `compute(new SinglePointContext(x, y, z))`, outside any `NoiseChunk`, where every marker
    /// is its input.
    pub fn compute(&self, x: i32, y: i32, z: i32) -> f64 {
        self.eval(&Ctx::point(x, y, z))
    }

    pub(crate) fn eval(&self, ctx: &Ctx<'_>) -> f64 {
        match &*self.node {
            Node::Constant(v) => *v,
            Node::Add(a, b) => a.eval(ctx) + b.eval(ctx),
            Node::Mul(a, b) => {
                let v = a.eval(ctx);
                // dcmpl/ifne: −0.0 and +0.0 both give +0.0, without evaluating argument2.
                if v == 0.0 { 0.0 } else { v * b.eval(ctx) }
            }
            Node::Min(a, b) => {
                let v = a.eval(ctx);
                if v < b.min {
                    v
                } else {
                    java::min_f64(v, b.eval(ctx))
                }
            }
            Node::Max(a, b) => {
                let v = a.eval(ctx);
                if v > b.max {
                    v
                } else {
                    java::max_f64(v, b.eval(ctx))
                }
            }
            Node::AddConstant(input, c) => input.eval(ctx) + c,
            Node::MulConstant(input, c) => input.eval(ctx) * c,
            Node::Mapped(op, input) => mapped(*op, input.eval(ctx)),
            Node::Clamp(input, min, max) => java::mth_clamp_f64(input.eval(ctx), *min, *max),
            Node::Noise {
                noise,
                xz_scale,
                y_scale,
            } => noise.get_value(
                f64::from(ctx.x) * xz_scale,
                f64::from(ctx.y) * y_scale,
                f64::from(ctx.z) * xz_scale,
            ),
            Node::ShiftedNoise {
                noise,
                xz_scale,
                y_scale,
                shift: [sx, sy, sz],
            } => {
                let x = f64::from(ctx.x) * xz_scale + sx.eval(ctx);
                let y = f64::from(ctx.y) * y_scale + sy.eval(ctx);
                let z = f64::from(ctx.z) * xz_scale + sz.eval(ctx);
                noise.get_value(x, y, z)
            }
            Node::ShiftA(noise) => shift(noise, f64::from(ctx.x), 0.0, f64::from(ctx.z)),
            Node::ShiftB(noise) => shift(noise, f64::from(ctx.z), f64::from(ctx.x), 0.0),
            Node::Shift(noise) => {
                shift(noise, f64::from(ctx.x), f64::from(ctx.y), f64::from(ctx.z))
            }
            Node::YClampedGradient {
                from_y,
                to_y,
                from_value,
                to_value,
            } => {
                // Mth.clampedMap: clampedLerp(inverseLerp(y, from, to), from_value, to_value).
                let t = (f64::from(ctx.y) - from_y) / (to_y - from_y);
                mth::clamped_lerp(t, *from_value, *to_value)
            }
            Node::RangeChoice {
                input,
                min_inclusive,
                max_exclusive,
                when_in_range,
                when_out_of_range,
            } => {
                let v = input.eval(ctx);
                if v >= *min_inclusive && v < *max_exclusive {
                    when_in_range.eval(ctx)
                } else {
                    when_out_of_range.eval(ctx)
                }
            }
            Node::IntervalSelect {
                input,
                thresholds,
                functions,
            } => {
                let v = input.eval(ctx);
                // dcmpg/ifge: NaN is below no threshold.
                let i = thresholds
                    .iter()
                    .position(|t| v < *t)
                    .unwrap_or(functions.len() - 1);
                functions[i].eval(ctx)
            }
            Node::Spline(spline) => f64::from(spline.sample(ctx)),
            Node::FindTopSurface {
                density,
                upper_bound,
                lower_bound,
                cell_height,
            } => {
                let upper = upper_bound.eval(ctx);
                let start = mth::floor(upper / f64::from(*cell_height)).wrapping_mul(*cell_height);
                if start <= *lower_bound {
                    return f64::from(*lower_bound);
                }
                let mut y = start;
                while y >= *lower_bound {
                    // dcmpl/ifle: NaN isn't above 0.
                    if density.eval(&ctx.foreign(ctx.x, y, ctx.z)) > 0.0 {
                        return f64::from(y);
                    }
                    y = y.wrapping_sub(*cell_height);
                }
                f64::from(*lower_bound)
            }
            Node::EndIslands(noise) => {
                (f64::from(end_island_height(noise, ctx.x / 8, ctx.z / 8)) - 8.0) / 128.0
            }
            Node::BlendedNoise(noise) => noise.compute(ctx.x, ctx.y, ctx.z),
            Node::Marker {
                marker,
                input,
                binder,
                slot,
            } => match ctx.chunk {
                None => input.eval(ctx),
                Some(chunk) => chunk.marker(*marker, input, *binder, *slot, ctx),
            },
        }
    }
}

/// `ShiftNoise.compute(x, y, z)`: the noise at a quarter of each coordinate, times 4.
fn shift(noise: &NormalNoise, x: f64, y: f64, z: f64) -> f64 {
    noise.get_value(x * 0.25, y * 0.25, z * 0.25) * 4.0
}

/// `Mapped.transform`.
fn mapped(op: Mapped, x: f64) -> f64 {
    match op {
        Mapped::Abs => x.abs(),
        Mapped::Square => x * x,
        Mapped::Cube => x * x * x,
        // dcmpl/ifle: NaN takes the product.
        Mapped::HalfNegative => {
            if x > 0.0 {
                x
            } else {
                x * 0.5
            }
        }
        Mapped::QuarterNegative => {
            if x > 0.0 {
                x
            } else {
                x * 0.25
            }
        }
        Mapped::Invert => 1.0 / x,
        Mapped::Squeeze => {
            let c = java::mth_clamp_f64(x, -1.0, 1.0);
            c / 2.0 - c * c * c / 24.0
        }
    }
}

/// `EndIslandDensityFunction.getHeightValue(noise, x, z)`, with `x` and `z` a block's divided by
/// 8: the central island, then the highest outer island over the 25 × 25 cells around
/// `(x / 2, z / 2)`, in `f32`.
fn end_island_height(noise: &SimplexNoise, x: i32, z: i32) -> f32 {
    let (xs, zs, xr, zr) = (x / 2, z / 2, x % 2, z % 2);
    // Mth.sqrt((float)(x·x + z·z)), the squares and sum in wrapping int.
    let d = x.wrapping_mul(x).wrapping_add(z.wrapping_mul(z)) as f32;
    let mut height = mth_clamp_f32(100.0 - (f64::from(d).sqrt() as f32) * 8.0, -100.0, 80.0);
    for dx in -12..=12 {
        for dz in -12..=12 {
            let px = i64::from(xs.wrapping_add(dx));
            let pz = i64::from(zs.wrapping_add(dz));
            // lcmp/ifle: strictly beyond 64 cells; dcmpg/ifge against (double) −0.9f.
            if px * px + pz * pz > 4096
                && noise.get_value_2d(px as f64, pz as f64) < f64::from(-0.9_f32)
            {
                let size = ((px as f32).abs() * 3439.0 + (pz as f32).abs() * 147.0) % 13.0 + 9.0;
                let fx = xr.wrapping_sub(dx.wrapping_mul(2)) as f32;
                let fz = zr.wrapping_sub(dz.wrapping_mul(2)) as f32;
                let length = f64::from(fx * fx + fz * fz).sqrt() as f32;
                let v = mth_clamp_f32(100.0 - length * size, -100.0, 80.0);
                height = java::max(height, v);
            }
        }
    }
    height
}

/// `Mth.clamp(float, float, float)`: `min` if `v < min`, else `Math.min(v, max)`.
fn mth_clamp_f32(v: f32, min: f32, max: f32) -> f32 {
    if v < min { min } else { java::min(v, max) }
}

impl BoundSpline {
    /// `CubicSpline.sample`, each coordinate `(float)` of its function's value.
    fn sample(&self, ctx: &Ctx<'_>) -> f32 {
        match self {
            BoundSpline::Constant(v) => *v,
            BoundSpline::Multipoint {
                coordinate,
                locations,
                values,
                derivatives,
                ..
            } => {
                let c = coordinate.eval(ctx) as f32;
                sample_multipoint(c, locations, values, derivatives, &mut |v: &BoundSpline| {
                    v.sample(ctx)
                })
            }
        }
    }

    fn min(&self) -> f32 {
        match self {
            BoundSpline::Constant(v) => *v,
            BoundSpline::Multipoint { min, .. } => *min,
        }
    }

    fn max(&self) -> f32 {
        match self {
            BoundSpline::Constant(v) => *v,
            BoundSpline::Multipoint { max, .. } => *max,
        }
    }
}

/// `CubicSpline.Multipoint`'s constructor's range: the coordinate's range extended past the end
/// points, every value's range, and each interval's bound on the cubic's overshoot, in `f32`.
fn spline_range(
    coordinate: (f32, f32),
    locations: &[f32],
    values: &[BoundSpline],
    derivatives: &[f32],
) -> (f32, f32) {
    let last = locations.len() - 1;
    let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
    let (c_min, c_max) = coordinate;
    // linearExtend.
    let extend = |c: f32, v: f32, i: usize| {
        let d = derivatives[i];
        if d == 0.0 {
            v
        } else {
            v + d * (c - locations[i])
        }
    };
    // fcmpg/ifge, fcmpl/ifle: a NaN bound extends neither end.
    if c_min < locations[0] {
        let a = extend(c_min, values[0].min(), 0);
        let b = extend(c_min, values[0].max(), 0);
        lo = java::min(lo, java::min(a, b));
        hi = java::max(hi, java::max(a, b));
    }
    if c_max > locations[last] {
        let a = extend(c_max, values[last].min(), last);
        let b = extend(c_max, values[last].max(), last);
        lo = java::min(lo, java::min(a, b));
        hi = java::max(hi, java::max(a, b));
    }
    for v in values {
        lo = java::min(lo, v.min());
        hi = java::max(hi, v.max());
    }
    for i in 0..last {
        let (d0, d1) = (derivatives[i], derivatives[i + 1]);
        if d0 == 0.0 && d1 == 0.0 {
            continue;
        }
        let dx = locations[i + 1] - locations[i];
        let (a0, b0) = (values[i].min(), values[i].max());
        let (a1, b1) = (values[i + 1].min(), values[i + 1].max());
        let p = d0 * dx;
        let q = d1 * dx;
        let low = java::min(a0, a1);
        let high = java::max(b0, b1);
        let e = p - b1 + a0;
        let f = p - a1 + b0;
        let g = -q + a1 - b0;
        let h = -q + b1 - a0;
        lo = java::min(lo, low + 0.25 * java::min(e, g));
        hi = java::max(hi, high + 0.25 * java::max(f, h));
    }
    (lo, hi)
}

/// Tells binders apart, so a `NoiseChunk` never reads another binder's slots.
static BINDERS: AtomicU64 = AtomicU64::new(0);

/// `RandomState`'s positional factory, which `legacy_random_source` chooses.
#[derive(Debug, Clone)]
enum WorldRandom {
    /// `XoroshiroRandomSource(seed).forkPositional()`.
    Xoroshiro(XoroshiroPositionalRandomFactory),
    /// `LegacyRandomSource(seed).forkPositional()`.
    Legacy(LegacyPositionalRandomFactory),
}

/// `Noises.TEMPERATURE_NETHER` and `VEGETATION_NETHER`, with the offsets their legacy sources
/// add to the seed (`NoiseWiringHelper.visitNoise`).
const NETHER_BIOME_NOISES: [(&str, i64); 2] = [
    ("minecraft:nether/temperature", 0),
    ("minecraft:nether/vegetation", 1),
];

/// Binds 26.2 functions for one seed, as `RandomState`'s noise wiring does: each noise is made
/// once (`getOrCreateNoise`), and shared nodes stay shared.
pub struct Binder<'n> {
    noise_parameters: &'n HashMap<String, NormalNoiseParameters>,
    seed: i64,
    legacy: bool,
    random: WorldRandom,
    noises: HashMap<String, Arc<NormalNoise>>,
    end_islands: Option<Arc<SimplexNoise>>,
    /// Bound functions by loaded function identity and wiring. Each entry keeps its function
    /// alive, so its address can't be reused while the binder holds it.
    bound: HashMap<(*const DensityFunction, Wiring), (Df, Function)>,
    id: u64,
    slots: u32,
}

impl<'n> Binder<'n> {
    /// A binder for `seed`, with noises from `noise_parameters` (full id → parameters), and the
    /// legacy random source if `legacy` (`legacy_random_source`).
    pub fn new(
        noise_parameters: &'n HashMap<String, NormalNoiseParameters>,
        seed: i64,
        legacy: bool,
    ) -> Self {
        // RandomState: algorithm.newInstance(seed).forkPositional().
        let random = if legacy {
            WorldRandom::Legacy(LegacyRandomSource::new(seed).fork_positional())
        } else {
            WorldRandom::Xoroshiro(XoroshiroRandomSource::new(seed).fork_positional())
        };
        Binder {
            noise_parameters,
            seed,
            legacy,
            random,
            noises: HashMap::new(),
            end_islands: None,
            bound: HashMap::new(),
            id: BINDERS.fetch_add(1, Ordering::Relaxed),
            slots: 0,
        }
    }

    /// Binds `function` as `RandomState`'s noise wiring maps it: what `RandomState.router()`
    /// holds, evaluated at a point outside any `NoiseChunk` by [`Function::compute`].
    pub fn bind(&mut self, function: &Df) -> Result<Function, LoadError> {
        self.bind_with(function, Wiring::Plain)
    }

    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    pub(crate) fn slots(&self) -> u32 {
        self.slots
    }

    /// `visitNoise`: the Nether biome noises by their own rule, others by `getOrCreateNoise`.
    fn noise(&mut self, id: &str) -> Result<Arc<NormalNoise>, LoadError> {
        if let Some(n) = self.noises.get(id) {
            return Ok(n.clone());
        }
        let error = |kind| LoadError::new(id, "", kind);
        let parameters = match self.noise_parameters.get(id) {
            Some(NormalNoiseParameters::V1_21(p)) => p,
            Some(NormalNoiseParameters::V26_3(_)) => {
                return Err(error(LoadErrorKind::Invalid(
                    "26.3 noise parameters in a 26.2 density function",
                )));
            }
            None => {
                return Err(error(LoadErrorKind::Invalid(
                    "noise wasn't loaded before binding",
                )));
            }
        };
        let nether = NETHER_BIOME_NOISES.iter().find(|(n, _)| *n == id);
        let noise = match (nether, &self.random) {
            (Some(&(_, offset)), _) => {
                let mut random = LegacyRandomSource::new(self.seed.wrapping_add(offset));
                NormalNoise::create_legacy_nether_biome(&mut random, parameters)
                    .map_err(|e| error(LoadErrorKind::Invalid(e.reason())))?
            }
            (None, WorldRandom::Xoroshiro(f)) => {
                NormalNoise::create(&mut f.from_hash_of(id), parameters)
            }
            (None, WorldRandom::Legacy(f)) => {
                NormalNoise::create(&mut f.from_hash_of(id), parameters)
            }
        };
        let noise = Arc::new(noise);
        self.noises.insert(id.to_string(), noise.clone());
        Ok(noise)
    }

    /// Whether `function` is a `Constant` once `wiring` has mapped it: references are stripped
    /// in the chunk and climate wirings, `blend_density` in the chunk's, every marker in the
    /// climate's.
    fn constant(function: &Df, wiring: Wiring) -> Option<f64> {
        match &**function {
            DensityFunction::Constant(v) => Some(*v),
            DensityFunction::Reference { function, .. } if wiring != Wiring::Plain => {
                Self::constant(function, wiring)
            }
            DensityFunction::Marker(marker, input)
                if wiring == Wiring::Climate
                    || (wiring == Wiring::Chunk && *marker == Marker::BlendDensity) =>
            {
                Self::constant(input, wiring)
            }
            _ => None,
        }
    }

    pub(crate) fn bind_with(
        &mut self,
        function: &Df,
        wiring: Wiring,
    ) -> Result<Function, LoadError> {
        let key = (Arc::as_ptr(function), wiring);
        if let Some((_, f)) = self.bound.get(&key) {
            return Ok(f.clone());
        }
        let bound = self.compile(function, wiring)?;
        self.bound.insert(key, (function.clone(), bound.clone()));
        Ok(bound)
    }

    fn compile(&mut self, function: &Df, wiring: Wiring) -> Result<Function, LoadError> {
        let mut bind = |f: &Df| self.bind_with(f, wiring);
        Ok(match &**function {
            DensityFunction::Constant(v) => Function::constant(*v),
            // HolderHolder: the referenced function's value and range.
            DensityFunction::Reference { function, .. } => bind(function)?,
            DensityFunction::TwoArgument(op, a, b) => {
                let (ca, cb) = (Self::constant(a, wiring), Self::constant(b, wiring));
                let (fa, fb) = (bind(a)?, bind(b)?);
                two_argument(*op, fa, fb, ca, cb)
            }
            DensityFunction::Mapped(op, input) => {
                let input = bind(input)?;
                let (min, max) = mapped_range(*op, input.min, input.max);
                Function::new(Node::Mapped(*op, input), min, max)
            }
            DensityFunction::Clamp { input, min, max } => {
                Function::new(Node::Clamp(bind(input)?, *min, *max), *min, *max)
            }
            DensityFunction::Noise {
                noise,
                xz_scale,
                y_scale,
            } => {
                let noise = self.noise(noise)?;
                let max = noise.max_value();
                Function::new(
                    Node::Noise {
                        noise,
                        xz_scale: *xz_scale,
                        y_scale: *y_scale,
                    },
                    -max,
                    max,
                )
            }
            DensityFunction::ShiftedNoise {
                noise,
                xz_scale,
                y_scale,
                shift_x,
                shift_y,
                shift_z,
            } => {
                let shift = [bind(shift_x)?, bind(shift_y)?, bind(shift_z)?];
                let noise = self.noise(noise)?;
                let max = noise.max_value();
                Function::new(
                    Node::ShiftedNoise {
                        noise,
                        xz_scale: *xz_scale,
                        y_scale: *y_scale,
                        shift,
                    },
                    -max,
                    max,
                )
            }
            DensityFunction::ShiftA { noise }
            | DensityFunction::ShiftB { noise }
            | DensityFunction::Shift { noise } => {
                let noise = self.noise(noise)?;
                // ShiftNoise: ±offsetNoise.maxValue() · 4.
                let max = noise.max_value() * 4.0;
                let node = match &**function {
                    DensityFunction::ShiftA { .. } => Node::ShiftA(noise),
                    DensityFunction::ShiftB { .. } => Node::ShiftB(noise),
                    _ => Node::Shift(noise),
                };
                Function::new(node, -max, max)
            }
            DensityFunction::YClampedGradient {
                from_y,
                to_y,
                from_value,
                to_value,
            } => Function::new(
                Node::YClampedGradient {
                    from_y: f64::from(*from_y),
                    to_y: f64::from(*to_y),
                    from_value: *from_value,
                    to_value: *to_value,
                },
                java::min_f64(*from_value, *to_value),
                java::max_f64(*from_value, *to_value),
            ),
            DensityFunction::RangeChoice {
                input,
                min_inclusive,
                max_exclusive,
                when_in_range,
                when_out_of_range,
            } => {
                let (input, when_in_range, when_out_of_range) =
                    (bind(input)?, bind(when_in_range)?, bind(when_out_of_range)?);
                let min = java::min_f64(when_in_range.min, when_out_of_range.min);
                let max = java::max_f64(when_in_range.max, when_out_of_range.max);
                Function::new(
                    Node::RangeChoice {
                        input,
                        min_inclusive: *min_inclusive,
                        max_exclusive: *max_exclusive,
                        when_in_range,
                        when_out_of_range,
                    },
                    min,
                    max,
                )
            }
            DensityFunction::IntervalSelect {
                input,
                thresholds,
                functions,
            } => {
                let input = bind(input)?;
                let functions = functions.iter().map(bind).collect::<Result<Vec<_>, _>>()?;
                // From ±Double.MAX_VALUE, Math.min (max) of each function's bound in turn.
                let min = functions
                    .iter()
                    .fold(f64::MAX, |m, f| java::min_f64(f.min, m));
                let max = functions
                    .iter()
                    .fold(-f64::MAX, |m, f| java::max_f64(f.max, m));
                Function::new(
                    Node::IntervalSelect {
                        input,
                        thresholds: thresholds.clone(),
                        functions,
                    },
                    min,
                    max,
                )
            }
            DensityFunction::Spline(spline) => {
                let spline = self.bind_spline(spline, wiring)?;
                let (min, max) = (f64::from(spline.min()), f64::from(spline.max()));
                Function::new(Node::Spline(spline), min, max)
            }
            DensityFunction::FindTopSurface {
                density,
                upper_bound,
                lower_bound,
                cell_height,
            } => {
                let (density, upper_bound) = (bind(density)?, bind(upper_bound)?);
                let min = f64::from(*lower_bound);
                let max = java::max_f64(min, upper_bound.max);
                Function::new(
                    Node::FindTopSurface {
                        density,
                        upper_bound,
                        lower_bound: *lower_bound,
                        cell_height: *cell_height,
                    },
                    min,
                    max,
                )
            }
            DensityFunction::EndIslands => {
                // NoiseWiringHelper: new EndIslandDensityFunction(seed), whatever the legacy
                // flag: LegacyRandomSource(seed), 17292 values skipped, then the simplex noise.
                let seed = self.seed;
                let noise = self
                    .end_islands
                    .get_or_insert_with(|| {
                        let mut random = LegacyRandomSource::new(seed);
                        random.consume_count(17292);
                        Arc::new(SimplexNoise::new(&mut random))
                    })
                    .clone();
                Function::new(Node::EndIslands(noise), -0.84375, 0.5625)
            }
            DensityFunction::OldBlendedNoise(p) => {
                // NoiseWiringHelper: withNewRandom(legacy ? LegacyRandomSource(seed + 0)
                // : random.fromHashOf("minecraft:terrain")), a fresh random each time.
                let new = |random: &mut _| {
                    BlendedNoise::new(
                        random,
                        p.xz_scale,
                        p.y_scale,
                        p.xz_factor,
                        p.y_factor,
                        p.smear_scale_multiplier,
                    )
                };
                let noise = match (&self.random, self.legacy) {
                    (_, true) => new(&mut LegacyRandomSource::new(self.seed)),
                    (WorldRandom::Xoroshiro(f), false) => BlendedNoise::new(
                        &mut f.from_hash_of("minecraft:terrain"),
                        p.xz_scale,
                        p.y_scale,
                        p.xz_factor,
                        p.y_factor,
                        p.smear_scale_multiplier,
                    ),
                    (WorldRandom::Legacy(_), false) => unreachable!("legacy random is the flag"),
                };
                let (min, max) = (noise.min_value(), noise.max_value());
                Function::new(Node::BlendedNoise(Arc::new(noise)), min, max)
            }
            // Not Constants (they're singletons of their own classes), but constant-valued
            // without blending or structures.
            DensityFunction::BlendAlpha => Function::constant(1.0),
            DensityFunction::BlendOffset | DensityFunction::Beardifier => Function::constant(0.0),
            DensityFunction::Marker(marker, input) => {
                let stripped = match wiring {
                    Wiring::Plain => false,
                    Wiring::Chunk => *marker == Marker::BlendDensity,
                    Wiring::Climate => true,
                };
                let input = bind(input)?;
                if stripped {
                    input
                } else {
                    // Marker.minValue: ±∞ for blend_density, else the input's.
                    let (min, max) = if *marker == Marker::BlendDensity {
                        (f64::NEG_INFINITY, f64::INFINITY)
                    } else {
                        (input.min, input.max)
                    };
                    let slot = self.slots;
                    self.slots += 1;
                    Function::new(
                        Node::Marker {
                            marker: *marker,
                            input,
                            binder: self.id,
                            slot,
                        },
                        min,
                        max,
                    )
                }
            }
        })
    }

    fn bind_spline(&mut self, spline: &Spline, wiring: Wiring) -> Result<BoundSpline, LoadError> {
        Ok(match spline {
            Spline::Constant(v) => BoundSpline::Constant(*v),
            Spline::Multipoint {
                coordinate,
                locations,
                values,
                derivatives,
            } => {
                let coordinate = self.bind_with(coordinate, wiring)?;
                let values = values
                    .iter()
                    .map(|v| self.bind_spline(v, wiring))
                    .collect::<Result<Vec<_>, _>>()?;
                // Spline.Coordinate: (float) minValue() and maxValue().
                let (min, max) = spline_range(
                    (coordinate.min as f32, coordinate.max as f32),
                    locations,
                    &values,
                    derivatives,
                );
                BoundSpline::Multipoint {
                    coordinate,
                    locations: locations.clone(),
                    values,
                    derivatives: derivatives.clone(),
                    min,
                    max,
                }
            }
        })
    }
}

/// `TwoArgumentSimpleFunction.create`: the range from the operands', then `MulOrAdd` if
/// `argument1`, else `argument2`, is a constant (add and mul only), else `Ap2`. Every function
/// is mapped at least once (by the noise wiring), and `MulOrAdd.mapChildren` computes its range
/// its own way, which is the one that stays.
fn two_argument(
    op: TwoArgument,
    a: Function,
    b: Function,
    ca: Option<f64>,
    cb: Option<f64>,
) -> Function {
    if matches!(op, TwoArgument::Add | TwoArgument::Mul) {
        let constant = match (ca, cb) {
            (Some(c), _) => Some((b.clone(), c)),
            (None, Some(c)) => Some((a.clone(), c)),
            (None, None) => None,
        };
        if let Some((input, c)) = constant {
            let (min, max) = (input.min, input.max);
            return if op == TwoArgument::Add {
                Function::new(Node::AddConstant(input, c), min + c, max + c)
            } else if c >= 0.0 {
                // dcmpl/iflt: a NaN constant takes the other branch.
                Function::new(Node::MulConstant(input, c), min * c, max * c)
            } else {
                Function::new(Node::MulConstant(input, c), max * c, min * c)
            };
        }
    }
    let (a_min, a_max, b_min, b_max) = (a.min, a.max, b.min, b.max);
    let (min, max) = match op {
        TwoArgument::Add => (a_min + b_min, a_max + b_max),
        TwoArgument::Min => (java::min_f64(a_min, b_min), java::min_f64(a_max, b_max)),
        TwoArgument::Max => (java::max_f64(a_min, b_min), java::max_f64(a_max, b_max)),
        TwoArgument::Mul => {
            // dcmpl/ifle and dcmpg/ifge: NaN bounds fall through to the general case.
            let min = if a_min > 0.0 && b_min > 0.0 {
                a_min * b_min
            } else if a_max < 0.0 && b_max < 0.0 {
                a_max * b_max
            } else {
                java::min_f64(a_min * b_max, a_max * b_min)
            };
            let max = if a_min > 0.0 && b_min > 0.0 {
                a_max * b_max
            } else if a_max < 0.0 && b_max < 0.0 {
                a_min * b_min
            } else {
                java::max_f64(a_min * b_min, a_max * b_max)
            };
            (min, max)
        }
    };
    let node = match op {
        TwoArgument::Add => Node::Add(a, b),
        TwoArgument::Mul => Node::Mul(a, b),
        TwoArgument::Min => Node::Min(a, b),
        TwoArgument::Max => Node::Max(a, b),
    };
    Function::new(node, min, max)
}

/// `Mapped.create`'s range: `invert` of a range across 0 is unbounded, else its bounds mapped and
/// swapped; `abs` and `square` from `max(0, min)`; the rest the bounds mapped.
fn mapped_range(op: Mapped, min: f64, max: f64) -> (f64, f64) {
    let (lo, hi) = (mapped(op, min), mapped(op, max));
    match op {
        Mapped::Invert => {
            if min < 0.0 && max > 0.0 {
                (f64::NEG_INFINITY, f64::INFINITY)
            } else {
                (hi, lo)
            }
        }
        Mapped::Abs | Mapped::Square => (java::max_f64(0.0, min), java::max_f64(lo, hi)),
        _ => (lo, hi),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn end_island_central_term_is_quantized_and_clamped() {
        let noise = SimplexNoise::new(&mut LegacyRandomSource::new(0));
        // At the origin the outer islands are out of reach (within 64 cells), so the central
        // term alone: 100 − sqrt(0)·8 = 100, clamped to 80.
        assert_eq!(end_island_height(&noise, 0, 0), 80.0);
        // (3, 4): 100 − 5·8 = 60.
        assert_eq!(end_island_height(&noise, 3, 4), 60.0);
        // Far beyond: −100 at least.
        assert!(end_island_height(&noise, 100, 0) >= -100.0);
    }

    #[test]
    fn mapped_ranges_follow_create() {
        assert_eq!(
            mapped_range(Mapped::Invert, -1.0, 2.0),
            (f64::NEG_INFINITY, f64::INFINITY)
        );
        assert_eq!(mapped_range(Mapped::Invert, 2.0, 4.0), (0.25, 0.5));
        assert_eq!(mapped_range(Mapped::Abs, -3.0, 2.0), (0.0, 3.0));
        assert_eq!(mapped_range(Mapped::Square, 1.0, 2.0), (1.0, 4.0));
        assert_eq!(mapped_range(Mapped::HalfNegative, -2.0, 2.0), (-1.0, 2.0));
    }
}
