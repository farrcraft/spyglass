//! The report of what spyglass read from a save and how its answers use it: every setting with
//! its source, every enabled pack and what it changes, what each dimension holds, and every
//! warning beside what it's about. See `docs/specs/world-saves.md`, "The report", and
//! `docs/specs/data-packs.md`, "What a pack changes".

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;

use spyglass_data::bundle::{Bundle, LayerChanges, registry_and_id};
use spyglass_world::chunk::CONFIGURED_STRUCTURES;
use spyglass_world::region::{ChunkIndex, ChunkRecord};
use spyglass_world::{BorderRead, SaveSource};

use super::{SaveReport, SaveWarning, Setting, describe};
use crate::{Dimension, PRESET};

/// Everything spyglass took from a save, where it came from and what it decides, from
/// [`SaveReport::details`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct SaveDetails {
    /// Every value spyglass reads, in a fixed order.
    pub settings: Vec<SaveSetting>,
    /// Every enabled pack, in load order, `vanilla` included.
    pub packs: Vec<SavePack>,
    /// The Default preset's dimensions, in [`Dimension::ALL`] order.
    pub dimensions: Vec<DimensionDetails>,
    /// Every warning: [`SaveReport::warnings`], and then any [`SaveWarning::PackError`]. Each is
    /// shown beside its [`subject`](SaveWarning::subject).
    pub warnings: Vec<SaveWarning>,
}

impl SaveDetails {
    /// The warnings about `subject`, in order.
    pub fn warnings_about(&self, subject: &super::SaveSubject) -> Vec<&SaveWarning> {
        self.warnings
            .iter()
            .filter(|w| w.subject() == *subject)
            .collect()
    }

    /// Whether spyglass answers in `dimension`: no warning blocks it.
    pub fn answerable_in(&self, dimension: Dimension) -> bool {
        !self.warnings.iter().any(|w| w.blocks(dimension))
    }
}

/// One value spyglass took from a save.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SaveSetting {
    /// Which value.
    pub setting: Setting,
    /// The value, for people: e.g. `26.3`, `minecraft:noise (minecraft:overworld)`.
    pub value: String,
    /// Where it came from.
    pub source: SaveSource,
}

impl SaveSetting {
    /// What it decides (`docs/specs/world-saves.md`, "Settings").
    pub fn decides(&self) -> &'static str {
        self.setting.decides()
    }
}

/// What spyglass did with an enabled pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PackUse {
    /// `vanilla`: the bundle itself.
    Bundle,
    /// A `file/` pack, applied from the save's `datapacks/`.
    Applied,
    /// A built-in feature pack, applied from the bundle.
    Feature,
    /// Not applied; a [`SaveWarning::DataPack`] says why, unless no bundle matches the save.
    NotApplied,
}

impl PackUse {
    /// A short machine-readable name, e.g. `applied`.
    pub fn kind(self) -> &'static str {
        match self {
            PackUse::Bundle => "bundle",
            PackUse::Applied => "applied",
            PackUse::Feature => "feature",
            PackUse::NotApplied => "not_applied",
        }
    }
}

/// An enabled pack.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SavePack {
    /// Its name, e.g. `file/terralith.zip`.
    pub name: String,
    /// What spyglass did with it.
    pub used: PackUse,
    /// For an applied pack, what it changes, by registry.
    pub changes: Option<LayerChanges>,
    /// The answers those changes touch, in [`Answer`] order. Empty for a pack that changes
    /// nothing spyglass reads.
    pub answers: Vec<Answer>,
}

/// A dimension of the Default preset.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DimensionDetails {
    /// Which.
    pub dimension: Dimension,
    /// Its region files' folder.
    pub region: PathBuf,
    /// Whether spyglass answers in it; the warnings that block it say why not.
    pub answerable: bool,
    /// What its region files hold, after [`Scan::Chunks`](crate::Scan::Chunks).
    pub holdings: Option<SaveHoldings>,
}

