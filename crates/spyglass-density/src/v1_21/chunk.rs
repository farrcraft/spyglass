//! 26.2's `NoiseChunk`: the chunk generator's evaluation context, where the router's markers
//! become caches and interpolators. See `docs/specs/density-functions.md#262-noisechunk`.
//!
//! The game's caches are stateful and filled in the generator's walk order. Here each is a table
//! filled on demand with the value the game's fill computes, which is the same wherever the
//! game's value doesn't depend on that order:
//!
//! - `interpolated`: its input at every cell corner of the chunk, the corners interpolated with
//!   `lerp3` for a cell's blocks, or by the incremental y, x, z lerps outside a fill.
//! - `flat_cache`: its input at each quart corner of the chunk's window, at y 0.
//! - `cache_once` while corners are filled: its input at each corner.
//! - `cache_2d`, `cache_all_in_cell`, and `cache_once` elsewhere: the input at the block. The game
//!   remembers the last `(x, z)`, the cell's values, or the last value in the same fill. Those
//!   equal the input there for vanilla's inputs (`cache_2d`'s don't depend on y, and every
//!   `cache_once` is under an `interpolated`); a data pack that broke that could differ.
//!
//! An `interpolated` inside another's input, evaluated while the outer one's corners are filled,
//! reads its own state in the game; here it's its input at the corner. Vanilla has none.

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;

use spyglass_noise::mth;

use super::bind::{Ctx, Function, Own, next_stamp, reserve_stamps};
use super::function::Marker;
use super::router::{CLIMATE, ChunkGraph, NoiseSettings, RandomState, RouterEntry};
use crate::java::{floor_div, floor_mod};

/// A `NoiseChunk` for a seed's [`RandomState`]: a chunk, or the one-cell column a base column
/// or height is computed in. Not thread-safe: it fills its caches as it's asked.
pub struct NoiseChunk {
    graph: Arc<ChunkGraph>,
    cell_width: i32,
    cell_height: i32,
    cell_count_xz: i32,
    cell_count_y: i32,
    cell_noise_min_y: i32,
    first_cell_x: i32,
    first_cell_z: i32,
    first_noise_x: i32,
    first_noise_z: i32,
    noise_size_xz: i32,
    /// Each marker slot's table, made on first use.
    tables: Vec<OnceCell<Box<[OnceCell<f64>]>>>,
    /// `preliminarySurfaceLevelCache`.
    surface_levels: RefCell<HashMap<(i32, i32), i32>>,
    /// The memo stamp of corner 0 ([`corner`](Self::corner)); each corner has its own, so the
    /// functions interpolated at one corner share what they have in common.
    corner_stamps: u64,
    /// The last single-point context [`compute`](Self::compute) made: its block and stamp, so
    /// router entries asked at one block share what they have in common.
    last_point: Cell<Option<([i32; 3], u64)>>,
    /// The memo's column stamp of each column contexts have been made at.
    columns: RefCell<HashMap<(i32, i32), u64>>,
}

impl NoiseChunk {
    /// `new NoiseChunk(cellCountXZ, randomState, x0, z0, noise, ...)` with no structures and
    /// nothing blending: `cell_count_xz` cells each way from block `(x0, z0)`, over `noise`'s y
    /// range (the settings clamped to the level, `NoiseSettings.clampToHeightAccessor`).
    pub fn new(
        state: &RandomState,
        cell_count_xz: i32,
        x0: i32,
        z0: i32,
        noise: NoiseSettings,
    ) -> Self {
        let graph = state.chunk_graph().clone();
        let cell_width = noise.cell_width();
        let cell_height = noise.cell_height();
        let noise_size_xz = cell_count_xz.wrapping_mul(cell_width) >> 2;
        let cell_count_y = floor_div(noise.height, cell_height);
        let corners = (cell_count_xz as u64 + 1).pow(2) * (cell_count_y.max(0) as u64 + 1);
        NoiseChunk {
            tables: (0..graph.slots).map(|_| OnceCell::new()).collect(),
            graph,
            cell_width,
            cell_height,
            cell_count_xz,
            cell_count_y,
            cell_noise_min_y: floor_div(noise.min_y, cell_height),
            first_cell_x: floor_div(x0, cell_width),
            first_cell_z: floor_div(z0, cell_width),
            first_noise_x: x0 >> 2,
            first_noise_z: z0 >> 2,
            noise_size_xz,
            surface_levels: RefCell::new(HashMap::new()),
            corner_stamps: reserve_stamps(corners),
            last_point: Cell::new(None),
            columns: RefCell::new(HashMap::new()),
        }
    }

