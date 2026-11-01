//! `MonsterRoomFeature`: a dungeon, a cobblestone room with a spawner and up to two chests
//! (`docs/specs/features.md#monster-room`). The same algorithm in every supported version.

use spyglass_blocks::State;
use spyglass_data::codec::LoadError;
use spyglass_density::Resources;
use spyglass_rng::{RandomSource, WorldgenRandom};

use crate::blocks::{Blocks, Tag};
use crate::outline::Outline;
use crate::place::Written;
use crate::region::Region;

/// `Direction.Plane.HORIZONTAL`: north, east, south, west, as offsets.
const HORIZONTAL: [[i32; 3]; 4] = [[0, 0, -1], [1, 0, 0], [0, 0, 1], [-1, 0, 0]];

/// A dungeon's blocks.
#[derive(Debug, Clone)]
pub struct MonsterRoom {
    cave_air: State,
    /// The walls, and a quarter of the floor.
    cobblestone: State,
    /// The rest of the floor.
    mossy: State,
    spawner: State,
    /// The chest, facing north, east, south and west.
    chests: [State; 4],
    /// Blocks no write replaces.
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
    fn safe_set(&self, region: &mut Region<'_>, pos: [i32; 3], state: State) {
        let here = region.get(pos);
        if !region.blocks().is(here, self.cannot_replace) {
            region.set(pos, state);
        }
    }

    /// Whether it surely declines at `origin` on the terrain before features, with `written`
    /// what earlier features could write (`docs/specs/features.md#ruling-out`), drawing what
    /// it draws when it declines: its size. A floor or ceiling block that surely isn't solid,
    /// or no wall block that could be air under air, and it declines.
    pub fn declines_on(
        &self,
        outline: &mut Outline<'_>,
        written: &[Written],
        random: &mut WorldgenRandom,
        origin: [i32; 3],
    ) -> bool {
        let half_x = random.next_int_bounded(2) + 2;
        let half_z = random.next_int_bounded(2) + 2;
        let [x, y, z] = origin;
        let (min_x, max_x) = (x - half_x - 1, x + half_x + 1);
        let (min_z, max_z) = (z - half_z - 1, z + half_z + 1);
        let shape = outline.shape([min_x, y - 1, min_z], [max_x, y + 4, max_z]);
        let written: Vec<&Written> = written
            .iter()
            .filter(|w| {
                w.min[0] <= max_x
                    && min_x <= w.max[0]
                    && w.min[1] <= y + 4
                    && y - 1 <= w.max[1]
                    && w.min[2] <= max_z
                    && min_z <= w.max[2]
            })
            .collect();
        let open = |p: [i32; 3]| shape.never_solid(p) && !written.iter().any(|w| w.holds(p));
        let air = |p: [i32; 3]| shape.maybe_air(p) || written.iter().any(|w| w.air && w.holds(p));
        let mut openings = false;
        for px in min_x..=max_x {
            for pz in min_z..=max_z {
                if open([px, y - 1, pz]) || open([px, y + 4, pz]) {
                    return true;
                }
                let wall = px == min_x || px == max_x || pz == min_z || pz == max_z;
                openings |= wall && air([px, y, pz]) && air([px, y + 1, pz]);
            }
        }
        !openings
    }

    /// `MonsterRoomFeature.place`: whether it placed.
    ///
    /// The room is a box of air, 4 high and 5 or 7 across on each of x and z, with a wall around
    /// it a block thick. It goes only where its floor and ceiling are solid and where between one
    /// and five doorways, air two high in the wall's line, open it to a cave. The walls are then
    /// cobblestone wherever they're solid (open walls stay open), the floor mostly mossy, and up
    /// to two chests are put against a wall, with the spawner in the middle.
    pub fn place(
        &self,
        region: &mut Region<'_>,
        random: &mut WorldgenRandom,
        origin: [i32; 3],
    ) -> bool {
        let blocks = region.blocks();
        let solid = |region: &mut Region<'_>, p: [i32; 3]| blocks.behaviour(region.get(p)).solid;
        let id = |s: State| blocks.palette().block_id(s);
        // How far the inside reaches from the origin on x and z (the game's xr and zr); the wall
        // is a block further.
        let half_x = random.next_int_bounded(2) + 2;
        let (min_x, max_x) = (-half_x - 1, half_x + 1);
        let half_z = random.next_int_bounded(2) + 2;
        let (min_z, max_z) = (-half_z - 1, half_z + 1);
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
        // A wall or floor block over a gap becomes air rather than hang over it; other solid ones
        // become cobblestone (mostly mossy on the floor), and the inside is carved to air. Top
        // down, as the game, so each test of the block below sees it unchanged.
        for dx in min_x..=max_x {
            for dy in (-1..=3).rev() {
                for dz in min_z..=max_z {
                    let p = add(origin, [dx, dy, dz]);
                    let state = region.get(p);
                    let boundary =
                        dx == min_x || dy == -1 || dz == min_z || dx == max_x || dz == max_z;
                    if boundary {
                        if p[1] >= region.min_y() && !solid(region, add(p, [0, -1, 0])) {
                            region.set(p, self.cave_air);
                        } else if blocks.behaviour(state).solid && id(state) != "minecraft:chest" {
                            let block = if dy == -1 && random.next_int_bounded(4) != 0 {
                                self.mossy
                            } else {
                                self.cobblestone
                            };
                            self.safe_set(region, p, block);
                        }
                    } else if id(state) != "minecraft:chest" && id(state) != "minecraft:spawner" {
                        self.safe_set(region, p, self.cave_air);
                    }
                }
            }
        }
        // Two chests, each with three tries at a random air block on the floor with exactly
        // one solid side: a spot against a wall, not in a corner.
        for _ in 0..2 {
            for _ in 0..3 {
                let x = origin[0]
                    .wrapping_add(random.next_int_bounded(half_x * 2 + 1))
                    .wrapping_sub(half_x);
                let z = origin[2]
                    .wrapping_add(random.next_int_bounded(half_z * 2 + 1))
                    .wrapping_sub(half_z);
                let spot = [x, origin[1], z];
                if !blocks.is_air(region.get(spot)) {
                    continue;
                }
                let neighbours = HORIZONTAL
                    .iter()
                    .filter(|&&d| solid(region, add(spot, d)))
                    .count();
                if neighbours != 1 {
                    continue;
                }
                let chest = self.reorient(region, spot);
                self.safe_set(region, spot, chest);
                // The region's chest block entity takes its loot table's seed.
                if id(region.get(spot)) == "minecraft:chest" {
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

    /// `StructurePiece.reorient` of a north-facing chest at `spot`: away from its one solid-render
    /// side, else the first of north, south, west, east that isn't solid-render.
    fn reorient(&self, region: &mut Region<'_>, spot: [i32; 3]) -> State {
        let blocks = region.blocks();
        let mut found: Option<usize> = None;
        for (d, offset) in HORIZONTAL.iter().enumerate() {
            let beside = region.get(add(spot, *offset));
            if blocks.palette().block_id(beside) == "minecraft:chest" {
                return self.chests[0];
            }
            if blocks.behaviour(beside).solid_render {
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
                .behaviour(region.get(add(spot, HORIZONTAL[d])))
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
