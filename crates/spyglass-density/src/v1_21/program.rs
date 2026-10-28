//! 26.2's bound functions compiled into a flat program (ADR 0031): one operation per distinct
//! bound function, children by index and constants as operands, evaluated at a context with the
//! same arithmetic, in the same order, as [`Function`]'s evaluation, which stays as the
//! reference.
//!
//! Evaluation stays lazy where the game's is: a branch not taken (`range_choice`,
//! `interval_select`, `min` and `max` against a range, `mul` by zero) isn't evaluated. Values
//! are remembered as [`Function`]'s are: an operation the program references more than once,
//! once per context, and a columnar one ([`Function`]'s `columnar`) once per column. The memo
//! is borrowed once per evaluation and passed down, rather than looked up per operation.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use spyglass_noise::mth;
use spyglass_noise::v1_21::{BlendedNoise, NormalNoise, SimplexNoise};

use super::bind::{BoundSpline, Ctx, Function, Node, end_island_height, mapped, shift};
use super::function::{Mapped, Marker};
use crate::java;
use crate::sampler::sample_multipoint;

/// An operation's input: a constant, or another operation's value.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Arg {
    Const(f64),
    Op(u32),
}

/// One operation: a bound function's node, with its inputs as [`Arg`]s.
#[derive(Debug)]
enum Op {
    Add(Arg, Arg),
    Mul(Arg, Arg),
    /// `argument1` if below `argument2`'s minimum, else `Math.min`.
    Min(Arg, Arg, f64),
    /// `argument1` if above `argument2`'s maximum, else `Math.max`.
    Max(Arg, Arg, f64),
    AddConstant(Arg, f64),
    MulConstant(Arg, f64),
    Mapped(Mapped, Arg),
    Clamp(Arg, f64, f64),
    Noise {
        noise: Arc<NormalNoise>,
        xz_scale: f64,
        y_scale: f64,
    },
    ShiftedNoise {
        noise: Arc<NormalNoise>,
        xz_scale: f64,
        y_scale: f64,
        shift: [Arg; 3],
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
        input: Arg,
        min_inclusive: f64,
        max_exclusive: f64,
        when_in_range: Arg,
        when_out_of_range: Arg,
    },
    IntervalSelect {
        input: Arg,
        thresholds: Vec<f64>,
        functions: Vec<Arg>,
    },
    Spline(Spline),
    FindTopSurface {
        density: Arg,
        upper_bound: Arg,
        lower_bound: i32,
        cell_height: i32,
    },
    EndIslands(Arc<SimplexNoise>),
    BlendedNoise(Arc<BlendedNoise>),
    Marker {
        marker: Marker,
        input: Arg,
        binder: u64,
        slot: u32,
    },
}

/// A spline with its coordinates as [`Arg`]s.
#[derive(Debug)]
enum Spline {
    Constant(f32),
    Multipoint {
        coordinate: Arg,
        locations: Vec<f32>,
        values: Vec<Spline>,
        derivatives: Vec<f32>,
    },
}

/// What an operation's value is remembered for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Remember {
    /// Nothing: it's referenced once, and isn't columnar.
    Not,
    /// Its context, or its column if it's columnar and the context shares one.
    Shared,
    /// Its column, when the context shares one; it's referenced once.
    Column,
}

/// A compiled graph: its operations, and the [`Arg`] of each root it was compiled from.
#[derive(Debug)]
pub(crate) struct Program {
    id: u64,
    ops: Vec<Op>,
    remember: Vec<Remember>,
    columnar: Vec<bool>,
}

static PROGRAMS: AtomicU64 = AtomicU64::new(0);

/// One remembered value: the stamp it's for, the program, and the value.
#[derive(Clone, Copy)]
struct Remembered {
    stamp: u64,
    program: u64,
    value: f64,
}

/// The remembered values of one thread, by operation.
pub(crate) struct Memo(Vec<Remembered>);

thread_local! {
    static MEMO: RefCell<Memo> = const { RefCell::new(Memo(Vec::new())) };
}

/// Builds a [`Program`] from bound functions, one operation per distinct node.
struct Compiler {
    ops: Vec<Op>,
    columnar: Vec<bool>,
    refs: Vec<u32>,
    by_node: HashMap<*const Node, u32>,
    /// The functions compiled, kept alive so their addresses stay theirs.
    keep: Vec<Function>,
}

