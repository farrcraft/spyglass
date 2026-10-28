//! The volume path: bound functions evaluated over a [`Volume`] at once, as the game's
//! `DensitySampler.sampleVolume` does. Chunk generation and the generator's columns sample
//! terrain this way. Each case cites the sampler it follows, and says where its arithmetic
//! differs from the point path.
//!
//! Values are computed per node over the whole volume, children first. A node already computed
//! over the same volume in the same call is reused, and a node whose value can't depend on y is
//! computed at y 0 and copied up the volume, as the game's `SLICE_UNIFORM_AXES` rewrite makes it.
//! Neither changes which floating-point operations run or their order, so the results are the
//! same bits.
//!
//! The game's caches (`CachingDensitySampler`) aren't modelled: over a volume they return what
//! the input computes over that same volume, so they only skip work. See
//! `docs/specs/density-functions.md` for where they could return something else, and why that
//! doesn't happen in the vanilla data.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use spyglass_noise::{Volume, mth};

use crate::function::{AXIS_X, AXIS_Z, Axis, UpscaleAxis, UpscaleMode};
use crate::java;
use crate::sampler::{
    BoundSpline, BoundUpscale, MEMO, Memo, NO_SLOT, Node, Pos, Sampler, Warp, clamp, find_surface,
    java_pow, round_to_integer, round_to_multiple, unary,
};

/// One volume evaluation's state: the point memo, for samplers that evaluate their inputs at
/// points, and each node's last volume and values, by slot.
struct Fill<'m> {
    point: &'m mut Memo,
    cells: Vec<Option<(Volume, Vec<f32>)>>,
}

impl Sampler {
    /// `sampleVolume`: the value at every position of `volume`, in its index order (y fastest,
    /// then x, then z).
    pub fn sample_volume(&self, volume: &Volume) -> Vec<f32> {
        MEMO.with_borrow_mut(|memo| self.volume_in(memo, volume))
    }

    /// [`sample_volume`](Self::sample_volume), with the point memo already borrowed.
    pub(crate) fn volume_in(&self, memo: &mut Memo, volume: &Volume) -> Vec<f32> {
        #[cfg(feature = "timing")]
        let _t = crate::timing::span("volume: call");
        let mut fill = Fill {
            point: memo,
            cells: Vec::new(),
        };
        let mut out = vec![0.0; volume.len()];
        self.fill(&mut fill, volume, &mut out);
        out
    }

    /// Writes the value over `volume` into `out`, reusing what this call already computed.
    fn fill(&self, f: &mut Fill, volume: &Volume, out: &mut [f32]) {
        let slot = self.slot as usize;
        // Only a node the graph shares can be asked for again in one call.
        let remembered = self.slot != NO_SLOT && self.shared.load(Ordering::Relaxed);
        if remembered
            && let Some(Some((v, values))) = f.cells.get(slot)
            && v == volume
        {
            #[cfg(feature = "timing")]
            let _t = crate::timing::detail("volume: memo hit");
            out.copy_from_slice(values);
            return;
        }
        if !self.depends_on_y && volume.size[1] > 1 {
            // SliceFunction.YSampler at 0, as SLICE_UNIFORM_AXES wraps such nodes.
            let flat = Volume::new(
                [volume.size[0], 1, volume.size[2]],
                [volume.min[0], 0, volume.min[2]],
                volume.step,
            );
            let mut values = vec![0.0; flat.len()];
            self.fill(f, &flat, &mut values);
            #[cfg(feature = "timing")]
            let _t = crate::timing::detail("volume: spread y");
            spread_y(&values, volume, out);
        } else {
            #[cfg(feature = "timing")]
            let _t = crate::timing::detail(self.node.kind());
            self.node.fill(f, volume, out);
        }
        #[cfg(feature = "timing")]
        let _t = crate::timing::detail("volume: memo store");
        if remembered {
            if f.cells.len() <= slot {
                f.cells.resize_with(slot + 1, || None);
            }
            f.cells[slot] = Some((*volume, out.to_vec()));
        }
    }

    /// The value over `volume`, in a new buffer.
    fn values(&self, f: &mut Fill, volume: &Volume) -> Vec<f32> {
        let mut out = vec![0.0; volume.len()];
        self.fill(f, volume, &mut out);
        out
    }
}

