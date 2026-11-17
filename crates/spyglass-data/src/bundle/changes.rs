//! What each pack applied over a bundle changes, worked out from the layers' file lists as
//! [`Bundle::look_up`] decides which file wins, reading only tag files, for `replace`. See
//! `docs/specs/data-packs.md`, "What a pack changes".

use std::collections::{BTreeMap, BTreeSet};

use super::{Bundle, Layer, is_tag, java_bool, reads_as_tag_file};

/// What one applied pack changes, by registry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LayerChanges {
    /// The pack's name.
    pub name: String,
    /// Its changes, by registry: the folder under `data/<namespace>/`, e.g. `worldgen/biome`,
    /// `tags/worldgen/biome`, or `structure` for templates.
    pub registries: BTreeMap<String, RegistryChanges>,
}

impl LayerChanges {
    /// Whether it changes anything that takes effect: every file it has is shadowed, and its
    /// filter hides nothing, or it has none.
    pub fn is_empty(&self) -> bool {
        self.registries.values().all(RegistryChanges::is_empty)
    }
}

/// What a pack does to one registry, each a sorted list of ids (`namespace:path`).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct RegistryChanges {
    /// Files a layer below, the bundle included, also has, still in view: the pack's replace
    /// them. For a tag, those with `"replace": true`.
    pub replaced: Vec<String>,
    /// Files no layer below has.
    pub added: Vec<String>,
    /// Tags a layer below also has, which this pack's file adds to.
    pub merged: Vec<String>,
    /// Files of the layers below that the pack's `filter` hides.
    pub hidden: Vec<String>,
    /// The pack's files that never take effect: a higher pack has the same file, or hides it.
    /// A tag only when a higher pack replaces or hides it.
    pub shadowed: Vec<String>,
    /// Tag files the game can't read as tags, so skips.
    pub unreadable: Vec<String>,
}

impl RegistryChanges {
    /// Whether nothing in it takes effect.
    pub fn is_empty(&self) -> bool {
        self.replaced.is_empty()
            && self.added.is_empty()
            && self.merged.is_empty()
            && self.hidden.is_empty()
    }
}

/// A file's registry and id, from its path in a pack (`data/<namespace>/<registry>/<path>.<ext>`).
/// Registries are named as the game names them: `worldgen/<name>` and `tags/<registry>` under
/// their folders, everything else one folder deep.
pub fn registry_and_id(path: &str) -> Option<(String, String)> {
    let rest = path.strip_prefix("data/")?;
    let (namespace, rest) = rest.split_once('/')?;
    let parts: Vec<&str> = rest.split('/').collect();
    let depth = match parts.as_slice() {
        ["worldgen", ..] => 2,
        ["tags", "worldgen", ..] => 3,
        ["tags", ..] => 2,
        _ => 1,
    };
    if parts.len() <= depth {
        return None;
    }
    let registry = parts[..depth].join("/");
    let file = parts[depth..].join("/");
    // The game lists a registry's files by its one extension (`FileToIdConverter`), so a file
    // with another, such as a pack's `x.json.disabled`, is no resource.
    let extension = match registry.as_str() {
        "structure" => ".nbt",
        "function" => ".mcfunction",
        _ => ".json",
    };
    let name = file.strip_suffix(extension)?;
    Some((registry, format!("{namespace}:{name}")))
}

impl Bundle {
    /// What each applied pack changes, lowest first (`docs/specs/data-packs.md`, "What a pack
    /// changes"). Empty without packs.
    pub fn layer_changes(&self) -> Vec<LayerChanges> {
        (0..self.layers.len())
            .map(|level| self.changes_at(level))
            .collect()
    }

    /// Whether a layer below `level`, the bundle included, has `path` in view at `level`:
    /// looking down from the layer under it, the first that has it, unless one hides it first.
    fn in_view_below(&self, path: &str, level: usize) -> bool {
        for layer in self.layers[..level].iter().rev() {
            if layer.files.contains_key(path) {
                return true;
            }
            if layer.hides(path) {
                return false;
            }
        }
        self.manifest.files.contains_key(path)
    }

    fn changes_at(&self, level: usize) -> LayerChanges {
        let layer = &self.layers[level];
        let above = &self.layers[level + 1..];
        // Each file's change, sorted into registries below.
        let mut sorted: Vec<(String, String, Change)> = Vec::new();
        for (path, bytes) in &layer.files {
            let Some((registry, id)) = registry_and_id(path) else {
                continue;
            };
            let tag = is_tag(path);
            let replaces_tag = tag.then(|| {
                serde_json::from_slice::<serde_json::Value>(bytes)
                    .ok()
                    .filter(reads_as_tag_file)
                    .map(|json| json.get("replace").and_then(java_bool) == Some(true))
            });
            if let Some(None) = replaces_tag {
                sorted.push((registry, id, Change::Unreadable));
                continue;
            }
            let shadowed = above.iter().any(|higher| {
                higher.hides(path)
                    || (higher.files.contains_key(path) && (!tag || replaces(higher, path)))
            });
            let below = self.in_view_below(path, level);
            let change = match (tag, below, replaces_tag.flatten()) {
                _ if shadowed => Change::Shadowed,
                (_, false, _) => Change::Added,
                (true, true, Some(false)) => Change::Merged,
                _ => Change::Replaced,
            };
            sorted.push((registry, id, change));
        }
        if !layer.filter.is_empty() {
            let lower: BTreeSet<&str> = self
                .manifest
                .files
                .keys()
                .map(String::as_str)
                .chain(
                    self.layers[..level]
                        .iter()
                        .flat_map(|l| l.files.keys().map(String::as_str)),
                )
                .collect();
            for path in lower {
                if layer.hides(path)
                    && self.in_view_below(path, level)
                    && let Some((registry, id)) = registry_and_id(path)
                {
                    sorted.push((registry, id, Change::Hidden));
                }
            }
        }
        let mut registries: BTreeMap<String, RegistryChanges> = BTreeMap::new();
        for (registry, id, change) in sorted {
            let changes = registries.entry(registry).or_default();
            let list = match change {
                Change::Replaced => &mut changes.replaced,
                Change::Added => &mut changes.added,
                Change::Merged => &mut changes.merged,
                Change::Hidden => &mut changes.hidden,
                Change::Shadowed => &mut changes.shadowed,
                Change::Unreadable => &mut changes.unreadable,
            };
            list.push(id);
        }
        for changes in registries.values_mut() {
            for list in [
                &mut changes.replaced,
                &mut changes.added,
                &mut changes.merged,
                &mut changes.hidden,
                &mut changes.shadowed,
                &mut changes.unreadable,
            ] {
                list.sort();
                list.dedup();
            }
        }
        LayerChanges {
            name: layer.name.clone(),
            registries,
        }
    }
}

/// What a layer does to one file.
enum Change {
    Replaced,
    Added,
    Merged,
    Hidden,
    Shadowed,
    Unreadable,
}

/// Whether `layer`'s tag file at `path` replaces what's below it: it reads as a tag and says
/// `"replace": true`.
fn replaces(layer: &Layer, path: &str) -> bool {
    layer
        .files
        .get(path)
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(bytes).ok())
        .filter(reads_as_tag_file)
        .is_some_and(|json| json.get("replace").and_then(java_bool) == Some(true))
}