    /// `NoiseChunk.forChunk` for the chunk at `(chunk_x, chunk_z)`, over the noise settings'
    /// y range clamped to a level from `level_min_y`, `level_height` blocks high.
    pub fn for_chunk(
        state: &RandomState,
        chunk_x: i32,
        chunk_z: i32,
        level_min_y: i32,
        level_height: i32,
    ) -> Self {
        let noise = state.noise_settings().clamped(level_min_y, level_height);
        NoiseChunk::new(
            state,
            16 / noise.cell_width(),
            chunk_x.wrapping_mul(16),
            chunk_z.wrapping_mul(16),
            noise,
        )
    }

    /// The one-cell `NoiseChunk` `iterateNoiseColumn` makes for block column `(x, z)`: at the
    /// column's cell, over the noise settings' y range clamped to the level.
    pub fn for_column(
        state: &RandomState,
        x: i32,
        z: i32,
        level_min_y: i32,
        level_height: i32,
    ) -> Self {
        let noise = state.noise_settings().clamped(level_min_y, level_height);
        let width = noise.cell_width();
        let x0 = floor_div(x, width).wrapping_mul(width);
        let z0 = floor_div(z, width).wrapping_mul(width);
        NoiseChunk::new(state, 1, x0, z0, noise)
    }

    /// `cellWidth`: `4 · size_horizontal`.
    pub fn cell_width(&self) -> i32 {
        self.cell_width
    }

    /// `cellHeight`: `4 · size_vertical`.
    pub fn cell_height(&self) -> i32 {
        self.cell_height
    }

    /// The lowest block of the cells, `cellNoiseMinY · cellHeight`.
    pub fn min_y(&self) -> i32 {
        self.cell_noise_min_y.wrapping_mul(self.cell_height)
    }

    /// The height of the cells, `cellCountY · cellHeight` (0 if the range holds no cell).
    pub fn height(&self) -> i32 {
        self.cell_count_y.max(0).wrapping_mul(self.cell_height)
    }

    /// `getInterpolatedDensity()` with the chunk positioned at block `(x, y, z)`: the final
    /// density plus the beardifier (0 here) as the cell's fill computes it, with every
    /// `interpolated` from its corners by `lerp3`. The value depends only on the block, not on
    /// the order blocks are asked for.
    ///
    /// # Panics
    ///
    /// If the block isn't in one of the chunk's cells.
    pub fn interpolated_density(&self, x: i32, y: i32, z: i32) -> f64 {
        #[cfg(feature = "timing")]
        let _t = crate::timing::span("26.2: interpolated density");
        let (cx, cy, cz, ix, iy, iz) = self.cell_of(x, y, z);
        let own = Own::Cell {
            cx,
            cy,
            cz,
            ix,
            iy,
            iz,
        };
        self.graph.density.eval(&self.ctx(x, y, z, own))
    }

    /// Router `entry` with the chunk itself as the context, positioned at block `(x, y, z)` by
    /// `updateForY`, `updateForX` and `updateForZ` outside a fill, as the ore veinifier reads
    /// `vein_toggle` and `vein_ridged`: every `interpolated` from its corners by y, then x, then
    /// z lerps.
    ///
    /// # Panics
    ///
    /// If the block isn't in one of the chunk's cells.
    pub fn compute_positioned(&self, entry: RouterEntry, x: i32, y: i32, z: i32) -> f64 {
        let (cx, cy, cz, ix, iy, iz) = self.cell_of(x, y, z);
        let own = Own::Incremental {
            cx,
            cy,
            cz,
            ix,
            iy,
            iz,
        };
        self.graph.entries[entry as usize].eval(&self.ctx(x, y, z, own))
    }

    /// Router `entry` of the chunk's wrapped router at `SinglePointContext(x, y, z)`, as the
    /// aquifer samples it: `interpolated` is its raw input, and `flat_cache` its quart corner's
    /// value inside the chunk's window.
    pub fn compute(&self, entry: RouterEntry, x: i32, y: i32, z: i32) -> f64 {
        let stamp = match self.last_point.get() {
            Some((at, stamp)) if at == [x, y, z] => stamp,
            _ => {
                let stamp = next_stamp();
                self.last_point.set(Some(([x, y, z], stamp)));
                stamp
            }
        };
        #[cfg(feature = "timing")]
        let _t = crate::timing::span("26.2: point compute (aquifer, climate)");
        self.graph.entries[entry as usize].eval(&self.ctx_stamped(x, y, z, Own::Foreign, stamp))
    }

