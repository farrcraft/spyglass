//! Jigsaw assembly after the start piece (`JigsawPlacement.addPieces` and its `Placer`): every
//! piece's box, and the start's bounds. See `docs/specs/structure-placement.md`.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::sync::Arc;

use spyglass_data::anchor::AnchorHeights;
use spyglass_data::bundle::Bundle;
use spyglass_data::codec::LoadResult;
use spyglass_noise::NormalNoiseParameters;
use spyglass_rng::{LegacyRandomSource, PositionalRandomFactory, RandomSource, worldgen};
use spyglass_terrain::Heightmap;

use crate::corner::CornerSamplers;
use crate::jigsaw::{
    AliasBinding, BoundingBox, EMPTY_POOL, GenerationHeights, Jigsaw, JigsawLoader, Pool,
    PoolElement, Projection, Rotation, shuffle,
};
use crate::load::StructureSets;

/// Every template pool the jigsaw structures of a [`StructureSets`] can reach, with their
/// templates: what assembly needs beyond the start pool. Loading them reads most of a bundle's
/// structure templates, so it's separate from loading the sets.
#[derive(Debug, Clone)]
pub struct JigsawPools {
    pools: HashMap<String, Arc<Pool>>,
    /// The noises corner constraints' functions use.
    noises: HashMap<String, NormalNoiseParameters>,
    /// How the dimension's `RandomState` binds them.
    legacy_random_source: bool,
    anchors: AnchorHeights,
}

impl JigsawPools {
    /// Loads every pool reachable from the sets' jigsaw structures: start pools, alias targets,
    /// fallbacks and every pool a template's jigsaw block names. A pool missing from the bundle
    /// is left out, and assembly skips the jigsaw blocks that name it, as the game does.
    pub fn load(bundle: &Bundle, sets: &StructureSets) -> LoadResult<Self> {
        let mut loader = JigsawLoader::new(bundle);
        let mut pools = HashMap::new();
        let mut queue: Vec<String> = Vec::new();
        for s in sets.structures() {
            if let Some(jigsaw) = &s.jigsaw {
                queue.extend(jigsaw.named_pools());
            }
        }
        while let Some(id) = queue.pop() {
            if pools.contains_key(&id) {
                continue;
            }
            if let Some(pool) = loader.optional_pool(&id)? {
                queue.extend(JigsawLoader::pools_named_by(&pool));
                pools.insert(id, pool);
            }
        }
        let jigsaw_pools = JigsawPools {
            pools,
            noises: loader.noises().clone(),
            legacy_random_source: sets.legacy_random_source(),
            anchors: sets.heights().anchors(),
        };
        // Binding depends on the dimension, not the seed: bind every corner constraint's
        // functions once, so assembly can't fail to.
        let mut samplers = jigsaw_pools.corner_samplers(0);
        for pool in jigsaw_pools.pools.values() {
            for element in &pool.templates {
                for function in element
                    .corner_constraint()
                    .into_iter()
                    .flat_map(|c| c.functions())
                {
                    samplers.bind(function)?;
                }
            }
        }
        drop(samplers);
        Ok(jigsaw_pools)
    }

    /// Corner constraints' functions, bound for `seed` as the dimension's `RandomState` binds.
    fn corner_samplers(&self, seed: i64) -> CornerSamplers<'_> {
        CornerSamplers::new(&self.noises, seed, self.legacy_random_source, self.anchors)
    }

    fn get(&self, id: &str) -> Option<&Pool> {
        self.pools.get(id).map(|p| &**p)
    }
}

/// A jigsaw start's pieces: `JigsawPlacement.addPieces`, as `StructureStart` holds them before
/// any is placed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// Each piece's box, in the game's order: the start piece first.
    pub pieces: Vec<BoundingBox>,
    /// `StructureStart.getBoundingBox`: the box around every piece, grown by 12 on every side if
    /// the structure adapts terrain.
    pub bounds: BoundingBox,
}

/// What assembly found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Assembly {
    /// No start: no start piece, a start outside the padding or failing its corner constraint,
    /// or `size` 0.
    NoStart,
    /// The start's pieces.
    Built(Layout),
    /// A height was needed that the caller couldn't give.
    NeedsTerrain,
}

/// A placed piece (`PoolElementStructurePiece`), as far as assembly needs it.
struct Piece<'a> {
    element: &'a PoolElement,
    pos: [i32; 3],
    rotation: Rotation,
    bbox: BoundingBox,
}

