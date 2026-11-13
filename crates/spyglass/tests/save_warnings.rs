//! `inspect_save` on saves built in the test, against the hand-written fixture bundle: each
//! warning, and when answers are blocked. No game data (ADR 0008).

use std::path::Path;

use spyglass::{PackConcern, SaveInspector, SaveWarning, Scan};
use spyglass_data::nbt::{self, Compound, Tag};
use spyglass_oracle::fixtures;

fn c<const N: usize>(entries: [(&str, Tag); N]) -> Tag {
    Tag::Compound(Compound::from(entries.map(|(k, v)| (k.to_string(), v))))
}

fn s(text: &str) -> Tag {
    Tag::String(text.into())
}

fn write(path: &Path, root: &Tag) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, nbt::write_compressed("", root)).unwrap();
}

/// The fixture's Default overworld generator, as a save stores it.
fn default_generator() -> Tag {
    c([
        ("type", s("minecraft:noise")),
        ("settings", s("minecraft:overworld")),
        (
            "biome_source",
            c([
                ("type", s("minecraft:multi_noise")),
                ("preset", s("minecraft:overworld")),
            ]),
        ),
    ])
}

/// A current-layout save for data version 1 (the fixture bundle's), with `generator`,
/// `structures` on or off, `packs` enabled and `modded` set as given.
fn save(dir: &Path, generator: Tag, structures: bool, packs: &[&str], modded: bool) {
    write(
        &dir.join("level.dat"),
        &c([(
            "Data",
            c([
                ("LevelName", s("Fixture")),
                ("DataVersion", Tag::Int(5023)),
                ("Version", c([("Name", s("test")), ("Id", Tag::Int(1))])),
                ("WasModded", Tag::Byte(i8::from(modded))),
                (
                    "DataPacks",
                    c([("Enabled", Tag::List(packs.iter().map(|p| s(p)).collect()))]),
                ),
            ]),
        )]),
    );
    write(
        &dir.join("data/minecraft/world_gen_settings.dat"),
        &c([
            (
                "data",
                c([
                    ("seed", Tag::Long(42)),
                    ("generate_structures", Tag::Byte(i8::from(structures))),
                    (
                        "dimensions",
                        c([(
                            "minecraft:overworld",
                            c([("type", s("minecraft:overworld")), ("generator", generator)]),
                        )]),
                    ),
                ]),
            ),
            ("DataVersion", Tag::Int(5023)),
        ]),
    );
}

#[test]
fn a_default_world_has_no_warnings_and_opens() {
    let bundle = fixtures::overworld(&[], false);
    let dir = tempfile::tempdir().unwrap();
    save(dir.path(), default_generator(), true, &["vanilla"], false);
    let report = spyglass::inspect_save(dir.path(), bundle.bundles(), Scan::Chunks).unwrap();
    assert_eq!(report.warnings, []);
    assert!(report.answerable());
    assert_eq!(report.bundle.as_ref().unwrap().dir, bundle.path());
    let world = report.open_world(spyglass::Dimension::Overworld).unwrap();
    assert_eq!(world.version(), "test");
}

const MCMETA: &str = r#"{"pack": {"description": "", "min_format": 121, "max_format": 121}}"#;