    /// `cachedClimateSampler(...).sample` before narrowing: the six climate entries of the
    /// chunk's wrapped router at block `4 · q` (`SinglePointContext`), in temperature, humidity,
    /// continentalness, erosion, depth, weirdness order.
    pub fn climate(&self, qx: i32, qy: i32, qz: i32) -> [f64; 6] {
        let (x, y, z) = (qx.wrapping_mul(4), qy.wrapping_mul(4), qz.wrapping_mul(4));
        CLIMATE.map(|entry| self.compute(entry, x, y, z))
    }

    /// `preliminarySurfaceLevel(x, z)`: `Mth.floor` of `preliminary_surface_level` at
    /// `SinglePointContext(x & ~3, 0, z & ~3)` (quart-aligned), remembered per position.
    pub fn preliminary_surface_level(&self, x: i32, z: i32) -> i32 {
        #[cfg(feature = "timing")]
        let _t = crate::timing::span("26.2: preliminary surface level");
        let (x, z) = ((x >> 2) << 2, (z >> 2) << 2);
        if let Some(level) = self.surface_levels.borrow().get(&(x, z)) {
            return *level;
        }
        let level = mth::floor(self.compute(RouterEntry::PreliminarySurfaceLevel, x, 0, z));
        self.surface_levels.borrow_mut().insert((x, z), level);
        level
    }

    /// `maxPreliminarySurfaceLevel(minX, minZ, maxX, maxZ)`: the highest preliminary surface
    /// level at every fourth block of the box, from `Integer.MIN_VALUE`.
    pub fn max_preliminary_surface_level(
        &self,
        min_x: i32,
        min_z: i32,
        max_x: i32,
        max_z: i32,
    ) -> i32 {
        let mut max = i32::MIN;
        let mut z = min_z;
        while z <= max_z {
            let mut x = min_x;
            while x <= max_x {
                max = max.max(self.preliminary_surface_level(x, z));
                x = x.wrapping_add(4);
            }
            z = z.wrapping_add(4);
        }
        max
    }

    /// The memo's column stamp for column `(x, z)`, the same for every context of this chunk
    /// there.
    pub(crate) fn column_stamp(&self, x: i32, z: i32) -> u64 {
        *self
            .columns
            .borrow_mut()
            .entry((x, z))
            .or_insert_with(next_stamp)
    }

