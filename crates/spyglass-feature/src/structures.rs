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
    let r = REACH_CHUNKS;
    for (set, s) in sets.sets().iter().enumerate() {
        if !s.possible
            || !s
                .entries
                .iter()
                .any(|e| sets.structures()[e.structure].step <= last_step)
        {
            continue;
        }
        for (start, selection) in starts.starts_in(set, [cx - r, cz - r], [cx + r, cz + r]) {
            for &(structure, check) in &selection.tries {
                let st = &sets.structures()[structure];
                if st.step > last_step || check == Check::Rejected {
                    continue;
                }
                let laid_out = match st.kind {
                    StructureType::Jigsaw => starts.assemble(structure, start),
                    _ => None,
                };
                match laid_out {
                    Some(Assembly::Built(layout)) => {
                        // A mineshaft's corridors drop supports below their box to solid
                        // ground and hang chains above it: whole columns.
                        let column = st.kind == StructureType::Mineshaft;
                        out.extend(layout.pieces.iter().map(|b| Piece {
                            min: [b.min[0], if column { min_y } else { b.min[1] }, b.min[2]],
                            max: [b.max[0], if column { max_y } else { b.max[1] }, b.max[2]],
                            step: st.step,
                            structure,
                        }));
                    }
                    Some(Assembly::NoStart) if check == Check::Passes => {}
                    // One piece of a fixed size at the start chunk's corner, its height
                    // following the surface (the treasure's chest scans down): whole columns.
                    _ if footprint(st.kind).is_some() => {
                        let (lo, hi) = footprint(st.kind).unwrap_or((0, 15));
                        out.push(Piece {
                            min: [start[0] * 16 + lo, min_y, start[1] * 16 + lo],
                            max: [start[0] * 16 + hi, max_y, start[1] * 16 + hi],
                            step: st.step,
                            structure,
                        });
                    }
                    _ => out.push(Piece {
                        min: [(start[0] - r) * 16, min_y, (start[1] - r) * 16],
                        max: [
                            (start[0] + r + 1) * 16 - 1,
                            max_y,
                            (start[1] + r + 1) * 16 - 1,
                        ],
                        step: st.step,
                        structure,
                    }),
                }
            }
        }
    }
    out
}
