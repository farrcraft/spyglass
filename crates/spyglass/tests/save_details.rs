//! `SaveReport::details` on saves built in the test, against the hand-written fixture bundles:
//! every warning kind appears once, beside its subject, and the settings, packs and holdings say
//! what the save holds. No game data (ADR 0008).

use std::path::Path;

use spyglass::{
    Dimension, PackUse, SaveDetails, SaveInspector, SaveSubject, SaveWarning, Scan, Setting,
};
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

fn put(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

const MCMETA: &str = r#"{"pack": {"description": "", "min_format": 121, "max_format": 121}}"#;

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

/// What a test save is made of.
struct Spec<'a> {
    generator: Tag,
    structures: bool,
    packs: &'a [&'a str],
    modded: bool,
}

impl Default for Spec<'_> {
    fn default() -> Self {
        Spec {
            generator: default_generator(),
            structures: true,
            packs: &["vanilla"],
            modded: false,
        }
    }
}

/// A current-layout save for the fixture bundle's data version.
fn save(dir: &Path, spec: Spec<'_>) {
    write(
        &dir.join("level.dat"),
        &c([(
            "Data",
            c([
                ("LevelName", s("Fixture")),
                ("DataVersion", Tag::Int(5023)),
                ("Version", c([("Name", s("test")), ("Id", Tag::Int(1))])),
                ("WasModded", Tag::Byte(i8::from(spec.modded))),
                (
                    "DataPacks",
                    c([(
                        "Enabled",
                        Tag::List(spec.packs.iter().map(|p| s(p)).collect()),
                    )]),
                ),
                (
                    "spawn",
                    c([
                        ("pos", Tag::IntArray(vec![8, 64, -8])),
                        ("dimension", s("minecraft:overworld")),
                    ]),
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
                    ("generate_structures", Tag::Byte(i8::from(spec.structures))),
                    (
                        "dimensions",
                        c([(
                            "minecraft:overworld",
                            c([
                                ("type", s("minecraft:overworld")),
                                ("generator", spec.generator),
                            ]),
                        )]),
                    ),
                ]),
            ),
            ("DataVersion", Tag::Int(5023)),
        ]),
    );
}

/// Writes region `r.0.0.mca` in `dir` with each chunk uncompressed at its header index.
fn write_region(dir: &Path, chunks: &[(usize, Tag)]) {
    write_region_at(dir, [0, 0], chunks);
}

/// Writes region `r.<x>.<z>.mca` in `dir` with each chunk uncompressed at its header index.
fn write_region_at(dir: &Path, [x, z]: [i32; 2], chunks: &[(usize, Tag)]) {
    std::fs::create_dir_all(dir).unwrap();
    let mut file = vec![0_u8; 8192];
    for (index, tag) in chunks {
        let data = nbt::write("", tag);
        let sector = file.len() / 4096;
        let mut body = ((data.len() + 1) as u32).to_be_bytes().to_vec();
        body.push(3);
        body.extend(data);
        let sectors = body.len().div_ceil(4096);
        body.resize(sectors * 4096, 0);
        file.extend(body);
        let entry = ((sector as u32) << 8) | sectors as u32;
        file[index * 4..index * 4 + 4].copy_from_slice(&entry.to_be_bytes());
    }
    std::fs::write(dir.join(format!("r.{x}.{z}.mca")), &file).unwrap();
}

fn overworld_region(dir: &Path) -> std::path::PathBuf {
    dir.join("dimensions/minecraft/overworld/region")
}

/// A chunk saved at `data_version`, with `status`.
fn chunk(data_version: i32, status: &str) -> Tag {
    c([
        ("DataVersion", Tag::Int(data_version)),
        ("Status", s(status)),
    ])
}

/// The report of the save in `dir` against `bundle`, scanned.
fn details(dir: &Path, bundles: &Path) -> SaveDetails {
    let inspector = SaveInspector::new(bundles).unwrap();
    inspector.inspect(dir, Scan::Chunks).unwrap().details()
}

/// The one warning of `kind`, which must be beside `subject`, and `subject` an item the report
/// has.
fn the_warning<'d>(details: &'d SaveDetails, kind: &str, subject: &SaveSubject) -> &'d SaveWarning {
    let found: Vec<&SaveWarning> = details
        .warnings
        .iter()
        .filter(|w| w.kind() == kind)
        .collect();
    assert_eq!(found.len(), 1, "{kind}: {:#?}", details.warnings);
    assert_eq!(found[0].subject(), *subject, "{kind}: {}", found[0]);
    assert!(details.warnings_about(subject).contains(&found[0]));
    let present = match subject {
        SaveSubject::Setting(setting) => details.settings.iter().any(|s| s.setting == *setting),
        SaveSubject::Pack(name) => details.packs.iter().any(|p| p.name == *name),
        SaveSubject::Dimension(d) => details.dimensions.iter().any(|x| x.dimension == *d),
        SaveSubject::Holdings(d) => details
            .dimensions
            .iter()
            .any(|x| x.dimension == *d && x.holdings.is_some()),
    };
    assert!(present, "{kind}: {subject:?} isn't in the report");
    found[0]
}

