//! Density functions bound to a seed, evaluated at a block position as the game's compiled
//! samplers do (`DensitySampler.sampleValue`). Each case cites the sampler it follows. The
//! volume path (`sampleVolume`) is in `volume.rs`.
//!
//! Binding follows `compileSampler`, including the choices the game makes while compiling:
//! constant operands, `min` and `max` by their operands' ranges, `pow` by its exponent. Those
//! choices change results in corner cases (signed zeros, NaN, skipped operands), so they're made
//! exactly as the game makes them.
//!
//! Evaluation remembers values per thread (see [`Sampler::sample`]): a node referenced from
//! several places is computed once per position, and a node whose value can't depend on y once
//! per column. That changes when each floating-point operation runs, never which ones run, so the
//! results are the same bits.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use spyglass_data::anchor::AnchorHeights;
use spyglass_data::codec::{LoadError, LoadErrorKind};
use spyglass_noise::mth::{self, exp2i};
use spyglass_noise::{
    Layer, NoiseStack, NormalNoise, NormalNoiseParameters, SimplexNoise, SmearedPerlinNoise, Volume,
};
use spyglass_rng::{
    LegacyPositionalRandomFactory, LegacyRandomSource, PositionalRandomFactory, RandomSource,
    XoroshiroPositionalRandomFactory, XoroshiroRandomSource,
};

use crate::function::{
    AXIS_Y, Axis, Binary, BlendedNoise, DensityFunction, Df, DistanceMetric, GradientCoordinate,
    Round, Spline, Tiling, Unary, UpscaleAxis, UpscaleMode, domain_axes,
};
use crate::interval::{Interval, IntervalError};
use crate::java;
use crate::range;
use crate::volume::Upscaler;

/// A density function bound to a seed's noises, ready to evaluate.
#[derive(Debug, Clone)]
pub struct Sampler {
    pub(crate) node: Arc<Node>,
    /// Whether the value can depend on y; see [`Sampler::depends_on_y`].
    pub(crate) depends_on_y: bool,
    /// The [`Binder`] that made it, which numbered its slots.
    binder: u64,
    /// Where its value is remembered, or [`NO_SLOT`] for constants.
    pub(crate) slot: u32,
}

/// The slot of a node that isn't worth remembering.
pub(crate) const NO_SLOT: u32 = u32::MAX;

#[derive(Debug)]
pub(crate) enum Node {
    Constant(f32),
    /// `NoiseFunction.Sampler`: no shifts.
    Noise {
        noise: Arc<NoiseStack>,
        xz_scale: f64,
        y_scale: f64,
    },
    /// `NoiseFunction.ShiftedXzSampler`: `shift_y` is zero.
    NoiseShiftedXz {
        noise: Arc<NoiseStack>,
        xz_scale: f64,
        y_scale: f64,
        sx: Sampler,
        sz: Sampler,
    },
    /// `NoiseFunction.ShiftedXyzSampler`.
    NoiseShiftedXyz {
        noise: Arc<NoiseStack>,
        xz_scale: f64,
        y_scale: f64,
        sx: Sampler,
        sy: Sampler,
        sz: Sampler,
    },
    /// `NoiseFunction.Sampler` over one of old blended noise's smeared stacks.
    SmearedNoise {
        noise: Arc<NoiseStack<SmearedPerlinNoise>>,
        xz_scale: f64,
        y_scale: f64,
    },
    ShiftA(Arc<NoiseStack>),
    ShiftB(Arc<NoiseStack>),
    Shift(Arc<NoiseStack>),
    /// `AddSampler`, and `ConstAddSampler`, which computes the same float.
    Add(Sampler, Sampler),
    /// `SubSampler` and `ConstSubSampler`; a constant right operand compiles to an add of its
    /// negation, which is the same float.
    Sub(Sampler, Sampler),
    /// `MulSampler`: both operands non-constant.
    Mul(Sampler, Sampler),
    /// `ConstMulSampler`, also a `div` by a constant (times its reciprocal).
    MulConst(Sampler, f32),
    /// `DivSampler`: both operands non-constant.
    Div(Sampler, Sampler),
    /// `ConstDivSampler`: a constant divided by the sampler.
    ConstDiv(f32, Sampler),
    /// `MinSampler`, with the right operand's range minimum.
    Min(Sampler, Sampler, f32),
    /// `ConstMinSampler`.
    MinConst(Sampler, f32),
    /// `MaxSampler`, with the right operand's range maximum.
    Max(Sampler, Sampler, f32),
    /// `ConstMaxSampler`.
    MaxConst(Sampler, f32),
    Unary(Unary, Sampler),
    /// `RoundFunction.Sampler`.
    Round(Round, Sampler, Sampler),
    /// `RoundFunction.IntegerMultipleSampler`: a multiple that's the constant 1.
    RoundInteger(Round, Sampler),
    /// `ClampFunction.Sampler`, with `Mth.clamp`.
    Clamp(Sampler, f32, f32),
    /// `ClampFunction.Sampler`, with `Math.clamp` (switch `clamp_positive_zero`).
    MathClamp(Sampler, f32, f32),
    /// `PowFunction.Sampler`.
    Pow(Sampler, Sampler),
    /// `PowFunction.ConstBaseSampler`.
    PowConstBase(f64, Sampler),
    /// `PowFunction.ConstExponentSampler`.
    PowConstExponent(Sampler, f64),
    Lerp {
        alpha: Sampler,
        first: Sampler,
        second: Sampler,
    },
    /// `RangeChoiceFunction.Sampler` (and `ConstSampler`, the same choice).
    RangeChoice {
        input: Sampler,
        min_inclusive: f32,
        max_exclusive: f32,
        when_in_range: Sampler,
        when_out_of_range: Sampler,
    },
    /// `IntervalSelectFunction.Sampler` (and `SingleThresholdSampler`, the same choice).
    IntervalSelect {
        input: Sampler,
        thresholds: Vec<f32>,
        samplers: Vec<Sampler>,
    },
    /// `SliceFunction`'s samplers: the input with one coordinate fixed.
    Slice {
        axis: Axis,
        coordinate: i32,
        input: Sampler,
    },
    /// `InterpolatedFunction.Sampler`, with the reciprocals of its cell sizes.
    Interpolated {
        input: Sampler,
        cell_xz: i32,
        cell_y: i32,
        inv_xz: f32,
        inv_y: f32,
    },
    /// `UpscaleFunction.Sampler`.
    Upscale(Box<BoundUpscale>),
    /// `UpscaleFunction.NearestNeighborSampler`: every axis `nearest_neighbor`, with the cell
    /// sizes.
    UpscaleNearest {
        input: Sampler,
        cell: [i32; 3],
    },
    /// `FindTopSurfaceFunction.Sampler`, compiled inside a slice at y 0.
    FindTopSurface {
        density: Sampler,
        upper_bound: Sampler,
        lower_bound: i32,
        cell_height: i32,
    },
    /// `GradientFunction.ClampedSampler`.
    Gradient {
        axis: Axis,
        from: i32,
        min: i32,
        max: i32,
        from_value: f32,
        factor: f32,
    },
    /// `GradientFunction.RepeatSampler`, and with `mirrored`, `MirroredRepeatSampler`.
    GradientRepeat {
        axis: Axis,
        from: i32,
        range: i32,
        from_value: f32,
        factor: f32,
        mirrored: bool,
    },
    Spline(BoundSpline),
    /// `DistanceToPointFunction.Sampler`.
    DistanceToPoint {
        point: [i32; 3],
        metric: DistanceMetric,
    },
    /// `EndIslandFunction.Sampler`.
    EndIslands(Arc<SimplexNoise>),
    /// `DomainWarpFunction`'s samplers, with the function's domain axes (see
    /// [`domain_axes`]), which decide the volume it sees.
    DomainWarp {
        input: Sampler,
        offsets: Warp,
        axes: u8,
    },
}

/// How a `domain_warp` moves its input's positions.
#[derive(Debug)]
pub(crate) enum Warp {
    /// `ConstOffsetSampler`: every offset a constant, truncated once.
    Const([i32; 3]),
    /// `FlatXzSampler`: the input doesn't depend on y, so it's sampled at y 0, moved in x and z.
    FlatXz { x: Sampler, z: Sampler },
    /// `Sampler`: moved along all three axes.
    General { x: Sampler, y: Sampler, z: Sampler },
}

/// `UpscaleFunction.Sampler`'s parts: the input, the axes, and the upscaler for the axes' own
/// modes and cell sizes (`valueUpscaler`).
#[derive(Debug)]
pub(crate) struct BoundUpscale {
    pub(crate) input: Sampler,
    pub(crate) axes: [UpscaleAxis; 3],
    pub(crate) upscaler: Upscaler,
    /// Switch `upscale_offset_by_step`.
    pub(crate) offset_by_step: bool,
}