/// What one dimension's region files hold, as the chunk scan counted it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct SaveHoldings {
    /// Chunks the region files list, readable or not.
    pub chunks: usize,
    /// Chunks whose generation finished (status `full`).
    pub full: usize,
    /// Chunks generated partway.
    pub proto: usize,
    /// Chunks that couldn't be read.
    pub unreadable: usize,
    /// Region files that couldn't be read, whose chunks aren't counted.
    pub unreadable_files: usize,
    /// Readable chunks by the data version that saved them, newest first; `None` before 1.9.
    pub versions: Vec<VersionCount>,
    /// Ticks players spent in the dimension (`InhabitedTime`, summed over chunks).
    pub inhabited_ticks: i64,
    /// The listed chunks' bounds, min then max, in chunk coordinates; `None` without any.
    pub bounds: Option<[[i32; 2]; 2]>,
    /// Structure starts answered from the save instead of the seed, by structure, most first.
    pub starts: Vec<(String, usize)>,
    /// Starts naming a structure the bundle (with the save's packs) doesn't have, which the
    /// game drops, by key. Empty without a bundle.
    pub dropped_starts: Vec<(String, usize)>,
    /// Starts in chunks saved before 1.18.2, by their old keys: the game data-fixes them, and
    /// spyglass predicts them.
    pub older_starts: Vec<(String, usize)>,
    /// Block entities with a loot table not yet filled, by block entity id, most first.
    pub sealed_containers: Vec<(String, usize)>,
}

/// How many chunks one data version saved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionCount {
    /// The data version; `None` for chunks saved before 1.9.
    pub data_version: Option<i32>,
    /// The game version of a bundle with that data version, if there's one.
    pub version: Option<String>,
    /// How many chunks.
    pub chunks: usize,
}

/// An answer spyglass gives that a pack's files can change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Answer {
    /// Biomes, and biome locates.
    Biomes,
    /// Terrain: heights, blocks, the spawn.
    Terrain,
    /// Structures.
    Structures,
    /// Features.
    Features,
    /// Container loot.
    Loot,
    /// Flowers.
    Flowers,
    /// Nether portals.
    Portals,
}

impl Answer {
    /// Every answer, in order.
    pub const ALL: [Answer; 7] = [
        Answer::Biomes,
        Answer::Terrain,
        Answer::Structures,
        Answer::Features,
        Answer::Loot,
        Answer::Flowers,
        Answer::Portals,
    ];

    /// Its name, e.g. `biomes`.
    pub fn name(self) -> &'static str {
        match self {
            Answer::Biomes => "biomes",
            Answer::Terrain => "terrain",
            Answer::Structures => "structures",
            Answer::Features => "features",
            Answer::Loot => "loot",
            Answer::Flowers => "flowers",
            Answer::Portals => "portals",
        }
    }
}

/// The answers a registry's files feed (`docs/specs/data-packs.md`, "What a pack changes"):
/// empty for a registry spyglass doesn't read, such as `recipe`.
pub fn answers_fed_by(registry: &str) -> &'static [Answer] {
    use Answer::{Biomes, Features, Flowers, Loot, Portals, Structures, Terrain};
    match registry {
        "worldgen/biome" => &[Biomes, Terrain, Features, Flowers],
        "worldgen/multi_noise_biome_source_parameter_list" => &[Biomes],
        "worldgen/density_function" | "worldgen/noise" | "worldgen/world_preset" => {
            &[Biomes, Terrain]
        }
        "worldgen/noise_settings" | "dimension" | "dimension_type" => &[Biomes, Terrain, Portals],
        "worldgen/material_rule"
        | "worldgen/material_condition"
        | "worldgen/configured_carver"
        | "worldgen/carver"
        | "tags/worldgen/carver"
        | "tags/worldgen/configured_carver" => &[Terrain],
        "tags/block" => &[Terrain, Features, Portals],
        "worldgen/structure"
        | "worldgen/structure_set"
        | "worldgen/template_pool"
        | "worldgen/processor_list"
        | "structure"
        | "tags/worldgen/biome"
        | "tags/worldgen/structure" => &[Structures],
        "worldgen/configured_feature"
        | "worldgen/placed_feature"
        | "worldgen/feature"
        | "worldgen/block_state_provider"
        | "tags/worldgen/feature"
        | "tags/worldgen/configured_feature" => &[Features, Flowers],
        "tags/worldgen/placed_feature" => &[Features],
        "loot_table" | "tags/item" | "tags/instrument" | "enchantment" | "tags/enchantment" => {
            &[Loot]
        }
        _ => &[],
    }
}