impl Node {
    fn fill(&self, f: &mut Fill, volume: &Volume, out: &mut [f32]) {
        match self {
            Node::Constant(v) => out.fill(*v),
            // NoiseFunction.Sampler: the stack's own volume path, from zero.
            Node::Noise {
                noise,
                xz_scale,
                y_scale,
            } => {
                out.fill(0.0);
                noise.add_to_volume(out, volume, *xz_scale, *y_scale, 1.0);
            }
            Node::SmearedNoise {
                noise,
                xz_scale,
                y_scale,
            } => {
                out.fill(0.0);
                noise.add_to_volume(out, volume, *xz_scale, *y_scale, 1.0);
            }
            // ShiftedXzSampler and ShiftedXyzSampler: the shifts over the volume, then the noise
            // at each position through its point path, at block · scale + shift.
            Node::NoiseShiftedXz {
                noise,
                xz_scale,
                y_scale,
                sx,
                sz,
            } => {
                let (a, b) = (sx.values(f, volume), sz.values(f, volume));
                for_each(volume, |i, x, y, z| {
                    out[i] = noise.get(
                        f64::from(x) * xz_scale + f64::from(a[i]),
                        f64::from(y) * y_scale,
                        f64::from(z) * xz_scale + f64::from(b[i]),
                    );
                });
            }
            Node::NoiseShiftedXyz {
                noise,
                xz_scale,
                y_scale,
                sx,
                sy,
                sz,
            } => {
                let a = sx.values(f, volume);
                let b = sy.values(f, volume);
                let c = sz.values(f, volume);
                for_each(volume, |i, x, y, z| {
                    out[i] = noise.get(
                        f64::from(x) * xz_scale + f64::from(a[i]),
                        f64::from(y) * y_scale + f64::from(b[i]),
                        f64::from(z) * xz_scale + f64::from(c[i]),
                    );
                });
            }
            // ShiftA and Shift: a ConstMulSampler of 4 over NoiseFunction.Sampler.
            Node::ShiftA(noise) => {
                out.fill(0.0);
                noise.add_to_volume(out, volume, 0.25, 0.0, 1.0);
                out.iter_mut().for_each(|v| *v *= 4.0);
            }
            Node::Shift(noise) => {
                out.fill(0.0);
                noise.add_to_volume(out, volume, 0.25, 0.25, 1.0);
                out.iter_mut().for_each(|v| *v *= 4.0);
            }
            // ShiftB's sampler: the noise at (z, x, 0) over a volume with x and z swapped, with
            // the 4 folded into every layer's weight, then copied up each column.
            Node::ShiftB(noise) => {
                let swapped = Volume::new(
                    [volume.size[2], volume.size[0], 1],
                    [volume.min[2], volume.min[0], 0],
                    [volume.step[2], volume.step[0], 1],
                );
                let mut values = vec![0.0; swapped.len()];
                noise.add_to_volume(&mut values, &swapped, 0.25, 0.25, 4.0);
                let flat: Vec<f32> = (0..volume.size[2])
                    .flat_map(|iz| (0..volume.size[0]).map(move |ix| (ix, iz)))
                    .map(|(ix, iz)| values[swapped.index(iz, ix, 0)])
                    .collect();
                spread_y(&flat, volume, out);
            }
            Node::Add(a, b) => binary(f, volume, out, a, b, |l, r| l + r),
            // SubSampler: left + (−right), the same float as left − right.
            Node::Sub(a, b) => binary(f, volume, out, a, b, |l, r| l + -r),
            // MulSampler and DivSampler: no zero test here, unlike the point path.
            Node::Mul(a, b) => binary(f, volume, out, a, b, |l, r| l * r),
            Node::Div(a, b) => binary(f, volume, out, a, b, |l, r| l / r),
            Node::MulConst(a, c) => map(f, volume, out, a, |v| v * c),
            Node::ConstDiv(c, b) => map(f, volume, out, b, |v| c / v),
            // MinSampler and MaxSampler: the right value replaces the left only if it's below
            // (above) it, not Math.min (Math.max), and the right's range isn't consulted.
            Node::Min(a, b, _) => binary(f, volume, out, a, b, |l, r| if r < l { r } else { l }),
            Node::Max(a, b, _) => binary(f, volume, out, a, b, |l, r| if r > l { r } else { l }),
            Node::MinConst(a, c) => map(f, volume, out, a, |v| if *c < v { *c } else { v }),
            Node::MaxConst(a, c) => map(f, volume, out, a, |v| if *c > v { *c } else { v }),
            Node::Unary(op, a) => map(f, volume, out, a, |v| unary(*op, v)),
            Node::Round(kind, input, multiple) => {
                binary(f, volume, out, input, multiple, |v, m| {
                    round_to_multiple(v, m, *kind)
                });
            }
            Node::RoundInteger(kind, input) => {
                map(f, volume, out, input, |v| round_to_integer(v, *kind));
            }
            Node::Clamp(input, min, max) => map(f, volume, out, input, |v| clamp(v, *min, *max)),
            Node::MathClamp(input, min, max) => {
                map(f, volume, out, input, |v| java::clamp(v, *min, *max));
            }
            Node::Pow(base, exponent) => binary(f, volume, out, base, exponent, |b, e| {
                java_pow(f64::from(b), f64::from(e))
            }),
            Node::PowConstBase(base, exponent) => {
                map(f, volume, out, exponent, |e| java_pow(*base, f64::from(e)));
            }
            Node::PowConstExponent(base, exponent) => {
                map(f, volume, out, base, |b| java_pow(f64::from(b), *exponent));
            }
            // LerpFunction.Sampler: all three over the volume, then the point path's choice.
            Node::Lerp {
                alpha,
                first,
                second,
            } => {
                alpha.fill(f, volume, out);
                let a = first.values(f, volume);
                let b = second.values(f, volume);
                for (i, t) in out.iter_mut().enumerate() {
                    *t = if *t == 0.0 {
                        a[i]
                    } else if *t == 1.0 {
                        b[i]
                    } else {
                        mth::lerp(*t, a[i], b[i])
                    };
                }
            }
            // RangeChoiceFunction.Sampler: both branches over the volume.
            Node::RangeChoice {
                input,
                min_inclusive,
                max_exclusive,
                when_in_range,
                when_out_of_range,
            } => {
                when_in_range.fill(f, volume, out);
                let v = input.values(f, volume);
                let other = when_out_of_range.values(f, volume);
                for i in 0..out.len() {
                    if !(v[i] >= *min_inclusive && v[i] < *max_exclusive) {
                        out[i] = other[i];
                    }
                }
            }
            // IntervalSelectFunction.Sampler: every function over the volume.
            Node::IntervalSelect {
                input,
                thresholds,
                samplers,
            } => {
                input.fill(f, volume, out);
                let values: Vec<Vec<f32>> = samplers.iter().map(|s| s.values(f, volume)).collect();
                for (i, v) in out.iter_mut().enumerate() {
                    let k = thresholds
                        .iter()
                        .position(|t| *v < *t)
                        .unwrap_or(samplers.len() - 1);
                    *v = values[k][i];
                }
            }
            // SliceFunction's samplers: the input over the volume flattened onto the
            // coordinate, copied back along that axis.
            Node::Slice {
                axis,
                coordinate,
                input,
            } => slice(f, volume, out, *axis, *coordinate, input),
            Node::Interpolated {
                input,
                cell_xz,
                cell_y,
                inv_xz,
                inv_y,
                corners,
            } => interpolate(
                f,
                volume,
                out,
                (input, corners),
                [*cell_xz, *cell_y],
                [*inv_xz, *inv_y],
            ),
            Node::Upscale(u) => upscale(f, volume, out, u),
            Node::UpscaleNearest { input, cell } => upscale_nearest(f, volume, out, input, *cell),
            // FindTopSurfaceFunction.Sampler, which the game only samples one y deep (it's
            // inside a slice): the upper bound over the volume, then findSurfaceFrom per column.
            Node::FindTopSurface {
                density,
                upper_bound,
                lower_bound,
                cell_height,
            } => {
                let flat = Volume::new(
                    [volume.size[0], 1, volume.size[2]],
                    [volume.min[0], 0, volume.min[2]],
                    volume.step,
                );
                let upper = upper_bound.values(f, &flat);
                let mut surfaces = vec![0.0; flat.len()];
                for_each(&flat, |i, x, _, z| {
                    surfaces[i] =
                        find_surface(f.point, density, x, z, upper[i], *lower_bound, *cell_height);
                });
                spread_y(&surfaces, volume, out);
            }
            Node::DomainWarp {
                input,
                offsets,
                axes,
            } => domain_warp(f, volume, out, input, offsets, *axes),
            // GradientFunction's, DistanceToPointFunction's and EndIslandFunction's samplers
            // compute each position's value as the point path does.
            Node::Gradient { .. }
            | Node::GradientRepeat { .. }
            | Node::DistanceToPoint { .. }
            | Node::EndIslands(_) => {
                for_each(volume, |i, x, y, z| {
                    out[i] = self.compute(f.point, Pos::new(x, y, z));
                });
            }
            // SplineFunction.Sampler: every coordinate over the volume, then the spline per
            // position from those values.
            Node::Spline(spline) => {
                let mut coordinates = Vec::new();
                spline.coordinates(&mut coordinates);
                let mut values: HashMap<*const Node, Vec<f32>> = HashMap::new();
                for c in coordinates {
                    if let Entry::Vacant(e) = values.entry(Arc::as_ptr(&c.node)) {
                        e.insert(c.values(f, volume));
                    }
                }
                for (i, v) in out.iter_mut().enumerate() {
                    *v = spline.sample_with(&mut |c: &Sampler| values[&Arc::as_ptr(&c.node)][i]);
                }
            }
        }
    }
}