#[derive(Debug)]
pub(crate) enum BoundSpline {
    Constant(f32),
    Multipoint {
        coordinate: Sampler,
        locations: Vec<f32>,
        values: Vec<BoundSpline>,
        derivatives: Vec<f32>,
        /// Switch `spline_nan_is_nan`: sample as 26.4 does.
        nan_is_nan: bool,
    },
}

/// Tells binders apart, so remembered values are never read by another binder's nodes.
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
/// add to the seed (`RandomState`'s `createNoiseSampler`).
const NETHER_BIOME_NOISES: [(&str, i64); 2] = [
    ("minecraft:nether/temperature", 0),
    ("minecraft:nether/vegetation", 1),
];

/// Binds functions for one seed. Shared nodes stay shared, and each noise is created once, as
/// `RandomState.getOrCreateNoise` does.
pub struct Binder<'n> {
    noise_parameters: &'n HashMap<String, NormalNoiseParameters>,
    seed: i64,
    random: WorldRandom,
    noises: HashMap<String, Arc<NoiseStack>>,
    /// Samplers by function identity. Each entry keeps its function alive, so its address can't
    /// be reused by another function while the binder holds it.
    bound: HashMap<*const DensityFunction, (Df, Sampler)>,
    /// Ranges by function identity, kept alive the same way.
    ranges: HashMap<*const DensityFunction, (Df, Interval)>,
    id: u64,
    slots: u32,
    /// What vertical anchors resolve against, if known.
    anchors: Option<AnchorHeights>,
}

impl<'n> Binder<'n> {
    /// A binder for `seed`, with noises from `noise_parameters` (full id → parameters), and the
    /// Xoroshiro random source (`legacy_random_source: false`).
    pub fn new(noise_parameters: &'n HashMap<String, NormalNoiseParameters>, seed: i64) -> Self {
        Self::with_legacy_random(noise_parameters, seed, false)
    }

