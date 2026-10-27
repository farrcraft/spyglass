//! Loading structure sets and structures from a bundle, mirroring the game's codecs (ADR 0010).

use std::collections::HashMap;

use serde_json::{Map, Value};
use spyglass_biome::{BiomeNames, BiomeSource};
use spyglass_data::bundle::Bundle;
use spyglass_data::codec::{
    self, At, LoadErrorKind, LoadResult, field, full_id, id, int_field, int_in, object,
};

use spyglass_noise::NormalNoiseParameters;

use crate::biomes::{BiomeSet, Tags};
use crate::corner::CornerSamplers;
use crate::jigsaw::{GenerationHeights, HeightProvider, Jigsaw, JigsawLoader, height_provider};
use crate::placement::{
    ConcentricRings, ExclusionZone, FrequencyReduction, Placement, PlacementKind, RandomSpread,
    SpreadType,
};
use crate::portal::RuinedPortal;

const STRUCTURE_SET: &str = "worldgen/structure_set";
const STRUCTURE: &str = "worldgen/structure";

/// The type a structure's `type` names (`StructureType`). Each has its own placement logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StructureType {
    /// `minecraft:buried_treasure`
    BuriedTreasure,
    /// `minecraft:desert_pyramid`
    DesertPyramid,
    /// `minecraft:end_city`
    EndCity,
    /// `minecraft:fortress`
    Fortress,
    /// `minecraft:igloo`
    Igloo,
    /// `minecraft:jigsaw`
    Jigsaw,
    /// `minecraft:jungle_temple`
    JungleTemple,
    /// `minecraft:mineshaft`
    Mineshaft,
    /// `minecraft:nether_fossil`
    NetherFossil,
    /// `minecraft:ocean_monument`
    OceanMonument,
    /// `minecraft:ocean_ruin`
    OceanRuin,
    /// `minecraft:ruined_portal`
    RuinedPortal,
    /// `minecraft:shipwreck`
    Shipwreck,
    /// `minecraft:stronghold`
    Stronghold,
    /// `minecraft:swamp_hut`
    SwampHut,
    /// `minecraft:woodland_mansion`
    WoodlandMansion,
}

impl StructureType {
    fn from_id(id: &str) -> Option<Self> {
        use StructureType::*;
        Some(match full_id(id).as_str() {
            "minecraft:buried_treasure" => BuriedTreasure,
            "minecraft:desert_pyramid" => DesertPyramid,
            "minecraft:end_city" => EndCity,
            "minecraft:fortress" => Fortress,
            "minecraft:igloo" => Igloo,
            "minecraft:jigsaw" => Jigsaw,
            "minecraft:jungle_temple" => JungleTemple,
            "minecraft:mineshaft" => Mineshaft,
            "minecraft:nether_fossil" => NetherFossil,
            "minecraft:ocean_monument" => OceanMonument,
            "minecraft:ocean_ruin" => OceanRuin,
            "minecraft:ruined_portal" => RuinedPortal,
            "minecraft:shipwreck" => Shipwreck,
            "minecraft:stronghold" => Stronghold,
            "minecraft:swamp_hut" => SwampHut,
            "minecraft:woodland_mansion" => WoodlandMansion,
            _ => return None,
        })
    }
}

/// A structure (`Structure`): its type and the biomes it may start in.
#[derive(Debug, Clone)]
pub struct Structure {
    /// Its id, e.g. `minecraft:village_plains`.
    pub id: String,
    /// Its type.
    pub kind: StructureType,
    /// `biomes`: where its start may be.
    pub biomes: BiomeSet,
    /// For [`StructureType::Jigsaw`], its start settings.
    pub jigsaw: Option<Jigsaw>,
    /// For [`StructureType::Shipwreck`], `is_beached`: checked on `WORLD_SURFACE_WG` rather
    /// than `OCEAN_FLOOR_WG`.
    pub beached: bool,
    /// For [`StructureType::RuinedPortal`], its setups and templates.
    pub ruined_portal: Option<RuinedPortal>,
    /// For [`StructureType::NetherFossil`], `height`: where its scan starts.
    pub height: Option<HeightProvider>,
}