impl BoundSpline {
    /// Every coordinate the spline and its nested splines read, in visiting order, with repeats.
    fn coordinates<'s>(&'s self, out: &mut Vec<&'s Sampler>) {
        if let BoundSpline::Multipoint {
            coordinate, values, ..
        } = self
        {
            out.push(coordinate);
            for v in values {
                v.coordinates(out);
            }
        }
    }
}

/// An interpolator's or upscaler's input values at its corners, for one seed, kept across
/// queries by corner column (x and z, and the column's y positions). A column's values are the
/// input's own wherever they were computed, when the input is [position-only](Sampler): so
/// columns asked again, by a neighboring column's cell or a later query, aren't recomputed.
/// Only small corner volumes, a few columns' worth, are kept; larger ones share their corners
/// within the query already. Cleared when it holds [`MAX_KEPT_COLUMNS`].
#[derive(Debug, Default)]
pub(crate) struct CornerCache(Mutex<HashMap<CornerColumn, Arc<[f32]>>>);

/// A corner column: block x and z, the lowest y, the y step and how many positions.
type CornerColumn = (i32, i32, i32, i32, i32);

/// The most corner columns a volume can have and be kept.
const MAX_CACHED_VOLUME_COLUMNS: i32 = 16;

/// How many corner columns a [`CornerCache`] holds before it's cleared.
const MAX_KEPT_COLUMNS: usize = 4096;

impl CornerCache {
    /// `input` over `corners`, from the columns kept if every one is, else computed (and kept).
    fn values(&self, f: &mut Fill, input: &Sampler, corners: &Volume) -> Vec<f32> {
        let [sx, sy, sz] = corners.size;
        if !input.position_only || sx * sz > MAX_CACHED_VOLUME_COLUMNS {
            return input.values(f, corners);
        }
        let key = |ix: i32, iz: i32| {
            (
                corners.block_x(ix),
                corners.block_z(iz),
                corners.min[1],
                corners.step[1],
                sy,
            )
        };
        let height = sy as usize;
        {
            // A panic while holding the lock leaves only whole entries behind.
            let kept = self.0.lock().unwrap_or_else(|e| e.into_inner());
            let mut out = Vec::with_capacity(corners.len());
            'columns: for iz in 0..sz {
                for ix in 0..sx {
                    match kept.get(&key(ix, iz)) {
                        Some(column) => out.extend_from_slice(column),
                        None => break 'columns,
                    }
                }
            }
            if out.len() == corners.len() {
                return out;
            }
        }
        let values = input.values(f, corners);
        let mut kept = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if kept.len() + (sx * sz) as usize > MAX_KEPT_COLUMNS {
            kept.clear();
        }
        for iz in 0..sz {
            for ix in 0..sx {
                // Index order is y fastest, then x, then z, so a column is contiguous.
                let first = (iz * sx + ix) as usize * height;
                kept.insert(key(ix, iz), values[first..first + height].into());
            }
        }
        values
    }
}

/// Visits every position of `volume` in index order: `(index, x, y, z)` with block coordinates.
fn for_each(volume: &Volume, mut visit: impl FnMut(usize, i32, i32, i32)) {
    let mut i = 0;
    for iz in 0..volume.size[2] {
        let z = volume.block_z(iz);
        for ix in 0..volume.size[0] {
            let x = volume.block_x(ix);
            for iy in 0..volume.size[1] {
                visit(i, x, volume.block_y(iy), z);
                i += 1;
            }
        }
    }
}

/// Copies one value per column (`flat`, x then z) up every y of `volume` (`setRange`).
fn spread_y(flat: &[f32], volume: &Volume, out: &mut [f32]) {
    let height = volume.size[1] as usize;
    for (column, v) in out.chunks_exact_mut(height).zip(flat) {
        column.fill(*v);
    }
}

/// An elementwise operation on one input: the input into `out`, then `op` in place.
fn map(f: &mut Fill, volume: &Volume, out: &mut [f32], a: &Sampler, op: impl Fn(f32) -> f32) {
    a.fill(f, volume, out);
    out.iter_mut().for_each(|v| *v = op(*v));
}

/// An elementwise operation on two inputs, left into `out` first, as the binary samplers do.
fn binary(
    f: &mut Fill,
    volume: &Volume,
    out: &mut [f32],
    a: &Sampler,
    b: &Sampler,
    op: impl Fn(f32, f32) -> f32,
) {
    a.fill(f, volume, out);
    let right = b.values(f, volume);
    for (v, r) in out.iter_mut().zip(right) {
        *v = op(*v, r);
    }
}

/// `SliceFunction`'s `XSampler`, `YSampler` and `ZSampler.sampleVolume`.
fn slice(
    f: &mut Fill,
    volume: &Volume,
    out: &mut [f32],
    axis: Axis,
    coordinate: i32,
    input: &Sampler,
) {
    let a = match axis {
        Axis::X => 0,
        Axis::Y => 1,
        Axis::Z => 2,
    };
    let mut flat = *volume;
    flat.size[a] = 1;
    flat.min[a] = coordinate;
    let values = input.values(f, &flat);
    for_each_index(volume, |i, mut p| {
        p[a] = 0;
        out[i] = values[flat.index(p[0], p[1], p[2])];
    });
}

/// `DomainWarpFunction`'s samplers over a volume, as the game samples them after
/// `SLICE_UNIFORM_AXES`: flattened onto 0 along each axis the function doesn't depend on (y is
/// already, by [`Sampler::fill`]), then copied back along it. What follows depends on the
/// volume's size, so the flattening has to match the game's.
fn domain_warp(
    f: &mut Fill,
    volume: &Volume,
    out: &mut [f32],
    input: &Sampler,
    offsets: &Warp,
    axes: u8,
) {
    let mut flat = *volume;
    for (a, bit) in [(0, AXIS_X), (2, AXIS_Z)] {
        if axes & bit == 0 {
            flat.size[a] = 1;
            flat.min[a] = 0;
        }
    }
    if flat == *volume {
        warp(f, volume, out, input, offsets);
        return;
    }
    let mut values = vec![0.0; flat.len()];
    warp(f, &flat, &mut values, input, offsets);
    for_each_index(volume, |i, [ix, iy, iz]| {
        let at = |a: usize, n: i32| if flat.size[a] == 1 { 0 } else { n };
        out[i] = values[flat.index(at(0, ix), iy, at(2, iz))];
    });
}