    /// [`new`](Self::new), with the legacy random source if `legacy`
    /// (`legacy_random_source: true`).
    pub fn with_legacy_random(
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
            random,
            noises: HashMap::new(),
            bound: HashMap::new(),
            ranges: HashMap::new(),
            id: BINDERS.fetch_add(1, Ordering::Relaxed),
            slots: 0,
            anchors: None,
        }
    }

    /// Resolves vertical anchors against `heights` (`RandomState.verticalAnchors()`): the
    /// level's, narrowed by the noise settings ([`AnchorHeights::within_level`]), or the noise
    /// settings' alone. Without them, a function with an anchor doesn't bind.
    pub fn with_anchors(mut self, heights: AnchorHeights) -> Self {
        self.anchors = Some(heights);
        self
    }

    /// `Coordinate.resolve(context)`.
    fn resolve(&self, coordinate: GradientCoordinate) -> Result<i32, LoadError> {
        match coordinate {
            GradientCoordinate::Absolute(c) => Ok(c),
            GradientCoordinate::Anchor(anchor) => {
                let heights = self.anchors.ok_or_else(|| {
                    LoadError::new(
                        "",
                        "",
                        LoadErrorKind::Invalid("a vertical anchor needs the level's heights"),
                    )
                })?;
                Ok(anchor.resolve(heights))
            }
        }
    }

    /// The parameters' `NormalNoise`, which doesn't depend on the seed.
    pub(crate) fn normal_noise(&self, id: &str) -> Result<NormalNoise, LoadError> {
        let error = |kind| LoadError::new(id, "", kind);
        let parameters = self
            .noise_parameters
            .get(id)
            .ok_or_else(|| error(LoadErrorKind::Invalid("noise wasn't loaded before binding")))?;
        NormalNoise::new(parameters).map_err(|e| error(LoadErrorKind::Invalid(e.reason())))
    }

    /// `CompileContext.createNoiseSampler`: the Nether biome noises by their own rule, others by
    /// `RandomState.getOrCreateNoise`, via `Noises.instantiate`. Each is made once per binder;
    /// the game makes the Nether's anew each time, which gives the same noise.
    fn noise(&mut self, id: &str) -> Result<Arc<NoiseStack>, LoadError> {
        if let Some(n) = self.noises.get(id) {
            return Ok(n.clone());
        }
        let normal = self.normal_noise(id)?;
        let nether = NETHER_BIOME_NOISES.iter().find(|(n, _)| *n == id);
        let stack = match (nether, &self.random) {
            (Some(&(_, offset)), _) => {
                let mut random = LegacyRandomSource::new(self.seed.wrapping_add(offset));
                normal
                    .create_for_legacy_nether_biome(&mut random)
                    .map_err(|e| LoadError::new(id, "", LoadErrorKind::Invalid(e.reason())))?
            }
            (None, WorldRandom::Xoroshiro(f)) => normal.create(&mut f.from_hash_of(id)),
            (None, WorldRandom::Legacy(f)) => normal.create(&mut f.from_hash_of(id)),
        };
        let stack = Arc::new(stack);
        self.noises.insert(id.to_string(), stack.clone());
        Ok(stack)
    }

    /// `DensityFunction.range()`, remembered per function.
    pub(crate) fn range(&mut self, function: &Df) -> Result<Interval, LoadError> {
        let key = Arc::as_ptr(function);
        if let Some((_, r)) = self.ranges.get(&key) {
            return Ok(*r);
        }
        let r = range::of(self, function)?;
        self.ranges.insert(key, (function.clone(), r));
        Ok(r)
    }

    /// A sampler for `node`, with a slot unless it's a constant.
    fn sampler(&mut self, node: Node) -> Sampler {
        let slot = if matches!(node, Node::Constant(_)) {
            NO_SLOT
        } else {
            self.slots += 1;
            self.slots - 1
        };
        Sampler {
            depends_on_y: node.depends_on_y(),
            node: Arc::new(node),
            binder: self.id,
            slot,
        }
    }

    /// Binds `function` (`compileSampler`).
    pub fn bind(&mut self, function: &Df) -> Result<Sampler, LoadError> {
        let key = Arc::as_ptr(function);
        if let Some((_, s)) = self.bound.get(&key) {
            return Ok(s.clone());
        }
        let sampler = match self.compile(function)? {
            Compiled::Node(node) => self.sampler(node),
            Compiled::Alias(sampler) => sampler,
        };
        self.bound.insert(key, (function.clone(), sampler.clone()));
        Ok(sampler)
    }

    /// The node `function` compiles to, or another sampler it compiles to as is.
    fn compile(&mut self, function: &Df) -> Result<Compiled, LoadError> {
        use Compiled::{Alias, Node as N};
        Ok(N(match &**function {
            DensityFunction::Constant(v) => Node::Constant(*v),
            DensityFunction::Noise {
                noise,
                xz_scale,
                y_scale,
                shift_x,
                shift_y,
                shift_z,
            } => {
                let noise = self.noise(noise)?;
                let (xz_scale, y_scale) = (*xz_scale, *y_scale);
                if shift_x.is_zero() && shift_y.is_zero() && shift_z.is_zero() {
                    Node::Noise {
                        noise,
                        xz_scale,
                        y_scale,
                    }
                } else if shift_y.is_zero() {
                    Node::NoiseShiftedXz {
                        noise,
                        xz_scale,
                        y_scale,
                        sx: self.bind(shift_x)?,
                        sz: self.bind(shift_z)?,
                    }
                } else {
                    Node::NoiseShiftedXyz {
                        noise,
                        xz_scale,
                        y_scale,
                        sx: self.bind(shift_x)?,
                        sy: self.bind(shift_y)?,
                        sz: self.bind(shift_z)?,
                    }
                }
            }
            DensityFunction::ShiftA { noise } => Node::ShiftA(self.noise(noise)?),
            DensityFunction::ShiftB { noise } => Node::ShiftB(self.noise(noise)?),
            DensityFunction::Shift { noise } => Node::Shift(self.noise(noise)?),
            DensityFunction::Binary(op, left, right) => {
                return self.compile_binary(*op, left, right);
            }
            DensityFunction::Unary(op, input) => Node::Unary(*op, self.bind(input)?),
            DensityFunction::Round {
                kind,
                input,
                multiple,
            } => {
                let input = self.bind(input)?;
                // A multiple that's the constant 1 skips the division and multiplication.
                if multiple.as_constant() == Some(1.0) {
                    Node::RoundInteger(*kind, input)
                } else {
                    Node::Round(*kind, input, self.bind(multiple)?)
                }
            }
            DensityFunction::Clamp {
                input,
                min,
                max,
                math_clamp,
            } => {
                if *math_clamp {
                    return self.compile_math_clamp(input, *min, *max);
                }
                Node::Clamp(self.bind(input)?, *min, *max)
            }
            DensityFunction::Pow { base, exponent } => {
                return self.compile_pow(base, exponent);
            }
            // The compiler deduplicates caches; on the point path the value is the input's.
            DensityFunction::Cache(input) => return Ok(Alias(self.bind(input)?)),
            // BlendDensityFunction.Sampler: the input, when nothing is blending.
            DensityFunction::BlendDensity(input) => return Ok(Alias(self.bind(input)?)),
            DensityFunction::Lerp {
                alpha,
                first,
                second,
            } => Node::Lerp {
                alpha: self.bind(alpha)?,
                first: self.bind(first)?,
                second: self.bind(second)?,
            },
            DensityFunction::RangeChoice {
                input,
                min_inclusive,
                max_exclusive,
                when_in_range,
                when_out_of_range,
            } => Node::RangeChoice {
                input: self.bind(input)?,
                min_inclusive: *min_inclusive,
                max_exclusive: *max_exclusive,
                when_in_range: self.bind(when_in_range)?,
                when_out_of_range: self.bind(when_out_of_range)?,
            },
            DensityFunction::IntervalSelect {
                input,
                thresholds,
                functions,
            } => Node::IntervalSelect {
                input: self.bind(input)?,
                thresholds: thresholds.clone(),
                samplers: functions
                    .iter()
                    .map(|f| self.bind(f))
                    .collect::<Result<_, _>>()?,
            },
            DensityFunction::Slice {
                axis,
                coordinate,
                input,
            } => Node::Slice {
                axis: *axis,
                coordinate: *coordinate,
                input: self.bind(input)?,
            },
            DensityFunction::FindTopSurface {
                density,
                upper_bound,
                lower_bound,
                cell_height,
            } => Node::FindTopSurface {
                density: self.bind(density)?,
                upper_bound: self.bind(upper_bound)?,
                lower_bound: *lower_bound,
                cell_height: *cell_height,
            },
            DensityFunction::OldBlendedNoise(noise) => return self.compile_blended(noise),
            DensityFunction::Gradient(g) => {
                // compileSampler: both ends resolved, the end moved up one if they meet, and the
                // factor computed once, from the wrapped int difference.
                let from = self.resolve(g.from_coordinate)?;
                let mut to = self.resolve(g.to_coordinate)?;
                if from == to {
                    to = from.wrapping_add(1);
                }
                let range = to.wrapping_sub(from);
                let factor = (g.to_value - g.from_value) / range as f32;
                match g.tiling {
                    Tiling::ClampToEdge => Node::Gradient {
                        axis: g.axis,
                        from,
                        min: from.min(to),
                        max: from.max(to),
                        from_value: g.from_value,
                        factor,
                    },
                    Tiling::Repeat | Tiling::MirroredRepeat => Node::GradientRepeat {
                        axis: g.axis,
                        from,
                        range,
                        from_value: g.from_value,
                        factor,
                        mirrored: g.tiling == Tiling::MirroredRepeat,
                    },
                }
            }
            DensityFunction::Spline(spline) => Node::Spline(self.bind_spline(spline)?),
            // SimpleDensityFunction: context-bound constants, 1 and 0 when nothing blends, and
            // 0 with no structures.
            DensityFunction::BlendAlpha => Node::Constant(1.0),
            DensityFunction::BlendOffset | DensityFunction::Beardifier => Node::Constant(0.0),
            // InterpolatedFunction.compileSampler: the reciprocals are 1.0f / (float) size.
            DensityFunction::Interpolated {
                input,
                cell_size_xz,
                cell_size_y,
            } => Node::Interpolated {
                input: self.bind(input)?,
                cell_xz: *cell_size_xz,
                cell_y: *cell_size_y,
                inv_xz: 1.0 / *cell_size_xz as f32,
                inv_y: 1.0 / *cell_size_y as f32,
            },
            // UpscaleFunction.compileSampler: all three axes nearest is NearestNeighborSampler,
            // anything else the general sampler, with an upscaler for the axes as given.
            DensityFunction::Upscale {
                input,
                x,
                y,
                z,
                offset_by_step,
            } => {
                let input = self.bind(input)?;
                let axes = [*x, *y, *z];
                if axes.iter().all(|a| a.mode == UpscaleMode::NearestNeighbor) {
                    Node::UpscaleNearest {
                        input,
                        cell: axes.map(|a| a.cell_size),
                    }
                } else {
                    let upscaler = Upscaler::new(axes.map(|a| a.mode), axes.map(|a| a.cell_size));
                    Node::Upscale(Box::new(BoundUpscale {
                        input,
                        axes,
                        upscaler,
                        offset_by_step: *offset_by_step,
                    }))
                }
            }
            // DomainWarpFunction.compileSampler: three constant offsets are truncated once;
            // else an input without y is sampled flat; else the general sampler.
            DensityFunction::DomainWarp {
                input,
                offset_x,
                offset_y,
                offset_z,
            } => {
                let axes = domain_axes(function, &mut HashMap::new());
                let flat = domain_axes(input, &mut HashMap::new()) & AXIS_Y == 0;
                let bound = self.bind(input)?;
                let offsets = match [offset_x, offset_y, offset_z].map(|o| o.as_constant()) {
                    // f2i: truncation, saturating, NaN to 0.
                    [Some(x), Some(y), Some(z)] => Warp::Const([x as i32, y as i32, z as i32]),
                    _ if flat => Warp::FlatXz {
                        x: self.bind(offset_x)?,
                        z: self.bind(offset_z)?,
                    },
                    _ => Warp::General {
                        x: self.bind(offset_x)?,
                        y: self.bind(offset_y)?,
                        z: self.bind(offset_z)?,
                    },
                };
                Node::DomainWarp {
                    input: bound,
                    offsets,
                    axes,
                }
            }
            DensityFunction::DistanceToPoint { point, metric } => Node::DistanceToPoint {
                point: *point,
                metric: *metric,
            },
            // EndIslandFunction.compileSampler: createEndIslandRandom is LegacyRandomSource(seed)
            // whatever the settings say, then 17292 draws skipped, then zero-offset simplex.
            DensityFunction::EndOuterIslands => {
                let mut random = LegacyRandomSource::new(self.seed);
                random.consume_count(17_292);
                Node::EndIslands(Arc::new(SimplexNoise::new(&mut random, true)))
            }
        }))
    }

    /// `BinaryFunction.compileSampler`. Both operands are compiled first; then constants and,
    /// for `min` and `max`, ranges choose the sampler.
    fn compile_binary(&mut self, op: Binary, left: &Df, right: &Df) -> Result<Compiled, LoadError> {
        let (a, b) = (self.bind(left)?, self.bind(right)?);
        let (ca, cb) = (left.as_constant(), right.as_constant());
        Ok(Compiled::Node(match op {
            // IEEE addition is commutative, and x − c is x + (−c), so the constant samplers
            // compute the same floats as the general ones.
            Binary::Add => Node::Add(a, b),
            Binary::Sub => Node::Sub(a, b),
            Binary::Mul => match (ca, cb) {
                (Some(c), _) => Node::MulConst(b, c),
                (None, Some(c)) => Node::MulConst(a, c),
                (None, None) => Node::Mul(a, b),
            },
            Binary::Div => match (ca, cb) {
                (Some(c), _) => Node::ConstDiv(c, b),
                (None, Some(c)) => Node::MulConst(a, 1.0 / c),
                (None, None) => Node::Div(a, b),
            },
            Binary::Min => {
                let (ra, rb) = (self.range(left)?, self.range(right)?);
                // An operand that's never above the other is the value.
                if ra.max() < rb.min() {
                    return Ok(Compiled::Alias(a));
                }
                if rb.max() < ra.min() {
                    return Ok(Compiled::Alias(b));
                }
                match (ca, cb) {
                    (Some(c), _) => Node::MinConst(b, c),
                    (None, Some(c)) => Node::MinConst(a, c),
                    (None, None) => Node::Min(a, b, rb.min()),
                }
            }
            Binary::Max => {
                let (ra, rb) = (self.range(left)?, self.range(right)?);
                if ra.min() > rb.max() {
                    return Ok(Compiled::Alias(a));
                }
                if rb.min() > ra.max() {
                    return Ok(Compiled::Alias(b));
                }
                match (ca, cb) {
                    (Some(c), _) => Node::MaxConst(b, c),
                    (None, Some(c)) => Node::MaxConst(a, c),
                    (None, None) => Node::Max(a, b, rb.max()),
                }
            }
        }))
    }

    /// `ClampFunction.compileSampler` under switch `clamp_positive_zero`: by the input's range
    /// `r`, the input itself if the bounds hold all of `r`, a `ConstMaxSampler` with `min` if
    /// only `max` does, a `ConstMinSampler` with `max` if only `min` does, else the clamp, with
    /// `Math.clamp`. A NaN bound of `r` fails every test.
    fn compile_math_clamp(
        &mut self,
        input: &Df,
        min: f32,
        max: f32,
    ) -> Result<Compiled, LoadError> {
        let r = self.range(input)?;
        let s = self.bind(input)?;
        Ok(if min <= r.min() && max >= r.max() {
            Compiled::Alias(s)
        } else if max >= r.max() {
            Compiled::Node(Node::MaxConst(s, min))
        } else if min <= r.min() {
            Compiled::Node(Node::MinConst(s, max))
        } else {
            Compiled::Node(Node::MathClamp(s, min, max))
        })
    }

    /// `PowFunction.compileSampler` and `compileConstExponent`.
    fn compile_pow(&mut self, base: &Df, exponent: &Df) -> Result<Compiled, LoadError> {
        let (b, e) = (self.bind(base)?, self.bind(exponent)?);
        if let Some(c) = base.as_constant() {
            return Ok(Compiled::Node(Node::PowConstBase(f64::from(c), e)));
        }
        let Some(c) = exponent.as_constant() else {
            return Ok(Compiled::Node(Node::Pow(b, e)));
        };
        // |c| of ½, 1, 2 or 3 compiles to a cheaper sampler, then a reciprocal if c < 0.
        let magnitude = c.abs();
        let positive = if magnitude == 0.5 {
            self.sampler(Node::Unary(Unary::Sqrt, b))
        } else if magnitude == 1.0 {
            b
        } else if magnitude == 2.0 {
            self.sampler(Node::Unary(Unary::Square, b))
        } else if magnitude == 3.0 {
            self.sampler(Node::Unary(Unary::Cube, b))
        } else {
            return Ok(Compiled::Node(Node::PowConstExponent(b, f64::from(c))));
        };
        Ok(if c < 0.0 {
            Compiled::Node(Node::Unary(Unary::Reciprocal, positive))
        } else {
            Compiled::Alias(positive)
        })
    }

    /// `BlendedNoise.compileSampler`: `lerp(clamp(main + 0.5, 0, 1), minLimit, maxLimit)`, the
    /// three noises sampled by `NoiseFunction.Sampler`.
    fn compile_blended(&mut self, p: &BlendedNoise) -> Result<Compiled, LoadError> {
        // CompileContext.createRandom(NOISE_SEED): with legacy randomness LegacyRandomSource(seed)
        // itself, else the factory's fromHashOf; then createFbmSet.
        match &self.random {
            WorldRandom::Xoroshiro(f) => {
                let mut random = f.from_hash_of("minecraft:terrain");
                self.compile_blended_with(p, &mut random)
            }
            WorldRandom::Legacy(_) => {
                let mut random = LegacyRandomSource::new(self.seed);
                self.compile_blended_with(p, &mut random)
            }
        }
    }

    fn compile_blended_with(
        &mut self,
        p: &BlendedNoise,
        random: &mut impl RandomSource,
    ) -> Result<Compiled, LoadError> {
        let xz_multiplier = 684.412 * p.xz_scale;
        let y_multiplier = 684.412 * p.y_scale;
        let smear = y_multiplier * p.smear_scale_multiplier;
        let main_smear = smear / p.y_factor;
        let min_limit = create_fbm(random, -15, smear, LIMIT_FACTOR);
        let max_limit = create_fbm(random, -15, smear, LIMIT_FACTOR);
        let main = create_fbm(random, -7, main_smear, 12.75);
        let mut noise = |noise, xz_scale, y_scale| {
            self.sampler(Node::SmearedNoise {
                noise: Arc::new(noise),
                xz_scale,
                y_scale,
            })
        };
        let first = noise(min_limit, xz_multiplier, y_multiplier);
        let second = noise(max_limit, xz_multiplier, y_multiplier);
        let main = noise(main, xz_multiplier / p.xz_factor, y_multiplier / p.y_factor);
        let half = self.sampler(Node::Constant(0.5));
        let shifted = self.sampler(Node::Add(main, half));
        // 26.4 clamps with Math.clamp here, whatever the data says. main + 0.5 is never −0, so
        // that's the same value as Mth.clamp, and switch clamp_positive_zero isn't needed.
        let alpha = self.sampler(Node::Clamp(shifted, 0.0, 1.0));
        Ok(Compiled::Node(Node::Lerp {
            alpha,
            first,
            second,
        }))
    }

    fn bind_spline(&mut self, spline: &Spline) -> Result<BoundSpline, LoadError> {
        Ok(match spline {
            Spline::Constant(v) => BoundSpline::Constant(*v),
            Spline::Multipoint(points) => BoundSpline::Multipoint {
                coordinate: self.bind(points.coordinate())?,
                locations: points.locations().to_vec(),
                values: points
                    .values()
                    .iter()
                    .map(|v| self.bind_spline(v))
                    .collect::<Result<_, _>>()?,
                derivatives: points.derivatives().to_vec(),
                nan_is_nan: points.nan_is_nan(),
            },
        })
    }
}

