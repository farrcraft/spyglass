//! The game's nearest-structure search (`ChunkGenerator.findNearestMapStructure`, used by
//! `/locate structure`), with each placement chunk's start decided as far as spyglass can
//! without terrain. See `docs/specs/locate.md`.

use serde_json::Value;
use spyglass_data::bundle::Bundle;
use spyglass_density::LoadErrorKind;

use crate::check::{Check, Starts};
use crate::load::{At, LoadResult, StructureSets, full_id};
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
                Value::String(s) => (s.as_str(), true),
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
        let sets = self.placements().sets();
        // Placements by first appearance, as getPlacementsForStructure fills the map.
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
        let mut search = StructureSearch {
            complete: true,
            ..StructureSearch::default()
        };
        // The answer if candidates fail: (distance, hit index).
        let mut best: Option<(f64, usize)> = None;
        let locate_pos = |set: usize, [x, z]: [i32; 2]| {
            let offset = sets.sets()[set].placement.locate_offset;
            [
                x.wrapping_shl(4).wrapping_add(offset[0]),
                offset[1],
                z.wrapping_shl(4).wrapping_add(offset[2]),
            ]
        };
        let push = |search: &mut StructureSearch, set, chunk, structures, certain| {
            let position = locate_pos(set, chunk);
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
            let Some(ring) = self.placements().ring_positions(set) else {
                continue;
            };
            // Every chunk a ring position may be in: spyglass's, then the alternatives a tied
            // biome cell or another platform may give, which are only ever candidates.
            let mut chunks: Vec<([i32; 2], bool)> = Vec::new();
            for position in ring {
                let [x, z] = position.chunk;
                if self.placements().is_structure_chunk(set, x, z) {
                    let exact = !position.tie_dependent && !position.platform_dependent;
                    chunks.push((position.chunk, exact));
                }
                chunks.extend(position.alternatives.iter().map(|&c| (c, false)));
                // A position whose alternatives weren't enumerated may be anywhere nearby, so
                // the list can't promise to hold the game's answer.
                search.complete &= position.alternatives_known;
            }
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
                let Some((structures, certain)) = self.targets_at(set, chunk, targets) else {
                    continue;
                };
                let certain = certain && exact;
                let index = push(&mut search, set, chunk, structures, certain);
                if certain {
                    let d = search.hits[index].distance_sqr;
                    if best.is_none_or(|(b, _)| d < b) {
                        best = Some((d, index));
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
                PlacementKind::ConcentricRings(_) => None,
            })
            .collect();
        if spreads.is_empty() {
            search.certain_answer = best.map(|(_, i)| i);
            return search;
        }
        let (cx, cz) = (from[0] >> 4, from[2] >> 4);
        for r in 0..=radius.max(-1) {
            let mut ring_has_certain = false;
            for &(set, spread) in &spreads {
                let salt = sets.sets()[set].placement.salt;
                'scan: for dx in -r..=r {
                    let x_edge = dx == -r || dx == r;
                    for dz in -r..=r {
                        if !x_edge && dz != -r && dz != r {
                            continue;
                        }
                        let chunk = spread.potential_chunk(
                            self.placements().seed(),
                            salt,
                            cx.wrapping_add(spread.spacing.wrapping_mul(dx)),
                            cz.wrapping_add(spread.spacing.wrapping_mul(dz)),
                        );
                        if !self
                            .placements()
                            .is_structure_chunk(set, chunk[0], chunk[1])
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
                            let d = search.hits[index].distance_sqr;
                            if best.is_none_or(|(b, _)| d < b) {
                                best = Some((d, index));
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
}