/// `ConstOffsetSampler`, `FlatXzSampler` and `Sampler.sampleVolume`. The constant case is the
/// input over the moved volume. Otherwise every position's moved block is computed (offsets
/// sampled over the volume, truncated), and the input is sampled either at each one on the point
/// path or, when the box they span is at most 16 times the volume, over that box on the volume
/// path (`shouldSampleNaively`).
fn warp(f: &mut Fill, volume: &Volume, out: &mut [f32], input: &Sampler, offsets: &Warp) {
    let (ox, oy, oz) = match offsets {
        Warp::Const(o) => {
            let moved = Volume::new(
                volume.size,
                std::array::from_fn(|a| volume.min[a].wrapping_add(o[a])),
                volume.step,
            );
            input.fill(f, &moved, out);
            return;
        }
        Warp::FlatXz { x, z } => (x.values(f, volume), None, z.values(f, volume)),
        Warp::General { x, y, z } => (
            x.values(f, volume),
            Some(y.values(f, volume)),
            z.values(f, volume),
        ),
    };
    let mut blocks = Vec::with_capacity(volume.len());
    let (mut min, mut max) = ([i32::MAX; 3], [i32::MIN; 3]);
    for_each(volume, |i, x, y, z| {
        let b = [
            x.wrapping_add(ox[i] as i32),
            match &oy {
                Some(oy) => y.wrapping_add(oy[i] as i32),
                None => 0,
            },
            z.wrapping_add(oz[i] as i32),
        ];
        for a in 0..3 {
            min[a] = min[a].min(b[a]);
            max[a] = max[a].max(b[a]);
        }
        blocks.push(b);
    });
    // shouldSampleNaively: the box's volume in exact long arithmetic. On overflow the game
    // returns false and then fails to make the box; spyglass samples naively instead.
    let span = |a: usize| i64::from(max[a]) - i64::from(min[a]) + 1;
    let naive = match span(0)
        .checked_mul(span(1))
        .and_then(|v| v.checked_mul(span(2)))
    {
        Some(v) => v > i64::from(i32::MAX) || v > volume.len() as i64 * 16,
        None => true,
    };
    if naive {
        for (v, [x, y, z]) in out.iter_mut().zip(&blocks) {
            *v = input.eval_at(f.point, *x, *y, *z);
        }
        return;
    }
    let shape = Volume::blocks(std::array::from_fn(|a| span(a) as i32), min);
    let values = input.values(f, &shape);
    for (v, b) in out.iter_mut().zip(&blocks) {
        *v = values[shape.index(b[0] - min[0], b[1] - min[1], b[2] - min[2])];
    }
}

/// Visits every position of `volume` in index order, with its position counted along each axis.
fn for_each_index(volume: &Volume, mut visit: impl FnMut(usize, [i32; 3])) {
    let mut i = 0;
    for iz in 0..volume.size[2] {
        for ix in 0..volume.size[0] {
            for iy in 0..volume.size[1] {
                visit(i, [ix, iy, iz]);
                i += 1;
            }
        }
    }
}

/// `InterpolatedFunction.Sampler.sampleVolume`. A volume on cell corners, one cell apart (or one
/// position deep), is the input's own volume. Otherwise the input is sampled at the corners of
/// every cell the volume touches and interpolated block by block; a volume with steps above 1
/// is interpolated over all its blocks and then picked from.
fn interpolate(
    f: &mut Fill,
    volume: &Volume,
    out: &mut [f32],
    (input, cache): (&Sampler, &CornerCache),
    [cxz, cy]: [i32; 2],
    inv: [f32; 2],
) {
    let cell = [cxz, cy, cxz];
    let aligned = (0..3).all(|a| {
        (volume.step[a] == cell[a] || volume.size[a] == 1)
            && java::floor_mod(volume.min[a], cell[a]) == 0
    });
    if aligned {
        input.fill(f, volume, out);
        return;
    }
    if volume.step == [1, 1, 1] {
        interpolate_blocks(f, volume, out, (input, cache), cell, inv);
        return;
    }
    // sampleWithNonBlockStep: every block the volume spans, then each position's.
    let blocks = Volume::blocks(
        std::array::from_fn(|a| volume.size[a].wrapping_mul(volume.step[a])),
        volume.min,
    );
    let mut all = vec![0.0; blocks.len()];
    interpolate_blocks(f, &blocks, &mut all, (input, cache), cell, inv);
    for_each_index(volume, |i, [ix, iy, iz]| {
        out[i] = all[blocks.index(
            ix * volume.step[0],
            iy * volume.step[1],
            iz * volume.step[2],
        )];
    });
}

/// `sampleWithBlockStep` and `fillCell`, for a volume of steps 1.
fn interpolate_blocks(
    f: &mut Fill,
    volume: &Volume,
    out: &mut [f32],
    (input, cache): (&Sampler, &CornerCache),
    cell: [i32; 3],
    [inv_xz, inv_y]: [f32; 2],
) {
    let max = volume.max_block();
    let first: [i32; 3] = std::array::from_fn(|a| java::floor_div(volume.min[a], cell[a]));
    let last: [i32; 3] = std::array::from_fn(|a| java::floor_div(max[a], cell[a]));
    let cells: [i32; 3] = std::array::from_fn(|a| last[a] - first[a] + 1);
    // One more corner than cells, unless the last block is itself on a corner.
    let corners = Volume::new(
        std::array::from_fn(|a| {
            if java::floor_mod(max[a], cell[a]) == 0 {
                cells[a]
            } else {
                cells[a] + 1
            }
        }),
        std::array::from_fn(|a| first[a].wrapping_mul(cell[a])),
        cell,
    );
    let c = cache.values(f, input, &corners);
    let at = |x, y, z| c[corners.index(x, y, z)];
    for iz in 0..cells[2] {
        let iz1 = (iz + 1).min(corners.size[2] - 1);
        for ix in 0..cells[0] {
            let ix1 = (ix + 1).min(corners.size[0] - 1);
            // Corners named by their x, y and z offsets.
            let (mut c000, mut c100) = (at(ix, 0, iz), at(ix1, 0, iz));
            let (mut c001, mut c101) = (at(ix, 0, iz1), at(ix1, 0, iz1));
            for iy in 0..cells[1] {
                let iy1 = (iy + 1).min(corners.size[1] - 1);
                let (c010, c110) = (at(ix, iy1, iz), at(ix1, iy1, iz));
                let (c011, c111) = (at(ix, iy1, iz1), at(ix1, iy1, iz1));
                let origin = [
                    corners.block_x(ix) - volume.min[0],
                    corners.block_y(iy) - volume.min[1],
                    corners.block_z(iz) - volume.min[2],
                ];
                fill_cell(
                    volume,
                    out,
                    origin,
                    cell,
                    [inv_xz, inv_y],
                    [c000, c100, c010, c110, c001, c101, c011, c111],
                );
                (c000, c100, c001, c101) = (c010, c110, c011, c111);
            }
        }
    }
}