/// `DistanceMetric.compute` on `(float)(point - pos)` per axis (`DistanceToPointFunction.Sampler`):
/// the subtraction in wrapping `i32`, the metric in `f32` (Euclidean's square root through
/// `f64`).
fn distance(metric: DistanceMetric, point: [i32; 3], pos: [i32; 3]) -> f32 {
    let [dx, dy, dz] = [0, 1, 2].map(|i| point[i].wrapping_sub(pos[i]) as f32);
    match metric {
        // Mth.length: Mth.sqrt(lengthSquared), (float) Math.sqrt((double) f).
        DistanceMetric::Euclidean => f64::from((dx * dx + dy * dy) + dz * dz).sqrt() as f32,
        DistanceMetric::EuclideanSquared => (dx * dx + dy * dy) + dz * dz,
        DistanceMetric::Manhattan => (dx.abs() + dy.abs()) + dz.abs(),
        // Math.max(float, float): NaN can't occur from finite ints.
        DistanceMetric::Chebyshev => dx.abs().max(dy.abs()).max(dz.abs()),
    }
}

/// `EndIslandFunction.getHeightValue(noise, x, z)`, with `x` and `z` a block's divided by 8:
/// the highest outer island over the 25 × 25 cells around `(x / 2, z / 2)`, from −100.
fn end_island_height(noise: &SimplexNoise, x: i32, z: i32) -> f32 {
    let (xs, zs, xr, zr) = (x / 2, z / 2, x % 2, z % 2);
    let mut height = -100.0_f32;
    for dx in -12..=12 {
        for dz in -12..=12 {
            let px = i64::from(xs.wrapping_add(dx));
            let pz = i64::from(zs.wrapping_add(dz));
            // lcmp/ifle: strictly beyond 64 cells; fcmpg/ifge: NaN fails the test.
            if px * px + pz * pz > 4096 && noise.get(px as f64, pz as f64) < -0.9_f32 {
                let size = ((px as f32).abs() * 3439.0 + (pz as f32).abs() * 147.0) % 13.0 + 9.0;
                let fx = xr.wrapping_sub(dx.wrapping_mul(2)) as f32;
                let fz = zr.wrapping_sub(dz.wrapping_mul(2)) as f32;
                // Mth.sqrt(float): (float) Math.sqrt((double) f).
                let length = f64::from(fx * fx + fz * fz).sqrt() as f32;
                let v = 100.0 - length * size;
                // Mth.clamp(float): v < min ? min : Math.min(v, max).
                let v = if v < -100.0 { -100.0 } else { v.min(80.0) };
                height = height.max(v);
            }
        }
    }
    height
}

/// What a function compiles to: a new node, or a sampler that already exists (an input passed
/// through, or a `min` or `max` operand chosen by range).
enum Compiled {
    Node(Node),
    Alias(Sampler),
}

/// `BlendedNoise.LIMIT_FACTOR`.
pub(crate) const LIMIT_FACTOR: f64 = 0.999_984_741_210_937_5;

