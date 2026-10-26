//! The placement prefilter: necessary conditions for required constraints that the seed's lower
//! 48 bits decide (ADR 0021; `docs/specs/structure-placement.md#the-lower-48-bits`).
//!
//! A filter may reject a seed only if the full evaluation would too. Each one counts placement
//! chunks, a superset of the structure starts, so it bounds a count from above, and it can only
//! refute "at least N". Slime chunks are exact. Spawn anchors are unknown here, so their discs
//! are widened to every place the spawn can be.

use spyglass_structure::{PlacementKind, SeedPlacements, StructureSets, is_slime_chunk};

use crate::group::{distance_sqr, largest_group};
use crate::query::{Anchor, Constraint, Count, Importance, Kind, Place, Region};

/// The farthest the spawn can be from (0, 0): its climate search looks within 2,048 blocks of
/// (0, 0), then within 512 of the best so far (`docs/specs/spawn.md`).
pub const SPAWN_REACH: u32 = 2048 + 512;

/// The structures a constraint names, as indices into [`StructureSets::structures`].
#[derive(Debug, Clone, Copy, Default)]
pub struct Named<'a> {
    /// What's counted, for structure counts and clusters.
    pub target: Option<&'a [usize]>,
    /// A structure anchor's structure.
    pub anchor: Option<&'a [usize]>,
}

/// The placement prefilter for one query.
#[derive(Debug, Clone)]
pub struct Prefilter {
    filters: Vec<Filter>,
}

#[derive(Debug, Clone)]
enum Filter {
    /// At least `count` placement chunks of `sets` in the region.
    Starts {
        count: usize,
        sets: Vec<usize>,
        region: Within,
    },
    /// A group of `size` placement chunks of `sets` in `area`, every two at most `apart` apart.
    Cluster {
        size: usize,
        sets: Vec<usize>,
        apart: u32,
        area: Area,
    },
    /// At least `count` slime chunks in `area`.
    Slime { count: usize, area: Area },
}

#[derive(Debug, Clone)]
enum Within {
    Area(Area),
    /// Within `radius` of a placement chunk of `sets` in `area`, other than itself.
    Near {
        radius: u32,
        sets: Vec<usize>,
        area: Area,
    },
}

#[derive(Debug, Clone, Copy)]
enum Area {
    Disc { centre: [i32; 2], radius: u32 },
    Box { min: [i32; 2], max: [i32; 2] },
}

impl Area {
    /// The place, with a spawn anchor widened to every place the spawn can be.
    fn new(place: &Place) -> Area {
        match *place {
            Place::Within { radius, anchor } => match anchor {
                Anchor::Spawn => Area::Disc {
                    centre: [0, 0],
                    radius: radius + SPAWN_REACH,
                },
                Anchor::Point(centre) => Area::Disc { centre, radius },
            },
            Place::Box { min, max } => Area::Box { min, max },
        }
    }

    fn bounds(self) -> ([i32; 2], [i32; 2]) {
        match self {
            Area::Disc { centre, radius } => {
                let r = radius as i32; // at most 32,767 + SPAWN_REACH
                (centre.map(|c| c - r), centre.map(|c| c + r))
            }
            Area::Box { min, max } => (min, max),
        }
    }

    fn contains(self, p: [i32; 2]) -> bool {
        match self {
            Area::Disc { centre, radius } => distance_sqr(centre, p) <= u64::from(radius).pow(2),
            Area::Box { min, max } => (0..2).all(|i| min[i] <= p[i] && p[i] <= max[i]),
        }
    }
}

fn spawn_anchored(place: &Place) -> bool {
    matches!(
        place,
        Place::Within {
            anchor: Anchor::Spawn,
            ..
        }
    )
}

/// The possible sets that can start one of `structures`, or `None` if one of them is on
/// concentric rings, whose positions the lower 48 bits don't decide.
fn sets_of(sets: &StructureSets, structures: &[usize]) -> Option<Vec<usize>> {
    let mut out = Vec::new();
    for (i, set) in sets.sets().iter().enumerate() {
        if set.possible
            && set
                .entries
                .iter()
                .any(|e| structures.contains(&e.structure))
        {
            if let PlacementKind::ConcentricRings(_) = set.placement.kind {
                return None;
            }
            out.push(i);
        }
    }
    Some(out)
}

