//! The template pieces of shipwrecks and igloos (`ShipwreckPieces`, `IglooPieces`): which
//! templates a start lays out, where and turned how, from its generation random. Their heights
//! are settled when a chunk places them. See `docs/specs/loot.md#containers`.

use std::sync::Arc;

use spyglass_rng::{RandomSource, worldgen};

use crate::jigsaw::{Rotation, Template};
use crate::load::{Structure, StructureType};

/// `ShipwreckPieces.STRUCTURE_LOCATION_BEACHED`, in the game's order, which the pick indexes.
pub(crate) const SHIPWRECKS_BEACHED: [&str; 11] = [
    "shipwreck/with_mast",
    "shipwreck/sideways_full",
    "shipwreck/sideways_fronthalf",
    "shipwreck/sideways_backhalf",
    "shipwreck/rightsideup_full",
    "shipwreck/rightsideup_fronthalf",
    "shipwreck/rightsideup_backhalf",
    "shipwreck/with_mast_degraded",
    "shipwreck/rightsideup_full_degraded",
    "shipwreck/rightsideup_fronthalf_degraded",
    "shipwreck/rightsideup_backhalf_degraded",
];

/// `ShipwreckPieces.STRUCTURE_LOCATION_OCEAN`, in the game's order.
pub(crate) const SHIPWRECKS_OCEAN: [&str; 20] = [
    "shipwreck/with_mast",
    "shipwreck/upsidedown_full",
    "shipwreck/upsidedown_fronthalf",
    "shipwreck/upsidedown_backhalf",
    "shipwreck/sideways_full",
    "shipwreck/sideways_fronthalf",
    "shipwreck/sideways_backhalf",
    "shipwreck/rightsideup_full",
    "shipwreck/rightsideup_fronthalf",
    "shipwreck/rightsideup_backhalf",
    "shipwreck/with_mast_degraded",
    "shipwreck/upsidedown_full_degraded",
    "shipwreck/upsidedown_fronthalf_degraded",
    "shipwreck/upsidedown_backhalf_degraded",
    "shipwreck/sideways_full_degraded",
    "shipwreck/sideways_fronthalf_degraded",
    "shipwreck/sideways_backhalf_degraded",
    "shipwreck/rightsideup_full_degraded",
    "shipwreck/rightsideup_fronthalf_degraded",
    "shipwreck/rightsideup_backhalf_degraded",
];

/// `IglooPieces`' templates: the igloo, the ladder and the laboratory.
pub(crate) const IGLOO: [&str; 3] = ["igloo/top", "igloo/middle", "igloo/bottom"];

/// `ShipwreckPieces.PIVOT`.
const SHIPWRECK_PIVOT: [i32; 3] = [4, 0, 15];

/// `IglooPieces.PIVOTS` and `OFFSETS`, for the top, ladder and laboratory.
const IGLOO_PIVOTS: [[i32; 3]; 3] = [[3, 5, 5], [1, 3, 1], [3, 6, 7]];
const IGLOO_OFFSETS: [[i32; 3]; 3] = [[0, 0, 0], [2, -3, 4], [0, -3, -2]];

/// One template piece of a start, before its height is settled.
#[derive(Debug, Clone, PartialEq)]
pub struct TemplatePiece {
    /// The template.
    pub template: Arc<Template>,
    /// Where its corner goes (`templatePosition`), at the height it was laid out at.
    pub pos: [i32; 3],
    /// Its rotation, about `pivot`; none is mirrored.
    pub rotation: Rotation,
    /// The rotation's pivot.
    pub pivot: [i32; 3],
    /// For an igloo's piece, its template's offset from the start (`OFFSETS`), which says
    /// where its height is read.
    pub offset: [i32; 3],
}

impl TemplatePiece {
    /// A template position `local` in the world, with the piece's corner at `pos`.
    pub fn world(&self, pos: [i32; 3], local: [i32; 3]) -> [i32; 3] {
        let p = self.rotation.transform_about(local, false, self.pivot);
        [0, 1, 2].map(|i| pos[i].wrapping_add(p[i]))
    }
}

/// The template pieces of a shipwreck's or an igloo's start in `chunk` for world `seed`, in
/// the order they place; empty for other types. From the context random,
/// `setLargeFeatureSeed(seed, chunk)`: the rotation, then a shipwreck's template, or whether an
/// igloo has a laboratory and how deep.
pub fn template_pieces(structure: &Structure, seed: i64, chunk: [i32; 2]) -> Vec<TemplatePiece> {
    let mut random = worldgen::large_feature(seed, chunk[0], chunk[1]);
    let corner = [chunk[0].wrapping_shl(4), 90, chunk[1].wrapping_shl(4)];
    let rotation = Rotation::random(&mut random);
    match structure.kind {
        StructureType::Shipwreck => {
            let i = random.next_int_bounded(structure.templates.len() as i32) as usize;
            vec![TemplatePiece {
                template: structure.templates[i].clone(),
                pos: corner,
                rotation,
                pivot: SHIPWRECK_PIVOT,
                offset: [0; 3],
            }]
        }
        StructureType::Igloo => {
            // IglooPiece: the template's offset from the start, `down` blocks lower.
            let piece = |t: usize, down: i32| {
                let offset = IGLOO_OFFSETS[t];
                TemplatePiece {
                    template: structure.templates[t].clone(),
                    pos: [
                        corner[0] + offset[0],
                        corner[1] + offset[1] - down,
                        corner[2] + offset[2],
                    ],
                    rotation,
                    pivot: IGLOO_PIVOTS[t],
                    offset,
                }
            };
            let mut pieces = Vec::new();
            if random.next_double() < 0.5 {
                let depth = random.next_int_bounded(8) + 4;
                pieces.push(piece(2, depth * 3));
                for i in 0..depth - 1 {
                    pieces.push(piece(1, i * 3));
                }
            }
            pieces.push(piece(0, 0));
            pieces
        }
        _ => Vec::new(),
    }
}