/// `BlendedNoise.createFbm`: `1 − first_octave` smeared Perlin noises from one random, in order,
/// the first at frequency 1 and each next at half the last, amplitudes doubling from
/// `factor / (2^n − 1)`.
fn create_fbm(
    random: &mut impl RandomSource,
    first_octave: i32,
    smear: f64,
    factor: f64,
) -> NoiseStack<SmearedPerlinNoise> {
    let n = 1 - first_octave;
    let mut frequency = 1.0;
    let mut amplitude = factor / (exp2i(n) - 1.0);
    let layers = (0..n)
        .map(|_| {
            let layer = Layer {
                noise: SmearedPerlinNoise::new(random, smear * frequency),
                frequency,
                amplitude: amplitude as f32,
            };
            frequency /= 2.0;
            amplitude *= 2.0;
            layer
        })
        .collect();
    NoiseStack::new(layers)
}

impl From<IntervalError> for LoadError {
    fn from(e: IntervalError) -> Self {
        LoadError::new("", "", LoadErrorKind::Invalid(e.reason()))
    }
}

/// Remembered values, valid for one binder at one column and one y. Stamps tell the current
/// column's and point's entries from stale ones, so moving on never clears anything.
#[derive(Default)]
pub(crate) struct Memo {
    binder: u64,
    column: Option<(i32, i32)>,
    y: Option<i32>,
    stamps: u64,
    column_stamp: u64,
    point_stamp: u64,
    /// By slot: the stamp the value was computed under, and the value. Stamp 0 is never valid.
    entries: Vec<(u64, f32)>,
}

/// Where a [`Memo`] was, to go back after evaluating a child somewhere else.
struct Place {
    binder: u64,
    column: Option<(i32, i32)>,
    y: Option<i32>,
    column_stamp: u64,
    point_stamp: u64,
}

impl Memo {
    /// Starts evaluating `binder`'s samplers at `(x, y, z)`.
    fn begin(&mut self, binder: u64, x: i32, y: i32, z: i32) {
        if binder != self.binder || self.column != Some((x, z)) {
            self.binder = binder;
            self.column = Some((x, z));
            self.y = None;
            self.stamps += 1;
            self.column_stamp = self.stamps;
        }
        if self.y != Some(y) {
            self.y = Some(y);
            self.stamps += 1;
            self.point_stamp = self.stamps;
        }
    }

    fn place(&self) -> Place {
        Place {
            binder: self.binder,
            column: self.column,
            y: self.y,
            column_stamp: self.column_stamp,
            point_stamp: self.point_stamp,
        }
    }

    /// Returns to `place`. Values computed elsewhere in between carry other stamps, so they're
    /// never mistaken for this place's.
    fn restore(&mut self, place: Place) {
        self.binder = place.binder;
        self.column = place.column;
        self.y = place.y;
        self.column_stamp = place.column_stamp;
        self.point_stamp = place.point_stamp;
    }
}

thread_local! {
    pub(crate) static MEMO: RefCell<Memo> = RefCell::new(Memo::default());
}

/// A block position, in both the `int` and `double` forms the samplers use.
#[derive(Clone, Copy)]
pub(crate) struct Pos {
    x: i32,
    y: i32,
    z: i32,
    xd: f64,
    yd: f64,
    zd: f64,
}

impl Pos {
    pub(crate) fn new(x: i32, y: i32, z: i32) -> Self {
        Pos {
            x,
            y,
            z,
            xd: f64::from(x),
            yd: f64::from(y),
            zd: f64::from(z),
        }
    }
}

impl Sampler {
    /// Whether the value can depend on y. When this is false, the value is the same at every y
    /// for a given x and z, bit for bit (see `docs/specs/density-functions.md`).
    pub fn depends_on_y(&self) -> bool {
        self.depends_on_y
    }

    /// `sampleValue` at block `(x, y, z)`.
    ///
    /// Values are remembered per thread while calls stay in one column: sampling a column top to
    /// bottom, or several functions bound by one [`Binder`] at the same position, computes each
    /// shared or y-independent node once.
    pub fn sample(&self, x: i32, y: i32, z: i32) -> f32 {
        #[cfg(feature = "timing")]
        let _t = crate::timing::span("point: sample");
        MEMO.with_borrow_mut(|memo| {
            memo.begin(self.binder, x, y, z);
            self.eval(memo, Pos::new(x, y, z))
        })
    }

    /// The value at `pos`, remembered in its slot.
    pub(crate) fn eval(&self, memo: &mut Memo, pos: Pos) -> f32 {
        #[cfg(feature = "timing")]
        let _t = crate::timing::detail(self.node.point_kind());
        if self.slot == NO_SLOT {
            return self.node.compute(memo, pos);
        }
        let slot = self.slot as usize;
        let stamp = if self.depends_on_y {
            memo.point_stamp
        } else {
            memo.column_stamp
        };
        if let Some(&(s, value)) = memo.entries.get(slot)
            && s == stamp
        {
            return value;
        }
        let value = self.node.compute(memo, pos);
        if memo.entries.len() <= slot {
            memo.entries.resize(slot + 1, (0, 0.0));
        }
        memo.entries[slot] = (stamp, value);
        value
    }

    /// The value at another position than the one being evaluated, for samplers that move their
    /// inputs (slices, `find_top_surface`).
    pub(crate) fn eval_at(&self, memo: &mut Memo, x: i32, y: i32, z: i32) -> f32 {
        #[cfg(feature = "timing")]
        let _t = crate::timing::span("point: moved (slice, find_top_surface)");
        let place = memo.place();
        memo.begin(self.binder, x, y, z);
        let value = self.eval(memo, Pos::new(x, y, z));
        memo.restore(place);
        value
    }
}