/// `fillCell`: one cell's blocks inside the volume, z outermost, then x, then y. Per z, the four
/// z lerps; per x, the two x lerps; then y accumulates a step from the first block.
fn fill_cell(
    volume: &Volume,
    out: &mut [f32],
    origin: [i32; 3],
    cell: [i32; 3],
    [inv_xz, inv_y]: [f32; 2],
    [c000, c100, c010, c110, c001, c101, c011, c111]: [f32; 8],
) {
    let start: [i32; 3] = std::array::from_fn(|a| 0.max(-origin[a]));
    let end: [i32; 3] = std::array::from_fn(|a| cell[a].min(volume.size[a] - origin[a]) - 1);
    for dz in start[2]..=end[2] {
        let z = origin[2] + dz;
        let tz = dz as f32 * inv_xz;
        let x0y0 = mth::lerp(tz, c000, c001);
        let x0y1 = mth::lerp(tz, c010, c011);
        let x1y0 = mth::lerp(tz, c100, c101);
        let x1y1 = mth::lerp(tz, c110, c111);
        for dx in start[0]..=end[0] {
            let x = origin[0] + dx;
            let tx = dx as f32 * inv_xz;
            let b0 = mth::lerp(tx, x0y0, x1y0);
            let b1 = mth::lerp(tx, x0y1, x1y1);
            let step = (b1 - b0) * inv_y;
            let mut value = b0 + step * start[1] as f32;
            let first = volume.index(x, origin[1] + start[1], z);
            let blocks = (end[1] - start[1] + 1) as usize;
            for v in &mut out[first..first + blocks] {
                *v = value;
                value += step;
            }
        }
    }
}

/// `UpscaleFunction.Sampler.sampleVolume`: each axis planned (`planUpsample`); if any can't be,
/// the block-step box the volume spans is upscaled and every `step`th value picked
/// (`sampleAndUpscaleFallback`).
fn upscale(f: &mut Fill, volume: &Volume, out: &mut [f32], u: &BoundUpscale) {
    let plans: [Option<AxisPlan>; 3] = std::array::from_fn(|a| {
        AxisPlan::new(
            u.axes[a],
            volume.min[a],
            volume.size[a],
            volume.step[a],
            u.offset_by_step,
        )
    });
    if let [Some(x), Some(y), Some(z)] = plans {
        sample_and_upscale(f, volume, out, u, [x, y, z]);
        return;
    }
    let blocks = Volume::blocks(
        std::array::from_fn(|a| {
            (volume.size[a] - 1)
                .wrapping_mul(volume.step[a])
                .wrapping_add(1)
        }),
        volume.min,
    );
    let plans = std::array::from_fn(|a| {
        AxisPlan::new(
            u.axes[a],
            blocks.min[a],
            blocks.size[a],
            1,
            u.offset_by_step,
        )
        .expect("an axis with step 1 always has a plan")
    });
    let mut all = vec![0.0; blocks.len()];
    sample_and_upscale(f, &blocks, &mut all, u, plans);
    for_each_index(volume, |i, [ix, iy, iz]| {
        out[i] = all[blocks.index(
            ix * volume.step[0],
            iy * volume.step[1],
            iz * volume.step[2],
        )];
    });
}

/// `UpscaleFunction.Sampler.sampleAndUpscale`: the input's own volume if every axis is exactly
/// aligned; else the input over the corner volume the plans give, upscaled by the upscaler for
/// the plans' modes and cell sizes (`Upscaler.upscale`).
fn sample_and_upscale(
    f: &mut Fill,
    volume: &Volume,
    out: &mut [f32],
    u: &BoundUpscale,
    plans: [AxisPlan; 3],
) {
    if plans.iter().all(|p| p.exactly_aligned) {
        u.input.fill(f, volume, out);
        return;
    }
    let corners = Volume::new(
        plans.map(|p| p.sample_count),
        plans.map(|p| p.sample_min),
        plans.map(|p| p.sample_step),
    );
    let samples = u.corners.values(f, &u.input, &corners);
    let (modes, cells) = (plans.map(|p| p.mode), plans.map(|p| p.cell));
    if modes == u.upscaler.modes && cells == u.upscaler.cells {
        u.upscaler.upscale(&plans, out, volume, &samples, &corners);
    } else {
        Upscaler::new(modes, cells).upscale(&plans, out, volume, &samples, &corners);
    }
}

/// `NearestNeighborSampler.sampleVolume`: the input's own volume if it fits the grid
/// (`DensityVolume.fitsOnGrid`); else the corners of every cell from the volume's minimum to its
/// last position, each position taking its cell's corner.
fn upscale_nearest(
    f: &mut Fill,
    volume: &Volume,
    out: &mut [f32],
    input: &Sampler,
    cell: [i32; 3],
) {
    let fits = (0..3).all(|a| {
        (volume.size[a] == 1 || volume.step[a].wrapping_rem(cell[a]) == 0)
            && java::floor_mod(volume.min[a], cell[a]) == 0
    });
    if fits {
        input.fill(f, volume, out);
        return;
    }
    // getCellSampleVolume, with 26.4's maxBlock: the last position, min + (size − 1)·step.
    let first: [i32; 3] = std::array::from_fn(|a| java::floor_div(volume.min[a], cell[a]));
    let last: [i32; 3] = std::array::from_fn(|a| {
        let max = volume.min[a].wrapping_add((volume.size[a] - 1).wrapping_mul(volume.step[a]));
        java::floor_div(max, cell[a])
    });
    let grid = Volume::new(
        std::array::from_fn(|a| last[a] - first[a] + 1),
        std::array::from_fn(|a| first[a].wrapping_mul(cell[a])),
        cell,
    );
    let corners = input.values(f, &grid);
    let at = |a: usize, block: i32| java::floor_div(block, cell[a]) - first[a];
    for_each(volume, |i, x, y, z| {
        out[i] = corners[grid.index(at(0, x), at(1, y), at(2, z))];
    });
}