/// A free space (the game's `VoxelShape` in a `MutableObject`): the blocks of `bound` that
/// aren't in any of `taken`. The game builds it from whole blocks, and tests a piece by its box
/// shrunk by a quarter, so a piece fits exactly when each of its blocks is free.
struct Free {
    bound: BoundingBox,
    taken: Vec<BoundingBox>,
}

impl Free {
    fn fits(&self, b: &BoundingBox) -> bool {
        self.bound.contains_box(b) && !self.taken.iter().any(|t| t.intersects(b))
    }
}

/// A piece waiting for its children (`PieceState`).
struct Waiting {
    piece: usize,
    free: usize,
    depth: i32,
}

/// The game's `SequencedPriorityIterator`: highest priority first, first in first out among
/// equals.
#[derive(Default)]
struct Queue {
    heap: BinaryHeap<(i32, Reverse<u64>, usize)>,
    items: Vec<Option<Waiting>>,
    next: u64,
}

impl Queue {
    fn push(&mut self, w: Waiting, priority: i32) {
        self.heap
            .push((priority, Reverse(self.next), self.items.len()));
        self.items.push(Some(w));
        self.next += 1;
    }

    fn pop(&mut self) -> Option<Waiting> {
        let (_, _, i) = self.heap.pop()?;
        self.items[i].take()
    }
}

/// `PoolAliasLookup.create(bindings, pos, seed)`: each binding resolved in order with a random
/// of its own, `LegacyRandomSource(seed).forkPositional().at(pos)`.
fn resolve_aliases(bindings: &[AliasBinding], pos: [i32; 3], seed: i64) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if bindings.is_empty() {
        return map;
    }
    let mut random = LegacyRandomSource::new(seed)
        .fork_positional()
        .at(pos[0], pos[1], pos[2]);
    // WeightedList.getRandomOrThrow: nextInt(total), then the entry that index falls in.
    fn pick<'t, T>(list: &'t [(T, i32)], random: &mut LegacyRandomSource) -> &'t T {
        let total: i32 = list.iter().map(|(_, w)| w).sum();
        let mut n = random.next_int_bounded(total);
        for (item, w) in list {
            n -= w;
            if n < 0 {
                return item;
            }
        }
        unreachable!("an index below the total weight falls in some entry")
    }
    fn resolve(
        b: &AliasBinding,
        random: &mut LegacyRandomSource,
        map: &mut HashMap<String, String>,
    ) {
        match b {
            AliasBinding::Direct { alias, target } => {
                map.insert(alias.clone(), target.clone());
            }
            AliasBinding::Random { alias, targets } => {
                map.insert(alias.clone(), pick(targets, random).clone());
            }
            AliasBinding::RandomGroup { groups } => {
                for b in pick(groups, random) {
                    resolve(b, random, map);
                }
            }
        }
    }
    for b in bindings {
        resolve(b, &mut random, &mut map);
    }
    map
}

/// The game's `Placer`, for one start.
struct Placer<'a, 'h> {
    jigsaw: &'a Jigsaw,
    pools: &'a JigsawPools,
    aliases: HashMap<String, String>,
    random: LegacyRandomSource,
    pieces: Vec<Piece<'a>>,
    free: Vec<Free>,
    queue: Queue,
    /// `ChunkGenerator.getFirstFreeHeight(x, z, WORLD_SURFACE_WG)`, or `None` without terrain.
    first_free: &'h mut dyn FnMut(i32, i32, Heightmap) -> Option<i32>,
    /// For pieces with a corner constraint.
    corners: CornerSamplers<'a>,
}

/// Assembly stopped for want of a height.
struct NoTerrain;

