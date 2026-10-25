//! Biome sets (`HolderSet<Biome>`) and biome tags, resolved against a biome source's biomes.

use std::collections::HashMap;

use serde_json::Value;
use spyglass_biome::{BiomeId, ParameterList};
use spyglass_data::bundle::Bundle;
use spyglass_density::{LoadError, LoadErrorKind};

use crate::load::{At, LoadResult, full_id};

const BIOME: &str = "worldgen/biome";
const BIOME_TAG: &str = "tags/worldgen/biome";

/// A set of biomes, as a bit set over a biome source's [`BiomeId`]s. Biomes the source can't
/// produce (the Nether's, in the overworld) aren't in it, since no lookup can return them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BiomeSet {
    words: Vec<u64>,
}

impl BiomeSet {
    /// Whether `biome` is in the set.
    pub fn contains(&self, biome: BiomeId) -> bool {
        let i = usize::from(biome.0);
        self.words
            .get(i / 64)
            .is_some_and(|word| word >> (i % 64) & 1 == 1)
    }

    /// Whether the set is empty.
    pub fn is_empty(&self) -> bool {
        self.words.iter().all(|&w| w == 0)
    }

    /// The biomes in the set, by id.
    pub fn iter(&self) -> impl Iterator<Item = BiomeId> + '_ {
        self.words.iter().enumerate().flat_map(|(i, &word)| {
            (0..64)
                .filter(move |bit| word >> bit & 1 == 1)
                .map(move |bit| BiomeId((i * 64 + bit) as u16))
        })
    }

    fn insert(&mut self, biome: BiomeId) {
        let i = usize::from(biome.0);
        if self.words.len() <= i / 64 {
            self.words.resize(i / 64 + 1, 0);
        }
        self.words[i / 64] |= 1 << (i % 64);
    }

    fn union(&mut self, other: &BiomeSet) {
        if self.words.len() < other.words.len() {
            self.words.resize(other.words.len(), 0);
        }
        for (a, b) in self.words.iter_mut().zip(&other.words) {
            *a |= b;
        }
    }
}

/// Resolves biome holder sets and tags, loading each tag once.
pub(crate) struct Tags<'a> {
    bundle: &'a Bundle,
    biomes: &'a ParameterList,
    tags: HashMap<String, BiomeSet>,
    /// Tags being resolved, outermost first, for cycle detection.
    loading: Vec<String>,
}

impl<'a> Tags<'a> {
    pub(crate) fn new(bundle: &'a Bundle, biomes: &'a ParameterList) -> Self {
        Tags {
            bundle,
            biomes,
            tags: HashMap::new(),
            loading: Vec::new(),
        }
    }

    /// `RegistryCodecs.homogeneousList(Registries.BIOME)`: `"#tag"`, a single biome id, or a
    /// list of biome ids.
    pub(crate) fn holder_set(&mut self, json: &Value, at: &At<'_>) -> LoadResult<BiomeSet> {
        match json {
            Value::String(s) => match s.strip_prefix('#') {
                Some(tag) => self.tag(tag, at),
                None => self.biome(s, at),
            },
            Value::Array(items) => {
                let mut set = BiomeSet::default();
                for (i, item) in items.iter().enumerate() {
                    let at = at.child(&format!("[{i}]"));
                    let id = item
                        .as_str()
                        .ok_or_else(|| at.error(LoadErrorKind::Shape("a biome id")))?;
                    set.union(&self.biome(id, &at)?);
                }
                Ok(set)
            }
            _ => Err(at.error(LoadErrorKind::Shape("a biome tag, id or list of ids"))),
        }
    }

    /// A tag the game names in code (`BiomeTags`): empty if the bundle doesn't have it, as
    /// `Holder.is(tag)` is false for a tag that doesn't exist.
    pub(crate) fn optional_tag(&mut self, id: &str) -> LoadResult<BiomeSet> {
        if !self.exists(BIOME_TAG, id) {
            return Ok(BiomeSet::default());
        }
        let resource = format!("#{id}");
        self.tag(id, &At::root(&resource))
    }

    /// One biome, which must exist in the bundle's biome registry.
    fn biome(&self, id: &str, at: &At<'_>) -> LoadResult<BiomeSet> {
        let mut set = BiomeSet::default();
        if !self.exists(BIOME, id) {
            return Err(at.error(LoadErrorKind::Unsupported {
                what: "biome",
                name: id.to_string(),
            }));
        }
        if let Some(biome) = self.biomes.id(&full_id(id)) {
            set.insert(biome);
        }
        Ok(set)
    }

    /// A biome tag: `{"values": [...]}`, each entry an id, a `#tag`, or
    /// `{"id": ..., "required": bool}` (`TagEntry.CODEC`). A missing optional entry is skipped.
    fn tag(&mut self, id: &str, at: &At<'_>) -> LoadResult<BiomeSet> {
        let id = full_id(id);
        if let Some(set) = self.tags.get(&id) {
            return Ok(set.clone());
        }
        if self.loading.contains(&id) {
            let mut cycle = self.loading.clone();
            cycle.push(id);
            return Err(at.error(LoadErrorKind::Cycle(cycle)));
        }
        let json = self
            .bundle
            .resource(BIOME_TAG, &id)
            .map_err(|e| LoadError {
                resource: format!("#{id}"),
                path: String::new(),
                kind: Box::new(e.into()),
            })?;
        self.loading.push(id.clone());
        let resource = format!("#{id}");
        let root = At::root(&resource);
        let values = json
            .get("values")
            .and_then(Value::as_array)
            .ok_or_else(|| root.error(LoadErrorKind::Missing("values")))?;
        let values_at = root.child("values");
        let mut set = BiomeSet::default();
        for (i, entry) in values.iter().enumerate() {
            let at = values_at.child(&format!("[{i}]"));
            let (entry_id, required) = match entry {
                Value::String(s) => (s.as_str(), true),
                Value::Object(obj) => (
                    obj.get("id")
                        .and_then(Value::as_str)
                        .ok_or_else(|| at.error(LoadErrorKind::Missing("id")))?,
                    match obj.get("required") {
                        None => true,
                        Some(v) => v.as_bool().ok_or_else(|| {
                            at.child("required")
                                .error(LoadErrorKind::Shape("a boolean"))
                        })?,
                    },
                ),
                _ => return Err(at.error(LoadErrorKind::Shape("a tag entry"))),
            };
            let (registry, name) = match entry_id.strip_prefix('#') {
                Some(tag) => (BIOME_TAG, tag),
                None => (BIOME, entry_id),
            };
            if !required && !self.exists(registry, name) {
                continue;
            }
            let entry_set = if registry == BIOME_TAG {
                self.tag(name, &at)?
            } else {
                self.biome(name, &at)?
            };
            set.union(&entry_set);
        }
        self.loading.pop();
        self.tags.insert(id, set.clone());
        Ok(set)
    }

    fn exists(&self, registry: &str, id: &str) -> bool {
        Bundle::resource_path(registry, id)
            .is_ok_and(|path| self.bundle.manifest().files.contains_key(&path))
    }
}