impl SaveReport {
    /// The full report: every setting with its source, every enabled pack and what it changes,
    /// each dimension and what it holds, and every warning (`docs/specs/world-saves.md`, "The
    /// report"). With packs applied, it opens every answerable dimension with them, as
    /// [`pack_errors`](Self::pack_errors) does, which takes as long as a first query in each.
    pub fn details(&self) -> SaveDetails {
        let layered = self
            .bundle
            .as_ref()
            .and_then(|b| self.bundle_with_packs(b).ok());
        let mut warnings = self.warnings.clone();
        warnings.extend(self.pack_warnings(layered.as_ref()));
        let dimensions = Dimension::ALL
            .into_iter()
            .map(|dimension| {
                let folders = &self.dimensions[dimension.index()];
                DimensionDetails {
                    dimension,
                    region: folders.region.clone(),
                    answerable: !warnings.iter().any(|w| w.blocks(dimension)),
                    holdings: folders
                        .index
                        .as_ref()
                        .map(|index| self.holdings(index, layered.as_ref())),
                }
            })
            .collect();
        SaveDetails {
            settings: self.settings(),
            packs: self.pack_details(layered.as_ref()),
            dimensions,
            warnings,
        }
    }

    /// A [`SaveWarning::PackError`] for each answerable dimension that can't be opened with the
    /// packs.
    fn pack_warnings(&self, layered: Option<&Bundle>) -> Vec<SaveWarning> {
        self.pack_errors()
            .into_iter()
            .map(|(dimension, error)| {
                let message = error.to_string();
                let (pack, file) = layered
                    .map(|bundle| pack_named_by(bundle, &message))
                    .unwrap_or_default();
                SaveWarning::PackError {
                    dimension,
                    pack,
                    file,
                    message,
                }
            })
            .collect()
    }

    fn settings(&self) -> Vec<SaveSetting> {
        let save = &self.read;
        let sources = &save.sources;
        let setting = |setting, value: String, source: &SaveSource| SaveSetting {
            setting,
            value,
            source: source.clone(),
        };
        let mut settings = vec![
            setting(
                Setting::Version,
                save.version_name
                    .clone()
                    .unwrap_or_else(|| "not saved".into()),
                &sources.version_name,
            ),
            setting(
                Setting::DataVersion,
                match (save.data_version, &self.bundle) {
                    (None, _) => "not saved (before 1.9)".into(),
                    (Some(v), Some(bundle)) => format!("{v}, the {} bundle's", bundle.version),
                    (Some(v), None) => format!("{v}, which no bundle has"),
                },
                &sources.data_version,
            ),
            setting(Setting::Seed, save.seed.to_string(), &sources.seed),
            setting(
                Setting::Structures,
                if save.generate_structures {
                    "generate"
                } else {
                    "don't generate"
                }
                .into(),
                &sources.generate_structures,
            ),
        ];
        for dimension in Dimension::ALL {
            let value = match save.generators.get(dimension.id()) {
                None => "none saved".to_string(),
                Some(generator) => {
                    let described = describe(generator);
                    match &self.presets[dimension.index()] {
                        Some(p) if p == PRESET => format!("{described}: the Default preset's"),
                        Some(p) => format!("{described}: the {p} preset's"),
                        None if self.bundle.is_some() => format!("{described}: no preset's"),
                        None => described,
                    }
                }
            };
            settings.push(setting(
                Setting::Generator(dimension),
                value,
                &sources.generator(dimension.id()),
            ));
        }
        for dimension in Dimension::ALL {
            let (value, source) = match &self.borders[dimension.index()] {
                Some((read, source)) => (border_text(read), source.clone()),
                None => ("couldn't be read".to_string(), SaveSource::default()),
            };
            settings.push(setting(Setting::Border(dimension), value, &source));
        }
        let location = |l: &Option<spyglass_world::Location>, none: &str| match l {
            Some(l) => {
                let [x, y, z] = l.pos;
                format!("{x}, {y}, {z} in {}", l.dimension)
            }
            None => none.to_string(),
        };
        settings.push(setting(
            Setting::Spawn,
            location(&save.spawn, "not saved"),
            &sources.spawn,
        ));
        settings.push(setting(
            Setting::Player,
            location(&save.player, "none (a server's save, or never played)"),
            &sources.player,
        ));
        settings.push(setting(
            Setting::Modded,
            if save.was_modded { "yes" } else { "no" }.into(),
            &sources.was_modded,
        ));
        settings
    }

    fn pack_details(&self, layered: Option<&Bundle>) -> Vec<SavePack> {
        let changes = layered.map(Bundle::layer_changes).unwrap_or_default();
        let mut seen = BTreeSet::new();
        self.read
            .enabled_packs
            .iter()
            .filter(|name| seen.insert(name.as_str()))
            .map(|name| {
                let applied = self.packs.iter().position(|p| p.name() == name);
                let used = match applied {
                    _ if name == "vanilla" => PackUse::Bundle,
                    Some(_) if name.starts_with("file/") => PackUse::Applied,
                    Some(_) => PackUse::Feature,
                    None => PackUse::NotApplied,
                };
                let changes = applied.and_then(|i| changes.get(i).cloned());
                let answers = changes.as_ref().map(answers_changed).unwrap_or_default();
                SavePack {
                    name: name.clone(),
                    used,
                    changes,
                    answers,
                }
            })
            .collect()
    }