/// One weighted entry of a structure set (`StructureSet.StructureSelectionEntry`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    /// Index into [`StructureSets::structures`].
    pub structure: usize,
    /// `weight`, positive.
    pub weight: i32,
}

/// A structure set (`StructureSet`): where structures may go, and which.
#[derive(Debug, Clone)]
pub struct StructureSet {
    /// Its id, e.g. `minecraft:villages`.
    pub id: String,
    /// `placement`.
    pub placement: Placement,
    /// `structures`, in file order, which is the order the weighted pick walks.
    pub entries: Vec<Entry>,
    /// Whether the dimension can generate it: some structure's biomes intersect the biome
    /// source's (`ChunkGeneratorStructureState.hasBiomesForStructureSet`).
    pub possible: bool,
}

/// Every structure set in a bundle, and the structures they name, for one dimension's biome
/// source. Independent of the seed.
#[derive(Debug, Clone)]
pub struct StructureSets {
    sets: Vec<StructureSet>,
    structures: Vec<Structure>,
    heights: GenerationHeights,
    sea_level: i32,
    /// The noise settings' `legacy_random_source`.
    legacy_random_source: bool,
    /// The noises the start pools' corner constraints use.
    corner_noises: HashMap<String, NormalNoiseParameters>,
    monument_surroundings: BiomeSet,
}

impl StructureSets {
    /// The overworld of the default world preset (`minecraft:normal`), for its biome source.
    pub fn overworld(bundle: &Bundle, source: &BiomeSource) -> LoadResult<Self> {
        Self::load(bundle, "minecraft:normal", "minecraft:overworld", source)
    }

    /// Loads every structure set in the bundle, sorted by id, for `dimension` of world preset
    /// `preset` and its biome source. Sets the dimension can't generate are kept (an exclusion
    /// zone may name one) with `possible` false.
    pub fn load(
        bundle: &Bundle,
        preset: &str,
        dimension: &str,
        source: &BiomeSource,
    ) -> LoadResult<Self> {
        Self::load_with(bundle, preset, dimension, source.biome_names())
    }

    /// [`load`](Self::load), given only the biome source's biomes.
    pub fn load_with(
        bundle: &Bundle,
        preset: &str,
        dimension: &str,
        biomes: &BiomeNames,
    ) -> LoadResult<Self> {
        let (heights, sea_level, legacy_random_source) =
            dimension_heights(bundle, preset, dimension)?;
        let ids = bundle.registry_ids(STRUCTURE_SET);
        let index: HashMap<String, usize> = ids
            .iter()
            .enumerate()
            .map(|(i, id)| (id.clone(), i))
            .collect();
        let mut loader = SetLoader {
            bundle,
            tags: Tags::new(bundle, biomes),
            structures: Vec::new(),
            structure_index: HashMap::new(),
            set_index: &index,
            jigsaw: JigsawLoader::new(bundle),
        };
        let mut sets = Vec::with_capacity(ids.len());
        for id in &ids {
            let json = bundle
                .resource(STRUCTURE_SET, id)
                .map_err(|e| At::root(id).error(e.into()))?;
            sets.push(loader.set(id, &json)?);
        }
        // BiomeTags.REQUIRED_OCEAN_MONUMENT_SURROUNDING. A missing tag matches nothing, as in
        // the game.
        let monument_surroundings = loader
            .tags
            .optional_tag("minecraft:required_ocean_monument_surrounding")?;
        let sets = StructureSets {
            sets,
            corner_noises: loader.jigsaw.noises().clone(),
            structures: loader.structures,
            heights,
            sea_level,
            legacy_random_source,
            monument_surroundings,
        };
        // Bind the start pools' corner constraints once, so that checks can't fail to: binding
        // depends on the dimension, not the seed.
        let mut samplers = sets.corner_samplers(0);
        for s in &sets.structures {
            let Some(jigsaw) = &s.jigsaw else { continue };
            for element in &jigsaw.start_pool.templates {
                for f in element
                    .corner_constraint()
                    .into_iter()
                    .flat_map(|c| c.functions())
                {
                    samplers.bind(f)?;
                }
            }
        }
        drop(samplers);
        Ok(sets)
    }