impl<'a> Placer<'a, '_> {
    /// `PoolAliasLookup.lookup`: an alias's target, or the id itself.
    fn lookup<'s>(&'s self, id: &'s str) -> &'s str {
        self.aliases.get(id).map_or(id, String::as_str)
    }

    /// The pool (after aliases) and its fallback, if the parent jigsaw can use them: both
    /// exist, and each is non-empty or the empty pool itself.
    fn pool_and_fallback(&self, id: &str) -> Option<(&'a Pool, &'a Pool)> {
        let pools: &'a JigsawPools = self.pools;
        let usable = |id: &str| {
            pools
                .get(id)
                .filter(|p| !p.templates.is_empty() || id == EMPTY_POOL)
        };
        let pool = usable(self.lookup(id))?;
        Some((pool, usable(&pool.fallback)?))
    }

    /// The expansion hack's height for a child's jigsaw at the origin: if it faces into the
    /// child's own box, the tallest element its pool or the pool's fallback has.
    fn expansion(&self, child_box: &BoundingBox, j: &crate::jigsaw::PlacedJigsaw<'_>) -> i32 {
        let step = j.front.step();
        let facing = [0, 1, 2].map(|i| j.pos[i].wrapping_add(step[i]));
        if !child_box.contains(facing) {
            return 0;
        }
        let Some(pool) = self.pools.get(self.lookup(&j.data.pool)) else {
            return 0;
        };
        let fallback = self.pools.get(&pool.fallback).map_or(0, |f| f.max_size);
        pool.max_size.max(fallback)
    }

    /// `tryPlacingChildren`: one piece's children, each attached to one of its jigsaw blocks.
    fn try_placing_children(&mut self, at: Waiting) -> Result<(), NoTerrain> {
        let size = self.jigsaw.size;
        let piece = &self.pieces[at.piece];
        let (element, pos, rotation, bbox) = (piece.element, piece.pos, piece.rotation, piece.bbox);
        let rigid = element.projection() == Projection::Rigid;
        let min_y = bbox.min[1];
        // The piece's own box as free space, made once, for jigsaws that face into it.
        let mut internal: Option<usize> = None;
        'parents: for parent in element.shuffled_jigsaws(pos, rotation, &mut self.random) {
            let step = parent.front.step();
            let target = [0, 1, 2].map(|i| parent.pos[i].wrapping_add(step[i]));
            let rel_y = parent.pos[1].wrapping_sub(min_y);
            let mut surface: Option<i32> = None;
            let Some((pool, fallback)) = self.pool_and_fallback(&parent.data.pool) else {
                continue;
            };
            let free = if bbox.contains(target) {
                *internal.get_or_insert_with(|| {
                    self.free.push(Free {
                        bound: bbox,
                        taken: Vec::new(),
                    });
                    self.free.len() - 1
                })
            } else {
                at.free
            };
            let mut candidates: Vec<&'a PoolElement> = Vec::new();
            if at.depth != size {
                let mut shuffled: Vec<&PoolElement> = pool.templates.iter().collect();
                shuffle(&mut shuffled, &mut self.random);
                candidates.extend(shuffled);
            }
            let mut shuffled: Vec<&PoolElement> = fallback.templates.iter().collect();
            shuffle(&mut shuffled, &mut self.random);
            candidates.extend(shuffled);
            for candidate in candidates {
                if matches!(candidate, PoolElement::Empty) {
                    continue 'parents;
                }
                let mut rotations = Rotation::ALL;
                shuffle(&mut rotations, &mut self.random);
                for child_rotation in rotations {
                    let child_jigsaws =
                        candidate.shuffled_jigsaws([0; 3], child_rotation, &mut self.random);
                    let origin_box = candidate
                        .bounding_box([0; 3], child_rotation)
                        .expect("non-empty elements have a box");
                    let expand = if self.jigsaw.use_expansion_hack && origin_box.y_span() <= 16 {
                        child_jigsaws
                            .iter()
                            .map(|j| self.expansion(&origin_box, j))
                            .max()
                            .unwrap_or(0)
                    } else {
                        0
                    };
                    for child in &child_jigsaws {
                        if !parent.can_attach(child) {
                            continue;
                        }
                        let offset = [0, 1, 2].map(|i| target[i].wrapping_sub(child.pos[i]));
                        let child_box = candidate
                            .bounding_box(offset, child_rotation)
                            .expect("non-empty elements have a box");
                        let child_rigid = candidate.projection() == Projection::Rigid;
                        let child_y = child.pos[1];
                        let jy = rel_y.wrapping_sub(child_y).wrapping_add(step[1]);
                        let target_y = if rigid && child_rigid {
                            min_y.wrapping_add(jy)
                        } else {
                            let s = match surface {
                                Some(s) => s,
                                None => {
                                    let s = (self.first_free)(
                                        parent.pos[0],
                                        parent.pos[2],
                                        Heightmap::WorldSurfaceWg,
                                    )
                                    .ok_or(NoTerrain)?;
                                    *surface.insert(s)
                                }
                            };
                            s.wrapping_sub(child_y)
                        };
                        let dy = target_y.wrapping_sub(child_box.min[1]);
                        let mut moved = child_box.moved([0, dy, 0]);
                        let place = [offset[0], offset[1].wrapping_add(dy), offset[2]];
                        if expand > 0 {
                            let h = expand
                                .wrapping_add(1)
                                .max(moved.max[1].wrapping_sub(moved.min[1]));
                            let top = [moved.min[0], moved.min[1].wrapping_add(h), moved.min[2]];
                            moved = moved.encapsulate(BoundingBox { min: top, max: top });
                        }
                        if !self.free[free].fits(&moved) {
                            continue;
                        }
                        self.free[free].taken.push(moved);
                        // The corner test comes after the space is taken, so a piece it rejects
                        // still blocks the others.
                        if let Some(corner) = candidate.corner_constraint()
                            && !corner.test(&moved, &mut self.corners)
                        {
                            continue;
                        }
                        self.pieces.push(Piece {
                            element: candidate,
                            pos: place,
                            rotation: child_rotation,
                            bbox: moved,
                        });
                        if at.depth.wrapping_add(1) <= size {
                            self.queue.push(
                                Waiting {
                                    piece: self.pieces.len() - 1,
                                    free,
                                    depth: at.depth.wrapping_add(1),
                                },
                                parent.data.placement_priority,
                            );
                        }
                        continue 'parents;
                    }
                }
            }
        }
        Ok(())
    }
}