    /// A context of this chunk at block `(x, y, z)`, with a fresh memo stamp.
    fn ctx(&self, x: i32, y: i32, z: i32, own: Own) -> Ctx<'_> {
        self.ctx_stamped(x, y, z, own, next_stamp())
    }

    /// A context of this chunk at block `(x, y, z)`, with `stamp`, which must be fresh or one
    /// this chunk gave the same context before.
    fn ctx_stamped(&self, x: i32, y: i32, z: i32, own: Own, stamp: u64) -> Ctx<'_> {
        Ctx {
            x,
            y,
            z,
            chunk: Some(self),
            own,
            stamp,
            // A block of a cell's fill shares its column with nothing worth remembering.
            column: match own {
                Own::Cell { .. } | Own::Incremental { .. } => stamp,
                Own::Corner { .. } | Own::Foreign => self.column_stamp(x, z),
            },
        }
    }

    /// The cell holding block `(x, y, z)`, relative to the first, and the block's offsets in it.
    fn cell_of(&self, x: i32, y: i32, z: i32) -> (usize, usize, usize, i32, i32, i32) {
        let cx = floor_div(x, self.cell_width).wrapping_sub(self.first_cell_x);
        let cz = floor_div(z, self.cell_width).wrapping_sub(self.first_cell_z);
        let cy = floor_div(y, self.cell_height).wrapping_sub(self.cell_noise_min_y);
        assert!(
            (0..self.cell_count_xz).contains(&cx)
                && (0..self.cell_count_xz).contains(&cz)
                && (0..self.cell_count_y).contains(&cy),
            "block ({x}, {y}, {z}) is outside the NoiseChunk's cells"
        );
        (
            cx as usize,
            cy as usize,
            cz as usize,
            floor_mod(x, self.cell_width),
            floor_mod(y, self.cell_height),
            floor_mod(z, self.cell_width),
        )
    }

    /// Slot `slot`'s table, `len` entries, made on first use.
    fn table(&self, slot: u32, len: usize) -> &[OnceCell<f64>] {
        self.tables[slot as usize].get_or_init(|| (0..len).map(|_| OnceCell::new()).collect())
    }

    /// The input of the `interpolated` or `cache_once` in `slot` at corner `(cx, cy, cz)`, as
    /// `fillSlice` computes it: with the chunk itself as the context, at the corner.
    fn corner(&self, slot: u32, input: &Function, cx: usize, cy: usize, cz: usize) -> f64 {
        let n = self.cell_count_xz as usize + 1;
        let ny = self.cell_count_y as usize + 1;
        let table = self.table(slot, n * n * ny);
        let index = (cx * n + cz) * ny + cy;
        *table[index].get_or_init(|| {
            #[cfg(feature = "timing")]
            let _t = crate::timing::span("26.2: cell corner (fillSlice)");
            let x = self
                .first_cell_x
                .wrapping_add(cx as i32)
                .wrapping_mul(self.cell_width);
            let y = (cy as i32)
                .wrapping_add(self.cell_noise_min_y)
                .wrapping_mul(self.cell_height);
            let z = self
                .first_cell_z
                .wrapping_add(cz as i32)
                .wrapping_mul(self.cell_width);
            let stamp = self.corner_stamps + index as u64;
            input.eval(&self.ctx_stamped(x, y, z, Own::Corner { cx, cy, cz }, stamp))
        })
    }

    /// A marker of this chunk's wrapped graph, evaluated at `ctx`.
    pub(crate) fn marker(
        &self,
        marker: Marker,
        input: &Function,
        binder: u64,
        slot: u32,
        ctx: &Ctx<'_>,
    ) -> f64 {
        assert_eq!(
            binder, self.graph.binder,
            "a function bound for another random state, evaluated in this NoiseChunk"
        );
        match marker {
            Marker::Interpolated => match ctx.own {
                Own::Cell {
                    cx,
                    cy,
                    cz,
                    ix,
                    iy,
                    iz,
                } => {
                    let n = |dx, dy, dz| self.corner(slot, input, cx + dx, cy + dy, cz + dz);
                    let (cw, ch) = (f64::from(self.cell_width), f64::from(self.cell_height));
                    // NoiseInterpolator.compute: Mth.lerp3, x, then y, then z, each t a division.
                    mth::lerp3_f64(
                        f64::from(ix) / cw,
                        f64::from(iy) / ch,
                        f64::from(iz) / cw,
                        n(0, 0, 0),
                        n(1, 0, 0),
                        n(0, 1, 0),
                        n(1, 1, 0),
                        n(0, 0, 1),
                        n(1, 0, 1),
                        n(0, 1, 1),
                        n(1, 1, 1),
                    )
                }
                Own::Incremental {
                    cx,
                    cy,
                    cz,
                    ix,
                    iy,
                    iz,
                } => {
                    let n = |dx, dy, dz| self.corner(slot, input, cx + dx, cy + dy, cz + dz);
                    let (cw, ch) = (f64::from(self.cell_width), f64::from(self.cell_height));
                    let (tx, ty, tz) = (f64::from(ix) / cw, f64::from(iy) / ch, f64::from(iz) / cw);
                    // updateForY, then updateForX, then updateForZ.
                    let xz00 = mth::lerp_f64(ty, n(0, 0, 0), n(0, 1, 0));
                    let xz10 = mth::lerp_f64(ty, n(1, 0, 0), n(1, 1, 0));
                    let xz01 = mth::lerp_f64(ty, n(0, 0, 1), n(0, 1, 1));
                    let xz11 = mth::lerp_f64(ty, n(1, 0, 1), n(1, 1, 1));
                    let z0 = mth::lerp_f64(tx, xz00, xz10);
                    let z1 = mth::lerp_f64(tx, xz01, xz11);
                    mth::lerp_f64(tz, z0, z1)
                }
                // Another context gets the raw input; see the module docs for corners.
                Own::Corner { .. } | Own::Foreign => input.eval(ctx),
            },
            Marker::FlatCache => {
                // QuartPos.fromBlock, relative to the window's first quart.
                let (qx, qz) = (ctx.x >> 2, ctx.z >> 2);
                let i = qx.wrapping_sub(self.first_noise_x);
                let j = qz.wrapping_sub(self.first_noise_z);
                let size = self.noise_size_xz + 1;
                if (0..size).contains(&i) && (0..size).contains(&j) {
                    let table = self.table(slot, (size * size) as usize);
                    *table[(i + j * size) as usize]
                        .get_or_init(|| input.eval(&self.ctx(qx << 2, 0, qz << 2, Own::Foreign)))
                } else {
                    input.eval(ctx)
                }
            }
            Marker::CacheOnce => match ctx.own {
                Own::Corner { cx, cy, cz } => self.corner(slot, input, cx, cy, cz),
                _ => input.eval(ctx),
            },
            Marker::Cache2D | Marker::CacheAllInCell | Marker::BlendDensity => input.eval(ctx),
        }
    }
}
