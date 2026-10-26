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
use std::sync::Arc;

use spyglass_noise::{Volume, mth};

use crate::function::Axis;
use crate::java;
use crate::sampler::{
    BoundSpline, MEMO, Memo, NO_SLOT, Node, Pos, Sampler, clamp, find_surface, java_pow,
    round_to_integer, round_to_multiple, unary,
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
        if self.slot != NO_SLOT
            && let Some(Some((v, values))) = f.cells.get(slot)
            && v == volume
        {
            out.copy_from_slice(values);
            return;
        }
        if !self.depends_on_y && volume.size[1] > 1 {
            // SliceFunction.YSampler at 0, as SLICE_UNIFORM_AXES wraps such nodes.
            let flat = Volume {
                size: [volume.size[0], 1, volume.size[2]],
                min: [volume.min[0], 0, volume.min[2]],
                step: volume.step,
            };
            let mut values = vec![0.0; flat.len()];
            self.fill(f, &flat, &mut values);
            spread_y(&values, volume, out);
        } else {
            self.node.fill(f, volume, out);
        }
        if self.slot != NO_SLOT {
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
                let swapped = Volume {
                    size: [volume.size[2], volume.size[0], 1],
                    min: [volume.min[2], volume.min[0], 0],
                    step: [volume.step[2], volume.step[0], 1],
                };
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
            } => interpolate(
                f,
                volume,
                out,
                input,
                [*cell_xz, *cell_y],
                [*inv_xz, *inv_y],
            ),
            // FindTopSurfaceFunction.Sampler, which the game only samples one y deep (it's
            // inside a slice): the upper bound over the volume, then findSurfaceFrom per column.
            Node::FindTopSurface {
                density,
                upper_bound,
                lower_bound,
                cell_height,
            } => {
                let flat = Volume {
                    size: [volume.size[0], 1, volume.size[2]],
                    min: [volume.min[0], 0, volume.min[2]],
                    step: volume.step,
                };
                let upper = upper_bound.values(f, &flat);
                let mut surfaces = vec![0.0; flat.len()];
                for_each(&flat, |i, x, _, z| {
                    surfaces[i] =
                        find_surface(f.point, density, x, z, upper[i], *lower_bound, *cell_height);
                });
                spread_y(&surfaces, volume, out);
            }
            // GradientFunction's samplers compute each coordinate's value as the point path does.
            Node::Gradient { .. } | Node::GradientRepeat { .. } => {
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
    input: &Sampler,
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
        interpolate_blocks(f, volume, out, input, cell, inv);
        return;
    }
    // sampleWithNonBlockStep: every block the volume spans, then each position's.
    let blocks = Volume::blocks(
        std::array::from_fn(|a| volume.size[a].wrapping_mul(volume.step[a])),
        volume.min,
    );
    let mut all = vec![0.0; blocks.len()];
    interpolate_blocks(f, &blocks, &mut all, input, cell, inv);
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
    input: &Sampler,
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
    let c = input.values(f, &corners);
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