    /// Every set, sorted by id.
    pub fn sets(&self) -> &[StructureSet] {
        &self.sets
    }

    /// Every structure a set names, in the order first named.
    pub fn structures(&self) -> &[Structure] {
        &self.structures
    }

    /// A set's index by id.
    pub fn set_index(&self, id: &str) -> Option<usize> {
        let id = full_id(id);
        self.sets.iter().position(|s| s.id == id)
    }

    /// The dimension's heights, as structures see them.
    pub fn heights(&self) -> &GenerationHeights {
        &self.heights
    }

    /// The noise settings' `sea_level`.
    pub fn sea_level(&self) -> i32 {
        self.sea_level
    }

    /// The noise settings' `legacy_random_source`: how the dimension's `RandomState` makes
    /// noises.
    pub fn legacy_random_source(&self) -> bool {
        self.legacy_random_source
    }

    /// The start pools' corner constraints' functions, bound for `seed` as the dimension's
    /// `RandomState` binds.
    pub(crate) fn corner_samplers(&self, seed: i64) -> CornerSamplers<'_> {
        CornerSamplers::new(
            &self.corner_noises,
            seed,
            self.legacy_random_source,
            self.heights.anchors(),
        )
    }

    /// `#minecraft:required_ocean_monument_surrounding`, which ocean monuments check.
    pub fn monument_surroundings(&self) -> &BiomeSet {
        &self.monument_surroundings
    }

    /// A structure's index by id.
    pub fn structure_index(&self, id: &str) -> Option<usize> {
        let id = full_id(id);
        self.structures.iter().position(|s| s.id == id)
    }
}

/// A dimension's heights, sea level and random source: the dimension type's `min_y` and
/// `height`, and its noise settings' `noise.min_y`, `noise.height`, `sea_level` and
/// `legacy_random_source` (false if absent), as named by the world preset.
fn dimension_heights(
    bundle: &Bundle,
    preset: &str,
    dimension: &str,
) -> LoadResult<(GenerationHeights, i32, bool)> {
    let read = |registry: &str, id: &str| {
        bundle
            .resource(registry, id)
            .map_err(|e| At::root(id).error(e.into()))
    };
    let preset_json = read("worldgen/world_preset", preset)?;
    let at = At::root(preset);
    let dim_at = at.child(&format!("dimensions.{dimension}"));
    let dim = preset_json
        .get("dimensions")
        .and_then(|d| d.get(dimension))
        .ok_or_else(|| dim_at.error(LoadErrorKind::Missing("dimension")))?;
    let type_id = full_id(id(
        field(object(dim, &dim_at)?, "type", &dim_at)?,
        &dim_at.child("type"),
    )?);
    let settings_id = full_id(
        dim.get("generator")
            .and_then(|g| g.get("settings"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                dim_at
                    .child("generator.settings")
                    .error(LoadErrorKind::Shape("a noise settings id"))
            })?,
    );
    let int = |json: &Value, path: &[&'static str], resource: &str| -> LoadResult<i32> {
        path.iter()
            .try_fold(json, |v, k| v.get(*k))
            .and_then(codec::int_of)
            .ok_or_else(|| {
                At::root(resource)
                    .child(&path.join("."))
                    .error(LoadErrorKind::Missing(path[path.len() - 1]))
            })
    };
    let dimension_type = read("dimension_type", &type_id)?;
    let settings = read("worldgen/noise_settings", &settings_id)?;
    let level_min_y = int(&dimension_type, &["min_y"], &type_id)?;
    let level_height = int(&dimension_type, &["height"], &type_id)?;
    let gen_min_y = int(&settings, &["noise", "min_y"], &settings_id)?;
    let gen_height = int(&settings, &["noise", "height"], &settings_id)?;
    let sea_level = int(&settings, &["sea_level"], &settings_id)?;
    let legacy_random_source = match settings.get("legacy_random_source") {
        None => false,
        Some(v) => v.as_bool().ok_or_else(|| {
            At::root(&settings_id)
                .child("legacy_random_source")
                .error(LoadErrorKind::Shape("true or false"))
        })?,
    };
    Ok((
        GenerationHeights {
            min_y: level_min_y.max(gen_min_y),
            height: level_height.min(gen_height),
            level_min_y,
            level_height,
        },
        sea_level,
        legacy_random_source,
    ))
}

