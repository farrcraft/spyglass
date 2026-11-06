//! The game's nearest-structure search (`ChunkGenerator.findNearestMapStructure`, used by
//! `/locate structure`), with each placement chunk's start decided as far as spyglass can
//! without terrain. See `docs/specs/locate.md`.

use serde_json::Value;
use spyglass_data::bundle::Bundle;

use crate::check::{Check, Starts};
use crate::jigsaw::BoundingBox;
use spyglass_data::codec::{At, LoadErrorKind, LoadResult, full_id};

use crate::load::StructureSets;
use crate::placement::PlacementKind;

const STRUCTURE: &str = "worldgen/structure";
const STRUCTURE_TAG: &str = "tags/worldgen/structure";

/// `/locate structure`'s default: random-spread rings searched (`LocateCommand`).
pub const LOCATE_RADIUS: i32 = 100;

/// The most uncertain hits [`Starts::locate`] collects before giving up.
pub const MAX_UNCERTAIN: usize = 32;

impl StructureSets {
    /// The structures a `/locate structure` argument names, in the holder set's order: one
    /// structure id, or `#tag` (`TagEntry`s, nested tags expanded in place, duplicates kept
    /// at their first position). Structures that no set places can't be found, so they're
    /// left out. Unknown ids and tags are errors.
    pub fn resolve(&self, bundle: &Bundle, argument: &str) -> LoadResult<Vec<usize>> {
        let mut out = Vec::new();
        let mut loading = Vec::new();
        match argument.strip_prefix('#') {
            Some(tag) => self.tag(bundle, tag, &mut loading, &mut out)?,
            None => self.structure(bundle, argument, &At::root(argument), &mut out)?,
        }
        Ok(out)
    }

    fn structure(
        &self,
        bundle: &Bundle,
        id: &str,
        at: &At<'_>,
        out: &mut Vec<usize>,
    ) -> LoadResult<()> {
        if !exists(bundle, STRUCTURE, id) {
            return Err(at.error(LoadErrorKind::Unsupported {
                what: "structure",
                name: id.to_string(),
            }));
        }
        if let Some(i) = self.structure_index(id)
            && !out.contains(&i)
        {
            out.push(i);
        }
        Ok(())
    }

    fn tag(
        &self,
        bundle: &Bundle,
        id: &str,
        loading: &mut Vec<String>,
        out: &mut Vec<usize>,
    ) -> LoadResult<()> {
        let id = full_id(id);
        let resource = format!("#{id}");
        let root = At::root(&resource);
        if loading.contains(&id) {
            let mut cycle = loading.clone();
            cycle.push(id);
            return Err(root.error(LoadErrorKind::Cycle(cycle)));
        }
        let json = bundle
            .resource(STRUCTURE_TAG, &id)
            .map_err(|e| root.error(e.into()))?;
        let values = json
            .get("values")
            .and_then(Value::as_array)
            .ok_or_else(|| root.error(LoadErrorKind::Missing("values")))?;
        loading.push(id);
        let values_at = root.child("values");
        for (i, entry) in values.iter().enumerate() {
            let at = values_at.child(&format!("[{i}]"));
            // TagEntry.CODEC: an id or #tag, or {"id": ..., "required": bool}.
            let (name, required) = match entry {
                Value::String(name) => (name.as_str(), true),
                Value::Object(obj) => (
                    obj.get("id")
                        .and_then(Value::as_str)
                        .ok_or_else(|| at.error(LoadErrorKind::Missing("id")))?,
                    obj.get("required").and_then(Value::as_bool).unwrap_or(true),
                ),
                _ => return Err(at.error(LoadErrorKind::Shape("a tag entry"))),
            };
            match name.strip_prefix('#') {
                Some(tag) if required || exists(bundle, STRUCTURE_TAG, tag) => {
                    self.tag(bundle, tag, loading, out)?
                }
                None if required || exists(bundle, STRUCTURE, name) => {
                    self.structure(bundle, name, &at, out)?
                }
                _ => {}
            }
        }
        loading.pop();
        Ok(())
    }
}

