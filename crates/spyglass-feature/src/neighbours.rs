//! What could write a chunk's blocks that spyglass doesn't place exactly: its neighbours, if
//! they're decorated first (ADR 0055), and its own features that spyglass doesn't place or
//! isn't sure of. Every try as a box, grown by where its feature can write
//! (`docs/specs/features.md#certainty`).

use spyglass_biome::BiomeId;
use spyglass_data::codec::{At, LoadError};
use spyglass_density::Resources;

use crate::blocks::Heightmap;
use crate::decorate::{Decorator, Ground};
use crate::order::PlacedRef;
use crate::reach::{Changes, Reach};
use crate::region::Region;
use crate::settings::FeatureSettings;

/// How far below the carved terrain's a live heightmap can be when a feature reads it: the
/// deepest earlier features carve near the surface (a lake's air 4 above its fluid, a
/// dungeon's 5, a crevasse's 10, a geode's 16), with room to spare.
const SURFACE_DROP: i32 = 48;

/// Blocks a feature could write, in the level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Foreign {
    /// The least corner.
    pub min: [i32; 3],
    /// The greatest corner.
    pub max: [i32; 3],
    /// What its writes change.
    pub changes: Changes,
    /// The placed feature.
    pub placed: PlacedRef,
    /// Its decoration step and index, for a chunk's own.
    pub order: (usize, usize),
}

/// Every placed feature's write bounds, seed-independent.
#[derive(Debug, Clone)]
pub struct NeighbourBounds {
    reaches: Vec<Option<Reach>>,
}

impl NeighbourBounds {
    /// The write bounds of every placed feature in `settings`' order.
    pub fn load(resources: &dyn Resources, settings: &FeatureSettings) -> Result<Self, LoadError> {
        let mut reaches = Vec::new();
        for &r in settings.order().iter().flatten() {
            if reaches.len() <= r {
                reaches.resize(r + 1, None);
            }
            if reaches[r].is_none() {
                let p = settings.placed(r);
                let name =
                    p.id.clone()
                        .unwrap_or_else(|| "an inline placed feature".into());
                reaches[r] = Some(Reach::of_placed(resources, &p.json, &At::root(&name))?);
            }
        }
        Ok(NeighbourBounds { reaches })
    }

    /// What the 8 neighbours of `region`'s chunk could write inside it, with `decorator` asking
    /// for every placed feature of `settings`. `region` is as carvers left it: its heightmaps
    /// bound the neighbours'. `biomes` gives the biomes a chunk can store
    /// (`ChunkGenerator::chunk_biomes`).
    pub fn writes(
        &self,
        decorator: &Decorator<'_>,
        settings: &FeatureSettings,
        region: &Region<'_>,
        biomes: &mut dyn FnMut([i32; 2]) -> Vec<BiomeId>,
    ) -> Vec<Foreign> {
        let [cx, cz] = region.center();
        let mut out = Vec::new();
        for dz in -1..=1 {
            for dx in -1..=1 {
                if dx != 0 || dz != 0 {
                    self.chunk_writes(
                        decorator,
                        settings,
                        Heights::Region(region),
                        biomes,
                        [cx + dx, cz + dz],
                        &mut out,
                    );
                }
            }
        }
        out
    }

    /// What every placed feature of `region`'s own chunk could write, wherever its tries can
    /// be: for the features spyglass doesn't place, and those it places once it isn't sure of
    /// them.
    pub fn own_writes(
        &self,
        decorator: &Decorator<'_>,
        settings: &FeatureSettings,
        region: &Region<'_>,
        biomes: &mut dyn FnMut([i32; 2]) -> Vec<BiomeId>,
    ) -> Vec<Foreign> {
        let mut out = Vec::new();
        self.chunk_writes(
            decorator,
            settings,
            Heights::Region(region),
            biomes,
            region.center(),
            &mut out,
        );
        out
    }

