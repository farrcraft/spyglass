//! `Game`, saves assessed per dimension, and the map layers each dimension has (ADR 0023), on
//! the hand-written two-dimension fixture bundle. No game data (ADR 0008).

use std::path::Path;

use spyglass::{Dimension, Error, Game, Layers, MapArea, SaveWarning, Scan};
use spyglass_data::nbt::{self, Compound, Tag};
use spyglass_oracle::fixtures;

fn c<const N: usize>(entries: [(&str, Tag); N]) -> Tag {
    Tag::Compound(Compound::from(entries.map(|(k, v)| (k.to_string(), v))))
}

fn s(text: &str) -> Tag {
    Tag::String(text.into())
}

/// A JSON generator as a save stores it.
fn tag(value: &serde_json::Value) -> Tag {
    match value {
        serde_json::Value::String(text) => s(text),
        serde_json::Value::Object(map) => Tag::Compound(Compound::from_iter(
            map.iter().map(|(k, v)| (k.clone(), tag(v))),
        )),
        other => panic!("not in a fixture generator: {other}"),
    }
}

fn write(path: &Path, root: &Tag) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, nbt::write_compressed("", root)).unwrap();
}

/// A current-layout save for the fixture bundle's data version, with the fixture's Default
/// Overworld and `nether` as its Nether generator, if any.
fn save(dir: &Path, nether: Option<Tag>) {
    write(
        &dir.join("level.dat"),
        &c([(
            "Data",
            c([
                ("LevelName", s("Fixture")),
                ("DataVersion", Tag::Int(1)),
                ("DataPacks", c([("Enabled", Tag::List(vec![s("vanilla")]))])),
            ]),
        )]),
    );
    let overworld = c([
        ("type", s("minecraft:noise")),
        ("settings", s("minecraft:overworld")),
        (
            "biome_source",
            c([
                ("type", s("minecraft:multi_noise")),
                ("preset", s("minecraft:overworld")),
            ]),
        ),
    ]);
    let mut dimensions = vec![(
        "minecraft:overworld".to_string(),
        c([("generator", overworld)]),
    )];
    if let Some(nether) = nether {
        dimensions.push((
            "minecraft:the_nether".to_string(),
            c([("generator", nether)]),
        ));
    }
    write(
        &dir.join("data/minecraft/world_gen_settings.dat"),
        &c([(
            "data",
            c([
                ("seed", Tag::Long(42)),
                ("generate_structures", Tag::Byte(1)),
                ("dimensions", Tag::Compound(Compound::from_iter(dimensions))),
            ]),
        )]),
    );
}

#[test]
fn a_game_opens_each_dimension_once() {
    let bundle = fixtures::overworld_and_nether();
    let game = Game::open(bundle.path()).unwrap();
    assert_eq!(game.version(), "test");
    let overworld = game.world(Dimension::Overworld).unwrap();
    assert!(std::ptr::eq(
        overworld,
        game.world(Dimension::Overworld).unwrap()
    ));
    let nether = game.world(Dimension::Nether).unwrap();
    assert_eq!(nether.dimension_kind(), Dimension::Nether);
    assert!(nether.has_ceiling());
    assert!(!overworld.has_ceiling());
    // The fixture's preset has no End.
    assert!(matches!(
        game.world(Dimension::End),
        Err(Error::Unknown {
            what: "dimension",
            ..
        })
    ));

    let session = game.session(42);
    assert_eq!(session.seed(), 42);
    let in_nether = session.dimension(Dimension::Nether).unwrap();
    assert!(std::ptr::eq(in_nether.world(), nether));
    assert_eq!(
        in_nether.biome_at([1000, 64, -1000]).biome,
        "minecraft:nether_wastes"
    );
    assert!(std::ptr::eq(
        in_nether,
        session.dimension(Dimension::Nether).unwrap()
    ));
}