fn exists(bundle: &Bundle, registry: &str, id: &str) -> bool {
    Bundle::resource_path(registry, &full_id(id))
        .is_ok_and(|path| bundle.manifest().files.contains_key(&path))
}

/// A placement chunk where [`Starts::locate`]'s answer may be.
#[derive(Debug, Clone, PartialEq)]
pub struct StructureHit {
    /// The structure set.
    pub set: usize,
    /// The placement chunk.
    pub chunk: [i32; 2],
    /// `StructurePlacement.getLocatePos`: the chunk's minimum block plus `locate_offset`. What
    /// `/locate` reports.
    pub position: [i32; 3],
    /// The searched structures that may start here, in the order the game tries them.
    pub structures: Vec<usize>,
    /// Whether the game certainly starts one of them here. Otherwise a start depends on
    /// terrain, or a tied biome cell, or (for ring placements) the ring position itself is
    /// tie- or platform-dependent.
    pub certain: bool,
    /// `BlockPos.distSqr` from the search's origin to [`position`](Self::position).
    pub distance_sqr: f64,
}

/// The answer to [`Starts::locate`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StructureSearch {
    /// Every chunk the game's answer may be in, in its search order: ring placements first,
    /// each nearest first; then random-spread rings outwards, each placement in scan order.
    /// Each placement's list ends at its first certain hit.
    pub hits: Vec<StructureHit>,
    /// Whether the search ended where the game's does: a ring with a certain hit, or the
    /// radius. Then the game's answer is one of the hits, or nothing if every hit is a
    /// candidate the game rejects. False if it stopped at [`MAX_UNCERTAIN`] candidates first,
    /// or a ring position's alternatives are unknown (too many tied cells to replay).
    pub complete: bool,
    /// The game's answer if every candidate fails: an index into
    /// [`hits`](Self::hits).
    pub certain_answer: Option<usize>,
}

/// `Vec3i.distSqr`: the squared distance in `double`, in x, y, z order.
fn dist_sqr(a: [i32; 3], b: [i32; 3]) -> f64 {
    let d = |i: usize| f64::from(a[i]) - f64::from(b[i]);
    let (dx, dy, dz) = (d(0), d(1), d(2));
    dx * dx + dy * dy + dz * dz
}