impl Program {
    /// Compiles `roots` into one program; returns it and each root's [`Arg`].
    pub(crate) fn compile(roots: &[&Function]) -> (Program, Vec<Arg>) {
        let mut c = Compiler {
            ops: Vec::new(),
            columnar: Vec::new(),
            refs: Vec::new(),
            by_node: HashMap::new(),
            keep: Vec::new(),
        };
        let args = roots.iter().map(|f| c.arg(f)).collect();
        let remember = (0..c.ops.len())
            .map(|i| match (c.refs[i] > 1, c.columnar[i]) {
                (true, _) => Remember::Shared,
                (false, true) => Remember::Column,
                (false, false) => Remember::Not,
            })
            .collect();
        let program = Program {
            id: PROGRAMS.fetch_add(1, Ordering::Relaxed),
            ops: c.ops,
            remember,
            columnar: c.columnar,
        };
        (program, args)
    }

    /// `arg`'s value at `ctx`, with this thread's memo.
    pub(crate) fn eval_root(&self, arg: Arg, ctx: &Ctx<'_>) -> f64 {
        MEMO.with_borrow_mut(|memo| self.eval(arg, ctx, memo))
    }

    /// `arg`'s value at `ctx`.
    pub(crate) fn eval(&self, arg: Arg, ctx: &Ctx<'_>, memo: &mut Memo) -> f64 {
        match arg {
            Arg::Const(v) => v,
            Arg::Op(i) => self.eval_op(i as usize, ctx, memo),
        }
    }

    fn eval_op(&self, i: usize, ctx: &Ctx<'_>, memo: &mut Memo) -> f64 {
        let shares_column = ctx.column != ctx.stamp;
        let stamp = match self.remember[i] {
            Remember::Not => return self.compute(i, ctx, memo),
            Remember::Column if !shares_column => return self.compute(i, ctx, memo),
            Remember::Column => ctx.column,
            Remember::Shared if self.columnar[i] && shares_column => ctx.column,
            Remember::Shared => ctx.stamp,
        };
        if let Some(r) = memo.0.get(i)
            && r.stamp == stamp
            && r.program == self.id
        {
            return r.value;
        }
        let value = self.compute(i, ctx, memo);
        if memo.0.len() <= i {
            memo.0.resize(
                i + 1,
                Remembered {
                    stamp: 0,
                    program: u64::MAX,
                    value: 0.0,
                },
            );
        }
        memo.0[i] = Remembered {
            stamp,
            program: self.id,
            value,
        };
        value
    }