#[test]
fn no_bundle_is_beside_the_data_version() {
    let dir = tempfile::tempdir().unwrap();
    save(dir.path(), Spec::default());
    let details = details(dir.path(), &dir.path().join("no-bundles"));
    the_warning(
        &details,
        "no_bundle",
        &SaveSubject::Setting(Setting::DataVersion),
    );
    assert!(details.dimensions.iter().all(|d| !d.answerable));
}

#[test]
fn another_generator_is_beside_its_dimension_s_generator() {
    let bundle = fixtures::overworld(&[], false);
    let dir = tempfile::tempdir().unwrap();
    let flat = c([("type", s("minecraft:flat")), ("settings", c([]))]);
    save(
        dir.path(),
        Spec {
            generator: flat,
            ..Spec::default()
        },
    );
    let details = details(dir.path(), bundle.bundles());
    the_warning(
        &details,
        "unsupported_generator",
        &SaveSubject::Setting(Setting::Generator(Dimension::Overworld)),
    );
    assert!(!details.answerable_in(Dimension::Overworld));
    let generator = details
        .settings
        .iter()
        .find(|s| s.setting == Setting::Generator(Dimension::Overworld))
        .unwrap();
    assert_eq!(generator.value, "minecraft:flat: no preset's");
}

#[test]
fn settings_warnings_are_beside_their_settings() {
    let bundle = fixtures::overworld(&[], false);
    let dir = tempfile::tempdir().unwrap();
    save(
        dir.path(),
        Spec {
            structures: false,
            modded: true,
            packs: &["vanilla", "file/gone"],
            ..Spec::default()
        },
    );
    let details = details(dir.path(), bundle.bundles());
    the_warning(
        &details,
        "structures_disabled",
        &SaveSubject::Setting(Setting::Structures),
    );
    the_warning(&details, "modded", &SaveSubject::Setting(Setting::Modded));
    the_warning(
        &details,
        "data_pack",
        &SaveSubject::Pack("file/gone".into()),
    );
    let gone = details
        .packs
        .iter()
        .find(|p| p.name == "file/gone")
        .unwrap();
    assert_eq!(gone.used, PackUse::NotApplied);
    assert_eq!(details.packs[0].used, PackUse::Bundle);
    assert_eq!(details.warnings.len(), 3, "{:#?}", details.warnings);
}

#[test]
fn chunk_warnings_are_beside_the_holdings() {
    let bundle = fixtures::overworld(&[], false);
    let dir = tempfile::tempdir().unwrap();
    save(dir.path(), Spec::default());
    let region = overworld_region(dir.path());
    write_region(
        &region,
        &[
            (0, chunk(5023, "minecraft:full")),
            (1, chunk(4671, "minecraft:full")),
        ],
    );
    // A folder with a region file's name can't be read as one.
    std::fs::create_dir_all(region.join("r.1.1.mca")).unwrap();
    let details = details(dir.path(), bundle.bundles());
    let holdings = SaveSubject::Holdings(Dimension::Overworld);
    the_warning(&details, "older_chunks", &holdings);
    the_warning(&details, "unreadable_regions", &holdings);
}

#[test]
fn a_moving_border_is_beside_the_border() {
    let bundle = fixtures::overworld(&[], false);
    let dir = tempfile::tempdir().unwrap();
    save(dir.path(), Spec::default());
    write(
        &dir.path()
            .join("dimensions/minecraft/overworld/data/minecraft/world_border.dat"),
        &c([(
            "data",
            c([
                ("center_x", Tag::Double(0.0)),
                ("center_z", Tag::Double(0.0)),
                ("size", Tag::Double(1000.0)),
                ("lerp_time", Tag::Long(60_000)),
                ("lerp_target", Tag::Double(200.0)),
                ("damage_per_block", Tag::Double(0.2)),
                ("safe_zone", Tag::Double(5.0)),
                ("warning_blocks", Tag::Int(5)),
                ("warning_time", Tag::Int(15)),
            ]),
        )]),
    );
    let details = details(dir.path(), bundle.bundles());
    let border = SaveSubject::Setting(Setting::Border(Dimension::Overworld));
    let warning = the_warning(&details, "moving_border", &border);
    assert!(warning.is_note() && !warning.blocks_answers());
    let setting = details
        .settings
        .iter()
        .find(|s| s.setting == Setting::Border(Dimension::Overworld))
        .unwrap();
    assert_eq!(
        setting.value,
        "centre 0, 0, side 1000, moving to 200 over 60 s"
    );
    assert!(setting.source.present);
    assert!(details.answerable_in(Dimension::Overworld));
}