impl Starts<'_> {
    /// Which of `targets` may start in placement chunk `chunk` of set `set`, in the order the
    /// game tries them, and whether one certainly does: the game starts the first structure of
    /// the [`select`](Self::select)ion that passes. `None` if none of them can.
    pub fn targets_at(
        &self,
        set: usize,
        chunk: [i32; 2],
        targets: &[usize],
    ) -> Option<(Vec<usize>, bool)> {
        let tries = self.select(set, chunk).tries;
        let structures: Vec<usize> = tries
            .iter()
            .map(|&(s, _)| s)
            .filter(|s| targets.contains(s))
            .collect();
        if structures.is_empty() {
            return None;
        }
        // Some target certainly starts if the last try passes and nothing else could come
        // first.
        let certain = matches!(tries.last(), Some((_, Check::Passes)))
            && tries.iter().all(|(s, _)| targets.contains(s));
        Some((structures, certain))
    }

    /// `ChunkGenerator.findNearestMapStructure(level, targets, from, radius, false)`, as
    /// `/locate structure` runs it with radius [`LOCATE_RADIUS`]. `targets` are structure
    /// indices in holder-set order, e.g. from [`StructureSets::resolve`].
    ///
    /// The game groups the targets by the sets that place them. Ring placements are searched
    /// first, keeping the nearest generated position. Random-spread placements then share one
    /// loop over square rings of cells around `from`'s chunk, `r` from 0 to `radius`. In each
    /// ring, a placement's answer is its first generated chunk in scan order, and the best
    /// overall is replaced only by a strictly nearer one; the search ends after the first ring
    /// with an answer. Distances are `BlockPos.distSqr` from `from` to the locate position.
    pub fn locate(&self, targets: &[usize], from: [i32; 3], radius: i32) -> StructureSearch {
        self.search(targets, from, radius, &[])
    }

    /// An exploration map's search (`findNearestMapStructure` with `skipKnownStructures`,
    /// `docs/specs/loot.md#maps`): as [`locate`](Self::locate), passing over the starts already
    /// `claimed` by a reference, each a set and its placement chunk. A start takes one
    /// reference, and a map claims the one it finds; in a fresh world nothing is claimed but
    /// what the container stands in.
    pub fn map_search(
        &self,
        targets: &[usize],
        from: [i32; 3],
        radius: i32,
        claimed: &[(usize, [i32; 2])],
    ) -> StructureSearch {
        self.search(targets, from, radius, claimed)
    }

    /// Where an exploration map from a container at `from` points
    /// (`ExplorationMapFunction`, `docs/specs/loot.md#maps`): the search for `targets` within
    /// `radius` cells. One that skips known structures (`skip`) first claims the destination
    /// start whose box holds the container (where `claims_containing`, the versions that do),
    /// then passes over every start in `claims`, and claims what it finds. `claims` carries a
    /// world's references from map to map; a fresh world starts with none.
    ///
    /// `Ok(None)` if it surely finds nothing; `Err` with the search where spyglass can't tell
    /// which hit is the game's, or which start the container stands in.
    pub fn map_target(
        &self,
        targets: &[usize],
        from: [i32; 3],
        radius: i32,
        skip: bool,
        claims_containing: bool,
        claims: &mut Vec<(usize, [i32; 2])>,
    ) -> Result<Option<StructureHit>, Option<StructureSearch>> {
        if skip && claims_containing {
            match self.start_holding(targets, from) {
                Err(()) => return Err(None),
                Ok(Some(start)) if !claims.contains(&start) => claims.push(start),
                Ok(_) => {}
            }
        }
        let search = self.map_search(targets, from, radius, if skip { claims } else { &[] });
        // Certain only if every hit is: then the game's answer is the nearest certain one.
        if !search.complete || search.hits.iter().any(|h| !h.certain) {
            return Err(Some(search));
        }
        let Some(i) = search.certain_answer else {
            return Ok(None);
        };
        let hit = search.hits[i].clone();
        if skip {
            claims.push((hit.set, hit.chunk));
        }
        Ok(Some(hit))
    }

    /// The start of one of `targets` whose box holds `pos`, as `StructureManager.getStructureAt`
    /// finds it among the references of `pos`'s chunk: its set and placement chunk, if spyglass
    /// can tell. `Err` where a start it can't lay out, or one only maybe there, could hold it.
    fn start_holding(
        &self,
        targets: &[usize],
        pos: [i32; 3],
    ) -> Result<Option<(usize, [i32; 2])>, ()> {
        // A start reaches at most 8 chunks from its own (`createReferences`).
        let (cx, cz) = (pos[0] >> 4, pos[2] >> 4);
        let mut found = None;
        for set in self.placements_for(targets) {
            for (chunk, selection) in self.starts_in(set, [cx - 8, cz - 8], [cx + 8, cz + 8]) {
                for &(structure, check) in &selection.tries {
                    if !targets.contains(&structure) || check == Check::Rejected {
                        continue;
                    }
                    let (bounds, exact) = self.start_bounds(structure, chunk);
                    if !bounds.contains(pos) {
                        continue;
                    }
                    if !exact {
                        return Err(());
                    }
                    let sure = selection.tries.len() == 1 && check == Check::Passes;
                    if !sure || found.is_some() {
                        return Err(());
                    }
                    found = Some((set, chunk));
                }
            }
        }
        Ok(found)
    }

    /// A start's box as references see it (`StructureStart.getBoundingBox`, made when the
    /// start is: `docs/specs/structure-placement.md#bounds`), and whether it's exact: a jigsaw's
    /// or mineshaft's layout, a single piece's or template's box before its height is
    /// settled, a ruined portal's template. Otherwise a box it surely stays inside.
    fn start_bounds(&self, structure: usize, chunk: [i32; 2]) -> (BoundingBox, bool) {
        use crate::{ScatteredPiece, StructureType};
        let config = &self.placements().sets().structures()[structure];
        let seed = self.placements().seed();
        let (x, z) = (chunk[0].wrapping_shl(4), chunk[1].wrapping_shl(4));
        let piece = |size: [i32; 3]| (ScatteredPiece::new(seed, chunk, size).bounds, true);
        // Within `blocks` of the start chunk on each side, over the level's height.
        let around = |blocks: i32| {
            let heights = self.placements().sets().heights();
            let bottom = heights.level_min_y;
            (
                BoundingBox {
                    min: [x - blocks, bottom, z - blocks],
                    max: [
                        x + 15 + blocks,
                        bottom + heights.level_height - 1,
                        z + 15 + blocks,
                    ],
                },
                false,
            )
        };
        // A start reaches at most 8 chunks from its own (`createReferences`).
        let anywhere = around(8 * 16);
        match config.kind {
            StructureType::Jigsaw | StructureType::Mineshaft => {
                match self.assemble(structure, chunk) {
                    Some(crate::Assembly::Built(layout)) => (layout.bounds, true),
                    _ => anywhere,
                }
            }
            StructureType::DesertPyramid => piece([21, 15, 21]),
            StructureType::JungleTemple => piece([12, 10, 15]),
            StructureType::SwampHut => piece([7, 7, 9]),
            // One block, at the chunk's (9, 90, 9).
            StructureType::BuriedTreasure => {
                let at = [x + 9, 90, z + 9];
                (BoundingBox { min: at, max: at }, true)
            }
            StructureType::OceanMonument => (
                BoundingBox {
                    min: [x - 29, 39, z - 29],
                    max: [x - 29 + 57, 39 + 22, z - 29 + 57],
                },
                true,
            ),
            StructureType::Shipwreck | StructureType::Igloo => {
                let pieces = crate::template_pieces(config, seed, chunk);
                let boxes = pieces.iter().map(|p| {
                    let far = p.template.size.map(|s| s - 1);
                    let (a, b) = (p.world(p.pos, [0, 0, 0]), p.world(p.pos, far));
                    BoundingBox {
                        min: [0, 1, 2].map(|i| a[i].min(b[i])),
                        max: [0, 1, 2].map(|i| a[i].max(b[i])),
                    }
                });
                match boxes.reduce(BoundingBox::encapsulate) {
                    Some(b) => (b, true),
                    None => anywhere,
                }
            }
            StructureType::RuinedPortal => match self.ruined_portal(structure, chunk) {
                Some(layout) => (layout.bounds, true),
                None => anywhere,
            },
            // A cluster's pieces stay within 48 blocks of the start chunk's corner
            // (`OceanRuinPieces.addClusterRuins`).
            StructureType::OceanRuin => around(64),
            _ => anywhere,
        }
    }

    fn search(
        &self,
        targets: &[usize],
        from: [i32; 3],
        radius: i32,
        claimed: &[(usize, [i32; 2])],
    ) -> StructureSearch {
        let sets = self.placements().sets();
        let placements = self.placements_for(targets);
        let mut search = StructureSearch {
            complete: true,
            ..StructureSearch::default()
        };
        // The answer if candidates fail: (distance, hit index).
        let mut best: Option<(f64, usize)> = None;
        let push = |search: &mut StructureSearch, set, chunk, structures, certain| {
            let position = self.locate_pos(set, chunk);
            search.hits.push(StructureHit {
                set,
                chunk,
                position,
                structures,
                certain,
                distance_sqr: dist_sqr(from, position),
            });
            search.hits.len() - 1
        };
        let uncertain =
            |search: &StructureSearch| search.hits.iter().filter(|h| !h.certain).count();

        for &set in &placements {
            let Some(mut chunks) = self.ring_chunks(set, &mut search.complete) else {
                continue;
            };
            // The game keeps the nearest by the chunk centre at y 32; a stable sort keeps the
            // list order on ties, as its strict comparison does.
            let centre = |[x, z]: [i32; 2]| {
                [
                    x.wrapping_shl(4).wrapping_add(8),
                    32,
                    z.wrapping_shl(4).wrapping_add(8),
                ]
            };
            chunks.sort_by(|a, b| {
                dist_sqr(centre(a.0), from).total_cmp(&dist_sqr(centre(b.0), from))
            });
            for (chunk, exact) in chunks {
                if claimed.contains(&(set, chunk)) {
                    continue;
                }
                let Some((structures, certain)) = self.targets_at(set, chunk, targets) else {
                    continue;
                };
                let certain = certain && exact;
                let index = push(&mut search, set, chunk, structures, certain);
                if certain {
                    let distance = search.hits[index].distance_sqr;
                    if best.is_none_or(|(b, _)| distance < b) {
                        best = Some((distance, index));
                    }
                    break;
                }
                if uncertain(&search) >= MAX_UNCERTAIN {
                    search.complete = false;
                    return search;
                }
            }
        }

        let spreads: Vec<(usize, crate::RandomSpread)> = placements
            .iter()
            .filter_map(|&set| match sets.sets()[set].placement.kind {
                PlacementKind::RandomSpread(spread) => Some((set, spread)),
                // findNearestMapStructure handles only these two: a dimension-origin set is
                // never found.
                PlacementKind::ConcentricRings(_) | PlacementKind::DimensionOrigin => None,
            })
            .collect();
        if spreads.is_empty() {
            search.certain_answer = best.map(|(_, i)| i);
            return search;
        }
        let (from_chunk_x, from_chunk_z) = (from[0] >> 4, from[2] >> 4);
        // `ring` is the game's `r`: how many cells out from `from`'s chunk the square ring is.
        for ring in 0..=radius.max(-1) {
            let mut ring_has_certain = false;
            for &(set, spread) in &spreads {
                let salt = sets.sets()[set].placement.salt;
                'scan: for dx in -ring..=ring {
                    let x_edge = dx == -ring || dx == ring;
                    for dz in -ring..=ring {
                        // Only the ring's edge: the inside was the earlier rings.
                        if !x_edge && dz != -ring && dz != ring {
                            continue;
                        }
                        let chunk = spread.potential_chunk(
                            self.placements().seed(),
                            salt,
                            from_chunk_x.wrapping_add(spread.spacing.wrapping_mul(dx)),
                            from_chunk_z.wrapping_add(spread.spacing.wrapping_mul(dz)),
                        );
                        if !self
                            .placements()
                            .is_structure_chunk(set, chunk[0], chunk[1])
                            || claimed.contains(&(set, chunk))
                        {
                            continue;
                        }
                        let Some((structures, certain)) = self.targets_at(set, chunk, targets)
                        else {
                            continue;
                        };
                        let index = push(&mut search, set, chunk, structures, certain);
                        if certain {
                            ring_has_certain = true;
                            let distance = search.hits[index].distance_sqr;
                            if best.is_none_or(|(b, _)| distance < b) {
                                best = Some((distance, index));
                            }
                            break 'scan;
                        }
                        if uncertain(&search) >= MAX_UNCERTAIN {
                            search.complete = false;
                            search.certain_answer = best.map(|(_, i)| i);
                            return search;
                        }
                    }
                }
            }
            if ring_has_certain {
                break;
            }
        }
        search.certain_answer = best.map(|(_, i)| i);
        search
    }

    /// The start of one of `targets` nearest to `from`, horizontally, among the placement
    /// cells [`locate`](Self::locate) searches with `radius`: hits nearest first, every
    /// candidate nearer than the nearest certain start, then that start. Ties go to the set
    /// that comes first among the targets' sets, then the lower chunk z, then x. See
    /// `docs/specs/locate.md`.
    ///
    /// Only chunks that could beat the best certain start so far are decided, nearest first,
    /// so terrain is asked about no more places than it must be. After each ring of cells, the
    /// search stops if no cell in the next ring can hold a start that near.
    pub fn nearest(&self, targets: &[usize], from: [i32; 3], radius: i32) -> StructureSearch {
        let sets = self.placements().sets();
        let placements = self.placements_for(targets);
        let mut nearest = Nearest {
            from,
            hits: Vec::new(),
            best: None,
            complete: true,
        };

        // Ring placements: every position is known, so they're one batch.
        let mut batch: Vec<Chunk> = Vec::new();
        for (rank, &set) in placements.iter().enumerate() {
            let Some(chunks) = self.ring_chunks(set, &mut nearest.complete) else {
                continue;
            };
            batch.extend(chunks.into_iter().map(|(chunk, exact)| Chunk {
                key: Key::of(from, self.locate_pos(set, chunk), rank, chunk),
                set,
                chunk,
                exact,
            }));
        }
        if !self.decide(&mut nearest, batch, targets) {
            return nearest.finish();
        }

        let spreads: Vec<(usize, usize, crate::RandomSpread)> = placements
            .iter()
            .enumerate()
            .filter_map(|(rank, &set)| match sets.sets()[set].placement.kind {
                PlacementKind::RandomSpread(spread) => Some((rank, set, spread)),
                PlacementKind::ConcentricRings(_) | PlacementKind::DimensionOrigin => None,
            })
            .collect();
        if spreads.is_empty() {
            return nearest.finish();
        }
        let (from_chunk_x, from_chunk_z) = (from[0] >> 4, from[2] >> 4);
        // The cell holding chunk (from_chunk_x + spacing·dx, from_chunk_z + spacing·dz), as
        // locate takes it.
        let cell = |spread: &crate::RandomSpread, dx: i32, dz: i32| {
            [
                from_chunk_x
                    .wrapping_add(spread.spacing.wrapping_mul(dx))
                    .div_euclid(spread.spacing),
                from_chunk_z
                    .wrapping_add(spread.spacing.wrapping_mul(dz))
                    .div_euclid(spread.spacing),
            ]
        };
        // `ring_radius` is locate's `r`: how many cells out the square ring is.
        for ring_radius in 0..=radius {
            let mut batch: Vec<Chunk> = Vec::new();
            for &(rank, set, spread) in &spreads {
                let salt = sets.sets()[set].placement.salt;
                for [dx, dz] in ring(ring_radius) {
                    let [cell_x, cell_z] = cell(&spread, dx, dz);
                    let chunk = spread.cell_chunk(self.placements().seed(), salt, cell_x, cell_z);
                    if self
                        .placements()
                        .is_structure_chunk(set, chunk[0], chunk[1])
                    {
                        batch.push(Chunk {
                            key: Key::of(from, self.locate_pos(set, chunk), rank, chunk),
                            set,
                            chunk,
                            exact: true,
                        });
                    }
                }
            }
            if !self.decide(&mut nearest, batch, targets) {
                return nearest.finish();
            }
            let Some(best) = nearest.best else {
                continue;
            };
            // The least distance any start in the next ring can have.
            let next = spreads
                .iter()
                .flat_map(|&(_, set, spread)| {
                    let offset = sets.sets()[set].placement.locate_offset;
                    ring(ring_radius + 1).map(move |[dx, dz]| {
                        let [cell_x, cell_z] = cell(&spread, dx, dz);
                        // The blocks the cell's locate positions span on one axis; `None` where
                        // they'd wrap, as locate_pos's i32 arithmetic would there.
                        let span = |cell: i32, axis_offset: i32| {
                            let low = i64::from(cell) * i64::from(spread.spacing);
                            let high = low + i64::from(spread.spacing - spread.separation) - 1;
                            let block = |chunk: i64| {
                                i32::try_from(chunk * 16 + i64::from(axis_offset)).ok()
                            };
                            Some((i64::from(block(low)?), i64::from(block(high)?)))
                        };
                        // The gap from `from` on one axis; 0 where the span wraps.
                        let axis = |cell, axis_offset, origin| {
                            span(cell, axis_offset).map_or(0, |range| gap(range, origin))
                        };
                        axis(cell_x, offset[0], from[0]).pow(2)
                            + axis(cell_z, offset[2], from[2]).pow(2)
                    })
                })
                .min();
            if next.is_none_or(|least| least > best.distance) {
                break;
            }
        }
        nearest.finish()
    }

    /// Decides `batch`'s chunks nearest first, up to the best certain start, into `nearest`.
    /// False if it stopped at [`MAX_UNCERTAIN`] candidates nearer than any certain start.
    fn decide(&self, nearest: &mut Nearest, mut batch: Vec<Chunk>, targets: &[usize]) -> bool {
        batch.sort_by_key(|c| (c.key, !c.exact));
        // A chunk listed twice (a ring position and another's alternative) is decided once.
        batch.dedup_by_key(|c| c.key);
        for candidate in batch {
            // Past the best certain start, or (at an equal key) the same chunk again.
            if nearest.best.is_some_and(|b| candidate.key >= b) {
                break;
            }
            let Some((structures, certain)) =
                self.targets_at(candidate.set, candidate.chunk, targets)
            else {
                continue;
            };
            let certain = certain && candidate.exact;
            let position = self.locate_pos(candidate.set, candidate.chunk);
            nearest.hits.push((
                candidate.key,
                StructureHit {
                    set: candidate.set,
                    chunk: candidate.chunk,
                    position,
                    structures,
                    certain,
                    distance_sqr: dist_sqr(nearest.from, position),
                },
            ));
            if certain {
                // Sorted, and nothing past the best is decided: this is the new best.
                nearest.best = Some(candidate.key);
            } else if nearest.candidates() >= MAX_UNCERTAIN {
                nearest.complete = false;
                return false;
            }
        }
        true
    }

    /// The sets that place any of `targets`, by first appearance, as
    /// `getPlacementsForStructure` fills its map.
    fn placements_for(&self, targets: &[usize]) -> Vec<usize> {
        let sets = self.placements().sets();
        let mut placements: Vec<usize> = Vec::new();
        for &target in targets {
            for (i, set) in sets.sets().iter().enumerate() {
                if set.possible
                    && set.entries.iter().any(|e| e.structure == target)
                    && !placements.contains(&i)
                {
                    placements.push(i);
                }
            }
        }
        placements
    }

    /// `StructurePlacement.getLocatePos`: the chunk's minimum block plus the set's offset.
    fn locate_pos(&self, set: usize, [x, z]: [i32; 2]) -> [i32; 3] {
        let offset = self.placements().sets().sets()[set].placement.locate_offset;
        [
            x.wrapping_shl(4).wrapping_add(offset[0]),
            offset[1],
            z.wrapping_shl(4).wrapping_add(offset[2]),
        ]
    }

    /// Every chunk a concentric-ring set's positions may be in, and whether it's exact:
    /// spyglass's chunk where it passes the placement test, then the alternatives a tied
    /// biome cell or another platform may give, which are only ever candidates. Clears
    /// `complete` if some position's alternatives weren't enumerated: it may then be anywhere
    /// nearby. `None` for other placement types.
    fn ring_chunks(&self, set: usize, complete: &mut bool) -> Option<Vec<([i32; 2], bool)>> {
        let ring = self.placements().ring_positions(set)?;
        let mut chunks = Vec::new();
        for position in ring {
            let [x, z] = position.chunk;
            if self.placements().is_structure_chunk(set, x, z) {
                let exact = !position.tie_dependent && !position.platform_dependent;
                chunks.push((position.chunk, exact));
            }
            chunks.extend(position.alternatives.iter().map(|&c| (c, false)));
            *complete &= position.alternatives_known;
        }
        Some(chunks)
    }
}