struct SetLoader<'a> {
    bundle: &'a Bundle,
    tags: Tags<'a>,
    structures: Vec<Structure>,
    structure_index: HashMap<String, usize>,
    set_index: &'a HashMap<String, usize>,
    jigsaw: JigsawLoader<'a>,
}

impl SetLoader<'_> {
    /// `StructureSet.DIRECT_CODEC`: `structures` and `placement`.
    fn set(&mut self, id: &str, json: &Value) -> LoadResult<StructureSet> {
        let at = At::root(id);
        let obj = object(json, &at)?;
        let placement = self.placement(field(obj, "placement", &at)?, &at.child("placement"))?;
        let list_at = at.child("structures");
        let list = field(obj, "structures", &at)?
            .as_array()
            .ok_or_else(|| list_at.error(LoadErrorKind::Shape("a list")))?;
        let mut entries = Vec::with_capacity(list.len());
        for (i, entry) in list.iter().enumerate() {
            let at = list_at.child(&format!("[{i}]"));
            let entry_obj = object(entry, &at)?;
            let weight = codec::int_of(field(entry_obj, "weight", &at)?)
                .filter(|&w| w > 0)
                .ok_or_else(|| {
                    at.child("weight")
                        .error(LoadErrorKind::Shape("a positive integer"))
                })?;
            let structure =
                self.structure(field(entry_obj, "structure", &at)?, &at.child("structure"))?;
            entries.push(Entry { structure, weight });
        }
        let possible = entries
            .iter()
            .any(|e| !self.structures[e.structure].biomes.is_empty());
        Ok(StructureSet {
            id: id.to_string(),
            placement,
            entries,
            possible,
        })
    }

    /// `Structure.CODEC`: a reference by id, loaded once. Inline structures aren't supported.
    fn structure(&mut self, json: &Value, at: &At<'_>) -> LoadResult<usize> {
        let id = full_id(id(json, at)?);
        if let Some(&i) = self.structure_index.get(&id) {
            return Ok(i);
        }
        let file = self
            .bundle
            .resource(STRUCTURE, &id)
            .map_err(|e| At::root(&id).error(e.into()))?;
        let root = At::root(&id);
        let obj = object(&file, &root)?;
        let kind_name = id_field(obj, "type", &root)?;
        let kind = StructureType::from_id(kind_name).ok_or_else(|| {
            root.child("type").error(LoadErrorKind::Unsupported {
                what: "structure type",
                name: kind_name.to_string(),
            })
        })?;
        let biomes = self
            .tags
            .holder_set(field(obj, "biomes", &root)?, &root.child("biomes"))?;
        let jigsaw = match kind {
            StructureType::Jigsaw => Some(self.jigsaw.jigsaw(obj, &root)?),
            _ => None,
        };
        let ruined_portal = match kind {
            StructureType::RuinedPortal => Some(RuinedPortal::load(obj, &root, &mut self.jigsaw)?),
            _ => None,
        };
        let beached = kind == StructureType::Shipwreck
            && obj.get("is_beached").and_then(Value::as_bool) == Some(true);
        let height = match kind {
            StructureType::NetherFossil => Some(height_provider(
                field(obj, "height", &root)?,
                &root.child("height"),
            )?),
            _ => None,
        };
        self.structures.push(Structure {
            id: id.clone(),
            kind,
            biomes,
            jigsaw,
            beached,
            ruined_portal,
            height,
        });
        let i = self.structures.len() - 1;
        self.structure_index.insert(id, i);
        Ok(i)
    }

    /// `StructurePlacement.CODEC`, dispatched on `type`.
    fn placement(&mut self, json: &Value, at: &At<'_>) -> LoadResult<Placement> {
        let obj = object(json, at)?;
        let kind_name = id_field(obj, "type", at)?;
        let kind = match full_id(kind_name).as_str() {
            // RandomSpreadStructurePlacement.CODEC: spacing and separation in 0..=4096, spacing
            // strictly larger; spread_type defaults to linear.
            "minecraft:random_spread" => {
                let spacing = int_field(obj, "spacing", (0, 4096), None, at)?;
                let separation = int_field(obj, "separation", (0, 4096), None, at)?;
                if spacing <= separation {
                    return Err(at.error(LoadErrorKind::Invalid(
                        "Spacing has to be larger than separation",
                    )));
                }
                let spread_type = match obj.get("spread_type") {
                    None => SpreadType::Linear,
                    Some(v) => match v.as_str() {
                        Some("linear") => SpreadType::Linear,
                        Some("triangular") => SpreadType::Triangular,
                        _ => {
                            return Err(at
                                .child("spread_type")
                                .error(LoadErrorKind::Shape("\"linear\" or \"triangular\"")));
                        }
                    },
                };
                PlacementKind::RandomSpread(RandomSpread {
                    spacing,
                    separation,
                    spread_type,
                })
            }
            // ConcentricRingsStructurePlacement.CODEC: distance and spread in 0..=1023, count in
            // 1..=4095, preferred_biomes required.
            "minecraft:concentric_rings" => PlacementKind::ConcentricRings(ConcentricRings {
                distance: int_field(obj, "distance", (0, 1023), None, at)?,
                spread: int_field(obj, "spread", (0, 1023), None, at)?,
                count: int_field(obj, "count", (1, 4095), None, at)?,
                preferred_biomes: self.tags.holder_set(
                    field(obj, "preferred_biomes", at)?,
                    &at.child("preferred_biomes"),
                )?,
            }),
            _ => {
                return Err(at.child("type").error(LoadErrorKind::Unsupported {
                    what: "structure placement type",
                    name: kind_name.to_string(),
                }));
            }
        };

        // AbstractSpreadingStructurePlacement.placementCodec.
        let locate_offset = match obj.get("locate_offset") {
            None => [0; 3],
            Some(v) => offset(v, &at.child("locate_offset"))?,
        };
        let frequency_reduction = match obj.get("frequency_reduction_method") {
            None => FrequencyReduction::Default,
            Some(v) => match v.as_str() {
                Some("default") => FrequencyReduction::Default,
                Some("legacy_type_1") => FrequencyReduction::LegacyType1,
                Some("legacy_type_2") => FrequencyReduction::LegacyType2,
                Some("legacy_type_3") => FrequencyReduction::LegacyType3,
                _ => {
                    return Err(at
                        .child("frequency_reduction_method")
                        .error(LoadErrorKind::Shape("a frequency reduction method")));
                }
            },
        };
        // Codec.floatRange(0, 1), default 1.
        let frequency = match obj.get("frequency") {
            None => 1.0,
            Some(v) => {
                let at = at.child("frequency");
                let f =
                    codec::f32_of(v).ok_or_else(|| at.error(LoadErrorKind::Shape("a number")))?;
                if !(0.0..=1.0).contains(&f) {
                    return Err(at.error(LoadErrorKind::OutOfRange {
                        value: v.to_string(),
                        min: "0".into(),
                        max: "1".into(),
                    }));
                }
                f
            }
        };
        let salt = codec::int_of(field(obj, "salt", at)?)
            .ok_or_else(|| at.child("salt").error(LoadErrorKind::Shape("an integer")))?;
        // ExclusionZone.CODEC: other_set (a structure set reference) and chunk_count in 1..=16.
        let exclusion_zone = match obj.get("exclusion_zone") {
            None => None,
            Some(v) => {
                let at = at.child("exclusion_zone");
                let zone = object(v, &at)?;
                let other = full_id(id(field(zone, "other_set", &at)?, &at.child("other_set"))?);
                let other_set = *self.set_index.get(&other).ok_or_else(|| {
                    at.child("other_set").error(LoadErrorKind::Unsupported {
                        what: "structure set",
                        name: other.clone(),
                    })
                })?;
                Some(ExclusionZone {
                    other_set,
                    chunk_count: int_field(zone, "chunk_count", (1, 16), None, &at)?,
                })
            }
        };
        Ok(Placement {
            locate_offset,
            frequency_reduction,
            frequency,
            salt,
            exclusion_zone,
            kind,
        })
    }
}

