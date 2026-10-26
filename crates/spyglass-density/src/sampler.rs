//! Density functions bound to a seed, evaluated at a block position as the game's compiled
//! samplers do (`DensitySampler.sampleValue`). Each case cites the sampler it follows.

use std::collections::HashMap;
use std::sync::Arc;

use spyglass_noise::{NoiseError, NoiseStack, NormalNoise, NormalNoiseParameters};
use spyglass_rng::{
    PositionalRandomFactory, RandomSource, XoroshiroPositionalRandomFactory, XoroshiroRandomSource,
};

use crate::error::{LoadError, LoadErrorKind};
use crate::function::{Axis, DensityFunction, Df, Spline};

/// A density function bound to a seed's noises, ready to evaluate.
#[derive(Debug, Clone)]
pub struct Sampler {
    node: Arc<Node>,
    /// Whether the value can depend on y; see [`Sampler::depends_on_y`].
    y: bool,
}

#[derive(Debug)]
enum Node {
    Constant(f32),
    /// `NoiseFunction.Sampler`: no shifts.
    Noise {
        noise: Arc<NoiseStack>,
        xz: f64,
        y: f64,
    },
    /// `NoiseFunction.ShiftedXzSampler`: `shift_y` is zero.
    NoiseShiftedXz {
        noise: Arc<NoiseStack>,
        xz: f64,
        y: f64,
        sx: Sampler,
        sz: Sampler,
    },
    /// `NoiseFunction.ShiftedXyzSampler`.
    NoiseShiftedXyz {
        noise: Arc<NoiseStack>,
        xz: f64,
        y: f64,
        sx: Sampler,
        sy: Sampler,
        sz: Sampler,
    },
    ShiftA(Arc<NoiseStack>),
    ShiftB(Arc<NoiseStack>),
    Shift(Arc<NoiseStack>),
    Add(Sampler, Sampler),
    Mul(Sampler, Sampler),
    Abs(Sampler),
    Lerp {
        alpha: Sampler,
        first: Sampler,
        second: Sampler,
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
    Spline(BoundSpline),
}

#[derive(Debug)]
enum BoundSpline {
    Constant(f32),
    Multipoint {
        coordinate: Sampler,
        locations: Vec<f32>,
        values: Vec<BoundSpline>,
        derivatives: Vec<f32>,
    },
}

/// Binds functions for one seed. Shared nodes stay shared, and each noise is created once, as
/// `RandomState.getOrCreateNoise` does.
pub struct Binder<'n> {
    noise_parameters: &'n HashMap<String, NormalNoiseParameters>,
    random: XoroshiroPositionalRandomFactory,
    noises: HashMap<String, Arc<NoiseStack>>,
    /// Samplers by function identity. Each entry keeps its function alive, so its address can't
    /// be reused by another function while the binder holds it.
    bound: HashMap<*const DensityFunction, (Df, Sampler)>,
}

impl<'n> Binder<'n> {
    /// A binder for `seed`, with noises from `noise_parameters` (full id → parameters). Only the
    /// Xoroshiro random source (`legacy_random_source: false`) is supported.
    pub fn new(noise_parameters: &'n HashMap<String, NormalNoiseParameters>, seed: i64) -> Self {
        Binder {
            noise_parameters,
            // RandomState: XOROSHIRO.newInstance(seed).forkPositional().
            random: XoroshiroRandomSource::new(seed).fork_positional(),
            noises: HashMap::new(),
            bound: HashMap::new(),
        }
    }

