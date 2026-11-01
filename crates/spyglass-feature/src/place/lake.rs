//! `LakeFeature`: a blob of fluid with air above, walled with a barrier
//! (`docs/specs/features.md#lake`). The same algorithm in every supported version.

use serde_json::{Map, Value};
use spyglass_blocks::State;
use spyglass_blocks::behaviour::FluidKind;
use spyglass_data::codec::{At, LoadError, LoadErrorKind, field};
use spyglass_density::Resources;
use spyglass_rng::{RandomSource, WorldgenRandom};

use crate::blocks::Blocks;
use crate::predicate::BlockPredicate;
use crate::region::Region;

/// A lake's configuration.
#[derive(Debug, Clone)]
pub struct Lake {
    /// What fills the lower half (lava in vanilla).
    fluid: State,
    /// What walls the lake where its edge is solid (stone in vanilla).
    barrier: State,
    /// What every edge block must be, or the lake declines.
    can_place: BlockPredicate,
    /// Which of the lake's blocks are carved to fluid or air.
    replace_with_air_or_fluid: BlockPredicate,
    /// Which solid edge blocks become the barrier.
    replace_with_barrier: BlockPredicate,
    /// What fills the upper half.
    cave_air: State,
}

impl Lake {
    /// From the configured feature's fields.
    pub fn load(
        o: &Map<String, Value>,
        at: &At<'_>,
        resources: &dyn Resources,
        blocks: &mut Blocks,
    ) -> Result<Lake, LoadError> {
        let predicate = |key: &'static str, blocks: &mut Blocks| {
            BlockPredicate::parse(field(o, key, at)?, &at.child(key), resources, blocks)
        };
        let can_place = predicate("can_place_feature", blocks)?;
        let replace_with_air_or_fluid = predicate("can_replace_with_air_or_fluid", blocks)?;
        let replace_with_barrier = predicate("can_replace_with_barrier", blocks)?;
        let fluid = blocks.parse_simple_provider(field(o, "fluid", at)?, &at.child("fluid"))?;
        if blocks.fluid_is(fluid, FluidKind::Water) {
            // Freezing the top, which water lakes do, isn't ported: no supported version has one.
            return Err(at.child("fluid").error(LoadErrorKind::Unsupported {
                what: "lake fluid",
                name: blocks.palette().name(fluid).to_string(),
            }));
        }
        Ok(Lake {
            fluid,
            barrier: blocks
                .parse_simple_provider(field(o, "barrier", at)?, &at.child("barrier"))?,
            can_place,
            replace_with_air_or_fluid,
            replace_with_barrier,
            cave_air: blocks.state("minecraft:cave_air")?,
        })
    }

    /// `LakeFeature.place`: whether it placed.
    ///
    /// The lake is drawn in a 16 × 16 × 8 box as the union of a few random ellipsoids, kept a
    /// block in from the box's sides. Its lower half (y below 4) is fluid and its upper half air,
    /// so the fluid sits in a pit with a hollow above it. The blocks just outside it, its edge,
    /// are checked first, so a lake never opens into air below its surface or into liquid
    /// above it, then walled with the barrier.
    pub fn place(
        &self,
        region: &mut Region<'_>,
        random: &mut WorldgenRandom,
        origin: [i32; 3],
    ) -> bool {
        if origin[1] <= region.min_y() + 4 {
            return false;
        }
        // The box's least corner.
        let [corner_x, corner_y, corner_z] = [
            origin[0].wrapping_sub(8),
            origin[1].wrapping_sub(4),
            origin[2].wrapping_sub(8),
        ];
        let at = |x: i32, y: i32, z: i32| {
            [
                corner_x.wrapping_add(x),
                corner_y.wrapping_add(y),
                corner_z.wrapping_add(z),
            ]
        };
        // Which of the box's cells are in the lake (see `index`).
        let mut in_lake = [false; 2048];
        let blobs = random.next_int_bounded(4) + 4;
        for _ in 0..blobs {
            // Each blob's size on each axis, a diameter (the game's xr, yr and zr), then its
            // centre, placed so the whole blob fits inside the box with room to spare.
            let size_x = random.next_double() * 6.0 + 3.0;
            let size_y = random.next_double() * 4.0 + 2.0;
            let size_z = random.next_double() * 6.0 + 3.0;
            let centre_x = random.next_double() * (16.0 - size_x - 2.0) + 1.0 + size_x / 2.0;
            let centre_y = random.next_double() * (8.0 - size_y - 4.0) + 2.0 + size_y / 2.0;
            let centre_z = random.next_double() * (16.0 - size_z - 2.0) + 1.0 + size_z / 2.0;
            for x in 1..15 {
                for z in 1..15 {
                    for y in 1..7 {
                        let dx = (f64::from(x) - centre_x) / (size_x / 2.0);
                        let dy = (f64::from(y) - centre_y) / (size_y / 2.0);
                        let dz = (f64::from(z) - centre_z) / (size_z / 2.0);
                        let distance_sq = dx * dx + dy * dy + dz * dz;
                        if distance_sq < 1.0 {
                            in_lake[index(x, y, z)] = true;
                        }
                    }
                }
            }
        }
        let blocks = region.blocks();
        // Every edge is checked before anything is written.
        for x in 0..16 {
            for z in 0..16 {
                for y in 0..8 {
                    if !is_edge(&in_lake, x, y, z) {
                        continue;
                    }
                    let pos = at(x, y, z);
                    let state = region.get(pos);
                    let behaviour = blocks.behaviour(state);
                    // Liquid beside the air above the surface would flow in.
                    if y >= 4 && behaviour.liquid {
                        return false;
                    }
                    // So would the fluid itself, into an open side below the surface.
                    if y < 4 && !behaviour.solid && state != self.fluid {
                        return false;
                    }
                    if !self.can_place.test(region, pos) {
                        return false;
                    }
                }
            }
        }
        for x in 0..16 {
            for z in 0..16 {
                for y in 0..8 {
                    if !in_lake[index(x, y, z)] {
                        continue;
                    }
                    let pos = at(x, y, z);
                    if !self.replace_with_air_or_fluid.test(region, pos) {
                        continue;
                    }
                    region.set(pos, if y >= 4 { self.cave_air } else { self.fluid });
                }
            }
        }
        if !blocks.is_air(self.barrier) {
            for x in 0..16 {
                for z in 0..16 {
                    for y in 0..8 {
                        // The draw comes before the block is read, for edges at y 4 and up.
                        if !is_edge(&in_lake, x, y, z)
                            || (y >= 4 && random.next_int_bounded(2) == 0)
                        {
                            continue;
                        }
                        let pos = at(x, y, z);
                        let state = region.get(pos);
                        if blocks.behaviour(state).solid
                            && self.replace_with_barrier.test(region, pos)
                        {
                            region.set(pos, self.barrier);
                        }
                    }
                }
            }
        }
        // Freezing is water's only; spyglass's lakes are lava.
        true
    }
}

/// A cell's place in the lake's grid: x, then z, then y, as the game's.
fn index(x: i32, y: i32, z: i32) -> usize {
    ((x * 16 + z) * 8 + y) as usize
}

/// Not in the lake, with a face neighbour that is, tested in the game's order.
fn is_edge(in_lake: &[bool; 2048], x: i32, y: i32, z: i32) -> bool {
    !in_lake[index(x, y, z)]
        && ((x < 15 && in_lake[index(x + 1, y, z)])
            || (x > 0 && in_lake[index(x - 1, y, z)])
            || (z < 15 && in_lake[index(x, y, z + 1)])
            || (z > 0 && in_lake[index(x, y, z - 1)])
            || (y < 7 && in_lake[index(x, y + 1, z)])
            || (y > 0 && in_lake[index(x, y - 1, z)]))
}