impl Node {
    /// The node's type, for [`timing`](crate::timing) on the volume path.
    #[cfg(feature = "timing")]
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Node::Constant(..) => "volume: Constant",
            Node::Noise { .. } => "volume: Noise",
            Node::NoiseShiftedXz { .. } => "volume: NoiseShiftedXz",
            Node::NoiseShiftedXyz { .. } => "volume: NoiseShiftedXyz",
            Node::SmearedNoise { .. } => "volume: SmearedNoise",
            Node::ShiftA(..) => "volume: ShiftA",
            Node::ShiftB(..) => "volume: ShiftB",
            Node::Shift(..) => "volume: Shift",
            Node::Add(..) => "volume: Add",
            Node::Sub(..) => "volume: Sub",
            Node::Mul(..) => "volume: Mul",
            Node::MulConst(..) => "volume: MulConst",
            Node::Div(..) => "volume: Div",
            Node::ConstDiv(..) => "volume: ConstDiv",
            Node::Min(..) => "volume: Min",
            Node::MinConst(..) => "volume: MinConst",
            Node::Max(..) => "volume: Max",
            Node::MaxConst(..) => "volume: MaxConst",
            Node::Unary(..) => "volume: Unary",
            Node::Round(..) => "volume: Round",
            Node::RoundInteger(..) => "volume: RoundInteger",
            Node::Clamp(..) => "volume: Clamp",
            Node::MathClamp(..) => "volume: MathClamp",
            Node::Pow(..) => "volume: Pow",
            Node::PowConstBase(..) => "volume: PowConstBase",
            Node::PowConstExponent(..) => "volume: PowConstExponent",
            Node::Lerp { .. } => "volume: Lerp",
            Node::RangeChoice { .. } => "volume: RangeChoice",
            Node::IntervalSelect { .. } => "volume: IntervalSelect",
            Node::Slice { .. } => "volume: Slice",
            Node::Interpolated { .. } => "volume: Interpolated",
            Node::Upscale(..) => "volume: Upscale",
            Node::UpscaleNearest { .. } => "volume: UpscaleNearest",
            Node::FindTopSurface { .. } => "volume: FindTopSurface",
            Node::Gradient { .. } => "volume: Gradient",
            Node::GradientRepeat { .. } => "volume: GradientRepeat",
            Node::Spline(..) => "volume: Spline",
            Node::DistanceToPoint { .. } => "volume: DistanceToPoint",
            Node::EndIslands(..) => "volume: EndIslands",
            Node::DomainWarp { .. } => "volume: DomainWarp",
        }
    }

    /// The node's type, for [`timing`](crate::timing) on the point path.
    #[cfg(feature = "timing")]
    pub(crate) fn point_kind(&self) -> &'static str {
        match self {
            Node::Constant(..) => "point: Constant",
            Node::Noise { .. } => "point: Noise",
            Node::NoiseShiftedXz { .. } => "point: NoiseShiftedXz",
            Node::NoiseShiftedXyz { .. } => "point: NoiseShiftedXyz",
            Node::SmearedNoise { .. } => "point: SmearedNoise",
            Node::ShiftA(..) => "point: ShiftA",
            Node::ShiftB(..) => "point: ShiftB",
            Node::Shift(..) => "point: Shift",
            Node::Add(..) => "point: Add",
            Node::Sub(..) => "point: Sub",
            Node::Mul(..) => "point: Mul",
            Node::MulConst(..) => "point: MulConst",
            Node::Div(..) => "point: Div",
            Node::ConstDiv(..) => "point: ConstDiv",
            Node::Min(..) => "point: Min",
            Node::MinConst(..) => "point: MinConst",
            Node::Max(..) => "point: Max",
            Node::MaxConst(..) => "point: MaxConst",
            Node::Unary(..) => "point: Unary",
            Node::Round(..) => "point: Round",
            Node::RoundInteger(..) => "point: RoundInteger",
            Node::Clamp(..) => "point: Clamp",
            Node::MathClamp(..) => "point: MathClamp",
            Node::Pow(..) => "point: Pow",
            Node::PowConstBase(..) => "point: PowConstBase",
            Node::PowConstExponent(..) => "point: PowConstExponent",
            Node::Lerp { .. } => "point: Lerp",
            Node::RangeChoice { .. } => "point: RangeChoice",
            Node::IntervalSelect { .. } => "point: IntervalSelect",
            Node::Slice { .. } => "point: Slice",
            Node::Interpolated { .. } => "point: Interpolated",
            Node::Upscale(..) => "point: Upscale",
            Node::UpscaleNearest { .. } => "point: UpscaleNearest",
            Node::FindTopSurface { .. } => "point: FindTopSurface",
            Node::Gradient { .. } => "point: Gradient",
            Node::GradientRepeat { .. } => "point: GradientRepeat",
            Node::Spline(..) => "point: Spline",
            Node::DistanceToPoint { .. } => "point: DistanceToPoint",
            Node::EndIslands(..) => "point: EndIslands",
            Node::DomainWarp { .. } => "point: DomainWarp",
        }
    }

    /// The node's value at `pos`, its children's through `memo`.
    pub(crate) fn compute(&self, memo: &mut Memo, pos: Pos) -> f32 {
        let Pos {
            x,
            y,
            z,
            xd,
            yd,
            zd,
        } = pos;
        match self {
            Node::Constant(v) => *v,
            Node::Noise {
                noise,
                xz_scale,
                y_scale,
            } => noise.get(xd * xz_scale, yd * y_scale, zd * xz_scale),
            Node::NoiseShiftedXz {
                noise,
                xz_scale,
                y_scale,
                sx,
                sz,
            } => noise.get(
                xd * xz_scale + f64::from(sx.eval(memo, pos)),
                yd * y_scale,
                zd * xz_scale + f64::from(sz.eval(memo, pos)),
            ),
            Node::NoiseShiftedXyz {
                noise,
                xz_scale,
                y_scale,
                sx,
                sy,
                sz,
            } => noise.get(
                xd * xz_scale + f64::from(sx.eval(memo, pos)),
                yd * y_scale + f64::from(sy.eval(memo, pos)),
                zd * xz_scale + f64::from(sz.eval(memo, pos)),
            ),
            Node::SmearedNoise {
                noise,
                xz_scale,
                y_scale,
            } => noise.get(xd * xz_scale, yd * y_scale, zd * xz_scale),
            // ShiftA: NoiseFunction.Sampler(noise, 0.25, 0.0) times 4.
            Node::ShiftA(noise) => noise.get(xd * 0.25, yd * 0.0, zd * 0.25) * 4.0,
            // ShiftB: the noise at (z/4, x/4, 0) times 4.
            Node::ShiftB(noise) => noise.get(zd * 0.25, xd * 0.25, 0.0) * 4.0,
            Node::Shift(noise) => noise.get(xd * 0.25, yd * 0.25, zd * 0.25) * 4.0,
            Node::Add(a, b) => a.eval(memo, pos) + b.eval(memo, pos),
            Node::Sub(a, b) => a.eval(memo, pos) - b.eval(memo, pos),
            // MulSampler and DivSampler: a zero left operand (either sign) gives +0.0, and the
            // right isn't evaluated.
            Node::Mul(a, b) => {
                let left = a.eval(memo, pos);
                if left == 0.0 {
                    0.0
                } else {
                    left * b.eval(memo, pos)
                }
            }
            Node::MulConst(a, c) => a.eval(memo, pos) * c,
            Node::Div(a, b) => {
                let left = a.eval(memo, pos);
                if left == 0.0 {
                    0.0
                } else {
                    left / b.eval(memo, pos)
                }
            }
            Node::ConstDiv(c, b) => c / b.eval(memo, pos),
            // MinSampler: the left value if it's at or below anything the right can be.
            Node::Min(a, b, right_min) => {
                let left = a.eval(memo, pos);
                if left <= *right_min {
                    left
                } else {
                    java::min(left, b.eval(memo, pos))
                }
            }
            Node::MinConst(a, c) => java::min(a.eval(memo, pos), *c),
            Node::Max(a, b, right_max) => {
                let left = a.eval(memo, pos);
                if left >= *right_max {
                    left
                } else {
                    java::max(left, b.eval(memo, pos))
                }
            }
            Node::MaxConst(a, c) => java::max(a.eval(memo, pos), *c),
            Node::Unary(op, a) => unary(*op, a.eval(memo, pos)),
            Node::Round(kind, input, multiple) => {
                let v = input.eval(memo, pos);
                round_to_multiple(v, multiple.eval(memo, pos), *kind)
            }
            Node::RoundInteger(kind, input) => round_to_integer(input.eval(memo, pos), *kind),
            Node::Clamp(input, min, max) => clamp(input.eval(memo, pos), *min, *max),
            Node::MathClamp(input, min, max) => java::clamp(input.eval(memo, pos), *min, *max),
            Node::Pow(base, exponent) => {
                let b = f64::from(base.eval(memo, pos));
                java_pow(b, f64::from(exponent.eval(memo, pos)))
            }
            Node::PowConstBase(base, exponent) => {
                java_pow(*base, f64::from(exponent.eval(memo, pos)))
            }
            Node::PowConstExponent(base, exponent) => {
                java_pow(f64::from(base.eval(memo, pos)), *exponent)
            }
            // LerpFunction.Sampler: exactly first at 0 and second at 1, Mth.lerp in between.
            Node::Lerp {
                alpha,
                first,
                second,
            } => {
                let t = alpha.eval(memo, pos);
                if t == 0.0 {
                    first.eval(memo, pos)
                } else if t == 1.0 {
                    second.eval(memo, pos)
                } else {
                    let a = first.eval(memo, pos);
                    a + t * (second.eval(memo, pos) - a)
                }
            }
            // In range when min ≤ v < max; NaN isn't. Only the chosen branch is evaluated.
            Node::RangeChoice {
                input,
                min_inclusive,
                max_exclusive,
                when_in_range,
                when_out_of_range,
            } => {
                let v = input.eval(memo, pos);
                if v >= *min_inclusive && v < *max_exclusive {
                    when_in_range.eval(memo, pos)
                } else {
                    when_out_of_range.eval(memo, pos)
                }
            }
            // selectSamplerIndex: the first threshold above v, else the last function (NaN
            // included).
            Node::IntervalSelect {
                input,
                thresholds,
                samplers,
            } => {
                let v = input.eval(memo, pos);
                let i = thresholds
                    .iter()
                    .position(|t| v < *t)
                    .unwrap_or(samplers.len() - 1);
                samplers[i].eval(memo, pos)
            }
            Node::Slice {
                axis,
                coordinate,
                input,
            } => match axis {
                Axis::X => input.eval_at(memo, *coordinate, y, z),
                Axis::Y => input.eval_at(memo, x, *coordinate, z),
                Axis::Z => input.eval_at(memo, x, y, *coordinate),
            },
            // InterpolatedFunction.Sampler.sampleValue: the input itself on a cell corner, else
            // the cell's corners as a 2 × 2 × 2 volume, interpolated by Mth.lerp3 with each t a
            // division.
            Node::Interpolated {
                input,
                cell_xz,
                cell_y,
                ..
            } => {
                let (cxz, cy) = (*cell_xz, *cell_y);
                let (fx, fy, fz) = (
                    java::floor_mod(x, cxz),
                    java::floor_mod(y, cy),
                    java::floor_mod(z, cxz),
                );
                if fx == 0 && fy == 0 && fz == 0 {
                    return input.eval(memo, pos);
                }
                let corners = Volume::new(
                    [2, 2, 2],
                    [x.wrapping_sub(fx), y.wrapping_sub(fy), z.wrapping_sub(fz)],
                    [cxz, cy, cxz],
                );
                let c = input.volume_in(memo, &corners);
                let at = |x, y, z| c[corners.index(x, y, z)];
                mth::lerp3(
                    fx as f32 / cxz as f32,
                    fy as f32 / cy as f32,
                    fz as f32 / cxz as f32,
                    at(0, 0, 0),
                    at(1, 0, 0),
                    at(0, 1, 0),
                    at(1, 1, 0),
                    at(0, 0, 1),
                    at(1, 0, 1),
                    at(0, 1, 1),
                    at(1, 1, 1),
                )
            }
            // UpscaleFunction.Sampler.sampleValue: the input itself on a corner of every axis
            // whose mode is exact there. Otherwise the corners around the cell, sampleCount per
            // axis a cell apart, through the input's volume path, then upscaleValue: z, then x,
            // then y, with each axis's coefficients at its offset in the cell.
            Node::Upscale(u) => {
                let [ax, ay, az] = u.axes;
                let f = [
                    java::floor_mod(x, ax.cell_size),
                    java::floor_mod(y, ay.cell_size),
                    java::floor_mod(z, az.cell_size),
                ];
                if f == [0, 0, 0] && u.axes.iter().all(|a| a.mode.exact_on_corner()) {
                    return u.input.eval(memo, pos);
                }
                let first = |c: i32, f: i32, a: UpscaleAxis| {
                    c.wrapping_sub(f)
                        .wrapping_sub(a.mode.samples_before().wrapping_mul(a.cell_size))
                };
                let corners = Volume::new(
                    u.axes.map(|a| a.mode.sample_count()),
                    [first(x, f[0], ax), first(y, f[1], ay), first(z, f[2], az)],
                    u.axes.map(|a| a.cell_size),
                );
                let samples = u.input.volume_in(memo, &corners);
                u.upscaler.value(f, &samples)
            }
            // NearestNeighborSampler.sampleValue: the input at the cell's corner.
            Node::UpscaleNearest { input, cell } => {
                let corner = |c: i32, size: i32| java::floor_div(c, size).wrapping_mul(size);
                input.eval_at(
                    memo,
                    corner(x, cell[0]),
                    corner(y, cell[1]),
                    corner(z, cell[2]),
                )
            }
            // FindTopSurfaceFunction.Sampler, inside a slice at y 0: the upper bound at
            // (x, 0, z), then findSurfaceFrom.
            Node::FindTopSurface {
                density,
                upper_bound,
                lower_bound,
                cell_height,
            } => {
                let upper = upper_bound.eval_at(memo, x, 0, z);
                find_surface(memo, density, x, z, upper, *lower_bound, *cell_height)
            }
            Node::DistanceToPoint { point, metric } => distance(*metric, *point, [x, y, z]),
            // EndIslandFunction.Sampler.sampleValue: (getHeightValue(x / 8, z / 8) - 8) / 128.
            Node::EndIslands(noise) => (end_island_height(noise, x / 8, z / 8) - 8.0) / 128.0,
            // Each offset at the position, truncated (f2i), then the input where they point.
            Node::DomainWarp { input, offsets, .. } => match offsets {
                Warp::Const([ox, oy, oz]) => input.eval_at(
                    memo,
                    x.wrapping_add(*ox),
                    y.wrapping_add(*oy),
                    z.wrapping_add(*oz),
                ),
                Warp::FlatXz { x: sx, z: sz } => {
                    let ox = sx.eval(memo, pos) as i32;
                    let oz = sz.eval(memo, pos) as i32;
                    input.eval_at(memo, x.wrapping_add(ox), 0, z.wrapping_add(oz))
                }
                Warp::General {
                    x: sx,
                    y: sy,
                    z: sz,
                } => {
                    let ox = sx.eval(memo, pos) as i32;
                    let oy = sy.eval(memo, pos) as i32;
                    let oz = sz.eval(memo, pos) as i32;
                    input.eval_at(
                        memo,
                        x.wrapping_add(ox),
                        y.wrapping_add(oy),
                        z.wrapping_add(oz),
                    )
                }
            },
            // ClampedSampler.compute: fromValue + (clamp(c, min, max) - from) * factor.
            Node::Gradient {
                axis,
                from,
                min,
                max,
                from_value,
                factor,
            } => {
                let c = axis.choose(x, y, z);
                let clamped = if c < *min { *min } else { c.min(*max) };
                from_value + clamped.wrapping_sub(*from) as f32 * factor
            }
            // RepeatSampler and MirroredRepeatSampler.compute, with Java's floorMod and
            // floorDiv, which follow the sign of the range.
            Node::GradientRepeat {
                axis,
                from,
                range,
                from_value,
                factor,
                mirrored,
            } => {
                let d = axis.choose(x, y, z).wrapping_sub(*from);
                if !mirrored {
                    return from_value + java::floor_mod(d, *range) as f32 * factor;
                }
                let q = java::floor_div(d, *range);
                let rem = d.wrapping_sub(q.wrapping_mul(*range));
                if q & 1 == 0 {
                    from_value + rem as f32 * factor
                } else {
                    from_value + range.wrapping_sub(rem) as f32 * factor
                }
            }
            Node::Spline(spline) => spline.sample(memo, pos),
        }
    }

    /// Whether the value can depend on y, from the children's flags.
    ///
    /// A noise's y input only reaches the noise as `wrap(y * frequency) + offset`, with an offset
    /// from `nextDouble() * 256`, never `-0.0`. With a y scale of 0, `y * 0.0` is `+0.0` or `-0.0`
    /// depending on the sign of y, and adding the offset makes the two the same. So a noise with
    /// a y scale of 0 and a y-independent y shift doesn't depend on y.
    fn depends_on_y(&self) -> bool {
        match self {
            Node::Constant(_) | Node::ShiftA(_) | Node::ShiftB(_) => false,
            Node::Shift(_) => true,
            Node::Noise { y_scale, .. } | Node::SmearedNoise { y_scale, .. } => *y_scale != 0.0,
            Node::NoiseShiftedXz {
                y_scale, sx, sz, ..
            } => *y_scale != 0.0 || sx.depends_on_y || sz.depends_on_y,
            Node::NoiseShiftedXyz {
                y_scale,
                sx,
                sy,
                sz,
                ..
            } => *y_scale != 0.0 || sx.depends_on_y || sy.depends_on_y || sz.depends_on_y,
            Node::Add(a, b)
            | Node::Sub(a, b)
            | Node::Mul(a, b)
            | Node::Div(a, b)
            | Node::Min(a, b, _)
            | Node::Max(a, b, _)
            | Node::Round(_, a, b)
            | Node::Pow(a, b) => a.depends_on_y || b.depends_on_y,
            Node::MulConst(a, _)
            | Node::ConstDiv(_, a)
            | Node::MinConst(a, _)
            | Node::MaxConst(a, _)
            | Node::Unary(_, a)
            | Node::RoundInteger(_, a)
            | Node::Clamp(a, _, _)
            | Node::MathClamp(a, _, _)
            | Node::PowConstBase(_, a)
            | Node::PowConstExponent(a, _) => a.depends_on_y,
            Node::Lerp {
                alpha,
                first,
                second,
            } => alpha.depends_on_y || first.depends_on_y || second.depends_on_y,
            Node::RangeChoice {
                input,
                when_in_range,
                when_out_of_range,
                ..
            } => input.depends_on_y || when_in_range.depends_on_y || when_out_of_range.depends_on_y,
            Node::IntervalSelect {
                input, samplers, ..
            } => input.depends_on_y || samplers.iter().any(|s| s.depends_on_y),
            Node::Slice { axis, input, .. } => *axis != Axis::Y && input.depends_on_y,
            Node::Interpolated { input, .. } => input.depends_on_y,
            // With y cells over 1, equal corners along y still give values that depend on the
            // offset in the cell ((1 − t)·a + t·a isn't always a) and on whether a position is on
            // a corner, so only a y cell of 1 passes the input's flag through.
            Node::Upscale(u) => u.input.depends_on_y || u.axes[1].cell_size != 1,
            Node::UpscaleNearest { input, cell } => input.depends_on_y || cell[1] != 1,
            Node::FindTopSurface { .. } => false,
            Node::Gradient { axis, .. } | Node::GradientRepeat { axis, .. } => *axis == Axis::Y,
            Node::DistanceToPoint { .. } => true,
            Node::EndIslands(_) => false,
            Node::Spline(spline) => spline.depends_on_y(),
            // The game's own axes, so that it's flattened along y exactly when the game slices
            // it there: what it computes over a volume depends on the volume's size.
            Node::DomainWarp { axes, .. } => axes & AXIS_Y != 0,
        }
    }
}

