//! Ocean ruin layout (`OceanRuinStructure.generatePieces` and `OceanRuinPieces`): one ruin, or a
//! large one with a cluster of small ones around it, each a template, or three for a cold ruin
//! (brick, cracked and mossy layers of decreasing integrity). See
//! `docs/specs/structure-placement.md#ocean-ruins`.
//!
//! Pieces are laid out at y 90; each chunk that places one settles its height on the ocean floor
//! there. Nothing in the layout reads the world.

use std::sync::Arc;

use spyglass_rng::{LegacyRandomSource, RandomSource, worldgen};

use crate::assembly::Layout;
use crate::jigsaw::{BoundingBox, Rotation, Template};

/// A warm ruin's templates, `minecraft:underwater_ruin/<name>`: `WARM_RUINS`, then
/// `BIG_WARM_RUINS`.
pub(crate) const WARM: [&str; 12] = [
    "underwater_ruin/warm_1",
    "underwater_ruin/warm_2",
    "underwater_ruin/warm_3",
    "underwater_ruin/warm_4",
    "underwater_ruin/warm_5",
    "underwater_ruin/warm_6",
    "underwater_ruin/warm_7",
    "underwater_ruin/warm_8",
    "underwater_ruin/big_warm_4",
    "underwater_ruin/big_warm_5",
    "underwater_ruin/big_warm_6",
    "underwater_ruin/big_warm_7",
];

/// A cold ruin's templates: `RUINS_BRICK`, `RUINS_CRACKED`, `RUINS_MOSSY`, then
/// `BIG_RUINS_BRICK`, `BIG_RUINS_CRACKED`, `BIG_RUINS_MOSSY`.
pub(crate) const COLD: [&str; 36] = [
    "underwater_ruin/brick_1",
    "underwater_ruin/brick_2",
    "underwater_ruin/brick_3",
    "underwater_ruin/brick_4",
    "underwater_ruin/brick_5",
    "underwater_ruin/brick_6",
    "underwater_ruin/brick_7",
    "underwater_ruin/brick_8",
    "underwater_ruin/cracked_1",
    "underwater_ruin/cracked_2",
    "underwater_ruin/cracked_3",
    "underwater_ruin/cracked_4",
    "underwater_ruin/cracked_5",
    "underwater_ruin/cracked_6",
    "underwater_ruin/cracked_7",
    "underwater_ruin/cracked_8",
    "underwater_ruin/mossy_1",
    "underwater_ruin/mossy_2",
    "underwater_ruin/mossy_3",
    "underwater_ruin/mossy_4",
    "underwater_ruin/mossy_5",
    "underwater_ruin/mossy_6",
    "underwater_ruin/mossy_7",
    "underwater_ruin/mossy_8",
    "underwater_ruin/big_brick_1",
    "underwater_ruin/big_brick_2",
    "underwater_ruin/big_brick_3",
    "underwater_ruin/big_brick_8",
    "underwater_ruin/big_cracked_1",
    "underwater_ruin/big_cracked_2",
    "underwater_ruin/big_cracked_3",
    "underwater_ruin/big_cracked_8",
    "underwater_ruin/big_mossy_1",
    "underwater_ruin/big_mossy_2",
    "underwater_ruin/big_mossy_3",
    "underwater_ruin/big_mossy_8",
];

/// An ocean ruin's settings (`OceanRuinStructure`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OceanRuin {
    /// `biome_temp` is `cold`: three layers a ruin, rather than one warm template.
    pub cold: bool,
    /// `large_probability`: how often the ruin is large.
    pub large_probability: f32,
    /// `cluster_probability`: how often a large ruin has small ones around it.
    pub cluster_probability: f32,
}

/// One of an ocean ruin's pieces (`OceanRuinPiece`), as laid out.
#[derive(Debug, Clone, PartialEq)]
pub struct RuinPiece {
    /// Its template.
    pub template: Arc<Template>,
    /// Where its corner goes (`templatePosition`), at y 90 before its height is settled.
    pub pos: [i32; 3],
    /// Its rotation, about the corner.
    pub rotation: Rotation,
    /// The share of its blocks kept (`BlockRotProcessor`).
    pub integrity: f32,
    /// `isLarge`: its chest's table is the big ruin's.
    pub large: bool,
}

impl RuinPiece {
    /// Its box, at the height its corner is at.
    pub fn bounds(&self) -> BoundingBox {
        self.template.bounding_box(self.pos, self.rotation)
    }
}

/// One start's generation.
struct Ruins<'a> {
    config: &'a OceanRuin,
    templates: &'a [Arc<Template>],
    random: LegacyRandomSource,
    pieces: Vec<RuinPiece>,
}