    /// [`own_writes`](Self::own_writes) of chunk `chunk` before its blocks are built: a
    /// heightmap can give any height in the level, `min_y..=max_y`.
    pub fn outline_writes(
        &self,
        decorator: &Decorator<'_>,
        settings: &FeatureSettings,
        chunk: [i32; 2],
        [min_y, max_y]: [i32; 2],
        biomes: &mut dyn FnMut([i32; 2]) -> Vec<BiomeId>,
    ) -> Vec<Foreign> {
        let mut out = Vec::new();
        let heights = Heights::Anywhere {
            center: chunk,
            min_y,
            max_y,
        };
        self.chunk_writes(decorator, settings, heights, biomes, chunk, &mut out);
        out
    }

    fn chunk_writes(
        &self,
        decorator: &Decorator<'_>,
        settings: &FeatureSettings,
        heights: Heights<'_, '_>,
        biomes: &mut dyn FnMut([i32; 2]) -> Vec<BiomeId>,
        n: [i32; 2],
        out: &mut Vec<Foreign>,
    ) {
        let ([cx, cz], min_y, max_y) = match heights {
            Heights::Region(region) => (region.center(), region.min_y(), region.max_y()),
            Heights::Anywhere {
                center,
                min_y,
                max_y,
            } => (center, min_y, max_y),
        };
        let lo = [(cx - 1) * 16, min_y, (cz - 1) * 16];
        let hi = [(cx + 2) * 16 - 1, max_y, (cz + 2) * 16 - 1];
        // The placed features some biome around the chunk lists.
        let mut listed = vec![false; self.reaches.len()];
        for dz in -1..=1 {
            for dx in -1..=1 {
                for b in biomes([n[0] + dx, n[1] + dz]) {
                    for &r in settings.lists(b).iter().flatten() {
                        if let Some(l) = listed.get_mut(r) {
                            *l = true;
                        }
                    }
                }
            }
        }
        let ground = Chunk { listed, heights };
        for t in decorator.try_boxes(n, min_y, max_y, &ground) {
            let Some(Some(reach)) = self.reaches.get(t.placed) else {
                continue;
            };
            for w in &reach.0 {
                let mut min = [0; 3];
                let mut max = [0; 3];
                for i in 0..3 {
                    min[i] = t.min[i].saturating_add(w.min[i]).max(lo[i]);
                    max[i] = t.max[i].saturating_add(w.max[i]).min(hi[i]);
                }
                if w.column {
                    min[1] = min_y;
                    max[1] = max_y;
                }
                if (0..3).all(|i| min[i] <= max[i]) {
                    out.push(Foreign {
                        min,
                        max,
                        changes: w.changes,
                        placed: t.placed,
                        order: (t.step, t.index),
                    });
                }
            }
        }
    }
}

/// Where a chunk's heightmaps come from.
#[derive(Clone, Copy)]
enum Heights<'r, 'b> {
    /// The carved terrain's, around the region's chunk.
    Region(&'r Region<'b>),
    /// Anywhere in the level, around `center`.
    Anywhere {
        center: [i32; 2],
        min_y: i32,
        max_y: i32,
    },
}

/// A chunk's ground: which features run there, and the heightmaps.
struct Chunk<'r, 'b> {
    listed: Vec<bool>,
    heights: Heights<'r, 'b>,
}

impl Ground for Chunk<'_, '_> {
    fn runs(&self, placed: PlacedRef) -> bool {
        self.listed.get(placed).copied().unwrap_or(true)
    }

    fn heights(&self, h: Heightmap, lo: [i32; 2], hi: [i32; 2]) -> (i32, i32) {
        let frozen = matches!(h, Heightmap::WorldSurfaceWg | Heightmap::OceanFloorWg);
        let region = match self.heights {
            Heights::Region(region) => region,
            Heights::Anywhere { min_y, max_y, .. } => return (min_y, max_y + 1),
        };
        let anywhere = (region.min_y(), region.max_y() + 1);
        if hi[0] - lo[0] > 64 || hi[1] - lo[1] > 64 {
            return anywhere;
        }
        let (mut least, mut most) = (i32::MAX, i32::MIN);
        for x in lo[0]..=hi[0] {
            for z in lo[1]..=hi[1] {
                let Some(y) = region.terrain_height(h, x, z) else {
                    return anywhere;
                };
                least = least.min(y);
                most = most.max(y);
            }
        }
        if frozen {
            (least, most)
        } else {
            (
                (least - SURFACE_DROP).max(region.min_y()),
                region.max_y() + 1,
            )
        }
    }
}