/// `FindTopSurfaceFunction.Sampler.findSurfaceFrom`: from the cell below `upper` down to
/// `lower`, the first y where `density`, sampled at a point, is positive.
pub(crate) fn find_surface(
    memo: &mut Memo,
    density: &Sampler,
    x: i32,
    z: i32,
    upper: f32,
    lower: i32,
    cell_height: i32,
) -> f32 {
    let start = floor_f32(upper / cell_height as f32).wrapping_mul(cell_height);
    if start <= lower {
        return lower as f32;
    }
    let mut y = start;
    while y >= lower {
        if density.eval_at(memo, x, y, z) > 0.0 {
            return y as f32;
        }
        y = y.wrapping_sub(cell_height);
    }
    lower as f32
}

/// `UnaryFunction`'s samplers.
pub(crate) fn unary(op: Unary, x: f32) -> f32 {
    match op {
        Unary::Abs => x.abs(),
        // Mth.square and Mth.cube: (x·x)·x.
        Unary::Square => x * x,
        Unary::Cube => x * x * x,
        // Mth.sqrt: through double.
        Unary::Sqrt => f64::from(x).sqrt() as f32,
        // LeakyReLUSampler.apply: positive values pass, others are scaled.
        Unary::HalfNegative => leaky_relu(0.5, x),
        Unary::QuarterNegative => leaky_relu(0.25, x),
        Unary::Reciprocal => 1.0 / x,
        Unary::Negate => -x,
        Unary::Squeeze => squeeze(x),
        Unary::Log => f64::from(x).ln() as f32,
        Unary::Sign => java::signum(x),
    }
}