/// The order [`Starts::nearest`] ranks chunks in: horizontal distance squared from the
/// origin to the locate position, the set's rank among the targets' sets, chunk z, chunk x.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    /// Horizontal distance squared from the origin to the locate position.
    distance: i128,
    /// The set's rank among the targets' sets.
    rank: usize,
    /// Chunk z.
    z: i32,
    /// Chunk x.
    x: i32,
}

impl Key {
    fn of(from: [i32; 3], position: [i32; 3], rank: usize, [x, z]: [i32; 2]) -> Key {
        let delta = |i: usize| i128::from(position[i]) - i128::from(from[i]);
        Key {
            distance: delta(0) * delta(0) + delta(2) * delta(2),
            rank,
            z,
            x,
        }
    }
}

/// A chunk [`Starts::nearest`] may decide.
struct Chunk {
    /// Where it ranks.
    key: Key,
    /// The structure set.
    set: usize,
    /// The placement chunk.
    chunk: [i32; 2],
    /// False for a ring position's alternative, or one that's tie- or platform-dependent:
    /// only ever a candidate.
    exact: bool,
}

/// [`Starts::nearest`]'s progress.
struct Nearest {
    /// The search's origin.
    from: [i32; 3],
    /// Every chunk decided to hold a possible start, with its key, in the order decided.
    hits: Vec<(Key, StructureHit)>,
    /// The nearest certain start so far.
    best: Option<Key>,
    /// Whether the hits still hold the game's answer: false once the search gives up early.
    complete: bool,
}