#[test]
fn coordinates_convert_by_scale() {
    let bundle = fixtures::overworld_and_nether();
    let game = Game::open(bundle.path()).unwrap();
    assert_eq!(game.coordinate_scale(Dimension::Nether).unwrap(), 8.0);
    assert_eq!(game.coordinate_scale(Dimension::Overworld).unwrap(), 1.0);
    let (o, n) = (Dimension::Overworld, Dimension::Nether);
    // Rounded down, as a teleport's BlockPos is.
    assert_eq!(game.convert([17, -9], o, n).unwrap(), [2, -2]);
    assert_eq!(game.convert([-8, 7], o, n).unwrap(), [-1, 0]);
    assert_eq!(game.convert([2, -2], n, o).unwrap(), [16, -16]);
    assert_eq!(game.convert([5, 6], n, n).unwrap(), [5, 6]);
    // Kept in i32.
    assert_eq!(
        game.convert([i32::MAX, i32::MIN], n, o).unwrap(),
        [i32::MAX, i32::MIN]
    );
    assert!(matches!(
        game.convert([0, 0], o, Dimension::End),
        Err(Error::NotLinked { .. })
    ));
    assert!(matches!(
        game.convert([0, 0], Dimension::End, n),
        Err(Error::NotLinked { .. })
    ));
}

#[test]
fn saves_are_assessed_per_dimension() {
    let bundle = fixtures::overworld_and_nether();
    let dir = tempfile::tempdir().unwrap();
    save(dir.path(), Some(tag(&fixtures::nether_generator())));
    let report = spyglass::inspect_save(dir.path(), bundle.bundles(), Scan::Chunks).unwrap();
    assert_eq!(report.warnings, []);
    assert!(report.answerable_in(Dimension::Nether));
    let world = report.open_world(Dimension::Nether).unwrap();
    assert_eq!(world.dimension_kind(), Dimension::Nether);

    // A Nether that isn't the Default preset's blocks only the Nether.
    let flat = c([("type", s("minecraft:flat")), ("settings", c([]))]);
    save(dir.path(), Some(flat));
    let report = spyglass::inspect_save(dir.path(), bundle.bundles(), Scan::Quick).unwrap();
    let [warning] = report.warnings.as_slice() else {
        panic!("{:?}", report.warnings);
    };
    assert!(matches!(
        warning,
        SaveWarning::UnsupportedGenerator {
            dimension: Dimension::Nether,
            preset: None,
            ..
        }
    ));
    assert_eq!(warning.dimension(), Some(Dimension::Nether));
    assert!(
        warning
            .to_string()
            .starts_with("the Nether uses a custom generator")
    );
    assert!(!report.answerable());
    assert!(report.answerable_in(Dimension::Overworld));
    assert!(!report.answerable_in(Dimension::Nether));
    assert!(report.open_world(Dimension::Overworld).is_ok());
    assert!(matches!(
        report.open_world(Dimension::Nether),
        Err(Error::Unsupported(_))
    ));
    let game = report.open_game().unwrap();
    assert!(game.world(Dimension::Overworld).is_ok());
    assert!(matches!(
        game.world(Dimension::Nether),
        Err(Error::Unsupported(reason)) if reason == warning.to_string()
    ));

    // A dimension the preset has but the save doesn't is blocked too.
    save(dir.path(), None);
    let report = spyglass::inspect_save(dir.path(), bundle.bundles(), Scan::Quick).unwrap();
    assert!(matches!(
        report.warnings.as_slice(),
        [SaveWarning::UnsupportedGenerator {
            dimension: Dimension::Nether,
            generator,
            ..
        }] if generator == "none"
    ));
}

#[test]
fn layers_follow_the_dimension() {
    let bundle = fixtures::overworld_and_nether();
    let game = Game::open(bundle.path()).unwrap();
    let area = MapArea::centred_on([0, 0], [16, 16], 4, 64).unwrap();
    let nether = game.session(1);
    let nether = nether.dimension(Dimension::Nether).unwrap();
    assert!(nether.render(&area, &Layers::default()).is_ok());
    for layers in [
        Layers {
            slime_chunks: true,
            ..Layers::default()
        },
        Layers {
            spawn: true,
            ..Layers::default()
        },
        Layers {
            relief: true,
            ..Layers::default()
        },
        Layers {
            surface: true,
            ..Layers::default()
        },
    ] {
        assert!(
            matches!(nether.render(&area, &layers), Err(Error::InvalidMap(_))),
            "{layers:?}"
        );
    }
    let overworld = game.world(Dimension::Overworld).unwrap();
    let every = Layers {
        slime_chunks: true,
        spawn: true,
        relief: true,
        surface: true,
        ..Layers::default()
    };
    assert!(every.check(overworld).is_ok());
    // Surface biomes without the biome layer draw nothing, so they aren't refused.
    let unused = Layers {
        biomes: false,
        surface: true,
        ..Layers::default()
    };
    assert!(unused.check(nether.world()).is_ok());
}
