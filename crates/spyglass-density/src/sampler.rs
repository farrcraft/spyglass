//! Density functions bound to a seed, evaluated at a block position as the game's compiled
//! samplers do (`DensitySampler.sampleValue`). Each case cites the sampler it follows.
//!
//! Evaluation remembers values per thread (see [`Sampler::sample`]): a node referenced from
//! several places is computed once per position, and a node whose value can't depend on y once
//! per column. That changes when each floating-point operation runs, never which ones run, so the
//! results are the same bits.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use spyglass_data::codec::{LoadError, LoadErrorKind};
use spyglass_noise::{NoiseError, NoiseStack, NormalNoise, NormalNoiseParameters};
use spyglass_rng::{
    PositionalRandomFactory, RandomSource, XoroshiroPositionalRandomFactory, XoroshiroRandomSource,
};

use crate::function::{Axis, DensityFunction, Df, Spline};

/// A density function bound to a seed's noises, ready to evaluate.
#[derive(Debug, Clone)]
pub struct Sampler {
    node: Arc<Node>,
    /// Whether the value can depend on y; see [`Sampler::depends_on_y`].
    depends_on_y: bool,
    /// The [`Binder`] that made it, which numbered its slots.
    binder: u64,
    /// Where its value is remembered, or [`NO_SLOT`] for constants.
    slot: u32,
}

/// The slot of a node that isn't worth remembering.
const NO_SLOT: u32 = u32::MAX;

#[derive(Debug)]
enum Node {
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

/// Tells binders apart, so remembered values are never read by another binder's nodes.
static BINDERS: AtomicU64 = AtomicU64::new(0);

/// Binds functions for one seed. Shared nodes stay shared, and each noise is created once, as
/// `RandomState.getOrCreateNoise` does.
pub struct Binder<'n> {
    noise_parameters: &'n HashMap<String, NormalNoiseParameters>,
    random: XoroshiroPositionalRandomFactory,
    noises: HashMap<String, Arc<NoiseStack>>,
    /// Samplers by function identity. Each entry keeps its function alive, so its address can't
    /// be reused by another function while the binder holds it.
    bound: HashMap<*const DensityFunction, (Df, Sampler)>,
    id: u64,
    slots: u32,
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
            id: BINDERS.fetch_add(1, Ordering::Relaxed),
            slots: 0,
        }
    }

    /// `RandomState.getOrCreateNoise`, via `Noises.instantiate`.
    fn noise(&mut self, id: &str) -> Result<Arc<NoiseStack>, LoadError> {
        if let Some(n) = self.noises.get(id) {
            return Ok(n.clone());
        }
        let error = |kind| LoadError::new(id, "", kind);
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
                other => LoadErrorKind::Invalid(other.reason()),
            })
        })?;
        let stack = Arc::new(normal.create(&mut self.random.from_hash_of(id)));
        self.noises.insert(id.to_string(), stack.clone());
        Ok(stack)
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
        let sampler = self.sampler(node);
        self.bound.insert(key, (function.clone(), sampler.clone()));
        Ok(sampler)
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
            },
        })
    }
}

/// Remembered values, valid for one binder at one column and one y. Stamps tell the current
/// column's and point's entries from stale ones, so moving on never clears anything.
#[derive(Default)]
struct Memo {
    binder: u64,
    column: Option<(i32, i32)>,
    y: Option<i32>,
    stamps: u64,
    column_stamp: u64,
    point_stamp: u64,
    /// By slot: the stamp the value was computed under, and the value. Stamp 0 is never valid.
    entries: Vec<(u64, f32)>,
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
}

thread_local! {
    static MEMO: RefCell<Memo> = RefCell::new(Memo::default());
}

/// A block position, in both the `int` and `double` forms the samplers use.
#[derive(Clone, Copy)]
struct Pos {
    x: i32,
    y: i32,
    z: i32,
    xd: f64,
    yd: f64,
    zd: f64,
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
        MEMO.with_borrow_mut(|memo| {
            memo.begin(self.binder, x, y, z);
            let pos = Pos {
                x,
                y,
                z,
                xd: f64::from(x),
                yd: f64::from(y),
                zd: f64::from(z),
            };
            self.eval(memo, pos)
        })
    }

    /// The value at `pos`, remembered in its slot.
    fn eval(&self, memo: &mut Memo, pos: Pos) -> f32 {
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
}

impl Node {
    /// The node's value at `pos`, its children's through `memo`.
    fn compute(&self, memo: &mut Memo, pos: Pos) -> f32 {
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
            // ShiftA: NoiseFunction.Sampler(noise, 0.25, 0.0) times 4.
            Node::ShiftA(noise) => noise.get(xd * 0.25, yd * 0.0, zd * 0.25) * 4.0,
            // ShiftB: the noise at (z/4, x/4, 0) times 4.
            Node::ShiftB(noise) => noise.get(zd * 0.25, xd * 0.25, 0.0) * 4.0,
            Node::Shift(noise) => noise.get(xd * 0.25, yd * 0.25, zd * 0.25) * 4.0,
            Node::Add(a, b) => a.eval(memo, pos) + b.eval(memo, pos),
            Node::Mul(a, b) => a.eval(memo, pos) * b.eval(memo, pos),
            Node::Abs(a) => a.eval(memo, pos).abs(),
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
            Node::Noise { y_scale, .. } => *y_scale != 0.0,
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
            Node::Add(a, b) | Node::Mul(a, b) => a.depends_on_y || b.depends_on_y,
            Node::Abs(a) => a.depends_on_y,
            Node::Lerp {
                alpha,
                first,
                second,
            } => alpha.depends_on_y || first.depends_on_y || second.depends_on_y,
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
            } => coordinate.depends_on_y || values.iter().any(BoundSpline::depends_on_y),
        }
    }

    /// `CubicSpline.Multipoint.sample`, all in `f32`.
    fn sample(&self, memo: &mut Memo, pos: Pos) -> f32 {
        let (coordinate, locations, values, derivatives) = match self {
            BoundSpline::Constant(v) => return *v,
            BoundSpline::Multipoint {
                coordinate,
                locations,
                values,
                derivatives,
            } => (coordinate, locations, values, derivatives),
        };
        let c = coordinate.eval(memo, pos);
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
            let value = values[i].sample(memo, pos);
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
        let (f0, f1) = (values[i].sample(memo, pos), values[i + 1].sample(memo, pos));
        let a = derivatives[i] * (x1 - x0) - (f1 - f0);
        let b = -derivatives[i + 1] * (x1 - x0) + (f1 - f0);
        let lerp = |t: f32, p: f32, q: f32| p + t * (q - p);
        lerp(t, f0, f1) + t * (1.0 - t) * lerp(t, a, b)
    }
}