    fn holdings(&self, index: &ChunkIndex, layered: Option<&Bundle>) -> SaveHoldings {
        let mut holdings = SaveHoldings {
            unreadable_files: index.unreadable_files,
            ..SaveHoldings::default()
        };
        let mut versions: BTreeMap<Option<i32>, usize> = BTreeMap::new();
        let mut starts = BTreeMap::new();
        let mut dropped = BTreeMap::new();
        let mut older = BTreeMap::new();
        let mut sealed = BTreeMap::new();
        // Whether the bundle with packs has a structure, by id, looked up once each.
        let mut known: HashMap<String, bool> = HashMap::new();
        let mut has_structure = |id: &str| match layered {
            None => true,
            Some(bundle) => *known
                .entry(id.to_string())
                .or_insert_with(|| bundle.has_resource("worldgen/structure", id)),
        };
        let mut bounds: Option<[[i32; 2]; 2]> = None;
        for (pos, record) in index.chunks() {
            holdings.chunks += 1;
            let [min, max] = bounds.get_or_insert([pos, pos]);
            *min = [min[0].min(pos[0]), min[1].min(pos[1])];
            *max = [max[0].max(pos[0]), max[1].max(pos[1])];
            let fields = match record {
                ChunkRecord::Read(fields) => fields,
                ChunkRecord::Unreadable => {
                    holdings.unreadable += 1;
                    continue;
                }
            };
            if fields.is_full() {
                holdings.full += 1;
            } else {
                holdings.proto += 1;
            }
            *versions.entry(fields.data_version).or_default() += 1;
            holdings.inhabited_ticks = holdings
                .inhabited_ticks
                .saturating_add(fields.inhabited_time.unwrap_or(0).max(0));
            let current = fields
                .data_version
                .is_some_and(|v| v >= CONFIGURED_STRUCTURES);
            for start in fields.starts.iter().flatten() {
                let counts = if !current {
                    &mut older
                } else if has_structure(&start.structure) && has_structure(&start.id) {
                    &mut starts
                } else {
                    &mut dropped
                };
                *counts.entry(start.structure.clone()).or_insert(0) += 1;
            }
            for id in &fields.sealed_containers {
                *sealed.entry(id.clone()).or_insert(0) += 1;
            }
        }
        // Chunks of listed region files whose names don't parse have no position.
        for region in index.regions.iter().filter(|r| r.region.is_none()) {
            holdings.chunks += region.chunks.len();
        }
        holdings.bounds = bounds;
        holdings.versions = versions
            .into_iter()
            .rev()
            .map(|(data_version, chunks)| VersionCount {
                data_version,
                version: data_version.and_then(|v| {
                    self.known_versions
                        .iter()
                        .find(|(known, _)| *known == v)
                        .map(|(_, name)| name.clone())
                }),
                chunks,
            })
            .collect();
        holdings.starts = most_first(starts);
        holdings.dropped_starts = most_first(dropped);
        holdings.older_starts = most_first(older);
        holdings.sealed_containers = most_first(sealed);
        holdings
    }
}

/// Counts by name, most first, then by name.
fn most_first(counts: BTreeMap<String, usize>) -> Vec<(String, usize)> {
    let mut sorted: Vec<(String, usize)> = counts.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    sorted
}

/// The answers a pack's changes touch: those fed by each registry with a change in effect.
fn answers_changed(changes: &LayerChanges) -> Vec<Answer> {
    let touched: BTreeSet<Answer> = changes
        .registries
        .iter()
        .filter(|(_, c)| !c.is_empty())
        .flat_map(|(registry, _)| answers_fed_by(registry).iter().copied())
        .collect();
    touched.into_iter().collect()
}

/// A border, for people.
fn border_text(read: &BorderRead) -> String {
    let border = read.settings();
    let [x, z] = border.center;
    let mut text = format!("centre {x}, {z}, side {}", border.size);
    if border.is_moving() {
        text.push_str(&format!(
            ", moving to {} over {} s",
            border.lerp_target,
            border.lerp_time / 1000
        ));
    }
    match read {
        BorderRead::Saved(_) => text,
        BorderRead::Missing => format!("the default: {text}"),
        BorderRead::Unreadable(why) => format!("unreadable ({why}), so the default: {text}"),
    }
}

