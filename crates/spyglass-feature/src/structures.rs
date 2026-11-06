//! Structure pieces placed before features: each decoration step places its structures'
//! pieces in the chunk being decorated before that step's features
//! (`docs/specs/features.md#structures`).

use spyglass_blocks::ChunkGenerator;
use spyglass_rng::WorldgenRandom;
use spyglass_structure::{
    Assembly, BoundingBox, Check, Layout, ShaftPiece, Starts, StructureSets, StructureType,
};

use crate::blocks::Blocks;
use crate::containers::{ContainerDoubt, Reads, StructureContainer, StructureContainers};
use crate::mineshaft::Shaft;
use crate::region::Region;
use crate::scattered;

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

/// The containers with loot tables the structures spyglass places put in the region's chunk,
/// in the order they place them (`docs/specs/loot.md#containers`), for world `seed`: jigsaw
/// structures, mineshafts and the single-piece ones but swamp huts. Each step's structures place
/// in registry order, each from the decoration random seeded with its index in the step, and
/// each start referencing the chunk places its pieces from that random. Only the structures of
/// step `step`, on the region as the steps before it left it; `reads` says why a block a
/// structure reads might not be the region's: what placed before it could have written there.
#[allow(
    clippy::too_many_arguments,
    reason = "the decoration's state, passed through"
)]
pub fn containers(
    starts: &Starts<'_>,
    sets: &StructureSets,
    containers: &StructureContainers,
    blocks: &Blocks,
    generator: &ChunkGenerator<'_>,
    region: &mut Region<'_>,
    seed: i64,
    step: usize,
    reads: &Reads<'_>,
) -> Vec<StructureContainer> {
    let [cx, cz] = region.center();
    let chunk = BoundingBox {
        min: [cx * 16, i32::MIN, cz * 16],
        max: [cx * 16 + 15, i32::MAX, cz * 16 + 15],
    };
    let decoration =
        WorldgenRandom::new(0).set_decoration_seed(seed, cx.wrapping_shl(4), cz.wrapping_shl(4));
    let mut order: Vec<usize> = (0..sets.structures().len())
        .filter(|&s| {
            let info = &sets.structures()[s];
            info.step == step && places_containers(info.kind)
        })
        .collect();
    order.sort_by_key(|&s| sets.structures()[s].step_index);
    let mut out = Vec::new();
    for structure in order {
        let info = &sets.structures()[structure];
        let laid_out = matches!(info.kind, StructureType::Jigsaw | StructureType::Mineshaft);
        // The starts whose pieces reach the chunk (`createReferences`), each with whether it's
        // surely there. The single-piece structures spyglass places put their containers near
        // their start chunk.
        let reach = if laid_out {
            REACH_CHUNKS
        } else {
            scattered::reach(info.kind)
        };
        let mut referencing = Vec::new();
        for (set, structure_set) in sets.sets().iter().enumerate() {
            if !structure_set.possible
                || !structure_set
                    .entries
                    .iter()
                    .any(|e| e.structure == structure)
            {
                continue;
            }
            for (start, selection) in
                starts.starts_in(set, [cx - reach, cz - reach], [cx + reach, cz + reach])
            {
                let Some(i) = selection.tries.iter().position(|&(s, _)| s == structure) else {
                    continue;
                };
                if selection.tries[i].1 == Check::Rejected {
                    continue;
                }
                // Certain only if it's the set's one try here and it passes.
                let sure = selection.tries.len() == 1 && selection.tries[i].1 == Check::Passes;
                if info.kind == StructureType::Mineshaft {
                    if let Some(pieces) = starts.mineshaft_pieces(structure, start)
                        && pieces.iter().any(|p| p.bounds().intersects(&chunk))
                    {
                        referencing.push((start, Laid::Shaft(pieces), sure));
                    }
                } else if !laid_out {
                    referencing.push((start, Laid::Single, sure));
                } else if let Some(Assembly::Built(layout)) = starts.assemble(structure, start)
                    && layout.bounds.intersects(&chunk)
                {
                    referencing.push((start, Laid::Jigsaw(layout), sure));
                }
            }
        }
        if referencing.is_empty() {
            continue;
        }
        // Whether a block's biome is one mineshafts don't place in; `None` on a tie that
        // decides it.
        let blocking = |pos: [i32; 3]| {
            let lookup = generator.block_biome(pos);
            let blocks = |b| info.mineshaft_blocking.contains(b);
            let answer = blocks(lookup.biome());
            lookup
                .candidates()
                .iter()
                .all(|&b| blocks(b) == answer)
                .then_some(answer)
        };
        // The starts place from one random in an order spyglass doesn't know, so with several,
        // each order's containers are candidates; past a few starts, only the one order.
        let place =
            |region: &mut Region<'_>, order: &[usize], out: &mut Vec<StructureContainer>| {
                let mut random = WorldgenRandom::new(0);
                random.set_feature_seed(decoration, info.step_index as i32, info.step as i32);
                let mut unknown = (order.len() > 1).then_some(ContainerDoubt::StartOrder);
                for &i in order {
                    let (start, layout, sure) = &referencing[i];
                    let first = out.len();
                    match layout {
                        Laid::Shaft(pieces) => {
                            let mut shaft = Shaft {
                                region: &mut *region,
                                blocks,
                                shaft_blocks: &containers.shaft,
                                info,
                                structure,
                                blocking: &blocking,
                                reads,
                                random: &mut random,
                                doubt: unknown.clone(),
                                out: &mut *out,
                            };
                            shaft.place(pieces);
                            // A start that might not be there makes the next one's draws unknown.
                            unknown = shaft.doubt.take().or(unknown);
                            if !sure {
                                unknown.get_or_insert(ContainerDoubt::Start);
                            }
                        }
                        Laid::Jigsaw(layout) => containers.jigsaw(
                            blocks,
                            region,
                            layout,
                            structure,
                            &mut random,
                            &mut unknown,
                            reads,
                            out,
                        ),
                        Laid::Single => {
                            let start = scattered::Start {
                                info,
                                structure,
                                chunk: *start,
                                starts,
                                blocks,
                                cannot_replace: containers.cannot_replace,
                            };
                            scattered::containers(&start, region, seed, &mut random, reads, out);
                        }
                    }
                    if !sure {
                        for c in &mut out[first..] {
                            c.doubt.get_or_insert(ContainerDoubt::Start);
                        }
                    }
                }
            };
        let orders = if referencing.len() <= MAX_ORDERED_STARTS {
            permutations(referencing.len())
        } else {
            vec![(0..referencing.len()).collect()]
        };
        // The first order places in the region, the rest in copies of it.
        let before = region.clone();
        place(region, &orders[0], &mut out);
        for order in &orders[1..] {
            let mut copy = before.clone();
            let mut more = Vec::new();
            place(&mut copy, order, &mut more);
            for c in more {
                if !out.contains(&c) {
                    out.push(c);
                }
            }
        }
    }
    out
}

/// Whether spyglass places `kind`'s containers ([`containers`]).
pub fn places_containers(kind: StructureType) -> bool {
    matches!(kind, StructureType::Jigsaw | StructureType::Mineshaft) || scattered::places(kind)
}

/// With at most this many starts of a structure reaching a chunk, every order they could place
/// in is tried: 24 orders.
const MAX_ORDERED_STARTS: usize = 4;

/// Every order of `0..n`.
fn permutations(n: usize) -> Vec<Vec<usize>> {
    if n == 0 {
        return vec![Vec::new()];
    }
    let mut out = Vec::new();
    for rest in permutations(n - 1) {
        for at in 0..=rest.len() {
            let mut order = rest.clone();
            order.insert(at, n - 1);
            out.push(order);
        }
    }
    out
}

/// How a start referencing the chunk is laid out, for placing its containers.
enum Laid {
    Jigsaw(Layout),
    Shaft(Vec<ShaftPiece>),
    /// A single-piece structure, laid out as it's placed.
    Single,
}