#[test]
fn a_pack_error_is_beside_the_pack() {
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
        Spec {
            packs: &["vanilla", "file/modded"],
            ..Spec::default()
        },
    );
    let inspector = SaveInspector::new(bundle.bundles()).unwrap();
    let report = inspector.inspect(dir.path(), Scan::Quick).unwrap();
    // Only the report finds it, since it opens the dimension.
    assert_eq!(report.warnings, []);
    let details = report.details();
    let warning = the_warning(
        &details,
        "pack_error",
        &SaveSubject::Pack("file/modded".into()),
    );
    assert!(matches!(
        warning,
        SaveWarning::PackError {
            dimension: Dimension::Overworld,
            ..
        }
    ));
    assert!(!details.answerable_in(Dimension::Overworld));
    assert!(!details.dimensions[0].answerable);
    let modded = &details.packs[1];
    assert_eq!(modded.used, PackUse::Applied);
    let changes = modded.changes.as_ref().unwrap();
    assert_eq!(
        changes.registries["worldgen/density_function"].replaced,
        ["minecraft:zero"]
    );
    let answers: Vec<&str> = modded.answers.iter().map(|a| a.name()).collect();
    assert_eq!(answers, ["biomes", "terrain"]);
    assert!(matches!(
        warning,
        SaveWarning::PackError { file: Some(file), .. }
            if file == "data/minecraft/worldgen/density_function/zero.json"
    ));
}

#[test]
fn the_holdings_count_what_the_save_holds() {
    let bundle = fixtures::structured_overworld(false);
    let dir = tempfile::tempdir().unwrap();
    save(dir.path(), Spec::default());
    let start = |id: &str| c([("id", s(id)), ("ChunkX", Tag::Int(0))]);
    let chest = |loot: bool| {
        let mut fields = Compound::from([("id".to_string(), s("minecraft:chest"))]);
        if loot {
            fields.insert("LootTable".into(), s("minecraft:chests/igloo"));
        }
        Tag::Compound(fields)
    };
    write_region(
        &overworld_region(dir.path()),
        &[
            (
                0,
                c([
                    ("DataVersion", Tag::Int(5023)),
                    ("Status", s("minecraft:full")),
                    ("InhabitedTime", Tag::Long(100)),
                    (
                        "structures",
                        c([(
                            "starts",
                            c([
                                ("minecraft:tower", start("minecraft:tower")),
                                ("pack:gone", start("pack:gone")),
                                ("minecraft:hut", start("INVALID")),
                            ]),
                        )]),
                    ),
                    (
                        "block_entities",
                        Tag::List(vec![chest(true), chest(true), chest(false)]),
                    ),
                ]),
            ),
            (33, chunk(5023, "minecraft:noise")),
            (
                34,
                c([
                    ("DataVersion", Tag::Int(2860)),
                    ("Status", s("minecraft:full")),
                    ("InhabitedTime", Tag::Long(20)),
                    (
                        "structures",
                        c([(
                            "starts",
                            c([("minecraft:village", start("minecraft:village"))]),
                        )]),
                    ),
                ]),
            ),
        ],
    );
    let details = details(dir.path(), bundle.bundles());
    let holdings = details.dimensions[0].holdings.as_ref().unwrap();
    assert_eq!(
        (
            holdings.chunks,
            holdings.full,
            holdings.proto,
            holdings.unreadable
        ),
        (3, 2, 1, 0)
    );
    assert_eq!(holdings.inhabited_ticks, 120);
    assert_eq!(holdings.bounds, Some([[0, 0], [2, 1]]));
    let versions: Vec<(Option<i32>, Option<&str>, usize)> = holdings
        .versions
        .iter()
        .map(|v| (v.data_version, v.version.as_deref(), v.chunks))
        .collect();
    assert_eq!(
        versions,
        [(Some(5023), Some("test"), 2), (Some(2860), None, 1)]
    );
    assert_eq!(holdings.starts, [("minecraft:tower".to_string(), 1)]);
    assert_eq!(holdings.dropped_starts, [("pack:gone".to_string(), 1)]);
    assert_eq!(
        holdings.older_starts,
        [("minecraft:village".to_string(), 1)]
    );
    assert_eq!(
        holdings.sealed_containers,
        [("minecraft:chest".to_string(), 2)]
    );
    // Dimensions without region files hold nothing.
    let nether = details.dimensions[1].holdings.as_ref().unwrap();
    assert_eq!(nether.chunks, 0);
    assert_eq!(nether.bounds, None);
}

