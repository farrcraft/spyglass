//! `FossilFeature`: a bone template buried below the ocean floor, with a coal or diamond
//! overlay (`docs/specs/features.md#fossil`). The same algorithm in every supported version.

use serde_json::{Map, Value};
use spyglass_data::codec::{At, LoadError, LoadErrorKind, field, full_id, int};
use spyglass_density::Resources;
use spyglass_rng::{RandomSource, WorldgenRandom};

use super::template::{Processor, Rotation, Template, place_in_world};
use crate::blocks::{Blocks, Heightmap};
use crate::region::Region;

/// A fossil's configuration, with its templates loaded.
#[derive(Debug, Clone)]
pub struct Fossil {
    fossils: Vec<Template>,
    overlays: Vec<Template>,
    fossil_processors: Vec<Processor>,
    overlay_processors: Vec<Processor>,
    max_empty_corners: i32,
}

impl Fossil {
    /// From the configured feature's fields.
    pub fn load(
        o: &Map<String, Value>,
        at: &At<'_>,
        resources: &dyn Resources,
        blocks: &mut Blocks,
    ) -> Result<Fossil, LoadError> {
        let templates =
            |key: &'static str, blocks: &mut Blocks| -> Result<Vec<Template>, LoadError> {
                let Value::Array(ids) = field(o, key, at)? else {
                    return Err(at
                        .child(key)
                        .error(LoadErrorKind::Shape("a list of template ids")));
                };
                ids.iter()
                    .map(|id| {
                        let id = id.as_str().map(full_id).ok_or_else(|| {
                            at.child(key).error(LoadErrorKind::Shape("a template id"))
                        })?;
                        Template::load(resources, blocks, &id)
                    })
                    .collect()
            };
        let fossils = templates("fossil_structures", blocks)?;
        let overlays = templates("overlay_structures", blocks)?;
        if fossils.is_empty() || fossils.len() != overlays.len() {
            return Err(at.error(LoadErrorKind::Invalid(
                "Fossil structure lists must be equal lengths",
            )));
        }
        let processors = |key: &'static str, blocks: &mut Blocks| {
            Processor::parse_list(field(o, key, at)?, &at.child(key), resources, blocks)
        };
        Ok(Fossil {
            fossils,
            overlays,
            fossil_processors: processors("fossil_processors", blocks)?,
            overlay_processors: processors("overlay_processors", blocks)?,
            max_empty_corners: int(
                field(o, "max_empty_corners_allowed", at)?,
                &at.child("max_empty_corners_allowed"),
            )?,
        })
    }

    /// `FossilFeature.place`: whether it placed.
    pub fn place(
        &self,
        region: &mut Region<'_>,
        random: &mut WorldgenRandom,
        origin: [i32; 3],
    ) -> bool {
        let blocks = region.blocks();
        let rotation = Rotation::ALL[random.next_int_bounded(4) as usize];
        let index = random.next_int_bounded(self.fossils.len() as i32) as usize;
        let (fossil, overlay) = (&self.fossils[index], &self.overlays[index]);
        let size = fossil.rotated_size(rotation);
        // -R / 2, truncating.
        let corner = [
            origin[0].wrapping_add(size[0].wrapping_neg() / 2),
            origin[1],
            origin[2].wrapping_add(size[2].wrapping_neg() / 2),
        ];
        let mut min_h = origin[1];
        for x in 0..size[0] {
            for z in 0..size[2] {
                let h = region.height(
                    Heightmap::OceanFloorWg,
                    corner[0].wrapping_add(x),
                    corner[2].wrapping_add(z),
                );
                min_h = min_h.min(h);
            }
        }
        let y = (min_h - 15 - random.next_int_bounded(10)).max(region.min_y() + 10);
        let zero = fossil.zero([corner[0], y, corner[2]], rotation);
        let (lo, hi) = fossil.bounding_box(zero, rotation);
        let mut empty = 0;
        for x in [lo[0], hi[0]] {
            for y in [lo[1], hi[1]] {
                for z in [lo[2], hi[2]] {
                    let s = region.get([x, y, z]);
                    let id = blocks.palette().block_id(s);
                    if blocks.is_air(s) || id == "minecraft:lava" || id == "minecraft:water" {
                        empty += 1;
                    }
                }
            }
        }
        if empty > self.max_empty_corners {
            return false;
        }
        place_in_world(
            region,
            fossil,
            zero,
            rotation,
            &self.fossil_processors,
            random,
        );
        place_in_world(
            region,
            overlay,
            zero,
            rotation,
            &self.overlay_processors,
            random,
        );
        true
    }
}