/// `Upscaler.AxisPlan`: how one axis of a volume is upscaled. `cell` is the upscaler's cell size
/// along the axis: the axis's, or its cell size in steps for a stepped volume, or 1 when the axis
/// is exactly aligned.
#[derive(Debug, Clone, Copy)]
struct AxisPlan {
    mode: UpscaleMode,
    exactly_aligned: bool,
    sample_count: i32,
    sample_min: i32,
    sample_step: i32,
    output_count: i32,
    min_output: i32,
    cell: i32,
}

impl AxisPlan {
    /// `UpscaleFunction.Axis.planUpsample(min, size, step)`, or `None` where the game plans
    /// nothing and falls back. `Mth.isMultipleOf` is Java's `%`, so a negative multiple counts.
    /// `offset_by_step` is switch `upscale_offset_by_step`.
    fn new(
        axis: UpscaleAxis,
        min: i32,
        size: i32,
        step: i32,
        offset_by_step: bool,
    ) -> Option<AxisPlan> {
        let (mode, cell) = (axis.mode, axis.cell_size);
        let step = if size == 1 { cell } else { step };
        let multiple = |a: i32, b: i32| a.wrapping_rem(b) == 0;
        let max = min.wrapping_add((size - 1).wrapping_mul(step));
        if multiple(step, cell) && multiple(min, cell) && mode.exact_on_corner() {
            // AxisPlan.exactlyAligned: the volume's own positions, as nearest with cell 1.
            return Some(AxisPlan {
                mode: UpscaleMode::NearestNeighbor,
                exactly_aligned: true,
                sample_count: size,
                sample_min: min,
                sample_step: step,
                output_count: size,
                min_output: 0,
                cell: 1,
            });
        }
        if !multiple(cell, step) {
            return None;
        }
        let first = java::floor_div(min, cell);
        let last = java::floor_div(max, cell);
        let s0 = first - mode.samples_before();
        let on_corner = i32::from(mode.exact_on_corner() && multiple(max, cell));
        let s1 = last + (mode.samples_after() - on_corner).max(0);
        // Where the first cell starts, relative to the volume: never after it.
        let offset = first.wrapping_mul(cell).wrapping_sub(min);
        let (min_output, cell_steps) = if step == 1 || size == 1 {
            (offset, cell)
        } else {
            // 26.4-snapshot-5 tests the offset against the cell size, not the step, so a
            // stepped volume whose minimum isn't on a corner falls back.
            if !multiple(offset, if offset_by_step { step } else { cell }) {
                return None;
            }
            (offset / step, cell / step)
        };
        Some(AxisPlan {
            mode,
            exactly_aligned: false,
            sample_count: s1 - s0 + 1,
            sample_min: s0.wrapping_mul(cell),
            sample_step: cell,
            output_count: last - first + 1,
            min_output,
            cell: cell_steps,
        })
    }
}

/// `Upscaler.Coefficients`: one entry of an axis's table, the weights of its samples.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Coefficients {
    /// `Nearest`: the first sample.
    Nearest,
    /// `Linear(c1, c2)`.
    Linear(f32, f32),
    /// `Cubic(c1, c2, c3, c4)`: Catmull-Rom and the B-spline.
    Cubic(f32, f32, f32, f32),
}

impl Coefficients {
    /// `computeTable(cellSize, mode)`: entry `i` at `t = (float) i / (float) cellSize`.
    pub(crate) fn table(cell: i32, mode: UpscaleMode) -> Vec<Coefficients> {
        (0..cell)
            .map(|i| Coefficients::at(mode, i as f32 / cell as f32))
            .collect()
    }

    /// `Coefficients.compute(mode, t)`, all in `f32`, with `Mth.cube` as `(t·t)·t`.
    fn at(mode: UpscaleMode, t: f32) -> Coefficients {
        let square = |t: f32| t * t;
        let cube = |t: f32| t * t * t;
        match mode {
            UpscaleMode::NearestNeighbor => Coefficients::Nearest,
            UpscaleMode::InterpolateLinear => Coefficients::Linear(1.0 - t, t),
            UpscaleMode::InterpolateCosine => {
                let t = in_out_sine(t);
                Coefficients::Linear(1.0 - t, t)
            }
            UpscaleMode::InterpolateCubic => Coefficients::Cubic(
                (-0.5 * cube(t) + square(t)) - 0.5 * t,
                (1.5 * cube(t) - 2.5 * square(t)) + 1.0,
                (-1.5 * cube(t) + 2.0 * square(t)) + 0.5 * t,
                0.5 * cube(t) - 0.5 * square(t),
            ),
            UpscaleMode::BSplineCubic => Coefficients::Cubic(
                cube(1.0 - t) / 6.0,
                ((3.0 * cube(t) - 6.0 * square(t)) + 4.0) / 6.0,
                (((-3.0 * cube(t) + 3.0 * square(t)) + 3.0 * t) + 1.0) / 6.0,
                cube(t) / 6.0,
            ),
        }
    }

    /// `multiply(samples, i, stride)`: the weighted samples `i`, `i + stride`, …, summed left to
    /// right.
    fn multiply(self, s: &[f32], i: usize, stride: usize) -> f32 {
        match self {
            Coefficients::Nearest => s[i],
            Coefficients::Linear(c1, c2) => c1 * s[i] + c2 * s[i + stride],
            Coefficients::Cubic(c1, c2, c3, c4) => {
                ((c1 * s[i] + c2 * s[i + stride]) + c3 * s[i + 2 * stride]) + c4 * s[i + 3 * stride]
            }
        }
    }

    /// `asArray()`: the weights, in order (nearest's is 1).
    pub(crate) fn as_array(self) -> Vec<f32> {
        match self {
            Coefficients::Nearest => vec![1.0],
            Coefficients::Linear(c1, c2) => vec![c1, c2],
            Coefficients::Cubic(c1, c2, c3, c4) => vec![c1, c2, c3, c4],
        }
    }
}

/// `Ease.inOutSine(t)`: `−(Mth.cos((double)(π_f · t)) − 1) / 2`, in `f32`.
fn in_out_sine(t: f32) -> f32 {
    -(mth_cos(f64::from(std::f32::consts::PI * t)) - 1.0) / 2.0
}

/// `Mth.cos(double)`: the sine table at `|x| · 65536 / 2π + 16384`, rounded half up, wrapped.
fn mth_cos(x: f64) -> f32 {
    const RADIANS_TO_INDEX: f64 = 10_430.378_350_470_453;
    // dcmpl: NaN takes the negated branch, and (long) NaN is 0 either way.
    let x = if x >= 0.0 { x } else { -x };
    sin_table((((x * RADIANS_TO_INDEX + 16_384.0) + 0.5) as i64 & 65_535) as usize)
}

