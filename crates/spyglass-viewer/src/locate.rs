//! Locating from the map: the nearest structure, biome or feature from a clicked point, through
//! the library's `/locate` ports and feature queries (`docs/specs/locate.md`), listed and
//! marked on the map.

use spyglass::{FeatureOptions, LOCATE_BIOME_RADIUS, LOCATE_RADIUS, Session};

use crate::icons;
use crate::userdata::{Kind, Place};

/// How far a feature locate looks, in chunks: placing takes about 30 ms a chunk.
pub const FEATURE_RADIUS: i32 = 8;

/// What to look for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A structure id or `#tag`.
    Structure(String),
    /// A biome id.
    Biome(String),
    /// Features: a name and the placed features it covers.
    Feature { name: String, placed: Vec<String> },
    /// The containers holding an item or item `#tag`.
    Loot(String),
}

impl Target {
    /// What it's called: the id or tag, or the features' name.
    pub fn name(&self) -> &str {
        match self {
            Target::Structure(id) | Target::Biome(id) | Target::Loot(id) => id,
            Target::Feature { name, .. } => name,
        }
    }
}

/// The place marking a feature done marks: the block at its x and z.
pub fn feature_place(feature: &spyglass::FoundFeature) -> Place {
    Place {
        kind: Kind::Place,
        id: String::new(),
        at: [feature.position[0], feature.position[2]],
    }
}

/// One place the game's answer may be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// The block `/locate` reports: x, y (`None` for structures, which report `~`) and z.
    pub x: i32,
    pub y: Option<i32>,
    pub z: i32,
    /// What's there: the structures that may start, or the biome (and its tie).
    pub what: String,
    /// Whether the game certainly finds it: otherwise it's a candidate that depends on terrain,
    /// a tied biome or the platform.
    pub certain: bool,
    /// The distance `/locate` prints.
    pub blocks_away: i32,
    /// Which answer a structure's certain hit is: "Nearest", "/locate's answer" or both.
    pub answer: Option<&'static str>,
    /// What marking it done marks: its structure start, or the biome at its block.
    pub place: Place,
}

/// A locate's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// What was looked for.
    pub target: Target,
    /// The block it was looked for from.
    pub from: [i32; 3],
    /// In the game's search order.
    pub hits: Vec<Hit>,
    /// A structure's nearest start, with the candidates nearer than it, nearest first; empty
    /// where it's `/locate`'s answer too, or for a biome.
    pub nearest: Vec<Hit>,
    /// What to say about the answer as a whole, if anything.
    pub note: Option<String>,
}

/// A structure search's hits as the viewer lists them, with `answer` on the certain one.
fn structure_hits(found: &spyglass::StructureLocate, answer: &'static str) -> Vec<Hit> {
    found
        .hits
        .iter()
        .enumerate()
        .map(|(i, h)| Hit {
            x: h.start.position[0],
            y: None,
            z: h.start.position[2],
            what: h.start.structures.join(" or "),
            certain: h.start.certain,
            blocks_away: h.blocks_away,
            answer: (found.certain_answer == Some(i)).then_some(answer),
            place: Place {
                kind: Kind::Structure,
                id: h.start.set.clone(),
                at: [h.start.position[0], h.start.position[2]],
            },
        })
        .collect()
}