    /// Operation `i`'s value at `ctx`, as [`Function`]'s evaluation of its node computes it.
    fn compute(&self, i: usize, ctx: &Ctx<'_>, memo: &mut Memo) -> f64 {
        let eval = |a: Arg, memo: &mut Memo| self.eval(a, ctx, memo);
        match &self.ops[i] {
            Op::Add(a, b) => eval(*a, memo) + eval(*b, memo),
            Op::Mul(a, b) => {
                let v = eval(*a, memo);
                // dcmpl/ifne: −0.0 and +0.0 both give +0.0, without evaluating argument2.
                if v == 0.0 { 0.0 } else { v * eval(*b, memo) }
            }
            Op::Min(a, b, b_min) => {
                let v = eval(*a, memo);
                if v < *b_min {
                    v
                } else {
                    java::min_f64(v, eval(*b, memo))
                }
            }
            Op::Max(a, b, b_max) => {
                let v = eval(*a, memo);
                if v > *b_max {
                    v
                } else {
                    java::max_f64(v, eval(*b, memo))
                }
            }
            Op::AddConstant(input, c) => eval(*input, memo) + c,
            Op::MulConstant(input, c) => eval(*input, memo) * c,
            Op::Mapped(op, input) => mapped(*op, eval(*input, memo)),
            Op::Clamp(input, min, max) => java::mth_clamp_f64(eval(*input, memo), *min, *max),
            Op::Noise {
                noise,
                xz_scale,
                y_scale,
            } => noise.get_value(
                f64::from(ctx.x) * xz_scale,
                f64::from(ctx.y) * y_scale,
                f64::from(ctx.z) * xz_scale,
            ),
            Op::ShiftedNoise {
                noise,
                xz_scale,
                y_scale,
                shift: [sx, sy, sz],
            } => {
                let x = f64::from(ctx.x) * xz_scale + eval(*sx, memo);
                let y = f64::from(ctx.y) * y_scale + eval(*sy, memo);
                let z = f64::from(ctx.z) * xz_scale + eval(*sz, memo);
                noise.get_value(x, y, z)
            }
            Op::ShiftA(noise) => shift(noise, f64::from(ctx.x), 0.0, f64::from(ctx.z)),
            Op::ShiftB(noise) => shift(noise, f64::from(ctx.z), f64::from(ctx.x), 0.0),
            Op::Shift(noise) => shift(noise, f64::from(ctx.x), f64::from(ctx.y), f64::from(ctx.z)),
            Op::YClampedGradient {
                from_y,
                to_y,
                from_value,
                to_value,
            } => {
                let t = (f64::from(ctx.y) - from_y) / (to_y - from_y);
                mth::clamped_lerp(t, *from_value, *to_value)
            }
            Op::RangeChoice {
                input,
                min_inclusive,
                max_exclusive,
                when_in_range,
                when_out_of_range,
            } => {
                let v = eval(*input, memo);
                if v >= *min_inclusive && v < *max_exclusive {
                    eval(*when_in_range, memo)
                } else {
                    eval(*when_out_of_range, memo)
                }
            }
            Op::IntervalSelect {
                input,
                thresholds,
                functions,
            } => {
                let v = eval(*input, memo);
                // dcmpg/ifge: NaN is below no threshold.
                let k = thresholds
                    .iter()
                    .position(|t| v < *t)
                    .unwrap_or(functions.len() - 1);
                eval(functions[k], memo)
            }
            Op::Spline(spline) => f64::from(self.spline(spline, ctx, memo)),
            Op::FindTopSurface {
                density,
                upper_bound,
                lower_bound,
                cell_height,
            } => {
                let upper = eval(*upper_bound, memo);
                let start = mth::floor(upper / f64::from(*cell_height)).wrapping_mul(*cell_height);
                if start <= *lower_bound {
                    return f64::from(*lower_bound);
                }
                let mut y = start;
                while y >= *lower_bound {
                    // dcmpl/ifle: NaN isn't above 0.
                    if self.eval(*density, &ctx.foreign(ctx.x, y, ctx.z), memo) > 0.0 {
                        return f64::from(y);
                    }
                    y = y.wrapping_sub(*cell_height);
                }
                f64::from(*lower_bound)
            }
            Op::EndIslands(noise) => {
                (f64::from(end_island_height(noise, ctx.x / 8, ctx.z / 8)) - 8.0) / 128.0
            }
            Op::BlendedNoise(noise) => noise.compute(ctx.x, ctx.y, ctx.z),
            Op::Marker {
                marker,
                input,
                binder,
                slot,
            } => match ctx.chunk {
                None => eval(*input, memo),
                Some(chunk) => chunk.marker(*marker, *binder, *slot, ctx, &mut |c: &Ctx<'_>| {
                    self.eval(*input, c, memo)
                }),
            },
        }
    }

    /// `CubicSpline.sample`, each coordinate `(float)` of its value.
    fn spline(&self, spline: &Spline, ctx: &Ctx<'_>, memo: &mut Memo) -> f32 {
        match spline {
            Spline::Constant(v) => *v,
            Spline::Multipoint {
                coordinate,
                locations,
                values,
                derivatives,
            } => {
                let c = self.eval(*coordinate, ctx, memo) as f32;
                sample_multipoint(c, locations, values, derivatives, &mut |v: &Spline| {
                    self.spline(v, ctx, memo)
                })
            }
        }
    }
}