/// `Mth.SIN[i]`: `(float) Math.sin(i / 10430.378350470453)`, with the quarter turns set exactly.
///
/// Rust's `sin` isn't required to give Java's `Math.sin` bit for bit; both are narrowed to
/// `f32`, so they differ only where the `f64` lies on a rounding boundary. Only the cosine mode,
/// unused in vanilla, reads the table.
fn sin_table(i: usize) -> f32 {
    match i {
        0 | 32_768 => 0.0,
        16_384 => 1.0,
        49_152 => -1.0,
        _ => (i as f64 / 10_430.378_350_470_453).sin() as f32,
    }
}

/// `PolymorphicUpscaler`: an upscaler for three modes and cell sizes (the game builds a
/// specialized copy of it per parameter set, with the same arithmetic). Samples are laid out
/// as a volume's, y fastest, then x, then z.
#[derive(Debug)]
pub(crate) struct Upscaler {
    pub(crate) modes: [UpscaleMode; 3],
    pub(crate) cells: [i32; 3],
    /// `sampleCount` per axis.
    counts: [usize; 3],
    /// `sampleStrideX` (the y count) and `sampleStrideZ` (x count times y count); y's is 1.
    stride_x: usize,
    stride_z: usize,
    /// `1.0f / (float) cellSizeX` and `cellSizeY`.
    inv_x: f32,
    inv_y: f32,
    /// `computeTable(cellSize, mode)` per axis.
    tables: [Vec<Coefficients>; 3],
}

impl Upscaler {
    /// `new PolymorphicUpscaler(new Parameters(modes, cells))`.
    pub(crate) fn new(modes: [UpscaleMode; 3], cells: [i32; 3]) -> Self {
        let counts = modes.map(|m| m.sample_count() as usize);
        Upscaler {
            modes,
            cells,
            counts,
            stride_x: counts[1],
            stride_z: counts[0] * counts[1],
            inv_x: 1.0 / cells[0] as f32,
            inv_y: 1.0 / cells[1] as f32,
            tables: std::array::from_fn(|a| Coefficients::table(cells[a], modes[a])),
        }
    }

    /// `upscaleValue(fx, fy, fz, buffer)`: the point path's value at offsets `f` in the cell,
    /// from the `sampleCount` corners per axis in `samples`: z, then x, then y.
    pub(crate) fn value(&self, [fx, fy, fz]: [i32; 3], samples: &[f32]) -> f32 {
        let z = self.tables[2][fz as usize];
        let tz: Vec<f32> = (0..self.stride_z)
            .map(|i| z.multiply(samples, i, self.stride_z))
            .collect();
        let x = self.tables[0][fx as usize];
        let tx: Vec<f32> = (0..self.stride_x)
            .map(|j| x.multiply(&tz, j, self.stride_x))
            .collect();
        self.tables[1][fy as usize].multiply(&tx, 0, 1)
    }

    /// `doUpscale`: for each output cell, z outer, then x, the samples around it (indices
    /// clamped to the corner volume) are gathered, then each y cell is filled, the y samples
    /// sliding down one corner per cell.
    fn upscale(
        &self,
        plans: &[AxisPlan; 3],
        out: &mut [f32],
        volume: &Volume,
        samples: &[f32],
        corners: &Volume,
    ) {
        let [count_x, count_y, count_z] = self.counts;
        let last_y = count_y - 1;
        let size = corners.size;
        let mut window = vec![0.0; count_x * count_y * count_z];
        let mut tx = vec![0.0; self.stride_x];
        let mut tz = vec![0.0; self.stride_z];
        let [px, py, pz] = plans;
        for cz in 0..pz.output_count {
            let oz = pz.min_output + cz * self.cells[2];
            for cx in 0..px.output_count {
                let ox = px.min_output + cx * self.cells[0];
                for k in 0..count_z {
                    let zi = (cz + k as i32).min(size[2] - 1);
                    for j in 0..count_x {
                        let xi = (cx + j as i32).min(size[0] - 1);
                        for l in 0..count_y {
                            let yi = (l as i32).min(size[1] - 1);
                            window[j * self.stride_x + l + k * self.stride_z] =
                                samples[corners.index(xi, yi, zi)];
                        }
                    }
                }
                let o = [ox, py.min_output, oz];
                self.fill_cell(out, volume, o, &window, &mut tx, &mut tz);
                for cy in 1..py.output_count {
                    let oy = py.min_output + cy * self.cells[1];
                    let yi = (cy + last_y as i32).min(size[1] - 1);
                    for k in 0..count_z {
                        let zi = (cz + k as i32).min(size[2] - 1);
                        for j in 0..count_x {
                            let xi = (cx + j as i32).min(size[0] - 1);
                            let base = j * self.stride_x + k * self.stride_z;
                            window.copy_within(base + 1..base + count_y, base);
                            window[base + last_y] = samples[corners.index(xi, yi, zi)];
                        }
                    }
                    self.fill_cell(out, volume, [ox, oy, oz], &window, &mut tx, &mut tz);
                }
            }
        }
    }

    /// `fillCell`: the cell at output position `o`, its offsets outside the volume skipped
    /// (an axis of cell 1 always takes offset 0). Per z offset, `tz` from the z coefficients.
    fn fill_cell(
        &self,
        out: &mut [f32],
        volume: &Volume,
        o: [i32; 3],
        samples: &[f32],
        tx: &mut [f32],
        tz: &mut [f32],
    ) {
        let lo: [i32; 3] =
            std::array::from_fn(|a| if self.cells[a] == 1 { 0 } else { 0.max(-o[a]) });
        let hi: [i32; 3] = std::array::from_fn(|a| {
            if self.cells[a] == 1 {
                0
            } else {
                self.cells[a].min(volume.size[a] - o[a]) - 1
            }
        });
        let (stride_x, stride_z) = (volume.size[1], volume.size[0] * volume.size[1]);
        let mut index =
            (o[1] + lo[1]) + ((o[0] + lo[0]) + (o[2] + lo[2]) * volume.size[0]) * volume.size[1];
        for dz in lo[2]..=hi[2] {
            let z = self.tables[2][dz as usize];
            for (i, t) in tz.iter_mut().enumerate() {
                *t = z.multiply(samples, i, self.stride_z);
            }
            self.fill_cell_xy(out, tz, tx, [lo[0], hi[0]], [lo[1], hi[1]], index, stride_x);
            index += stride_z;
        }
    }

