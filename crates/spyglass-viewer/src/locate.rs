//! Locating from the map: the nearest structure or biome from a clicked point, through the
//! library's `/locate` ports (`docs/specs/locate.md`), listed and marked on the map.

use spyglass::{LOCATE_BIOME_RADIUS, LOCATE_RADIUS, Session};

/// What to look for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A structure id or `#tag`.
    Structure(String),
    /// A biome id.
    Biome(String),
}

impl Target {
    pub fn name(&self) -> &str {
        match self {
            Target::Structure(id) | Target::Biome(id) => id,
        }
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
    pub certain: bool,
    /// The distance `/locate` prints.
    pub blocks_away: i32,
}

/// A locate's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub target: Target,
    pub from: [i32; 3],
    /// In the game's search order.
    pub hits: Vec<Hit>,
    /// What to say about the answer as a whole, if anything.
    pub note: Option<String>,
}

/// Runs `/locate` for `target` from `from`, with the game's radius.
pub fn run(session: &Session<'_>, target: Target, from: [i32; 3]) -> Result<Found, String> {
    let world = session.world();
    let (hits, note) = match &target {
        Target::Structure(id) => {
            let query = world.structure_query(id).map_err(|e| e.to_string())?;
            let found = session
                .locate_structure(&query, from, LOCATE_RADIUS)
                .map_err(|e| e.to_string())?;
            let hits = found
                .hits
                .iter()
                .map(|h| Hit {
                    x: h.start.position[0],
                    y: None,
                    z: h.start.position[2],
                    what: h.start.structures.join(" or "),
                    certain: h.start.certain,
                    blocks_away: h.blocks_away,
                })
                .collect::<Vec<_>>();
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
                Some(
                    "Candidates depend on terrain or a tied biome. The game's answer is the first of \
                     these it generates."
                        .to_string(),
                )
            } else {
                None
            };
            (hits, note)
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
            (hits, note)
        }
    };
    Ok(Found {
        target,
        from,
        hits,
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
                    json!({"type": "minecraft:stronghold", "biomes": "minecraft:plains"}),
                ),
            ],
            false,
        );
        let world = spyglass::World::open(fixture.path()).unwrap();
        let session = world.session(5).unwrap();
        let from = [300, 64, 20];

        let found = run(&session, Target::Structure("minecraft:tower".into()), from).unwrap();
        let query = world.structure_query("minecraft:tower").unwrap();
        let expected = session
            .locate_structure(&query, from, LOCATE_RADIUS)
            .unwrap();
        assert_eq!(found.hits.len(), expected.hits.len());
        let first = &expected.hits[0];
        assert_eq!(
            [found.hits[0].x, found.hits[0].z],
            [first.start.position[0], first.start.position[2]]
        );
        assert_eq!(found.hits[0].blocks_away, first.blocks_away);

        let found = run(
            &session,
            Target::Biome("minecraft:desert".into()),
            [-300, 64, 0],
        )
        .unwrap();
        let last = found.hits.last().unwrap();
        assert!(last.certain && last.x >= 0 && last.what == "minecraft:desert");

        assert!(run(&session, Target::Biome("minecraft:nowhere".into()), from).is_err());
    }
}
