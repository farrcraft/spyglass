//! A dimension's placed features: each biome's lists by decoration step, and the order they
//! sort into (`docs/specs/features.md`).

use std::collections::HashMap;

use serde_json::Value;
use spyglass_biome::BiomeNames;
use spyglass_data::codec::{At, LoadError, LoadErrorKind, full_id, object};
use spyglass_density::Resources;

use crate::order::{self, PlacedRef};

/// The placed-feature registry.
const PLACED_FEATURE: &str = "worldgen/placed_feature";
/// Placed-feature tags.
const PLACED_FEATURE_TAGS: &str = "tags/worldgen/placed_feature";

/// A placed feature in a biome's lists: a registry entry, or one written inline.
#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    /// The registry id; `None` for an inline entry.
    pub id: Option<String>,
    /// Its JSON: `feature` and `placement`.
    pub json: Value,
}

/// A dimension's placed features, seed-independent.
#[derive(Debug, Clone)]
pub struct FeatureSettings {
    /// Every placed feature the lists name, each once (`PlacedRef` indexes this).
    placed: Vec<Placed>,
    /// By biome id, then step: the biome's list.
    lists: Vec<Vec<Vec<PlacedRef>>>,
    /// Each step's features in decoration order.
    order: Vec<Vec<PlacedRef>>,
}

impl FeatureSettings {
    /// Loads the feature lists of `biomes`, the dimension's possible biomes in the biome
    /// source's order, and sorts them. `v1_21` is 26.2's data: its sorter counts equal placed
    /// features as one, which no vanilla list relies on, so lists that would are refused.
    pub fn load<R: Resources + ?Sized>(
        resources: &R,
        biomes: &BiomeNames,
        v1_21: bool,
    ) -> Result<Self, LoadError> {
        let mut placed: Vec<Placed> = Vec::new();
        let mut by_id: HashMap<String, PlacedRef> = HashMap::new();
        let mut lists = Vec::new();
        for name in biomes.names() {
            let id = full_id(name);
            let root = At::root(&id);
            let json = resources
                .resource("worldgen/biome", &id)
                .map_err(|e| root.error(e.into()))?;
            let biome = object(&json, &root)?;
            let at = root.child("features");
            let steps = match biome.get("features") {
                None => Vec::new(),
                Some(Value::Array(steps)) => steps.as_slice().to_vec(),
                Some(_) => return Err(at.error(LoadErrorKind::Shape("a list of feature lists"))),
            };
            let mut biome_lists = Vec::new();
            for (step, set) in steps.iter().enumerate() {
                let at = at.child(&step.to_string());
                let mut list = Vec::new();
                for entry in holder_set(resources, set, &at)? {
                    let r = match entry {
                        Entry::Id(id) => match by_id.get(&id) {
                            Some(&r) => r,
                            None => {
                                let json = resources
                                    .resource(PLACED_FEATURE, &id)
                                    .map_err(|e| At::root(&id).error(e.into()))?;
                                placed.push(Placed {
                                    id: Some(id.clone()),
                                    json,
                                });
                                by_id.insert(id, placed.len() - 1);
                                placed.len() - 1
                            }
                        },
                        Entry::Inline(json) => {
                            placed.push(Placed { id: None, json });
                            placed.len() - 1
                        }
                    };
                    list.push(r);
                }
                biome_lists.push(list);
            }
            lists.push(biome_lists);
        }
        if v1_21 {
            refuse_equal_features(&placed, &lists)?;
        }
        let order = order::features_per_step(&lists).ok_or_else(|| {
            LoadError::new(
                "worldgen/biome",
                "features",
                LoadErrorKind::Invalid(
                    "feature order cycle found: biomes list features in orders that can't agree",
                ),
            )
        })?;
        Ok(FeatureSettings {
            placed,
            lists,
            order,
        })
    }

    /// A placed feature.
    pub fn placed(&self, r: PlacedRef) -> &Placed {
        &self.placed[r]
    }

    /// Each step's features in decoration order: a feature's index in its step is what its
    /// seed uses.
    pub fn order(&self) -> &[Vec<PlacedRef>] {
        &self.order
    }

    /// A biome's list for each step, by its id in the biome source.
    pub fn lists(&self, biome: spyglass_biome::BiomeId) -> &[Vec<PlacedRef>] {
        &self.lists[usize::from(biome.0)]
    }
}

/// An entry of a `HolderSet<PlacedFeature>`.
enum Entry {
    Id(String),
    Inline(Value),
}

/// A holder set: a `#tag`, an id, an inline entry, or a list of ids and inline entries.
fn holder_set<R: Resources + ?Sized>(
    resources: &R,
    json: &Value,
    at: &At<'_>,
) -> Result<Vec<Entry>, LoadError> {
    Ok(match json {
        Value::String(s) => match s.strip_prefix('#') {
            Some(tag) => spyglass_terrain::tag(resources, PLACED_FEATURE_TAGS, tag)?
                .into_iter()
                .map(Entry::Id)
                .collect(),
            None => vec![Entry::Id(full_id(s))],
        },
        Value::Object(_) => vec![Entry::Inline(json.clone())],
        Value::Array(items) => items
            .iter()
            .enumerate()
            .map(|(i, item)| match item {
                Value::String(s) => Ok(Entry::Id(full_id(s))),
                Value::Object(_) => Ok(Entry::Inline(item.clone())),
                _ => Err(at
                    .child(&i.to_string())
                    .error(LoadErrorKind::Shape("a placed feature id or object"))),
            })
            .collect::<Result<_, _>>()?,
        _ => return Err(at.error(LoadErrorKind::Shape("a placed feature list or #tag"))),
    })
}

/// 26.2's sorter numbers placed features by equality (`Object2IntOpenHashMap`, and
/// `PlacedFeature` is a record), so two equal ones in the lists would be one node. spyglass
/// numbers them apart, as 26.3 does, and refuses lists where that would differ.
fn refuse_equal_features(
    placed: &[Placed],
    lists: &[Vec<Vec<PlacedRef>>],
) -> Result<(), LoadError> {
    let mut used: Vec<PlacedRef> = lists.iter().flatten().flatten().copied().collect();
    used.sort_unstable();
    used.dedup();
    for (i, &a) in used.iter().enumerate() {
        for &b in &used[..i] {
            if placed[a].json == placed[b].json {
                let name = |r: PlacedRef| {
                    placed[r]
                        .id
                        .clone()
                        .unwrap_or_else(|| "an inline feature".into())
                };
                return Err(LoadError::new(
                    "worldgen/biome",
                    "features",
                    LoadErrorKind::Unsupported {
                        what: "equal placed features in 26.2's feature lists",
                        name: format!("{} and {}", name(a), name(b)),
                    },
                ));
            }
        }
    }
    Ok(())
}