    /// `fillCellXy`. With x linear and y nearest, x takes the step form, `a + step · (float) dx`
    /// with `step = (b − a) · (1.0f / cellSizeX)`; otherwise `tx` from the x coefficients.
    #[allow(clippy::too_many_arguments, reason = "mirrors fillCellXy's parameters")]
    fn fill_cell_xy(
        &self,
        out: &mut [f32],
        tz: &[f32],
        tx: &mut [f32],
        [x_lo, x_hi]: [i32; 2],
        y: [i32; 2],
        mut index: i32,
        stride_x: i32,
    ) {
        if self.modes[0] == UpscaleMode::InterpolateLinear
            && self.modes[1] == UpscaleMode::NearestNeighbor
        {
            let (a, b) = (tz[0], tz[1]);
            let step = (b - a) * self.inv_x;
            for dx in x_lo..=x_hi {
                tx[0] = a + step * dx as f32;
                self.fill_cell_y(out, tx, y, index);
                index += stride_x;
            }
            return;
        }
        for dx in x_lo..=x_hi {
            let x = self.tables[0][dx as usize];
            for (j, t) in tx.iter_mut().enumerate() {
                *t = x.multiply(tz, j, self.stride_x);
            }
            self.fill_cell_y(out, tx, y, index);
            index += stride_x;
        }
    }

    /// `fillCellY`: consecutive y values from `index`. Nearest repeats `tx[0]`; linear is
    /// `a + step · (float) dy` for each `dy`, computed afresh, not accumulated; other modes use
    /// their coefficients.
    fn fill_cell_y(&self, out: &mut [f32], tx: &[f32], [lo, hi]: [i32; 2], index: i32) {
        let values = (lo..=hi).zip(index as usize..);
        match self.modes[1] {
            UpscaleMode::NearestNeighbor => {
                for (_, i) in values {
                    out[i] = tx[0];
                }
            }
            UpscaleMode::InterpolateLinear => {
                let a = tx[0];
                let step = (tx[1] - a) * self.inv_y;
                for (dy, i) in values {
                    out[i] = a + step * dy as f32;
                }
            }
            _ => {
                for (dy, i) in values {
                    out[i] = self.tables[1][dy as usize].multiply(tx, 0, 1);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(mode: UpscaleMode, cell_size: i32, min: i32, size: i32, step: i32) -> Option<AxisPlan> {
        AxisPlan::new(UpscaleAxis { mode, cell_size }, min, size, step, false)
    }

    /// `(sample_count, sample_min, output_count, min_output, cell)`.
    fn shape(p: AxisPlan) -> (i32, i32, i32, i32, i32) {
        (
            p.sample_count,
            p.sample_min,
            p.output_count,
            p.min_output,
            p.cell,
        )
    }

    #[test]
    fn axes_plan_their_corners() {
        use UpscaleMode::*;
        // Blocks −3..=16 in cells of 8: cells −1..=2, corners −8, 0, 8, 16 (16 is the last block
        // and a corner, so no corner after it), the first cell starting 5 blocks early.
        let p = plan(InterpolateLinear, 8, -3, 20, 1).unwrap();
        assert!(!p.exactly_aligned && p.sample_step == 8);
        assert_eq!(shape(p), (4, -8, 4, -5, 8));
        // One position deep: the step becomes the cell size. Block 5 is in cell 1, corners 4, 8.
        assert_eq!(
            shape(plan(InterpolateLinear, 4, 5, 1, 1).unwrap()),
            (2, 4, 1, -1, 4)
        );
        // On corners, a cell apart: the volume's own positions, as nearest with cell 1.
        let p = plan(InterpolateLinear, 4, 8, 3, 4).unwrap();
        assert!(p.exactly_aligned && p.mode == NearestNeighbor);
        assert_eq!(shape(p), (3, 8, 3, 0, 1));
        // A B-spline isn't exact on a corner: one corner before, two after, and the cell counted
        // in steps.
        let p = plan(BSplineCubic, 4, 8, 3, 4).unwrap();
        assert!(!p.exactly_aligned);
        assert_eq!(shape(p), (6, 4, 3, 0, 1));
        let p = plan(InterpolateLinear, 8, 0, 4, 2).unwrap();
        assert_eq!(shape(p), (2, 0, 1, 0, 4));
        // No plan: a step that doesn't divide the cell, or a stepped volume off the corners.
        assert!(plan(InterpolateLinear, 4, 0, 3, 3).is_none());
        assert!(plan(InterpolateLinear, 4, 1, 3, 2).is_none());
        // The minimum is tested against the cell even when the step would fit it.
        assert!(plan(InterpolateLinear, 4, 2, 3, 2).is_none());
        // Switch upscale_offset_by_step: against the step. Blocks 2, 4, 6 in cells of 4: cells
        // 0..=1, corners 0, 4, 8, the first cell starting one step early, each cell two steps.
        let axis = UpscaleAxis {
            mode: InterpolateLinear,
            cell_size: 4,
        };
        let p = AxisPlan::new(axis, 2, 3, 2, true).unwrap();
        assert_eq!(shape(p), (3, 0, 2, -1, 2));
        assert!(AxisPlan::new(axis, 1, 3, 2, true).is_none());
    }

    #[test]
    fn coefficient_tables() {
        use UpscaleMode::*;
        assert_eq!(
            Coefficients::table(4, InterpolateLinear),
            [(1.0, 0.0), (0.75, 0.25), (0.5, 0.5), (0.25, 0.75)]
                .map(|(a, b)| Coefficients::Linear(a, b))
        );
        // Ease.inOutSine at ½: cos(π/2) is the table's exact 0 at index 32768.
        assert_eq!(
            Coefficients::table(2, InterpolateCosine)[1],
            Coefficients::Linear(0.5, 0.5)
        );
        // At ¼: SIN[24576] = sin(3π/4), so t = (1 − √½) / 2.
        let Coefficients::Linear(_, t) = Coefficients::table(4, InterpolateCosine)[1] else {
            panic!("cosine is linear");
        };
        assert!((t - 0.146_446_6).abs() < 1e-6, "{t}");
        // Catmull-Rom passes through its second sample at t = 0; the B-spline weighs three.
        assert_eq!(
            Coefficients::table(2, InterpolateCubic)[0],
            Coefficients::Cubic(0.0, 1.0, 0.0, 0.0)
        );
        assert_eq!(
            Coefficients::table(2, BSplineCubic)[0],
            Coefficients::Cubic(1.0 / 6.0, 4.0 / 6.0, 1.0 / 6.0, 0.0)
        );
        assert_eq!(Coefficients::table(3, NearestNeighbor).len(), 3);
    }
}
