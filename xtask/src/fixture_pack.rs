//! `cargo xtask oracle fixture-pack`: a data pack that overrides vanilla worldgen, generated from
//! the user's own bundle, so every comparison with the game can run with packs applied
//! (docs/specs/oracle.md, "The fixture pack"). Vanilla files can't be committed (ADR 0008), and an
//! override must start from vanilla's, so the program that writes the pack is committed and its
//! output isn't.
//!
//! Each change is one the comparisons see: a climate noise (biomes, structures, spawn), a
//! density function's constant (heights, blocks), the sea level (blocks), an ore's size and a
//! placed feature's count (features), a dungeon chest's rolls (loot), and a biome tag
//! (structures). The pack also has an overlay that applies, holding the real noise over a decoy
//! at the root, an overlay that never applies, holding a broken file, and a `filter` that hides
//! vanilla's copy of the tag, so a reader that ignored either would disagree with the game.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::repo_root;

/// The overlay that applies for the version: it holds the noise as applied.
const APPLIES: &str = "spyglass_applies";

/// An overlay for formats no supported version has: what it holds must never be read.
const NEVER: &str = "spyglass_never";

/// The tag the filter hides vanilla's copy of, and the pack's own replaces.
const TAG: &str = "tags/worldgen/biome/has_structure/village_plains.json";

/// Writes the pack for `version`, from `bundles/<version>`, into
/// `target/oracle/<version>/packs/overrides/`, replacing what was there, and returns its folder.
pub(crate) fn write(version: &str) -> Result<PathBuf> {
    let bundle = repo_root().join("bundles").join(version);
    let manifest = spyglass_data::bundle::read_manifest(&bundle)
        .with_context(|| format!("no bundle for {version}; extract it first"))?;
    let format = manifest.pack_format.with_context(|| {
        format!("bundles/{version} doesn't record its data pack format; extract it again")
    })?;
    let pack = repo_root()
        .join("target")
        .join("oracle")
        .join(version)
        .join("packs")
        .join("overrides");
    if pack.exists() {
        fs::remove_dir_all(&pack)?;
    }
    let read = |path: &str| -> Result<Value> {
        let file = bundle.join("data/minecraft").join(path);
        serde_json::from_slice(
            &fs::read(&file).with_context(|| format!("reading {}", file.display()))?,
        )
        .with_context(|| format!("parsing {}", file.display()))
    };
    let put = |root: &str, path: &str, value: &Value| -> Result<()> {
        let file = pack.join(root).join("data/minecraft").join(path);
        fs::create_dir_all(file.parent().expect("a file in the pack"))?;
        fs::write(&file, serde_json::to_vec_pretty(value)?)?;
        Ok(())
    };
    let format = json!([format.major, format.minor]);
    put_meta(&pack, &format)?;

    // The temperature noise: an amplitude that's 0 in vanilla, made 0.75 in the overlay that
    // applies, over a decoy of 0.25 at the root.
    let noise = "worldgen/noise/temperature.json";
    let vanilla = read(noise)?;
    put("", noise, &with_amplitude(&vanilla, 1, 0.25)?)?;
    put(APPLIES, noise, &with_amplitude(&vanilla, 1, 0.75)?)?;
    put(
        NEVER,
        "worldgen/noise_settings/overworld.json",
        &json!({"broken": true}),
    )?;

    // The continents' horizontal scale.
    let continents = "worldgen/density_function/overworld/continents.json";
    let mut value = read(continents)?;
    set_first(&mut value, "xz_scale", json!(0.3))?;
    put("", continents, &value)?;

    // The Overworld's sea level.
    let settings = "worldgen/noise_settings/overworld.json";
    let mut value = read(settings)?;
    value["sea_level"] = json!(72);
    put("", settings, &value)?;

    // An ore's vein size: `configured_feature` before 26.3, `feature` from it.
    let ore = [
        "worldgen/configured_feature/ore_iron.json",
        "worldgen/feature/ore_iron.json",
    ]
    .into_iter()
    .find(|path| bundle.join("data/minecraft").join(path).is_file())
    .context("the bundle has no ore_iron feature")?;
    let mut value = read(ore)?;
    set_first(&mut value, "size", json!(14))?;
    put("", ore, &value)?;

    // Dungeons four times as often.
    let room = "worldgen/placed_feature/monster_room.json";
    let mut value = read(room)?;
    set_first(&mut value, "count", json!(40))?;
    put("", room, &value)?;

    // A dungeon chest's first pool, rolled two more times at most.
    let chest = "loot_table/chests/simple_dungeon.json";
    let mut value = read(chest)?;
    let rolls = value
        .pointer_mut("/pools/0/rolls/max")
        .context("simple_dungeon's first pool has no uniform rolls")?;
    *rolls = json!(rolls.as_i64().context("rolls aren't an integer")? + 2);
    put("", chest, &value)?;

    // Plains villages in forests instead: the filter hides vanilla's members, so the pack's own
    // are the only ones, where merging would have kept vanilla's too.
    put("", TAG, &json!({"values": ["minecraft:forest"]}))?;
    Ok(pack)
}