#[test]
fn settings_say_where_they_came_from() {
    let bundle = fixtures::overworld(&[], false);
    let dir = tempfile::tempdir().unwrap();
    save(dir.path(), Spec::default());
    let details = details(dir.path(), bundle.bundles());
    let kinds: Vec<&str> = details.settings.iter().map(|s| s.setting.kind()).collect();
    assert_eq!(
        kinds,
        [
            "version",
            "data_version",
            "seed",
            "structures",
            "generator",
            "generator",
            "generator",
            "border",
            "border",
            "border",
            "spawn",
            "player",
            "modded"
        ]
    );
    let get = |setting| {
        details
            .settings
            .iter()
            .find(|s| s.setting == setting)
            .unwrap()
    };
    assert_eq!(get(Setting::Seed).value, "42");
    assert_eq!(
        get(Setting::Seed).source.file,
        "data/minecraft/world_gen_settings.dat"
    );
    assert_eq!(get(Setting::DataVersion).value, "5023, the test bundle's");
    assert_eq!(
        get(Setting::Generator(Dimension::Overworld)).value,
        "minecraft:noise (minecraft:overworld): the Default preset's"
    );
    assert_eq!(
        get(Setting::Spawn).value,
        "8, 64, -8 in minecraft:overworld"
    );
    assert!(!get(Setting::Player).source.present);
    assert_eq!(
        get(Setting::Border(Dimension::Nether)).value,
        "the default: centre 0, 0, side 59999968"
    );
    assert!(!get(Setting::Seed).decides().is_empty());
    assert!(details.warnings.is_empty(), "{:#?}", details.warnings);
}

#[test]
fn answers_from_a_save_are_tallied() {
    let bundle = fixtures::structured_overworld(false);
    let dir = tempfile::tempdir().unwrap();
    save(dir.path(), Spec::default());
    // Two chunks the seed starts towers in: the save keeps one and has none in the other.
    let world = spyglass::World::open(bundle.path()).unwrap();
    let query = world.structure_query("minecraft:tower").unwrap();
    let predicted = world
        .session(42)
        .unwrap()
        .structures_in(&query, [-64, -64], [63, 63], true)
        .unwrap();
    // Two in one region file.
    let region_of = |[x, z]: [i32; 2]| [x >> 5, z >> 5];
    let region = predicted
        .iter()
        .map(|s| region_of(s.chunk))
        .find(|r| {
            predicted
                .iter()
                .filter(|s| region_of(s.chunk) == *r)
                .count()
                >= 2
        })
        .expect("two towers in a region");
    let towers: Vec<[i32; 2]> = predicted
        .iter()
        .map(|s| s.chunk)
        .filter(|c| region_of(*c) == region)
        .collect();
    let (min, max) = (
        [region[0] * 32, region[1] * 32],
        [region[0] * 32 + 31, region[1] * 32 + 31],
    );
    let header = |[x, z]: [i32; 2]| ((x & 31) + (z & 31) * 32) as usize;
    let start = c([
        ("id", s("minecraft:tower")),
        ("ChunkX", Tag::Int(towers[0][0])),
    ]);
    let with_starts = |starts: Tag| {
        c([
            ("DataVersion", Tag::Int(5023)),
            ("Status", s("minecraft:full")),
            ("structures", c([("starts", starts)])),
        ])
    };
    write_region_at(
        &overworld_region(dir.path()),
        region,
        &[
            (
                header(towers[0]),
                with_starts(c([("minecraft:tower", start)])),
            ),
            (header(towers[1]), with_starts(c([]))),
        ],
    );
    let report = SaveInspector::new(bundle.bundles())
        .unwrap()
        .inspect(dir.path(), Scan::Chunks)
        .unwrap();
    let chunks = std::sync::Arc::new(report.chunks(Dimension::Overworld));
    assert_eq!(chunks.usage(), spyglass::SaveUsage::default());
    let session = world
        .session(42)
        .unwrap()
        .with_save(std::sync::Arc::clone(&chunks))
        .unwrap();
    for _ in 0..2 {
        let answered = session.structures_in(&query, min, max, true).unwrap();
        assert!(answered.iter().any(|s| s.chunk == towers[0] && s.from_save));
        assert!(!answered.iter().any(|s| s.chunk == towers[1]));
    }
    // Asked twice, counted once.
    let usage = chunks.usage();
    assert_eq!(usage.chunks_with_starts, 1);
    assert_eq!(usage.starts_from_save, 1);
    assert_eq!(usage.chunks_without_starts, 1);
    assert_eq!(usage.chunks_read, 2);
}