impl Jigsaw {
    /// `findGenerationPoint` and `addPieces` in full, for the start in `chunk` of a world with
    /// `seed`: the start piece and every piece the placer adds, drawing from a fresh
    /// `setLargeFeatureSeed` random as the game does. The biome check isn't made: this is the
    /// layout if the structure starts here. `first_free` is the generator's first free height,
    /// or `None` where it isn't known.
    pub fn assemble(
        &self,
        seed: i64,
        chunk: [i32; 2],
        heights: &GenerationHeights,
        pools: &JigsawPools,
        first_free: &mut dyn FnMut(i32, i32, Heightmap) -> Option<i32>,
    ) -> Assembly {
        let mut random = worldgen::large_feature(seed, chunk[0], chunk[1]);
        let Some(start) = self.start_piece(chunk, heights, &mut random) else {
            return Assembly::NoStart;
        };
        let [x, z] = start.center();
        let y = match &self.project_start_to_heightmap {
            None => start.pos[1],
            Some(name) => {
                let Some(heightmap) = Heightmap::ALL.into_iter().find(|h| h.key() == name) else {
                    return Assembly::NeedsTerrain;
                };
                match first_free(x, z, heightmap) {
                    Some(h) => start.start[1].wrapping_add(h),
                    None => return Assembly::NeedsTerrain,
                }
            }
        };
        // The piece moves so its box's minimum y plus the ground level delta (1) is at y.
        let dy = y.wrapping_sub(start.bbox.min[1].wrapping_add(1));
        let bbox = start.bbox.moved([0, dy, 0]);
        if !self.fits_padding(y, [start.bbox.min[1], start.bbox.max[1]], heights) {
            return Assembly::NoStart;
        }
        let mut corners = pools.corner_samplers(seed);
        if let Some(corner) = start.element.corner_constraint()
            && !corner.test(&bbox, &mut corners)
        {
            return Assembly::NoStart;
        }
        if self.size <= 0 {
            return Assembly::NoStart;
        }
        let check_y = y.wrapping_add(start.offset[1]);
        // The free space: a box around the centre, padded off the world's limits, minus the
        // start piece. AABB orders its corners.
        let (h, v) = self.max_distance;
        let (bottom, top) = self.dimension_padding.unwrap_or((0, 0));
        let lo = check_y
            .wrapping_sub(v)
            .max(heights.level_min_y.wrapping_add(bottom));
        let hi = check_y
            .wrapping_add(v)
            .wrapping_add(1)
            .min(heights.level_max_y().wrapping_add(1).wrapping_sub(top));
        let bound = BoundingBox {
            min: [x.wrapping_sub(h), lo.min(hi), z.wrapping_sub(h)],
            max: [
                x.wrapping_add(h),
                lo.max(hi).wrapping_sub(1),
                z.wrapping_add(h),
            ],
        };
        let pos = [start.pos[0], start.pos[1].wrapping_add(dy), start.pos[2]];
        let aliases = resolve_aliases(&self.pool_aliases, start.start, seed);
        let mut placer = Placer {
            jigsaw: self,
            pools,
            aliases,
            random,
            pieces: vec![Piece {
                element: start.element,
                pos,
                rotation: start.rotation,
                bbox,
            }],
            free: vec![Free {
                bound,
                taken: vec![bbox],
            }],
            queue: Queue::default(),
            first_free,
            corners,
        };
        let mut next = Some(Waiting {
            piece: 0,
            free: 0,
            depth: 0,
        });
        while let Some(w) = next {
            if placer.try_placing_children(w).is_err() {
                return Assembly::NeedsTerrain;
            }
            next = placer.queue.pop();
        }
        let pieces: Vec<BoundingBox> = placer.pieces.iter().map(|p| p.bbox).collect();
        let union = pieces
            .iter()
            .copied()
            .reduce(BoundingBox::encapsulate)
            .expect("the start piece");
        let bounds = if self.adapts_terrain {
            union.inflated(12)
        } else {
            union
        };
        Assembly::Built(Layout { pieces, bounds })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jigsaw::{Direction, HeightProvider, JigsawData, Joint, Template, VerticalAnchor};

    const HEIGHTS: GenerationHeights = GenerationHeights {
        min_y: -64,
        height: 384,
        level_min_y: -64,
        level_height: 384,
    };

    fn data(name: &str, pool: &str, target: &str) -> JigsawData {
        JigsawData {
            name: Some(name.to_string()),
            pool: pool.to_string(),
            target: target.to_string(),
            joint: Joint::Aligned,
            placement_priority: 0,
            selection_priority: 0,
        }
    }

    fn pool(templates: Vec<PoolElement>) -> Arc<Pool> {
        Arc::new(Pool {
            templates,
            fallback: EMPTY_POOL.to_string(),
            max_size: 1,
        })
    }

    /// A 5 × 1 × 5 hub with one jigsaw on its east face into `t:links`, whose one element is a
    /// 3 × 1 × 3 link: a jigsaw on its west face that attaches, and one on its east face into
    /// `t:links` again, so links chain eastwards.
    fn chain(size: i32, reach: i32, links: Projection) -> (Jigsaw, JigsawPools) {
        use Direction::{East, Up, West};
        let hub = Template::for_test(
            [5, 1, 5],
            vec![([4, 0, 2], East, Up, data("t:hub", "t:links", "t:link"))],
        );
        let link = Template::for_test(
            [3, 1, 3],
            vec![
                ([0, 0, 1], West, Up, data("t:link", EMPTY_POOL, "t:hub")),
                ([2, 0, 1], East, Up, data("t:hub", "t:links", "t:link")),
            ],
        );
        let start = pool(vec![PoolElement::Single(Arc::new(hub), Projection::Rigid)]);
        let pools = JigsawPools {
            pools: HashMap::from([
                ("t:start".to_string(), start.clone()),
                (
                    "t:links".to_string(),
                    pool(vec![PoolElement::Single(Arc::new(link), links)]),
                ),
                (EMPTY_POOL.to_string(), pool(Vec::new())),
            ]),
            noises: HashMap::new(),
            legacy_random_source: false,
            anchors: HEIGHTS.anchors(),
        };
        let jigsaw = Jigsaw {
            start_pool_id: "t:start".to_string(),
            start_pool: start,
            start_jigsaw_name: None,
            size,
            start_height: HeightProvider::Constant(VerticalAnchor::Absolute(64)),
            use_expansion_hack: false,
            project_start_to_heightmap: None,
            max_distance: (reach, reach),
            pool_aliases: Vec::new(),
            dimension_padding: None,
            adapts_terrain: false,
        };
        (jigsaw, pools)
    }

    fn assemble(jigsaw: &Jigsaw, pools: &JigsawPools, height: Option<i32>) -> Assembly {
        jigsaw.assemble(1, [0, 0], &HEIGHTS, pools, &mut |_, _, _| height)
    }

    fn built(a: Assembly) -> Layout {
        match a {
            Assembly::Built(layout) => layout,
            other => panic!("expected a layout, got {other:?}"),
        }
    }

    #[test]
    fn children_attach_face_to_face_up_to_the_depth() {
        let (jigsaw, pools) = chain(1, 80, Projection::Rigid);
        let layout = built(assemble(&jigsaw, &pools, None));
        let [hub, link] = layout.pieces[..] else {
            panic!("expected two pieces, got {:?}", layout.pieces);
        };
        // The start moves so its box's minimum y plus 1 is the start height; a rigid child keeps
        // its parent's floor.
        assert_eq!(hub.min[1], 63);
        assert_eq!(link.min[1], 63);
        // The link touches the hub's side without overlapping it.
        assert!(!link.intersects(&hub));
        assert!(link.intersects(&hub.inflated(1)));
        assert_eq!(layout.bounds, hub.encapsulate(link));
        // One more level of depth adds one more link; size 0 has no pieces at all.
        assert_eq!(
            built(assemble(&chain(2, 80, Projection::Rigid).0, &pools, None))
                .pieces
                .len(),
            3
        );
        let (empty, _) = chain(0, 80, Projection::Rigid);
        assert_eq!(assemble(&empty, &pools, None), Assembly::NoStart);
    }

    #[test]
    fn pieces_stay_inside_the_free_space() {
        // The hub's centre is 2 blocks from its sides, links span 3 blocks: with a reach of 6,
        // the first link (3 to 5 blocks past the hub) fits and the second doesn't.
        let (near, pools) = chain(3, 6, Projection::Rigid);
        assert_eq!(built(assemble(&near, &pools, None)).pieces.len(), 2);
        let (far, _) = chain(3, 80, Projection::Rigid);
        assert_eq!(built(assemble(&far, &pools, None)).pieces.len(), 4);
    }

    #[test]
    fn terrain_matching_pieces_stand_on_the_surface() {
        let (jigsaw, pools) = chain(1, 80, Projection::TerrainMatching);
        assert_eq!(assemble(&jigsaw, &pools, None), Assembly::NeedsTerrain);
        // The child jigsaw's y (0 in its template) lands on the first free height.
        let layout = built(assemble(&jigsaw, &pools, Some(70)));
        assert_eq!(layout.pieces[1].min[1], 70);
    }

    #[test]
    fn the_queue_takes_the_highest_priority_first_then_the_oldest() {
        let mut queue = Queue::default();
        for (piece, priority) in [(0, 0), (1, 5), (2, 0), (3, 5)] {
            queue.push(
                Waiting {
                    piece,
                    free: 0,
                    depth: 0,
                },
                priority,
            );
        }
        let order: Vec<usize> = std::iter::from_fn(|| queue.pop().map(|w| w.piece)).collect();
        assert_eq!(order, [1, 3, 0, 2]);
    }

    #[test]
    fn a_piece_fits_where_all_its_blocks_are_free() {
        let b = |min: [i32; 3], max: [i32; 3]| BoundingBox { min, max };
        let free = Free {
            bound: b([0, 0, 0], [9, 9, 9]),
            taken: vec![b([0, 0, 0], [4, 9, 9])],
        };
        assert!(free.fits(&b([5, 0, 0], [9, 9, 9])));
        assert!(!free.fits(&b([4, 0, 0], [6, 1, 1])));
        assert!(!free.fits(&b([8, 0, 0], [10, 1, 1])));
    }

    #[test]
    fn aliases_resolve_in_order_with_their_own_random() {
        let s = |v: &str| v.to_string();
        let bindings = vec![
            AliasBinding::Direct {
                alias: s("a:direct"),
                target: s("a:one"),
            },
            AliasBinding::Random {
                alias: s("a:random"),
                targets: vec![(s("a:heavy"), 2), (s("a:light"), 1)],
            },
            AliasBinding::RandomGroup {
                groups: vec![
                    (
                        vec![AliasBinding::Direct {
                            alias: s("a:group"),
                            target: s("a:first"),
                        }],
                        1,
                    ),
                    (
                        vec![AliasBinding::Direct {
                            alias: s("a:group"),
                            target: s("a:second"),
                        }],
                        3,
                    ),
                ],
            },
        ];
        let (seed, pos) = (12345, [16, -30, -32]);
        let map = resolve_aliases(&bindings, pos, seed);
        // Replay the draws: nextInt(3) for the random binding (below 2 is the heavy target),
        // then nextInt(4) for the group (0 is the first).
        let mut random = LegacyRandomSource::new(seed)
            .fork_positional()
            .at(pos[0], pos[1], pos[2]);
        let random_target = if random.next_int_bounded(3) < 2 {
            "a:heavy"
        } else {
            "a:light"
        };
        let group_target = if random.next_int_bounded(4) < 1 {
            "a:first"
        } else {
            "a:second"
        };
        assert_eq!(map["a:direct"], "a:one");
        assert_eq!(map["a:random"], random_target);
        assert_eq!(map["a:group"], group_target);
        assert!(resolve_aliases(&[], pos, seed).is_empty());
    }
}