/// `UnaryFunction.LeakyReLUSampler.apply`.
pub(crate) fn leaky_relu(negative_factor: f32, x: f32) -> f32 {
    if x > 0.0 { x } else { x * negative_factor }
}

/// `UnaryFunction.SqueezeSampler.apply`: `c/2 − c³/24` with `c` clamped to `[−1, 1]`.
pub(crate) fn squeeze(x: f32) -> f32 {
    let c = clamp(x, -1.0, 1.0);
    c / 2.0 - c * c * c / 24.0
}

/// `Mth.clamp(float, float, float)`: `min` if below it, else `Math.min(value, max)`.
pub(crate) fn clamp(value: f32, min: f32, max: f32) -> f32 {
    if value < min {
        min
    } else {
        java::min(value, max)
    }
}

/// `Mth.floor(float)`: through `double`, saturating.
fn floor_f32(value: f32) -> i32 {
    f64::from(value).floor() as i32
}

/// `RoundFunction.Sampler.apply`: `x` if the multiple is 0, else `round(x / m) · m`.
pub(crate) fn round_to_multiple(x: f32, m: f32, kind: Round) -> f32 {
    if m == 0.0 {
        x
    } else {
        round_to_integer(x / m, kind) * m
    }
}

/// `RoundFunction.roundToInteger`: each rounding through `double`, except `round`, which is
/// `Math.round(float)` to an int.
pub(crate) fn round_to_integer(x: f32, kind: Round) -> f32 {
    match kind {
        Round::Floor => f64::from(x).floor() as f32,
        Round::Round => java::round(x) as f32,
        Round::Ceil => f64::from(x).ceil() as f32,
        Round::Truncate if x > 0.0 => f64::from(x).floor() as f32,
        Round::Truncate => f64::from(x).ceil() as f32,
    }
}

/// `(float) Math.pow(base, exponent)`.
pub(crate) fn java_pow(base: f64, exponent: f64) -> f32 {
    base.powf(exponent) as f32
}

impl BoundSpline {
    fn depends_on_y(&self) -> bool {
        match self {
            BoundSpline::Constant(_) => false,
            BoundSpline::Multipoint {
                coordinate, values, ..
            } => coordinate.depends_on_y || values.iter().any(BoundSpline::depends_on_y),
        }
    }

    /// `CubicSpline.Multipoint.sample` at a point.
    fn sample(&self, memo: &mut Memo, pos: Pos) -> f32 {
        self.sample_with(&mut |s: &Sampler| s.eval(memo, pos))
    }

    /// `CubicSpline.Multipoint.sample`, all in `f32`, with each coordinate's value from
    /// `coordinate_value`. Only the coordinates of points the value reaches are asked for.
    pub(crate) fn sample_with(&self, coordinate_value: &mut dyn FnMut(&Sampler) -> f32) -> f32 {
        let (coordinate, locations, values, derivatives, nan_is_nan) = match self {
            BoundSpline::Constant(v) => return *v,
            BoundSpline::Multipoint {
                coordinate,
                locations,
                values,
                derivatives,
                nan_is_nan,
            } => (coordinate, locations, values, derivatives, *nan_is_nan),
        };
        let c = coordinate_value(coordinate);
        if nan_is_nan {
            return sample_nan_is_nan(c, locations, values, derivatives, coordinate_value);
        }
        sample_multipoint(c, locations, values, derivatives, &mut |v: &BoundSpline| {
            v.sample_with(coordinate_value)
        })
    }
}

/// `CubicSpline.Multipoint.sample` at coordinate value `c`, all in `f32`, with each point's
/// value from `value`, asked for only the points the result reaches. The bytecode is the same in
/// 26.2 and 26.3, so both families share it.
pub(crate) fn sample_multipoint<V>(
    c: f32,
    locations: &[f32],
    values: &[V],
    derivatives: &[f32],
    mut value: impl FnMut(&V) -> f32,
) -> f32 {
    // findIntervalStart: Mth.binarySearch for the first location above c, minus one. The
    // game's test is `c < location`, so a NaN coordinate finds no location and extends past
    // the last point; the negation keeps that.
    #[allow(
        clippy::neg_cmp_op_on_partial_ord,
        reason = "mirrors the game's NaN behavior"
    )]
    let i = locations.partition_point(|&l| !(c < l)) as isize - 1;
    let last = locations.len() - 1;
    // linearExtend outside the points.
    let mut extend = |i: usize| {
        let v = value(&values[i]);
        let d = derivatives[i];
        if d == 0.0 {
            v
        } else {
            v + d * (c - locations[i])
        }
    };
    if i < 0 {
        return extend(0);
    }
    let i = i as usize;
    if i == last {
        return extend(last);
    }
    let (x0, x1) = (locations[i], locations[i + 1]);
    let t = (c - x0) / (x1 - x0);
    let f0 = value(&values[i]);
    let f1 = value(&values[i + 1]);
    let a = derivatives[i] * (x1 - x0) - (f1 - f0);
    let b = -derivatives[i + 1] * (x1 - x0) + (f1 - f0);
    let lerp = |t: f32, p: f32, q: f32| p + t * (q - p);
    lerp(t, f0, f1) + t * (1.0 - t) * lerp(t, a, b)
}

/// 26.4's `CubicSpline.Multipoint.sample` (switch `spline_nan_is_nan`), at coordinate value `c`:
/// NaN for a NaN `c`; the first point, extended, at or before the first location, and the last
/// at or after the last; else the interval a scan from the second location finds, with the same
/// formula as 26.3. See `docs/specs/density-functions.md#types`.
fn sample_nan_is_nan(
    c: f32,
    locations: &[f32],
    values: &[BoundSpline],
    derivatives: &[f32],
    coordinate_value: &mut dyn FnMut(&Sampler) -> f32,
) -> f32 {
    if c.is_nan() {
        return f32::NAN;
    }
    let last = locations.len() - 1;
    // linearExtend: the point's value, plus its derivative times the distance if that's not 0.
    let mut extend = |i: usize| {
        let value = values[i].sample_with(coordinate_value);
        let d = derivatives[i];
        if d == 0.0 {
            value
        } else {
            value + d * (c - locations[i])
        }
    };
    if c <= locations[0] {
        return extend(0);
    }
    if c >= locations[last] {
        return extend(last);
    }
    let mut i = 1;
    while i < last && c >= locations[i] {
        i += 1;
    }
    let j = i - 1;
    let (x0, x1) = (locations[j], locations[i]);
    let f0 = values[j].sample_with(coordinate_value);
    let f1 = values[i].sample_with(coordinate_value);
    let (dx, df) = (x1 - x0, f1 - f0);
    let t = (c - x0) / dx;
    let a = derivatives[j] * dx - df;
    let b = -derivatives[i] * dx + df;
    let lerp = |t: f32, p: f32, q: f32| p + t * (q - p);
    lerp(t, f0, f1) + t * (1.0 - t) * lerp(t, a, b)
}