impl Prefilter {
    /// The filters for `constraints`, each with the structures it names. Only required "at
    /// least" constraints the lower 48 bits can refute get one.
    pub fn new(sets: &StructureSets, constraints: &[Constraint], named: &[Named<'_>]) -> Prefilter {
        let filters = constraints
            .iter()
            .zip(named)
            .filter(|(c, _)| c.importance == Importance::Required)
            .filter_map(|(c, named)| Self::filter(sets, &c.kind, named))
            .collect();
        Prefilter { filters }
    }

    fn filter(sets: &StructureSets, kind: &Kind, named: &Named<'_>) -> Option<Filter> {
        match kind {
            Kind::Structures {
                count: Count::AtLeast(n),
                region,
                ..
            } => {
                let target = sets_of(sets, named.target?)?;
                let region = match region {
                    Region::Place(place) => Within::Area(Area::new(place)),
                    Region::NearStructure { radius, place, .. } => Within::Near {
                        radius: *radius,
                        sets: sets_of(sets, named.anchor?)?,
                        area: Area::new(place),
                    },
                };
                Some(Filter::Starts {
                    count: *n as usize,
                    sets: target,
                    region,
                })
            }
            Kind::Cluster {
                size, apart, place, ..
            } => Some(Filter::Cluster {
                size: *size as usize,
                sets: sets_of(sets, named.target?)?,
                apart: *apart,
                area: Area::new(place),
            }),
            // Widened to the spawn's reach, a slime count would almost never fail and costs
            // tens of thousands of chunks, so only fixed places are checked.
            Kind::Slime {
                count: Count::AtLeast(n),
                region: Region::Place(place),
            } if !spawn_anchored(place) => Some(Filter::Slime {
                count: *n as usize,
                area: Area::new(place),
            }),
            _ => None,
        }
    }

    /// Whether the query has any filter: if not, every seed passes.
    pub fn is_empty(&self) -> bool {
        self.filters.is_empty()
    }

    /// Whether `seed` may meet every required constraint, as far as its lower 48 bits tell. The
    /// same for every seed with the same lower 48 bits.
    pub fn accepts(&self, sets: &StructureSets, seed: i64) -> bool {
        let placements = sets.seed_placements(seed);
        self.filters.iter().all(|f| match f {
            Filter::Starts {
                count,
                sets: targets,
                region: Within::Area(area),
            } => {
                let (min, max) = area.bounds();
                possible_starts(sets, &placements, targets, min, max)
                    .filter(|s| area.contains(s.position))
                    .nth(count - 1)
                    .is_some()
            }
            Filter::Starts {
                count,
                sets: targets,
                region:
                    Within::Near {
                        radius,
                        sets: anchors,
                        area,
                    },
            } => {
                let (min, max) = area.bounds();
                let anchors: Vec<Start> = possible_starts(sets, &placements, anchors, min, max)
                    .filter(|s| area.contains(s.position))
                    .collect();
                let Some((min, max)) = reach(&anchors, *radius) else {
                    return false;
                };
                let limit = u64::from(*radius).pow(2);
                possible_starts(sets, &placements, targets, min, max)
                    .filter(|t| {
                        anchors.iter().any(|a| {
                            (a.set, a.chunk) != (t.set, t.chunk)
                                && distance_sqr(a.position, t.position) <= limit
                        })
                    })
                    .nth(count - 1)
                    .is_some()
            }
            Filter::Cluster {
                size,
                sets: targets,
                apart,
                area,
            } => {
                let (min, max) = area.bounds();
                let points: Vec<[i32; 2]> = possible_starts(sets, &placements, targets, min, max)
                    .map(|s| s.position)
                    .filter(|&p| area.contains(p))
                    .collect();
                largest_group(&points, *apart, *size).len() >= *size
            }
            Filter::Slime { count, area } => {
                let (min, max) = area.bounds();
                let lo = min.map(|b| (b - 8 + 15).div_euclid(16));
                let hi = max.map(|b| (b - 8).div_euclid(16));
                (lo[1]..=hi[1])
                    .flat_map(|z| (lo[0]..=hi[0]).map(move |x| [x, z]))
                    .filter(|&[x, z]| {
                        area.contains([x * 16 + 8, z * 16 + 8]) && is_slime_chunk(seed, x, z)
                    })
                    .nth(count - 1)
                    .is_some()
            }
        })
    }
}

/// A placement chunk and its `/locate` position.
#[derive(Debug, Clone, Copy)]
struct Start {
    set: usize,
    chunk: [i32; 2],
    position: [i32; 2],
}

/// The placement chunks of `targets` whose `/locate` position is in `min..=max`.
fn possible_starts<'a>(
    sets: &'a StructureSets,
    placements: &'a SeedPlacements<'a>,
    targets: &'a [usize],
    min: [i32; 2],
    max: [i32; 2],
) -> impl Iterator<Item = Start> + 'a {
    targets.iter().flat_map(move |&set| {
        let offset = sets.sets()[set].placement.locate_offset;
        let o = [offset[0], offset[2]];
        // position = chunk · 16 + offset, so the chunks run from ceil((min − offset) / 16) to
        // floor((max − offset) / 16).
        let lo = [0, 1].map(|i| (min[i] - o[i] + 15).div_euclid(16));
        let hi = [0, 1].map(|i| (max[i] - o[i]).div_euclid(16));
        placements
            .possible_chunks_in(set, lo, hi)
            .unwrap_or_default()
            .into_iter()
            .map(move |chunk| Start {
                set,
                chunk,
                position: [chunk[0] * 16 + o[0], chunk[1] * 16 + o[1]],
            })
    })
}

/// The box `radius` around every anchor, or `None` if there are none.
fn reach(anchors: &[Start], radius: u32) -> Option<([i32; 2], [i32; 2])> {
    let r = radius as i32; // at most 32,767
    let (first, rest) = anchors.split_first()?;
    let (mut min, mut max) = (first.position, first.position);
    for a in rest {
        for i in 0..2 {
            min[i] = min[i].min(a.position[i]);
            max[i] = max[i].max(a.position[i]);
        }
    }
    Some((min.map(|c| c - r), max.map(|c| c + r)))
}
