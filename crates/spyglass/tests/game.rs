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
                ("DataVersion", Tag::Int(5023)),
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

#[test]
fn constraints_infer_their_dimension() {
    let bundle = fixtures::structured_overworld_and_nether();
    let game = Game::open(bundle.path()).unwrap();
    let query = game
        .search_query(
            "require fort within 100 of spawn\n\
             require nether minecraft:fort within 100 of 0,0\n\
             require #minecraft:mixed within 100 of spawn\n\
             require nether #minecraft:mixed within 100 of spawn\n\
             prefer overworld tower within 30 of fort in 0,0 to 10,10\n\
             prefer biome nether_wastes within 10 of 0,0 at y 100\n\
             prefer 2 slime within 64 of spawn",
        )
        .unwrap();
    let dimensions: Vec<Dimension> = (0..7).map(|i| query.dimension(i).unwrap()).collect();
    let (o, n) = (Dimension::Overworld, Dimension::Nether);
    assert_eq!(dimensions, [n, n, o, n, o, n, o]);
    // A dimension is written only where it isn't the inferred one.
    assert_eq!(
        query.to_string(),
        "require minecraft:fort within 100 of spawn\n\
         require minecraft:fort within 100 of 0,0\n\
         require #minecraft:mixed within 100 of spawn\n\
         require nether #minecraft:mixed within 100 of spawn\n\
         prefer minecraft:tower within 30 of minecraft:fort in 0,0 to 10,10\n\
         prefer biome minecraft:nether_wastes within 10 of 0,0 at y 100\n\
         prefer 2 slime within 64 of spawn\n"
    );
    let again = game.search_query(&query.to_string()).unwrap();
    assert_eq!(again.to_string(), query.to_string());
}

#[test]
fn dimensions_that_contradict_are_errors() {
    let bundle = fixtures::structured_overworld_and_nether();
    let game = Game::open(bundle.path()).unwrap();
    let error = |text: &str| match game.search_query(text) {
        Err(Error::InvalidQuery {
            line,
            column: None,
            message,
        }) => (line, message),
        other => panic!("{text}: {other:?}"),
    };
    assert_eq!(
        error("prefer tower within 5 of 0,0\nrequire nether tower within 5 of 0,0"),
        (2, "minecraft:tower isn't placed in the Nether".to_string())
    );
    assert_eq!(
        error("require nether biome plains within 5 of 0,0").1,
        "the Nether has no minecraft:plains"
    );
    assert_eq!(
        error("require nether 2 slime within 5 of 0,0").1,
        "slime chunks are only in the Overworld"
    );
    assert_eq!(
        error("require nether spawn within 5 of 0,0").1,
        "the spawn is only in the Overworld"
    );
    // The fixture has no End.
    assert!(
        error("require end tower within 5 of 0,0")
            .1
            .contains("dimension")
    );
    assert!(
        error("require biome nether_wastes within 5 of 0,0 at y 300")
            .1
            .contains("build height of the Nether, 0 to 255")
    );
}

#[test]
fn a_world_searches_its_own_dimension() {
    let bundle = fixtures::structured_overworld_and_nether();
    let game = Game::open(bundle.path()).unwrap();
    let nether = game.world(Dimension::Nether).unwrap();
    let overworld = game.world(Dimension::Overworld).unwrap();
    // In a world, everything is its dimension, written or not.
    let query = nether
        .search_query("require nether fort within 50 of 0,0\nprefer #minecraft:mixed in 0,0 to 9,9")
        .unwrap();
    assert_eq!(query.dimension(1), Some(Dimension::Nether));
    assert_eq!(
        query.to_string(),
        "require minecraft:fort within 50 of 0,0\nprefer #minecraft:mixed in 0,0 to 9,9\n"
    );
    let session = nether.session(3).unwrap();
    assert_eq!(
        session.evaluate(&query, false).unwrap(),
        game.session(3).evaluate(&query, false).unwrap()
    );
    // The spawn and slime chunks need the Overworld.
    for text in [
        "require fort within 50 of spawn",
        "require 2 slime in 0,0 to 9,9",
        "require overworld tower in 0,0 to 9,9",
    ] {
        assert!(
            matches!(nether.search_query(text), Err(Error::InvalidQuery { .. })),
            "{text}"
        );
    }
    assert!(
        overworld
            .search_query("require nether fort within 50 of 0,0")
            .is_err()
    );
    // A query in two dimensions isn't one world's to judge.
    let both = game
        .search_query("require fort within 50 of 0,0\nrequire tower within 50 of 0,0")
        .unwrap();
    assert!(matches!(
        overworld.session(3).unwrap().evaluate(&both, false),
        Err(Error::OtherWorld(_))
    ));
    assert!(game.session(3).evaluate(&both, false).is_ok());
}