fn id_field<'v>(
    obj: &'v Map<String, Value>,
    name: &'static str,
    at: &At<'_>,
) -> LoadResult<&'v str> {
    id(field(obj, name, at)?, &at.child(name))
}

/// `Vec3i.offsetCodec(16)`: three ints, each in -16..=16.
fn offset(json: &Value, at: &At<'_>) -> LoadResult<[i32; 3]> {
    let items = json
        .as_array()
        .filter(|a| a.len() == 3)
        .ok_or_else(|| at.error(LoadErrorKind::Shape("a list of three integers")))?;
    let mut out = [0; 3];
    for (i, item) in items.iter().enumerate() {
        out[i] = int_in(item, -16, 16, &at.child(&format!("[{i}]")))?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use spyglass_oracle::fixtures::TestBundle;

    use super::*;
    use crate::placement::{FrequencyReduction, PlacementKind, SpreadType};

    /// A parameter list naming plains, desert and ocean, in that order.
    fn biomes() -> BiomeNames {
        let names = ["minecraft:plains", "minecraft:desert", "minecraft:ocean"];
        BiomeNames::new(names.map(str::to_string).to_vec())
    }

    /// Loads a bundle of the given `(path, json)` files, plus a minimal preset, dimension type,
    /// noise settings and biome registry.
    fn load(files: &[(String, Value)]) -> LoadResult<StructureSets> {
        let mut all: Vec<(String, Value)> = vec![
            (
                "data/minecraft/worldgen/world_preset/normal.json".into(),
                json!({"dimensions": {"minecraft:overworld": {"type": "minecraft:overworld",
                    "generator": {"type": "minecraft:noise", "settings": "minecraft:overworld"}}}}),
            ),
            (
                "data/minecraft/dimension_type/overworld.json".into(),
                json!({"min_y": -64, "height": 384}),
            ),
            (
                "data/minecraft/worldgen/noise_settings/overworld.json".into(),
                json!({"sea_level": 63, "noise": {"min_y": -64, "height": 384}}),
            ),
        ];
        for b in ["plains", "desert", "ocean", "nether_wastes"] {
            all.push((format!("data/minecraft/worldgen/biome/{b}.json"), json!({})));
        }
        all.extend(files.iter().cloned());
        let fixture = TestBundle::new(&all);
        let bundle = fixture.open();
        StructureSets::load_with(
            &bundle,
            "minecraft:normal",
            "minecraft:overworld",
            &biomes(),
        )
    }

    fn set(id: &str, placement: Value, structures: &[&str]) -> (String, Value) {
        let entries: Vec<Value> = structures
            .iter()
            .map(|s| json!({"structure": s, "weight": 1}))
            .collect();
        (
            format!("data/minecraft/worldgen/structure_set/{id}.json"),
            json!({"placement": placement, "structures": entries}),
        )
    }

    fn structure(id: &str, kind: &str, biomes: Value) -> (String, Value) {
        (
            format!("data/minecraft/worldgen/structure/{id}.json"),
            json!({"type": kind, "biomes": biomes}),
        )
    }

    fn tag(id: &str, values: Value) -> (String, Value) {
        (
            format!("data/minecraft/tags/worldgen/biome/{id}.json"),
            json!({ "values": values }),
        )
    }

    fn spread(spacing: i32, separation: i32) -> Value {
        json!({"type": "minecraft:random_spread", "salt": 7, "spacing": spacing, "separation": separation})
    }

    #[test]
    fn loads_random_spread_with_codec_defaults_and_nested_tags() {
        let sets = load(&[
            set("huts", spread(32, 8), &["minecraft:hut"]),
            structure("hut", "minecraft:swamp_hut", json!("#minecraft:has_hut")),
            tag(
                "has_hut",
                json!(["minecraft:plains", "#minecraft:dry",
                    {"id": "minecraft:missing", "required": false}]),
            ),
            tag("dry", json!(["desert"])),
        ])
        .unwrap_or_else(|e| panic!("{e}"));
        let s = &sets.sets()[0];
        assert_eq!(s.id, "minecraft:huts");
        assert!(s.possible);
        assert_eq!(s.placement.locate_offset, [0; 3]);
        assert_eq!(s.placement.frequency, 1.0);
        assert_eq!(s.placement.frequency_reduction, FrequencyReduction::Default);
        assert!(s.placement.exclusion_zone.is_none());
        let PlacementKind::RandomSpread(spread) = s.placement.kind else {
            panic!("not random spread")
        };
        assert_eq!(spread.spread_type, SpreadType::Linear);
        let hut = &sets.structures()[s.entries[0].structure];
        let ids: Vec<u16> = hut.biomes.iter().map(|b| b.0).collect();
        assert_eq!(ids, [0, 1], "plains, and desert through the nested tag");
        assert_eq!(sets.heights().min_y, -64);
        assert_eq!(sets.sea_level(), 63);
        assert!(
            sets.monument_surroundings().is_empty(),
            "a missing code-named tag is empty"
        );
    }

    #[test]
    fn a_set_with_only_foreign_biomes_is_impossible_but_kept() {
        let mut outposts = spread(32, 8);
        outposts["exclusion_zone"] =
            json!({"other_set": "minecraft:fortresses", "chunk_count": 10});
        let sets = load(&[
            set("fortresses", spread(27, 4), &["fortress"]),
            structure("fortress", "fortress", json!("minecraft:nether_wastes")),
            set("outposts", outposts, &["fortress"]),
        ])
        .unwrap_or_else(|e| panic!("{e}"));
        assert!(!sets.sets()[0].possible);
        let zone = sets.sets()[1].placement.exclusion_zone.unwrap();
        assert_eq!((zone.other_set, zone.chunk_count), (0, 10));
    }

    #[test]
    fn errors_name_the_resource_and_path() {
        let e = load(&[
            set("s", spread(2, 1), &["x"]),
            structure("x", "minecraft:castle", json!([])),
        ])
        .unwrap_err();
        assert_eq!(
            (e.resource.as_str(), e.path.as_str()),
            ("minecraft:x", "type")
        );
        assert!(matches!(*e.kind, LoadErrorKind::Unsupported { .. }));

        let e = load(&[set("s", spread(8, 8), &["x"])]).unwrap_err();
        assert!(matches!(*e.kind, LoadErrorKind::Invalid(_)), "{e}");

        let mut offset = spread(8, 1);
        offset["locate_offset"] = json!([0, 17, 0]);
        let e = load(&[set("s", offset, &["x"])]).unwrap_err();
        assert_eq!(e.path, "placement.locate_offset[1]");

        let e = load(&[
            set("s", spread(2, 1), &["x"]),
            structure(
                "x",
                "igloo",
                json!(["minecraft:plains", "minecraft:nowhere"]),
            ),
        ])
        .unwrap_err();
        assert_eq!(e.path, "biomes[1]");
    }

    #[test]
    fn tag_cycles_are_errors() {
        let e = load(&[
            set("s", spread(2, 1), &["x"]),
            structure("x", "igloo", json!("#a")),
            tag("a", json!(["#b"])),
            tag("b", json!(["#a"])),
        ])
        .unwrap_err();
        assert!(matches!(*e.kind, LoadErrorKind::Cycle(_)), "{e}");
    }
}
