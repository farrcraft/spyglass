//! `MonsterRoomFeature`: a dungeon, a cobblestone room with a spawner and up to two chests
//! (`docs/specs/features.md#monster-room`). The same algorithm in every supported version.

use spyglass_blocks::State;
use spyglass_data::codec::LoadError;
use spyglass_density::Resources;
use spyglass_rng::{RandomSource, WorldgenRandom};

use crate::blocks::{Blocks, Tag};
use crate::region::Region;

/// `Direction.Plane.HORIZONTAL`: north, east, south, west, as offsets.
const HORIZONTAL: [[i32; 3]; 4] = [[0, 0, -1], [1, 0, 0], [0, 0, 1], [-1, 0, 0]];

/// A dungeon's blocks.
#[derive(Debug, Clone)]
pub struct MonsterRoom {
    cave_air: State,
    cobblestone: State,
    mossy: State,
    spawner: State,
    /// The chest, facing north, east, south and west.
    chests: [State; 4],
    cannot_replace: Tag,
}

impl MonsterRoom {
    /// Its blocks; the configured feature has no fields.
    pub fn load(resources: &dyn Resources, blocks: &mut Blocks) -> Result<MonsterRoom, LoadError> {
        Ok(MonsterRoom {
            cave_air: blocks.state("minecraft:cave_air")?,
            cobblestone: blocks.state("minecraft:cobblestone")?,
            mossy: blocks.state("minecraft:mossy_cobblestone")?,
            spawner: blocks.state("minecraft:spawner")?,
            chests: [
                blocks.state("minecraft:chest")?,
                blocks.state("minecraft:chest[facing=east]")?,
                blocks.state("minecraft:chest[facing=south]")?,
                blocks.state("minecraft:chest[facing=west]")?,
            ],
            cannot_replace: blocks.tag(resources, "minecraft:features_cannot_replace")?,
        })
    }

    /// `safeSetBlock`: writes unless the block there is in `#features_cannot_replace`.
    fn safe_set(&self, region: &mut Region<'_>, pos: [i32; 3], s: State) {
        let here = region.get(pos);
        if !region.blocks().is(here, self.cannot_replace) {
            region.set(pos, s);
        }
    }

    /// `MonsterRoomFeature.place`: whether it placed.
    pub fn place(
        &self,
        region: &mut Region<'_>,
        random: &mut WorldgenRandom,
        origin: [i32; 3],
    ) -> bool {
        let blocks = region.blocks();
        let solid = |region: &mut Region<'_>, p: [i32; 3]| blocks.behaviour(region.get(p)).solid;
        let id = |s: State| blocks.palette().block_id(s);
        let xr = random.next_int_bounded(2) + 2;
        let (min_x, max_x) = (-xr - 1, xr + 1);
        let zr = random.next_int_bounded(2) + 2;
        let (min_z, max_z) = (-zr - 1, zr + 1);
        let mut openings = 0;
        for dx in min_x..=max_x {
            for dy in -1..=4 {
                for dz in min_z..=max_z {
                    let p = add(origin, [dx, dy, dz]);
                    let is_solid = solid(region, p);
                    if (dy == -1 || dy == 4) && !is_solid {
                        return false;
                    }
                    if (dx == min_x || dx == max_x || dz == min_z || dz == max_z)
                        && dy == 0
                        && blocks.is_air(region.get(p))
                        && blocks.is_air(region.get(add(p, [0, 1, 0])))
                    {
                        openings += 1;
                    }
                }
            }
        }
        if !(1..=5).contains(&openings) {
            return false;
        }
        for dx in min_x..=max_x {
            for dy in (-1..=3).rev() {
                for dz in min_z..=max_z {
                    let p = add(origin, [dx, dy, dz]);
                    let s = region.get(p);
                    let boundary =
                        dx == min_x || dy == -1 || dz == min_z || dx == max_x || dz == max_z;
                    if boundary {
                        if p[1] >= region.min_y() && !solid(region, add(p, [0, -1, 0])) {
                            region.set(p, self.cave_air);
                        } else if blocks.behaviour(s).solid && id(s) != "minecraft:chest" {
                            let block = if dy == -1 && random.next_int_bounded(4) != 0 {
                                self.mossy
                            } else {
                                self.cobblestone
                            };
                            self.safe_set(region, p, block);
                        }
                    } else if id(s) != "minecraft:chest" && id(s) != "minecraft:spawner" {
                        self.safe_set(region, p, self.cave_air);
                    }
                }
            }
        }
        for _ in 0..2 {
            for _ in 0..3 {
                let x = origin[0]
                    .wrapping_add(random.next_int_bounded(xr * 2 + 1))
                    .wrapping_sub(xr);
                let z = origin[2]
                    .wrapping_add(random.next_int_bounded(zr * 2 + 1))
                    .wrapping_sub(zr);
                let c = [x, origin[1], z];
                if !blocks.is_air(region.get(c)) {
                    continue;
                }
                let neighbours = HORIZONTAL
                    .iter()
                    .filter(|&&d| solid(region, add(c, d)))
                    .count();
                if neighbours != 1 {
                    continue;
                }
                let chest = self.reorient(region, c);
                self.safe_set(region, c, chest);
                // The region's chest block entity takes its loot table's seed.
                if id(region.get(c)) == "minecraft:chest" {
                    random.next_long();
                }
                break;
            }
        }
        self.safe_set(region, origin, self.spawner);
        if region.get(origin) == self.spawner {
            // The spawner's mob: nextInt(4) into skeleton, zombie, zombie, spider.
            random.next_int_bounded(4);
        }
        true
    }

    /// `StructurePiece.reorient` of a north-facing chest at `c`: away from its one solid-render
    /// side, else the first of north, south, west, east that isn't solid-render.
    fn reorient(&self, region: &mut Region<'_>, c: [i32; 3]) -> State {
        let blocks = region.blocks();
        let mut found: Option<usize> = None;
        for (d, offset) in HORIZONTAL.iter().enumerate() {
            let n = region.get(add(c, *offset));
            if blocks.palette().block_id(n) == "minecraft:chest" {
                return self.chests[0];
            }
            if blocks.behaviour(n).solid_render {
                if found.is_none() {
                    found = Some(d);
                } else {
                    found = None;
                    break;
                }
            }
        }
        if let Some(d) = found {
            return self.chests[(d + 2) % 4];
        }
        let solid_render = |region: &mut Region<'_>, d: usize| {
            blocks
                .behaviour(region.get(add(c, HORIZONTAL[d])))
                .solid_render
        };
        // North, then its opposite, then clockwise, then opposite: indices into HORIZONTAL.
        let mut d = 0;
        if solid_render(region, d) {
            d = (d + 2) % 4;
        }
        if solid_render(region, d) {
            d = (d + 1) % 4;
        }
        if solid_render(region, d) {
            d = (d + 2) % 4;
        }
        self.chests[d]
    }
}

fn add(a: [i32; 3], b: [i32; 3]) -> [i32; 3] {
    [
        a[0].wrapping_add(b[0]),
        a[1].wrapping_add(b[1]),
        a[2].wrapping_add(b[2]),
    ]
}
