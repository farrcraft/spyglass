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
    state: State,
    can_place_on: BlockPredicate,
}

impl BlockBlob {
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
            let a = random.next_int_bounded(2);
            let b = random.next_int_bounded(2);
            let c = random.next_int_bounded(2);
            let f = (a + b + c) as f32 * 0.333 + 0.5;
            let limit = f64::from(f * f);
            for z in -c..=c {
                for y in -b..=b {
                    for x in -a..=a {
                        let d = f64::from(x * x + y * y + z * z);
                        if d <= limit {
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