/// Writes `text` to `path`, making its folders.
fn put(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn each_warning_is_raised() {
    let bundle = fixtures::overworld(&[], false);
    let inspector = SaveInspector::new(bundle.bundles()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let packs = dir.path().join("datapacks");
    // A folder without pack.mcmeta isn't a pack to the game.
    put(&packs.join("bare/data/x/worldgen/noise/a.json"), "{}");
    put(
        &packs.join("old/pack.mcmeta"),
        r#"{"pack": {"description": "", "pack_format": 48}}"#,
    );
    put(&packs.join("bad.zip"), "not a zip");
    save(
        dir.path(),
        default_generator(),
        false,
        &[
            "vanilla",
            "file/bare",
            "file/gone",
            "file/bad.zip",
            "fabric",
            "file/old",
        ],
        true,
    );
    let report = inspector.inspect(dir.path(), Scan::Quick).unwrap();
    let concerns: Vec<(&str, &PackConcern)> = report
        .warnings
        .iter()
        .filter_map(|w| match w {
            SaveWarning::DataPack { name, concern } => Some((name.as_str(), concern)),
            _ => None,
        })
        .collect();
    assert!(
        matches!(
            concerns.as_slice(),
            [
                ("file/bare", PackConcern::Skipped { .. }),
                ("file/gone", PackConcern::Missing),
                ("file/bad.zip", PackConcern::Unreadable { .. }),
                ("fabric", PackConcern::Mod),
                ("file/old", PackConcern::OtherFormat { .. }),
            ]
        ),
        "{concerns:#?}"
    );
    assert_eq!(report.warnings[0], SaveWarning::StructuresDisabled);
    assert_eq!(report.warnings.last(), Some(&SaveWarning::Modded));
    // The pack of another format is applied anyway, as the game applies it.
    let applied: Vec<&str> = report.packs.iter().map(|p| p.name()).collect();
    assert_eq!(applied, ["file/old"]);
    // These make answers inexact, not impossible.
    assert!(report.answerable());
}

#[test]
fn a_saves_packs_are_applied() {
    let bundle = fixtures::overworld(&[], false);
    let inspector = SaveInspector::new(bundle.bundles()).unwrap();
    // The fixture's noise settings with the temperature gradient turned round: desert west of
    // x = 0, plains east of it.
    let mut settings = bundle
        .open()
        .resource("worldgen/noise_settings", "overworld")
        .unwrap();
    let temperature = &mut settings["noise_router"]["temperature"];
    let (from, to) = (
        temperature["from_value"].clone(),
        temperature["to_value"].clone(),
    );
    temperature["from_value"] = to;
    temperature["to_value"] = from;
    let dir = tempfile::tempdir().unwrap();
    let pack = dir.path().join("datapacks/turned");
    put(&pack.join("pack.mcmeta"), MCMETA);
    put(
        &pack.join("data/minecraft/worldgen/noise_settings/overworld.json"),
        &settings.to_string(),
    );
    save(
        dir.path(),
        default_generator(),
        true,
        &["vanilla", "file/turned"],
        false,
    );
    let report = inspector.inspect(dir.path(), Scan::Quick).unwrap();
    assert_eq!(report.warnings, []);
    assert_eq!(report.packs.len(), 1);
    let biome = |world: &spyglass::World| world.session(1).unwrap().biome_at([500, 64, 0]).biome;
    let world = report.open_world(spyglass::Dimension::Overworld).unwrap();
    assert_eq!(biome(&world), "minecraft:plains");
    assert_eq!(
        world.packs().map(|(name, _)| name).collect::<Vec<_>>(),
        ["file/turned"]
    );
    let game = report.open_game().unwrap();
    assert_eq!(
        biome(game.world(spyglass::Dimension::Overworld).unwrap()),
        "minecraft:plains"
    );
    // Without the pack, the fixture's own.
    let plain = spyglass::World::open(bundle.path()).unwrap();
    assert_eq!(biome(&plain), "minecraft:desert");
}

#[test]
fn another_generator_blocks_answers() {
    let bundle = fixtures::overworld(&[], false);
    let inspector = SaveInspector::new(bundle.bundles()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let flat = c([("type", s("minecraft:flat")), ("settings", c([]))]);
    save(dir.path(), flat, true, &["vanilla"], false);
    let report = inspector.inspect(dir.path(), Scan::Quick).unwrap();
    assert!(matches!(
        report.warnings.as_slice(),
        [SaveWarning::UnsupportedGenerator { preset: None, .. }]
    ));
    assert!(!report.answerable());
    assert!(matches!(
        report.open_world(spyglass::Dimension::Overworld),
        Err(spyglass::Error::Unsupported(_))
    ));
}

#[test]
fn no_bundles_folder_means_no_bundle() {
    let dir = tempfile::tempdir().unwrap();
    save(dir.path(), default_generator(), true, &["vanilla"], false);
    let missing = dir.path().join("no-bundles-here");
    let report = spyglass::inspect_save(dir.path(), &missing, Scan::Quick).unwrap();
    assert!(matches!(
        report.warnings.as_slice(),
        [SaveWarning::NoBundle {
            data_version: Some(5023),
            ..
        }]
    ));
    assert!(!report.answerable());
}

#[test]
fn a_pack_spyglass_cant_apply_fails_on_opening() {
    let bundle = fixtures::overworld(&[], false);
    let dir = tempfile::tempdir().unwrap();
    let pack = dir.path().join("datapacks/modded");
    put(&pack.join("pack.mcmeta"), MCMETA);
    put(
        &pack.join("data/minecraft/worldgen/density_function/zero.json"),
        r#"{"type": "mod:new"}"#,
    );
    save(
        dir.path(),
        default_generator(),
        true,
        &["vanilla", "file/modded"],
        false,
    );
    let report = spyglass::inspect_save(dir.path(), bundle.bundles(), Scan::Quick).unwrap();
    let error = report
        .open_world(spyglass::Dimension::Overworld)
        .err()
        .expect("an unsupported type");
    let text = error.to_string();
    assert!(text.contains("zero") && text.contains("mod:new"), "{text}");
}