impl Ruins<'_> {
    /// `addPiece`: a warm template, or a cold ruin's three layers from one index.
    fn add(&mut self, pos: [i32; 3], rotation: Rotation, large: bool, integrity: f32) {
        let piece = |t: &Arc<Template>, integrity: f32| RuinPiece {
            template: t.clone(),
            pos,
            rotation,
            integrity,
            large,
        };
        if self.config.cold {
            // Small: brick 0..8, cracked 8..16, mossy 16..24; big: 24.., 28.., 32...
            let (first, count) = if large { (24, 4) } else { (0, 8) };
            let i = self.random.next_int_bounded(count) as usize;
            for (layer, integrity) in [(0, integrity), (1, 0.7), (2, 0.5)] {
                let t = &self.templates[first + layer * count as usize + i];
                self.pieces.push(piece(t, integrity));
            }
        } else {
            let i = if large {
                8 + self.random.next_int_bounded(4) as usize
            } else {
                self.random.next_int_bounded(8) as usize
            };
            self.pieces.push(piece(&self.templates[i], integrity));
        }
    }

    /// `addClusterRuins`: up to eight spots around the large ruin, `nextInt(5) + 4` of them
    /// picked, each a small ruin turned at random unless its footprint meets the large one's.
    fn cluster(&mut self, rotation: Rotation, pos: [i32; 3]) {
        let start = [pos[0], 90, pos[2]];
        let far = rotation.transform([15, 0, 15]);
        let far = [start[0] + far[0], start[1] + far[1], start[2] + far[2]];
        let main = BoundingBox {
            min: [0, 1, 2].map(|i| start[i].min(far[i])),
            max: [0, 1, 2].map(|i| start[i].max(far[i])),
        };
        let corner = [start[0].min(far[0]), start[1], start[2].min(far[2])];
        // allPositions: each spot's x drawn before its z.
        let r = &mut self.random;
        let mut spot = |dx: (i32, i32), dz: (i32, i32)| {
            let x = dx.0 + r.next_int_bounded(dx.1) + 1;
            let z = dz.0 + r.next_int_bounded(dz.1) + 1;
            [
                corner[0].wrapping_add(x),
                corner[1],
                corner[2].wrapping_add(z),
            ]
        };
        let mut spots = vec![
            spot((-16, 8), (16, 7)),
            spot((-16, 8), (0, 7)),
            spot((-16, 8), (-13, 5)),
            spot((0, 7), (16, 7)),
            spot((0, 7), (-13, 3)),
            spot((16, 7), (18, 6)),
            spot((16, 7), (0, 7)),
            spot((16, 7), (-13, 5)),
        ];
        let count = self.random.next_int_bounded(5) + 4;
        for _ in 0..count {
            if spots.is_empty() {
                continue;
            }
            let i = self.random.next_int_bounded(spots.len() as i32) as usize;
            let p = spots.remove(i);
            let turn = Rotation::random(&mut self.random);
            let q = turn.transform([5, 0, 6]);
            let q = [p[0] + q[0], p[1] + q[1], p[2] + q[2]];
            let footprint = BoundingBox {
                min: [0, 1, 2].map(|i| p[i].min(q[i])),
                max: [0, 1, 2].map(|i| p[i].max(q[i])),
            };
            if footprint.intersects(&main) {
                continue;
            }
            self.add(p, turn, false, 0.8);
        }
    }
}

impl OceanRuin {
    /// The start's pieces in `chunk` for world `seed`, in the game's order. `templates` are
    /// [`COLD`]'s or [`WARM`]'s, in order.
    pub(crate) fn pieces(
        &self,
        templates: &[Arc<Template>],
        seed: i64,
        chunk: [i32; 2],
    ) -> Vec<RuinPiece> {
        let mut random = worldgen::large_feature(seed, chunk[0], chunk[1]);
        let pos = [chunk[0].wrapping_shl(4), 90, chunk[1].wrapping_shl(4)];
        let rotation = Rotation::random(&mut random);
        let mut ruins = Ruins {
            config: self,
            templates,
            random,
            pieces: Vec::new(),
        };
        let large = ruins.random.next_float() <= self.large_probability;
        ruins.add(pos, rotation, large, if large { 0.9 } else { 0.8 });
        if large && ruins.random.next_float() <= self.cluster_probability {
            ruins.cluster(rotation, pos);
        }
        ruins.pieces
    }
}

/// The pieces as a [`Layout`], at y 90: ocean ruins don't adapt terrain, so the bounds are the
/// pieces'.
pub(crate) fn layout(pieces: &[RuinPiece]) -> Option<Layout> {
    let boxes: Vec<BoundingBox> = pieces.iter().map(RuinPiece::bounds).collect();
    let bounds = boxes.iter().copied().reduce(BoundingBox::encapsulate)?;
    Some(Layout {
        pieces: boxes,
        placed: Vec::new(),
        bounds,
    })
}