/// Runs `/locate` for `target` from `from`, with the game's radius, and for a structure the
/// nearest start too; structure candidates are decided with the seed's terrain if `confirm`.
pub fn run(
    session: &Session<'_>,
    target: Target,
    from: [i32; 3],
    confirm: bool,
) -> Result<Found, String> {
    let world = session.world();
    let (hits, nearest, note) = match &target {
        Target::Structure(id) => {
            let query = world.structure_query(id).map_err(|e| e.to_string())?;
            let options = spyglass::LocateOptions {
                radius: LOCATE_RADIUS,
                confirm,
            };
            let found = session
                .locate_structure(&query, from, &options)
                .map_err(|e| e.to_string())?;
            let near = session
                .nearest_structure(&query, from, &options)
                .map_err(|e| e.to_string())?;
            // The two searches differ in order, not in what they find: when their certain
            // answers are the same start, one list says both.
            let answer_at = |locate: &spyglass::StructureLocate| {
                locate.certain_answer.map(|i| locate.hits[i].start.position)
            };
            let (hits, nearest) = if answer_at(&found) == answer_at(&near) {
                (
                    structure_hits(&found, "Nearest, and /locate's answer"),
                    Vec::new(),
                )
            } else {
                (
                    structure_hits(&found, "/locate's answer"),
                    structure_hits(&near, "Nearest"),
                )
            };
            let note = if hits.is_empty() {
                Some(format!(
                    "None within {LOCATE_RADIUS} rings of placement cells."
                ))
            } else if !found.complete {
                Some(
                    "spyglass stopped after too many candidates; the answer may be further out."
                        .to_string(),
                )
            } else if hits.iter().any(|h| !h.certain) {
                Some(if confirm {
                    "Candidates are behind a tied biome or a platform-dependent ring position. \
                     The game's answer is the first of these it generates."
                        .to_string()
                } else {
                    "Candidates depend on terrain (confirm with terrain to decide them) or a tied \
                     biome. The game's answer is the first of these it generates."
                        .to_string()
                })
            } else if !nearest.is_empty() {
                Some(
                    "/locate takes the first start in its search order, which isn't always the \
                     nearest."
                        .to_string(),
                )
            } else {
                None
            };
            (hits, nearest, note)
        }
        Target::Biome(id) => {
            let query = world.biome_query(id).map_err(|e| e.to_string())?;
            let found = session
                .locate_biome(&query, from, LOCATE_BIOME_RADIUS)
                .map_err(|e| e.to_string())?;
            let hits = found
                .hits
                .iter()
                .map(|h| Hit {
                    x: h.position[0],
                    y: Some(h.position[1]),
                    z: h.position[2],
                    what: if h.biome.tied.is_empty() {
                        h.biome.biome.clone()
                    } else {
                        format!("{} (tied: {})", h.biome.biome, h.biome.tied.join(", "))
                    },
                    certain: h.certain,
                    blocks_away: h.blocks_away,
                    answer: None,
                    place: Place {
                        kind: Kind::Biome,
                        id: h.biome.biome.clone(),
                        at: [h.position[0], h.position[2]],
                    },
                })
                .collect::<Vec<_>>();
            let note = if hits.is_empty() {
                Some(format!("None within {LOCATE_BIOME_RADIUS} blocks."))
            } else if hits.iter().any(|h| !h.certain) {
                Some(
                    "Tied cells come first: the game's biome there depends on lookup history."
                        .to_string(),
                )
            } else {
                None
            };
            (hits, Vec::new(), note)
        }
        Target::Loot(item) => {
            let query = world.item_query(item, None).map_err(|e| e.to_string())?;
            let found = session
                .nearest_container(&query, from, FEATURE_RADIUS)
                .map_err(|e| e.to_string())?;
            let away = |c: &spyglass::FoundContainer| {
                let (dx, dz) = (
                    f64::from(c.position[0] - from[0]),
                    f64::from(c.position[2] - from[2]),
                );
                (dx * dx + dz * dz).sqrt() as i32
            };
            let hits: Vec<Hit> = found
                .hits
                .iter()
                .map(|c| {
                    let by = match &c.source {
                        spyglass::ContainerSource::Structure(id)
                        | spyglass::ContainerSource::Feature(id) => spyglass::display_name(id),
                        _ => String::new(),
                    };
                    Hit {
                        x: c.position[0],
                        y: Some(c.position[1]),
                        z: c.position[2],
                        what: format!("{} in {by}", spyglass::display_name(&c.holder)),
                        certain: c.contents_certain(),
                        blocks_away: away(c),
                        answer: c.contents_certain().then_some("Nearest"),
                        place: Place {
                            kind: Kind::Place,
                            id: String::new(),
                            at: [c.position[0], c.position[2]],
                        },
                    }
                })
                .collect();
            let note = if hits.is_empty() {
                Some(format!(
                    "None within {FEATURE_RADIUS} chunks. Each chunk is placed, so the search stops there."
                ))
            } else if !found.complete {
                Some(format!(
                    "None certainly holds it within {FEATURE_RADIUS} chunks; these might."
                ))
            } else if hits.len() > 1 {
                Some(
                    "Containers that might hold it, nearer than the nearest that certainly does, come first."
                        .to_string(),
                )
            } else {
                None
            };
            (hits, Vec::new(), note)
        }
        Target::Feature { placed, .. } => {
            let mut found: Vec<spyglass::FoundFeature> = Vec::new();
            for id in placed {
                // A placed feature this version doesn't place is left out.
                let Ok(query) = world.feature_query(id) else {
                    continue;
                };
                let near = session
                    .nearest_feature(&query, from, FEATURE_RADIUS, FeatureOptions::default())
                    .map_err(|e| e.to_string())?;
                found.extend(near.hits);
            }
            let away = |f: &spyglass::FoundFeature| {
                let (dx, dz) = (
                    f64::from(f.position[0] - from[0]),
                    f64::from(f.position[2] - from[2]),
                );
                (dx * dx + dz * dz).sqrt() as i32
            };
            found.sort_by_key(|f| (away(f), !f.certain));
            // Every candidate nearer than the nearest certain one, then that one.
            if let Some(end) = found.iter().position(|f| f.certain) {
                found.truncate(end + 1);
            }
            let hits: Vec<Hit> = found
                .iter()
                .map(|f| Hit {
                    x: f.position[0],
                    y: Some(f.position[1]),
                    z: f.position[2],
                    what: icons::feature_name(&f.feature),
                    certain: f.certain,
                    blocks_away: away(f),
                    answer: f.certain.then_some("Nearest"),
                    place: feature_place(f),
                })
                .collect();
            let note = if hits.is_empty() {
                Some(format!(
                    "None within {FEATURE_RADIUS} chunks. Each chunk is placed, so the search stops there."
                ))
            } else if !hits.iter().any(|h| h.certain) {
                Some(format!(
                    "No certain one within {FEATURE_RADIUS} chunks; these placed on spyglass's blocks, but something it can't know could change that."
                ))
            } else if hits.len() > 1 {
                Some(
                    "Likely ones nearer than the nearest certain one come first: something spyglass can't know could change them."
                        .to_string(),
                )
            } else {
                None
            };
            (hits, Vec::new(), note)
        }
    };
    Ok(Found {
        target,
        from,
        hits,
        nearest,
        note,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use spyglass_oracle::fixtures::{self, resource};

    use super::*;

    #[test]
    fn answers_are_the_same_as_the_library() {
        let fixture = fixtures::overworld(
            &[
                resource(
                    "worldgen/structure_set",
                    "towers",
                    json!({"placement": {"type": "minecraft:random_spread", "salt": 1,
                            "spacing": 4, "separation": 1},
                        "structures": [{"structure": "minecraft:tower", "weight": 1}]}),
                ),
                resource(
                    "worldgen/structure",
                    "tower",
                    json!({"type": "minecraft:stronghold", "biomes": "minecraft:plains", "step": "surface_structures"}),
                ),
            ],
            false,
        );
        let world = spyglass::World::open(fixture.path()).unwrap();
        let session = world.session(5).unwrap();
        let from = [300, 64, 20];

        let query = world.structure_query("minecraft:tower").unwrap();
        for confirm in [false, true] {
            let target = Target::Structure("minecraft:tower".into());
            let found = run(&session, target, from, confirm).unwrap();
            let expected = session
                .locate_structure(
                    &query,
                    from,
                    &spyglass::LocateOptions {
                        radius: LOCATE_RADIUS,
                        confirm,
                    },
                )
                .unwrap();
            assert_eq!(found.hits.len(), expected.hits.len());
            let first = &expected.hits[0];
            assert_eq!(
                [found.hits[0].x, found.hits[0].z],
                [first.start.position[0], first.start.position[2]]
            );
            assert_eq!(found.hits[0].blocks_away, first.blocks_away);
            assert_eq!(found.hits[0].certain, first.start.certain);

            // The nearest is listed apart only where it isn't /locate's answer.
            let near = session
                .nearest_structure(
                    &query,
                    from,
                    &spyglass::LocateOptions {
                        radius: LOCATE_RADIUS,
                        confirm,
                    },
                )
                .unwrap();
            let answer = |hits: &[Hit], label| {
                hits.iter()
                    .find(|h| h.answer == Some(label))
                    .map(|h| [h.x, h.z])
            };
            let nearest = near.certain_answer.map(|i| {
                let position = near.hits[i].start.position;
                [position[0], position[2]]
            });
            if found.nearest.is_empty() {
                assert_eq!(
                    answer(&found.hits, "Nearest, and /locate's answer"),
                    nearest
                );
            } else {
                assert_eq!(answer(&found.nearest, "Nearest"), nearest);
                assert!(answer(&found.hits, "/locate's answer").is_some());
            }
        }

        let found = run(
            &session,
            Target::Biome("minecraft:desert".into()),
            [-300, 64, 0],
            true,
        )
        .unwrap();
        let last = found.hits.last().unwrap();
        assert!(last.certain && last.x >= 0 && last.what == "minecraft:desert");

        assert!(
            run(
                &session,
                Target::Biome("minecraft:nowhere".into()),
                from,
                true
            )
            .is_err()
        );
    }
}
