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
    fluid: State,
    barrier: State,
    can_place: BlockPredicate,
    replace_with_air_or_fluid: BlockPredicate,
    replace_with_barrier: BlockPredicate,
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
    pub fn place(
        &self,
        region: &mut Region<'_>,
        random: &mut WorldgenRandom,
        origin: [i32; 3],
    ) -> bool {
        if origin[1] <= region.min_y() + 4 {
            return false;
        }
        let [ox, oy, oz] = [
            origin[0].wrapping_sub(8),
            origin[1].wrapping_sub(4),
            origin[2].wrapping_sub(8),
        ];
        let at =
            |x: i32, y: i32, z: i32| [ox.wrapping_add(x), oy.wrapping_add(y), oz.wrapping_add(z)];
        let mut grid = [false; 2048];
        let blobs = random.next_int_bounded(4) + 4;
        for _ in 0..blobs {
            let xr = random.next_double() * 6.0 + 3.0;
            let yr = random.next_double() * 4.0 + 2.0;
            let zr = random.next_double() * 6.0 + 3.0;
            let cx = random.next_double() * (16.0 - xr - 2.0) + 1.0 + xr / 2.0;
            let cy = random.next_double() * (8.0 - yr - 4.0) + 2.0 + yr / 2.0;
            let cz = random.next_double() * (16.0 - zr - 2.0) + 1.0 + zr / 2.0;
            for x in 1..15 {
                for z in 1..15 {
                    for y in 1..7 {
                        let dx = (f64::from(x) - cx) / (xr / 2.0);
                        let dy = (f64::from(y) - cy) / (yr / 2.0);
                        let dz = (f64::from(z) - cz) / (zr / 2.0);
                        let d = dx * dx + dy * dy + dz * dz;
                        if d < 1.0 {
                            grid[index(x, y, z)] = true;
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
                    if !is_edge(&grid, x, y, z) {
                        continue;
                    }
                    let pos = at(x, y, z);
                    let s = region.get(pos);
                    let b = blocks.behaviour(s);
                    if y >= 4 && b.liquid {
                        return false;
                    }
                    if y < 4 && !b.solid && s != self.fluid {
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
                    if !grid[index(x, y, z)] {
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
                        if !is_edge(&grid, x, y, z) || (y >= 4 && random.next_int_bounded(2) == 0) {
                            continue;
                        }
                        let pos = at(x, y, z);
                        let s = region.get(pos);
                        if blocks.behaviour(s).solid && self.replace_with_barrier.test(region, pos)
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

fn index(x: i32, y: i32, z: i32) -> usize {
    ((x * 16 + z) * 8 + y) as usize
}

/// Not in the lake, with a face neighbour that is, tested in the game's order.
fn is_edge(g: &[bool; 2048], x: i32, y: i32, z: i32) -> bool {
    !g[index(x, y, z)]
        && ((x < 15 && g[index(x + 1, y, z)])
            || (x > 0 && g[index(x - 1, y, z)])
            || (z < 15 && g[index(x, y, z + 1)])
            || (z > 0 && g[index(x, y, z - 1)])
            || (y < 7 && g[index(x, y + 1, z)])
            || (y > 0 && g[index(x, y - 1, z)]))
}