impl Compiler {
    /// `f` as an operand: a constant, or its operation, compiled once.
    fn arg(&mut self, f: &Function) -> Arg {
        if let Node::Constant(v) = &*f.node {
            return Arg::Const(*v);
        }
        let key = Arc::as_ptr(&f.node);
        if let Some(&i) = self.by_node.get(&key) {
            self.refs[i as usize] += 1;
            return Arg::Op(i);
        }
        let op = self.op(f);
        let i = self.ops.len() as u32;
        self.ops.push(op);
        self.columnar.push(f.columnar);
        self.refs.push(1);
        self.by_node.insert(key, i);
        self.keep.push(f.clone());
        Arg::Op(i)
    }

    fn op(&mut self, f: &Function) -> Op {
        match &*f.node {
            Node::Constant(_) => unreachable!("constants are operands"),
            Node::Add(a, b) => Op::Add(self.arg(a), self.arg(b)),
            Node::Mul(a, b) => Op::Mul(self.arg(a), self.arg(b)),
            Node::Min(a, b) => Op::Min(self.arg(a), self.arg(b), b.min_value()),
            Node::Max(a, b) => Op::Max(self.arg(a), self.arg(b), b.max_value()),
            Node::AddConstant(input, c) => Op::AddConstant(self.arg(input), *c),
            Node::MulConstant(input, c) => Op::MulConstant(self.arg(input), *c),
            Node::Mapped(op, input) => Op::Mapped(*op, self.arg(input)),
            Node::Clamp(input, min, max) => Op::Clamp(self.arg(input), *min, *max),
            Node::Noise {
                noise,
                xz_scale,
                y_scale,
            } => Op::Noise {
                noise: noise.clone(),
                xz_scale: *xz_scale,
                y_scale: *y_scale,
            },
            Node::ShiftedNoise {
                noise,
                xz_scale,
                y_scale,
                shift,
            } => Op::ShiftedNoise {
                noise: noise.clone(),
                xz_scale: *xz_scale,
                y_scale: *y_scale,
                shift: [
                    self.arg(&shift[0]),
                    self.arg(&shift[1]),
                    self.arg(&shift[2]),
                ],
            },
            Node::ShiftA(noise) => Op::ShiftA(noise.clone()),
            Node::ShiftB(noise) => Op::ShiftB(noise.clone()),
            Node::Shift(noise) => Op::Shift(noise.clone()),
            Node::YClampedGradient {
                from_y,
                to_y,
                from_value,
                to_value,
            } => Op::YClampedGradient {
                from_y: *from_y,
                to_y: *to_y,
                from_value: *from_value,
                to_value: *to_value,
            },
            Node::RangeChoice {
                input,
                min_inclusive,
                max_exclusive,
                when_in_range,
                when_out_of_range,
            } => Op::RangeChoice {
                input: self.arg(input),
                min_inclusive: *min_inclusive,
                max_exclusive: *max_exclusive,
                when_in_range: self.arg(when_in_range),
                when_out_of_range: self.arg(when_out_of_range),
            },
            Node::IntervalSelect {
                input,
                thresholds,
                functions,
            } => Op::IntervalSelect {
                input: self.arg(input),
                thresholds: thresholds.clone(),
                functions: functions.iter().map(|f| self.arg(f)).collect(),
            },
            Node::Spline(spline) => Op::Spline(self.spline(spline)),
            Node::FindTopSurface {
                density,
                upper_bound,
                lower_bound,
                cell_height,
            } => Op::FindTopSurface {
                density: self.arg(density),
                upper_bound: self.arg(upper_bound),
                lower_bound: *lower_bound,
                cell_height: *cell_height,
            },
            Node::EndIslands(noise) => Op::EndIslands(noise.clone()),
            Node::BlendedNoise(noise) => Op::BlendedNoise(noise.clone()),
            Node::Marker {
                marker,
                input,
                binder,
                slot,
            } => Op::Marker {
                marker: *marker,
                input: self.arg(input),
                binder: *binder,
                slot: *slot,
            },
        }
    }

    fn spline(&mut self, spline: &BoundSpline) -> Spline {
        match spline {
            BoundSpline::Constant(v) => Spline::Constant(*v),
            BoundSpline::Multipoint {
                coordinate,
                locations,
                values,
                derivatives,
                ..
            } => Spline::Multipoint {
                coordinate: self.arg(coordinate),
                locations: locations.clone(),
                values: values.iter().map(|v| self.spline(v)).collect(),
                derivatives: derivatives.clone(),
            },
        }
    }
}