    /// `RandomState.getOrCreateNoise`, via `Noises.instantiate`.
    fn noise(&mut self, id: &str) -> Result<Arc<NoiseStack>, LoadError> {
        if let Some(n) = self.noises.get(id) {
            return Ok(n.clone());
        }
        let error = |kind| LoadError {
            resource: id.to_string(),
            path: String::new(),
            kind: Box::new(kind),
        };
        let parameters = self
            .noise_parameters
            .get(id)
            .ok_or_else(|| error(LoadErrorKind::Invalid("noise wasn't loaded before binding")))?;
        let normal = NormalNoise::new(parameters).map_err(|e| {
            error(match e {
                NoiseError::LegacyNormalization => LoadErrorKind::Unsupported {
                    what: "noise normalization",
                    name: "legacy".to_string(),
                },
                NoiseError::BaseOctave => LoadErrorKind::Invalid("base_octave is outside -32..=32"),
                NoiseError::OctaveCount => LoadErrorKind::Invalid("octave_count is outside 1..=32"),
                NoiseError::AmplitudeModifiers => LoadErrorKind::Invalid(
                    "amplitude_modifiers must be empty or have octave_count entries",
                ),
            })
        })?;
        let stack = Arc::new(normal.create(&mut self.random.from_hash_of(id)));
        self.noises.insert(id.to_string(), stack.clone());
        Ok(stack)
    }

    /// Binds `function` (`compileSampler`).
    pub fn bind(&mut self, function: &Df) -> Result<Sampler, LoadError> {
        let key = Arc::as_ptr(function);
        if let Some((_, s)) = self.bound.get(&key) {
            return Ok(s.clone());
        }
        let node = match &**function {
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
                let (xz, y) = (*xz_scale, *y_scale);
                if shift_x.is_zero() && shift_y.is_zero() && shift_z.is_zero() {
                    Node::Noise { noise, xz, y }
                } else if shift_y.is_zero() {
                    Node::NoiseShiftedXz {
                        noise,
                        xz,
                        y,
                        sx: self.bind(shift_x)?,
                        sz: self.bind(shift_z)?,
                    }
                } else {
                    Node::NoiseShiftedXyz {
                        noise,
                        xz,
                        y,
                        sx: self.bind(shift_x)?,
                        sy: self.bind(shift_y)?,
                        sz: self.bind(shift_z)?,
                    }
                }
            }
            DensityFunction::ShiftA { noise } => Node::ShiftA(self.noise(noise)?),
            DensityFunction::ShiftB { noise } => Node::ShiftB(self.noise(noise)?),
            DensityFunction::Shift { noise } => Node::Shift(self.noise(noise)?),
            // The game specializes constant operands (ConstAddSampler, ConstMulSampler), which
            // computes the same float: IEEE addition and multiplication are commutative.
            DensityFunction::Add(a, b) => Node::Add(self.bind(a)?, self.bind(b)?),
            DensityFunction::Mul(a, b) => Node::Mul(self.bind(a)?, self.bind(b)?),
            DensityFunction::Abs(a) => Node::Abs(self.bind(a)?),
            // The compiler deduplicates caches; on the point path the value is the input's.
            DensityFunction::Cache(a) => {
                return self.bind(a).inspect(|s| {
                    self.bound.insert(key, (function.clone(), s.clone()));
                });
            }
            DensityFunction::Lerp {
                alpha,
                first,
                second,
            } => Node::Lerp {
                alpha: self.bind(alpha)?,
                first: self.bind(first)?,
                second: self.bind(second)?,
            },
            DensityFunction::Gradient(g) => Node::Gradient {
                axis: g.axis,
                from: g.from_coordinate,
                min: g.from_coordinate.min(g.to_coordinate),
                max: g.from_coordinate.max(g.to_coordinate),
                from_value: g.from_value,
                // (toValue - fromValue) / (float)(toCoordinate - fromCoordinate).
                factor: (g.to_value - g.from_value)
                    / g.to_coordinate.wrapping_sub(g.from_coordinate) as f32,
            },
            DensityFunction::Spline(spline) => Node::Spline(self.bind_spline(spline)?),
            // SimpleDensityFunction: context-bound constants, 1 and 0 when nothing blends.
            DensityFunction::BlendAlpha => Node::Constant(1.0),
            DensityFunction::BlendOffset => Node::Constant(0.0),
        };
        let sampler = Sampler::new(node);
        self.bound.insert(key, (function.clone(), sampler.clone()));
        Ok(sampler)
    }

    fn bind_spline(&mut self, spline: &Spline) -> Result<BoundSpline, LoadError> {
        Ok(match spline {
            Spline::Constant(v) => BoundSpline::Constant(*v),
            Spline::Multipoint {
                coordinate,
                locations,
                values,
                derivatives,
            } => BoundSpline::Multipoint {
                coordinate: self.bind(coordinate)?,
                locations: locations.clone(),
                values: values
                    .iter()
                    .map(|v| self.bind_spline(v))
                    .collect::<Result<_, _>>()?,
                derivatives: derivatives.clone(),
            },
        })
    }
}

