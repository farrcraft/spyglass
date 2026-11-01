//! Structure pieces placed before features: each decoration step places its structures'
//! pieces in the chunk being decorated before that step's features
//! (`docs/specs/features.md#structures`).

use spyglass_structure::{Assembly, Check, Starts, StructureSets, StructureType};

/// Blocks a structure's pieces could write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Piece {
    /// The least corner.
    pub min: [i32; 3],
    /// The greatest corner.
    pub max: [i32; 3],
    /// The decoration step it places in, before that step's features.
    pub step: usize,
    /// The structure, by index in its [`StructureSets`].
    pub structure: usize,
}

/// A structure reaches at most this many chunks from its start: chunks further away don't
/// reference it (`ChunkGenerator.createReferences`).
const REACH_CHUNKS: i32 = 8;

/// Where a single-piece structure can write, in x and z from its start chunk's corner, either
/// way it's turned (`BuriedTreasurePieces`, `DesertPyramidPiece`, `JungleTemplePiece`,
/// `SwampHutPiece`, `IglooPieces`, which can reach back past the corner).
fn footprint(kind: StructureType) -> Option<(i32, i32)> {
    Some(match kind {
        // The chest at (9, 9) and its neighbours.
        StructureType::BuriedTreasure => (8, 10),
        StructureType::DesertPyramid => (0, 20),
        StructureType::JungleTemple => (0, 14),
        StructureType::SwampHut => (0, 8),
        StructureType::Igloo => (-3, 12),
        _ => return None,
    })
}

/// The pieces of every structure that could start within reach of chunk `[cx, cz]` and places
/// in a step up to `last_step`: a jigsaw start's own pieces, and for any other start, or a
/// jigsaw spyglass can't lay out, everything within reach of its start chunk. A start that only
/// might be there counts.
pub fn pieces(
    starts: &Starts<'_>,
    sets: &StructureSets,
    [cx, cz]: [i32; 2],
    last_step: usize,
    min_y: i32,
    max_y: i32,
) -> Vec<Piece> {
    let mut out = Vec::new();
    let reach = REACH_CHUNKS;
    for (set, structure_set) in sets.sets().iter().enumerate() {
        if !structure_set.possible
            || !structure_set
                .entries
                .iter()
                .any(|e| sets.structures()[e.structure].step <= last_step)
        {
            continue;
        }
        for (start, selection) in
            starts.starts_in(set, [cx - reach, cz - reach], [cx + reach, cz + reach])
        {
            for &(structure, check) in &selection.tries {
                let info = &sets.structures()[structure];
                if info.step > last_step || check == Check::Rejected {
                    continue;
                }
                let laid_out = match info.kind {
                    StructureType::Jigsaw => starts.assemble(structure, start),
                    _ => None,
                };
                match laid_out {
                    Some(Assembly::Built(layout)) => {
                        // A mineshaft's corridors drop supports below their box to solid
                        // ground and hang chains above it: whole columns.
                        let column = info.kind == StructureType::Mineshaft;
                        out.extend(layout.pieces.iter().map(|piece| Piece {
                            min: [
                                piece.min[0],
                                if column { min_y } else { piece.min[1] },
                                piece.min[2],
                            ],
                            max: [
                                piece.max[0],
                                if column { max_y } else { piece.max[1] },
                                piece.max[2],
                            ],
                            step: info.step,
                            structure,
                        }));
                    }
                    Some(Assembly::NoStart) if check == Check::Passes => {}
                    // One piece of a fixed size at the start chunk's corner, its height
                    // following the surface (the treasure's chest scans down): whole columns.
                    _ if footprint(info.kind).is_some() => {
                        let (lo, hi) = footprint(info.kind).unwrap_or((0, 15));
                        out.push(Piece {
                            min: [start[0] * 16 + lo, min_y, start[1] * 16 + lo],
                            max: [start[0] * 16 + hi, max_y, start[1] * 16 + hi],
                            step: info.step,
                            structure,
                        });
                    }
                    _ => out.push(Piece {
                        min: [(start[0] - reach) * 16, min_y, (start[1] - reach) * 16],
                        max: [
                            (start[0] + reach + 1) * 16 - 1,
                            max_y,
                            (start[1] + reach + 1) * 16 - 1,
                        ],
                        step: info.step,
                        structure,
                    }),
                }
            }
        }
    }
    out
}