/// `pack.mcmeta`: the version's format, the two overlays, and the filter on [`TAG`].
fn put_meta(pack: &Path, format: &Value) -> Result<()> {
    fs::create_dir_all(pack)?;
    let tag = TAG
        .strip_suffix(".json")
        .and_then(|t| t.strip_prefix("tags/"))
        .expect("a tag path");
    let meta = json!({
        "pack": {
            "description": "spyglass's fixture pack, generated from your bundle",
            "min_format": format,
            "max_format": format,
        },
        "overlays": {"entries": [
            {"directory": APPLIES, "min_format": format, "max_format": format},
            {"directory": NEVER, "min_format": 82, "max_format": 83},
        ]},
        "filter": {"block": [
            {"namespace": "^minecraft$", "path": format!("^tags/{}\\.json$", regex_escape(tag))},
        ]},
    });
    fs::write(pack.join("pack.mcmeta"), serde_json::to_vec_pretty(&meta)?)?;
    Ok(())
}

/// `text` with the regex metacharacters it could hold escaped.
fn regex_escape(text: &str) -> String {
    text.chars()
        .flat_map(|c| {
            let escape = ".+*?()[]{}|^$\\".contains(c);
            escape.then_some('\\').into_iter().chain([c])
        })
        .collect()
}

/// A noise with amplitude `index` set to `value`, in either form a version writes
/// (`amplitudes`, or `amplitude_modifiers` from 26.3).
fn with_amplitude(noise: &Value, index: usize, value: f64) -> Result<Value> {
    let mut noise = noise.clone();
    for key in ["amplitudes", "amplitude_modifiers"] {
        if let Some(slot) = noise.get_mut(key).and_then(|a| a.get_mut(index)) {
            *slot = json!(value);
            return Ok(noise);
        }
    }
    bail!("the noise has no amplitude {index}")
}

/// Sets the first field called `key`, depth first, to `value`.
fn set_first(value: &mut Value, key: &str, new: Value) -> Result<()> {
    fn walk(value: &mut Value, key: &str, new: &mut Option<Value>) {
        match value {
            Value::Object(map) => {
                if let Some(slot) = map.get_mut(key) {
                    if let Some(new) = new.take() {
                        *slot = new;
                    }
                    return;
                }
                for child in map.values_mut() {
                    if new.is_none() {
                        return;
                    }
                    walk(child, key, new);
                }
            }
            Value::Array(items) => {
                for child in items {
                    if new.is_none() {
                        return;
                    }
                    walk(child, key, new);
                }
            }
            _ => {}
        }
    }
    let mut new = Some(new);
    walk(value, key, &mut new);
    if new.is_some() {
        bail!("no field {key:?} to change");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amplitudes_in_either_form() {
        let old = json!({"amplitudes": [1.5, 0.0], "firstOctave": -10});
        assert_eq!(
            with_amplitude(&old, 1, 0.75).unwrap()["amplitudes"],
            json!([1.5, 0.75])
        );
        let new = json!({"amplitude_modifiers": [1.5, 0.0], "base_octave": -10});
        assert_eq!(
            with_amplitude(&new, 1, 0.75).unwrap()["amplitude_modifiers"],
            json!([1.5, 0.75])
        );
        assert!(with_amplitude(&json!({}), 1, 0.75).is_err());
    }

    #[test]
    fn sets_the_first_field_depth_first() {
        let mut value = json!({"a": {"size": 1}, "b": [{"size": 2}]});
        set_first(&mut value, "size", json!(9)).unwrap();
        assert_eq!(value, json!({"a": {"size": 9}, "b": [{"size": 2}]}));
        assert!(set_first(&mut value, "missing", json!(0)).is_err());
    }

    #[test]
    fn the_filter_matches_only_the_tag() {
        let pattern = format!(
            "^tags/{}\\.json$",
            regex_escape("worldgen/biome/has_structure/village_plains")
        );
        assert_eq!(
            pattern,
            r"^tags/worldgen/biome/has_structure/village_plains\.json$"
        );
    }
}