impl Sampler {
    fn new(node: Node) -> Self {
        let y = node.depends_on_y();
        Sampler {
            node: Arc::new(node),
            y,
        }
    }

    /// Whether the value can depend on y. When this is false, the value is the same at every y
    /// for a given x and z, bit for bit (see `docs/specs/density-functions.md`).
    pub fn depends_on_y(&self) -> bool {
        self.y
    }

    /// This sampler for the column at block `(x, z)`: every subtree whose value doesn't depend on
    /// y is evaluated once and replaced by a constant, so sampling the result at `(x, y, z)` gives
    /// exactly what this sampler gives there, only faster. The result is only valid at this `x`
    /// and `z`.
    pub fn at_column(&self, x: i32, z: i32) -> Sampler {
        if !self.y {
            return Sampler::new(Node::Constant(self.sample(x, 0, z)));
        }
        let col = |s: &Sampler| s.at_column(x, z);
        let node = match &*self.node {
            Node::NoiseShiftedXz {
                noise,
                xz,
                y,
                sx,
                sz,
            } => Node::NoiseShiftedXz {
                noise: noise.clone(),
                xz: *xz,
                y: *y,
                sx: col(sx),
                sz: col(sz),
            },
            Node::NoiseShiftedXyz {
                noise,
                xz,
                y,
                sx,
                sy,
                sz,
            } => Node::NoiseShiftedXyz {
                noise: noise.clone(),
                xz: *xz,
                y: *y,
                sx: col(sx),
                sy: col(sy),
                sz: col(sz),
            },
            Node::Add(a, b) => Node::Add(col(a), col(b)),
            Node::Mul(a, b) => Node::Mul(col(a), col(b)),
            Node::Abs(a) => Node::Abs(col(a)),
            Node::Lerp {
                alpha,
                first,
                second,
            } => Node::Lerp {
                alpha: col(alpha),
                first: col(first),
                second: col(second),
            },
            Node::Spline(spline) => Node::Spline(spline.at_column(x, z)),
            // Leaves that depend on y have nothing to fold.
            Node::Constant(_)
            | Node::Noise { .. }
            | Node::ShiftA(_)
            | Node::ShiftB(_)
            | Node::Shift(_)
            | Node::Gradient { .. } => return self.clone(),
        };
        Sampler::new(node)
    }