impl Nearest {
    /// Candidates that may still be nearer than the best certain start.
    fn candidates(&self) -> usize {
        self.hits
            .iter()
            .filter(|(k, h)| !h.certain && self.best.is_none_or(|b| *k < b))
            .count()
    }

    /// The hits nearest first, ending at the best certain start.
    fn finish(mut self) -> StructureSearch {
        self.hits
            .retain(|(k, h)| self.best.is_none_or(|b| *k < b || (*k == b && h.certain)));
        self.hits.sort_by_key(|(k, _)| *k);
        let certain_answer = self.best.map(|_| self.hits.len() - 1);
        StructureSearch {
            hits: self.hits.into_iter().map(|(_, h)| h).collect(),
            complete: self.complete,
            certain_answer,
        }
    }
}

/// The cells `(dx, dz)` on ring `r`'s edge, in locate's scan order: `dx` outer, `dz` inner.
fn ring(r: i32) -> impl Iterator<Item = [i32; 2]> {
    (-r..=r).flat_map(move |dx| {
        let x_edge = dx == -r || dx == r;
        (-r..=r)
            .filter(move |&dz| x_edge || dz == -r || dz == r)
            .map(move |dz| [dx, dz])
    })
}

/// The distance from `v` to the range `low..=high`, zero inside it.
fn gap((low, high): (i64, i64), v: i32) -> i128 {
    let v = i64::from(v);
    i128::from((low - v).max(v - high).max(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rings_are_edges_in_scan_order() {
        assert_eq!(ring(0).collect::<Vec<_>>(), [[0, 0]]);
        let one: Vec<_> = ring(1).collect();
        assert_eq!(
            one,
            [
                [-1, -1],
                [-1, 0],
                [-1, 1],
                [0, -1],
                [0, 1],
                [1, -1],
                [1, 0],
                [1, 1]
            ]
        );
        for r in 1..6 {
            let cells: Vec<_> = ring(r).collect();
            assert_eq!(cells.len(), 8 * r as usize);
            assert!(cells.iter().all(|c| c[0].abs() == r || c[1].abs() == r));
        }
    }

    #[test]
    fn gap_is_zero_inside_and_grows_outside() {
        assert_eq!(gap((0, 15), 0), 0);
        assert_eq!(gap((0, 15), 15), 0);
        assert_eq!(gap((0, 15), 16), 1);
        assert_eq!(gap((0, 15), -3), 3);
        assert_eq!(gap((-40, -20), i32::MAX), i128::from(i32::MAX) + 20);
    }

    #[test]
    fn keys_order_by_distance_then_set_then_z_then_x() {
        let from = [0, 64, 0];
        let a = Key::of(from, [16, 0, 0], 1, [1, 0]);
        let b = Key::of(from, [0, 0, 16], 0, [0, 1]);
        let c = Key::of(from, [-16, 0, 0], 0, [-1, 0]);
        // Same distance: rank, then z, then x.
        assert!(c < b && b < a);
        let far = Key::of(from, [i32::MAX, 0, i32::MIN], 0, [0, 0]);
        assert!(a < far);
    }
}
