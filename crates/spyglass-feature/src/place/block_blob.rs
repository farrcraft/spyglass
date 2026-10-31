//! `BlockBlobFeature`: a few small balls of one block, on ground it can sit on (`forest_rock`;
//! `docs/specs/features.md#block-blob`). The same in every supported version.

use serde_json::{Map, Value};
use spyglass_blocks::State;
use spyglass_data::codec::{At, LoadError, field};
use spyglass_density::Resources;
use spyglass_rng::{RandomSource, WorldgenRandom};

use crate::blocks::Blocks;
use crate::predicate::BlockPredicate;
use crate::region::Region;

/// A block blob's configuration.
#[derive(Debug, Clone)]
pub struct BlockBlob {
    /// The block the balls are made of (mossy cobblestone in vanilla).
    state: State,
    /// What the first ball must rest on.
    can_place_on: BlockPredicate,
}

impl BlockBlob {
    /// The one state it writes.
    pub(crate) fn state(&self) -> State {
        self.state
    }

    /// From the configured feature's fields.
    pub fn load(
        o: &Map<String, Value>,
        at: &At<'_>,
        resources: &dyn Resources,
        blocks: &mut Blocks,
    ) -> Result<BlockBlob, LoadError> {
        Ok(BlockBlob {
            state: blocks.parse_state(field(o, "state", at)?, &at.child("state"))?,
            can_place_on: BlockPredicate::parse(
                field(o, "can_place_on", at)?,
                &at.child("can_place_on"),
                resources,
                blocks,
            )?,
        })
    }

    /// `BlockBlobFeature.place`: whether it placed.
    ///
    /// It drops down to ground it can sit on, then draws three small balls, each from a box of
    /// half-size 0 or 1 on each axis, rounded to the blocks within a radius that grows with the
    /// box. Each ball after the first is a block or none west, down and north of the last.
    pub fn place(
        &self,
        region: &mut Region<'_>,
        random: &mut WorldgenRandom,
        origin: [i32; 3],
    ) -> bool {
        let floor = region.min_y() + 3;
        let mut pos = origin;
        while pos[1] > floor && !self.can_place_on.test(region, [pos[0], pos[1] - 1, pos[2]]) {
            pos[1] -= 1;
        }
        if pos[1] <= floor {
            return false;
        }
        for _ in 0..3 {
            let half_x = random.next_int_bounded(2);
            let half_y = random.next_int_bounded(2);
            let half_z = random.next_int_bounded(2);
            let radius = (half_x + half_y + half_z) as f32 * 0.333 + 0.5;
            let limit = f64::from(radius * radius);
            for z in -half_z..=half_z {
                for y in -half_y..=half_y {
                    for x in -half_x..=half_x {
                        let distance_sq = f64::from(x * x + y * y + z * z);
                        if distance_sq <= limit {
                            region.set(
                                [
                                    pos[0].wrapping_add(x),
                                    pos[1].wrapping_add(y),
                                    pos[2].wrapping_add(z),
                                ],
                                self.state,
                            );
                        }
                    }
                }
            }
            let dx = -1 + random.next_int_bounded(2);
            let dy = -random.next_int_bounded(2);
            let dz = -1 + random.next_int_bounded(2);
            pos = [
                pos[0].wrapping_add(dx),
                pos[1].wrapping_add(dy),
                pos[2].wrapping_add(dz),
            ];
        }
        true
    }
}