/// The pack an error message is about, and the file, from the highest layer of `bundle`: a
/// `data/...` path in it that a pack provides, or else an id (`namespace:path`) in a namespace
/// only packs have.
fn pack_named_by(bundle: &Bundle, message: &str) -> (Option<String>, Option<String>) {
    let layers = bundle.layers();
    let is_path_char = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || "_-./".contains(c);
    for (at, _) in message.match_indices("data/") {
        let path: String = message[at..]
            .chars()
            .take_while(|&c| is_path_char(c))
            .collect();
        let path = path.trim_end_matches('.');
        if let Some(layer) = layers.iter().rev().find(|l| l.files.contains_key(path)) {
            return (Some(layer.name.clone()), Some(path.to_string()));
        }
    }
    let is_id_char = |c: char| is_path_char(c) || c == ':';
    let ids: Vec<&str> = message
        .split(|c: char| !is_id_char(c))
        .map(|word| word.trim_end_matches(['.', ':']))
        .filter(|word| word.contains(':'))
        .collect();
    // An id a pack has a file for, in any registry: the first such id named.
    for id in &ids {
        for layer in layers.iter().rev() {
            let files: Vec<&String> = layer
                .files
                .keys()
                .filter(|path| registry_and_id(path).is_some_and(|(_, found)| found == *id))
                .collect();
            if let Some(&file) = files.first() {
                // Several registries may have the id; then the file is unknown.
                let file = (files.len() == 1).then(|| file.clone());
                return (Some(layer.name.clone()), file);
            }
        }
    }
    // Or else an id in a namespace only packs have.
    for word in ids {
        let Some((namespace, _)) = word.split_once(':') else {
            continue;
        };
        if namespace == "minecraft" || namespace.is_empty() {
            continue;
        }
        let prefix = format!("data/{namespace}/");
        if let Some(layer) = layers
            .iter()
            .rev()
            .find(|l| l.files.keys().any(|p| p.starts_with(&prefix)))
        {
            return (Some(layer.name.clone()), None);
        }
    }
    (None, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every string literal in the library crates' sources that names a registry the way
    /// loaders do: `"worldgen/..."`, `"tags/..."`, or a one-folder registry from a `const`.
    fn registries_read() -> BTreeSet<String> {
        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut found = BTreeSet::new();
        let not_libraries = [
            "spyglass-cli",
            "spyglass-viewer",
            "spyglass-extract",
            "spyglass-oracle",
        ];
        let mut stack: Vec<PathBuf> = std::fs::read_dir(&crates)
            .unwrap()
            .flatten()
            .filter(|e| !not_libraries.contains(&e.file_name().to_string_lossy().as_ref()))
            .map(|e| e.path().join("src"))
            .collect();
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().is_none_or(|e| e != "rs") {
                    continue;
                }
                let text = std::fs::read_to_string(&path).unwrap();
                // Only the code before a file's tests.
                let code = text.split("#[cfg(test)]").next().unwrap_or_default();
                for (i, literal) in code.split('"').enumerate() {
                    let registry_like = literal
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c == '_' || c == '/');
                    if i % 2 == 1
                        && registry_like
                        && (literal.starts_with("worldgen/") || literal.starts_with("tags/"))
                        && !literal.ends_with('/')
                    {
                        found.insert(literal.to_string());
                    }
                }
                for line in code.lines() {
                    let line = line.trim();
                    if let Some(rest) = line
                        .strip_prefix("const ")
                        .or_else(|| line.strip_prefix("pub const "))
                        && let Some((_, value)) = rest.split_once(": &str = \"")
                        && let Some(value) = value.strip_suffix("\";")
                        && ["loot_table", "dimension", "dimension_type", "enchantment"]
                            .contains(&value)
                    {
                        found.insert(value.to_string());
                    }
                }
            }
        }
        found
    }

    #[test]
    fn every_registry_a_loader_reads_feeds_an_answer() {
        let read = registries_read();
        assert!(read.contains("worldgen/biome"), "{read:?}");
        assert!(read.contains("loot_table"), "{read:?}");
        let missing: Vec<&String> = read
            .iter()
            .filter(|r| answers_fed_by(r).is_empty())
            .collect();
        assert!(
            missing.is_empty(),
            "registries without answers: {missing:?}"
        );
        // What a bundle reads by fixed path, not through a registry constant.
        for registry in ["dimension", "dimension_type", "structure"] {
            assert!(!answers_fed_by(registry).is_empty(), "{registry}");
        }
        assert!(answers_fed_by("recipe").is_empty());
        assert!(answers_fed_by("advancement").is_empty());
    }
}