    /// `sampleValue` at block `(x, y, z)`.
    pub fn sample(&self, x: i32, y: i32, z: i32) -> f32 {
        let (xd, yd, zd) = (f64::from(x), f64::from(y), f64::from(z));
        match &*self.node {
            Node::Constant(v) => *v,
            Node::Noise { noise, xz, y: ys } => noise.get(xd * xz, yd * ys, zd * xz),
            Node::NoiseShiftedXz {
                noise,
                xz,
                y: ys,
                sx,
                sz,
            } => noise.get(
                xd * xz + f64::from(sx.sample(x, y, z)),
                yd * ys,
                zd * xz + f64::from(sz.sample(x, y, z)),
            ),
            Node::NoiseShiftedXyz {
                noise,
                xz,
                y: ys,
                sx,
                sy,
                sz,
            } => noise.get(
                xd * xz + f64::from(sx.sample(x, y, z)),
                yd * ys + f64::from(sy.sample(x, y, z)),
                zd * xz + f64::from(sz.sample(x, y, z)),
            ),
            // ShiftA: NoiseFunction.Sampler(noise, 0.25, 0.0) times 4.
            Node::ShiftA(noise) => noise.get(xd * 0.25, yd * 0.0, zd * 0.25) * 4.0,
            // ShiftB: the noise at (z/4, x/4, 0) times 4.
            Node::ShiftB(noise) => noise.get(zd * 0.25, xd * 0.25, 0.0) * 4.0,
            Node::Shift(noise) => noise.get(xd * 0.25, yd * 0.25, zd * 0.25) * 4.0,
            Node::Add(a, b) => a.sample(x, y, z) + b.sample(x, y, z),
            Node::Mul(a, b) => a.sample(x, y, z) * b.sample(x, y, z),
            Node::Abs(a) => a.sample(x, y, z).abs(),
            // LerpFunction.Sampler: exactly first at 0 and second at 1, Mth.lerp in between.
            Node::Lerp {
                alpha,
                first,
                second,
            } => {
                let t = alpha.sample(x, y, z);
                if t == 0.0 {
                    first.sample(x, y, z)
                } else if t == 1.0 {
                    second.sample(x, y, z)
                } else {
                    let a = first.sample(x, y, z);
                    a + t * (second.sample(x, y, z) - a)
                }
            }
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
            Node::Spline(spline) => spline.sample(x, y, z),
        }
    }
}

impl Node {
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
            Node::Noise { y, .. } => *y != 0.0,
            Node::NoiseShiftedXz { y, sx, sz, .. } => *y != 0.0 || sx.y || sz.y,
            Node::NoiseShiftedXyz { y, sx, sy, sz, .. } => *y != 0.0 || sx.y || sy.y || sz.y,
            Node::Add(a, b) | Node::Mul(a, b) => a.y || b.y,
            Node::Abs(a) => a.y,
            Node::Lerp {
                alpha,
                first,
                second,
            } => alpha.y || first.y || second.y,
            Node::Gradient { axis, .. } => *axis == Axis::Y,
            Node::Spline(spline) => spline.depends_on_y(),
        }
    }
}

impl BoundSpline {
    fn depends_on_y(&self) -> bool {
        match self {
            BoundSpline::Constant(_) => false,
            BoundSpline::Multipoint {
                coordinate, values, ..
            } => coordinate.y || values.iter().any(BoundSpline::depends_on_y),
        }
    }

    /// [`Sampler::at_column`] for a spline: coordinates and values are folded where they don't
    /// depend on y. A folded spline stays a spline, so its arithmetic is unchanged.
    fn at_column(&self, x: i32, z: i32) -> BoundSpline {
        match self {
            BoundSpline::Constant(v) => BoundSpline::Constant(*v),
            BoundSpline::Multipoint {
                coordinate,
                locations,
                values,
                derivatives,
            } => BoundSpline::Multipoint {
                coordinate: coordinate.at_column(x, z),
                locations: locations.clone(),
                values: values.iter().map(|v| v.at_column(x, z)).collect(),
                derivatives: derivatives.clone(),
            },
        }
    }

    /// `CubicSpline.Multipoint.sample`, all in `f32`.
    fn sample(&self, x: i32, y: i32, z: i32) -> f32 {
        let BoundSpline::Multipoint {
            coordinate,
            locations,
            values,
            derivatives,
        } = self
        else {
            let BoundSpline::Constant(v) = self else {
                unreachable!()
            };
            return *v;
        };
        let c = coordinate.sample(x, y, z);
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
        let extend = |i: usize| {
            let value = values[i].sample(x, y, z);
            let d = derivatives[i];
            if d == 0.0 {
                value
            } else {
                value + d * (c - locations[i])
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
        let (f0, f1) = (values[i].sample(x, y, z), values[i + 1].sample(x, y, z));
        let a = derivatives[i] * (x1 - x0) - (f1 - f0);
        let b = -derivatives[i + 1] * (x1 - x0) + (f1 - f0);
        let lerp = |t: f32, p: f32, q: f32| p + t * (q - p);
        lerp(t, f0, f1) + t * (1.0 - t) * lerp(t, a, b)
    }
}
