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
                ("DataVersion", Tag::Int(1)),
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
            ("DataVersion", Tag::Int(1)),
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

#[test]
fn each_warning_is_raised() {
    let bundle = fixtures::overworld(&[], false);
    let inspector = SaveInspector::new(bundle.bundles()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let pack = dir
        .path()
        .join("datapacks/terrain/data/x/worldgen/noise/a.json");
    std::fs::create_dir_all(pack.parent().unwrap()).unwrap();
    std::fs::write(&pack, "{}").unwrap();
    save(
        dir.path(),
        default_generator(),
        false,
        &["vanilla", "file/terrain", "fabric"],
        true,
    );
    let report = inspector.inspect(dir.path(), Scan::Quick).unwrap();
    assert_eq!(
        report.warnings,
        [
            SaveWarning::StructuresDisabled,
            SaveWarning::DataPack {
                name: "file/terrain".into(),
                concern: PackConcern::Worldgen { files: 1 },
            },
            SaveWarning::DataPack {
                name: "fabric".into(),
                concern: PackConcern::Uninspectable,
            },
            SaveWarning::Modded,
        ]
    );
    // These make answers inexact, not impossible.
    assert!(report.answerable());
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
            data_version: Some(1),
            ..
        }]
    ));
    assert!(!report.answerable());
}
