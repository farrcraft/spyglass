//! Saves and region files built in the test: every layout, the single player, and each chunk
//! encoding. No game files (ADR 0008).

use std::io::Write;
use std::path::Path;

use flate2::Compression;
use flate2::write::{GzEncoder, ZlibEncoder};
use spyglass_data::nbt::{self, Compound, Tag};
use spyglass_world::{Layout, PackContents, Save, WorldError, region};

fn raw(root: &Tag) -> Vec<u8> {
    nbt::write("", root)
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut e = GzEncoder::new(Vec::new(), Compression::default());
    e.write_all(bytes).unwrap();
    e.finish().unwrap()
}

fn write(path: &Path, root: &Tag) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, nbt::write_compressed("", root)).unwrap();
}

/// A compound from `(name, tag)` pairs.
fn c<const N: usize>(entries: [(&str, Tag); N]) -> Tag {
    Tag::Compound(Compound::from(entries.map(|(k, v)| (k.to_string(), v))))
}

fn s(text: &str) -> Tag {
    Tag::String(text.into())
}

fn strings(items: &[&str]) -> Tag {
    Tag::List(items.iter().map(|i| s(i)).collect())
}

fn doubles(v: [f64; 3]) -> Tag {
    Tag::List(v.into_iter().map(Tag::Double).collect())
}

fn noise_overworld() -> Tag {
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

#[test]
fn current_layout() {
    let dir = tempfile::tempdir().unwrap();
    let uuid = [91_888_567, 345_196_078, -1_197_807_316, -1_277_292_734];
    write(
        &dir.path().join("level.dat"),
        &c([(
            "Data",
            c([
                ("LevelName", s("Seed Check")),
                ("DataVersion", Tag::Int(5023)),
                ("Version", c([("Name", s("26.3")), ("Id", Tag::Int(5023))])),
                ("WasModded", Tag::Byte(1)),
                (
                    "DataPacks",
                    c([
                        ("Enabled", strings(&["vanilla", "file/mine.zip"])),
                        ("Disabled", strings(&["trade_rebalance"])),
                    ]),
                ),
                (
                    "spawn",
                    c([
                        ("pos", Tag::IntArray(vec![-16, 61, 16])),
                        ("dimension", s("minecraft:overworld")),
                    ]),
                ),
                ("singleplayer_uuid", Tag::IntArray(uuid.to_vec())),
            ]),
        )]),
    );
    write(
        &dir.path().join("data/minecraft/world_gen_settings.dat"),
        &c([
            (
                "data",
                c([
                    ("seed", Tag::Long(-4_893_098_694_207_956_070)),
                    ("generate_structures", Tag::Byte(1)),
                    (
                        "dimensions",
                        c([(
                            "minecraft:overworld",
                            c([
                                ("type", s("minecraft:overworld")),
                                ("generator", noise_overworld()),
                            ]),
                        )]),
                    ),
                ]),
            ),
            ("DataVersion", Tag::Int(5023)),
        ]),
    );
    write(
        &dir.path()
            .join("players/data/057a1bb7-1493-462e-b89a-e92cb3de0f42.dat"),
        &c([
            ("Pos", doubles([587.5, 121.0, 2339.75])),
            ("Dimension", s("minecraft:overworld")),
        ]),
    );
    let save = Save::read(dir.path()).unwrap();
    assert_eq!(save.layout, Layout::Current);
    assert_eq!(save.name, "Seed Check");
    assert_eq!(save.data_version, Some(5023));
    assert_eq!(save.version_name.as_deref(), Some("26.3"));
    assert_eq!(save.seed, -4_893_098_694_207_956_070);
    assert!(save.generate_structures && save.was_modded);
    assert_eq!(
        save.generators["minecraft:overworld"],
        serde_json::json!({"type": "minecraft:noise", "settings": "minecraft:overworld",
            "biome_source": {"type": "minecraft:multi_noise", "preset": "minecraft:overworld"}})
    );
    assert_eq!(save.enabled_packs, ["vanilla", "file/mine.zip"]);
    assert_eq!(save.disabled_packs, ["trade_rebalance"]);
    assert_eq!(save.spawn.as_ref().unwrap().pos, [-16.0, 61.0, 16.0]);
    assert_eq!(save.player.as_ref().unwrap().pos, [587.5, 121.0, 2339.75]);
    assert_eq!(
        save.region_dir("minecraft:the_nether"),
        dir.path().join("dimensions/minecraft/the_nether/region")
    );
    assert_eq!(
        save.packs()[1].1,
        PackContents::Missing {
            path: dir.path().join("datapacks/mine.zip")
        }
    );
}

#[test]
fn legacy_layout() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("level.dat"),
        &c([(
            "Data",
            c([
                ("LevelName", s("Latest")),
                ("DataVersion", Tag::Int(4671)),
                ("SpawnX", Tag::Int(1)),
                ("SpawnY", Tag::Int(70)),
                ("SpawnZ", Tag::Int(-2)),
                (
                    "WorldGenSettings",
                    c([
                        ("seed", Tag::Long(7)),
                        ("generate_features", Tag::Byte(0)),
                        (
                            "dimensions",
                            c([("minecraft:overworld", c([("generator", noise_overworld())]))]),
                        ),
                    ]),
                ),
                (
                    "Player",
                    c([
                        ("Pos", doubles([1.0, 2.0, 3.0])),
                        ("Dimension", s("minecraft:the_nether")),
                    ]),
                ),
            ]),
        )]),
    );
    let save = Save::read(dir.path()).unwrap();
    assert_eq!(save.layout, Layout::Legacy);
    assert_eq!(save.seed, 7);
    assert!(!save.generate_structures);
    assert_eq!(save.version_name, None);
    assert_eq!(save.spawn.as_ref().unwrap().pos, [1.0, 70.0, -2.0]);
    assert_eq!(
        save.player.as_ref().unwrap().dimension,
        "minecraft:the_nether"
    );
    assert_eq!(
        save.region_dir("minecraft:the_end"),
        dir.path().join("DIM1/region")
    );
}

#[test]
fn before_1_16() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("level.dat"),
        &c([(
            "Data",
            c([
                ("RandomSeed", Tag::Long(-3)),
                ("generatorName", s("flat")),
                ("generatorOptions", s("3;minecraft:bedrock")),
                ("MapFeatures", Tag::Byte(0)),
                (
                    "Player",
                    c([
                        ("Pos", doubles([0.0, 4.0, 0.0])),
                        ("Dimension", Tag::Int(-1)),
                    ]),
                ),
            ]),
        )]),
    );
    let save = Save::read(dir.path()).unwrap();
    assert_eq!(save.layout, Layout::Before116);
    assert_eq!(save.data_version, None);
    assert_eq!(save.seed, -3);
    assert!(!save.generate_structures);
    assert_eq!(
        save.generators["minecraft:overworld"],
        serde_json::json!({"legacy_generator": {"name": "flat", "options": "3;minecraft:bedrock"}})
    );
    assert_eq!(
        save.player.as_ref().unwrap().dimension,
        "minecraft:the_nether"
    );
    assert_eq!(
        save.region_dir("minecraft:overworld"),
        dir.path().join("region")
    );
}

/// A pre-1.16 save in `dir` that enables `packs`.
fn save_with_packs(dir: &Path, packs: &[&str]) -> Save {
    write(
        &dir.join("level.dat"),
        &c([(
            "Data",
            c([
                ("RandomSeed", Tag::Long(1)),
                ("DataPacks", c([("Enabled", strings(packs))])),
            ]),
        )]),
    );
    Save::read(dir).unwrap()
}

fn touch(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, "{}").unwrap();
}

#[test]
fn data_pack_contents() {
    let dir = tempfile::tempdir().unwrap();
    let packs = dir.path().join("datapacks");
    touch(&packs.join("terrain/data/x/worldgen/noise/a.json"));
    touch(&packs.join("terrain/overlay/data/x/tags/worldgen/biome/b.json"));
    touch(&packs.join("terrain/data/x/function/c.mcfunction"));
    touch(&packs.join("loot/data/x/loot_table/d.json"));
    // A pack named outside datapacks/ isn't read, even when the path exists.
    touch(&dir.path().join("escape/data/x/worldgen/noise/e.json"));
    let save = save_with_packs(
        dir.path(),
        &[
            "vanilla",
            "file/terrain",
            "file/loot",
            "file/gone.zip",
            "file/../escape",
            "bundle",
        ],
    );
    let contents: Vec<PackContents> = save.packs().into_iter().map(|(_, c)| c).collect();
    assert_eq!(
        contents,
        [
            PackContents::Vanilla,
            PackContents::Files {
                path: packs.join("terrain"),
                worldgen_files: 2
            },
            PackContents::Files {
                path: packs.join("loot"),
                worldgen_files: 0
            },
            PackContents::Missing {
                path: packs.join("gone.zip")
            },
            PackContents::Missing {
                path: packs.join("../escape")
            },
            PackContents::Uninspectable,
        ]
    );
}

#[cfg(unix)]
#[test]
fn data_pack_symlink_cycles_end() {
    let dir = tempfile::tempdir().unwrap();
    let pack = dir.path().join("datapacks/loop");
    touch(&pack.join("data/x/worldgen/noise/a.json"));
    std::os::unix::fs::symlink(&pack, pack.join("data/x/again")).unwrap();
    let save = save_with_packs(dir.path(), &["file/loop"]);
    assert_eq!(
        save.packs()[0].1,
        PackContents::Files {
            path: pack,
            worldgen_files: 1
        }
    );
}

#[test]
fn errors_name_the_file_and_field() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(Save::read(dir.path()), Err(WorldError::Io { .. })));
    write(
        &dir.path().join("level.dat"),
        &c([("Data", c([("RandomSeed", s("x"))]))]),
    );
    match Save::read(dir.path()) {
        Err(WorldError::Field { field, .. }) => assert_eq!(field, "Data.RandomSeed"),
        other => panic!("{other:?}"),
    }
    std::fs::write(dir.path().join("level.dat"), b"not gzip").unwrap();
    assert!(matches!(
        Save::read(dir.path()),
        Err(WorldError::Nbt { .. })
    ));
}

#[test]
fn region_chunk_versions() {
    let dir = tempfile::tempdir().unwrap();
    let chunk = |version: Option<i32>| {
        let tag = match version {
            Some(v) => c([("DataVersion", Tag::Int(v)), ("Status", s("full"))]),
            None => c([("Level", c([]))]),
        };
        raw(&tag)
    };
    let zlib = |bytes: &[u8]| {
        let mut e = ZlibEncoder::new(Vec::new(), Compression::default());
        e.write_all(bytes).unwrap();
        e.finish().unwrap()
    };
    // (header index, compression byte, stored bytes).
    let chunks: Vec<(usize, u8, Vec<u8>)> = vec![
        (0, 2, zlib(&chunk(Some(5023)))),
        (1, 1, gzip(&chunk(Some(4671)))),
        (33, 3, chunk(None)),
        (40, 4, vec![1, 2, 3]),      // LZ4: unreadable
        (41, 2, vec![9, 9, 9]),      // corrupt zlib: unreadable
        (1023, 2 | 128, Vec::new()), // external .mcc
    ];
    let mut file = vec![0_u8; 8192];
    for (index, compression, data) in &chunks {
        let sector = file.len() / 4096;
        let mut body = ((data.len() + 1) as u32).to_be_bytes().to_vec();
        body.push(*compression);
        body.extend(data);
        let sectors = body.len().div_ceil(4096);
        body.resize(sectors * 4096, 0);
        file.extend(body);
        let entry = ((sector as u32) << 8) | sectors as u32;
        file[index * 4..index * 4 + 4].copy_from_slice(&entry.to_be_bytes());
    }
    let path = dir.path().join("r.-1.2.mca");
    std::fs::write(&path, &file).unwrap();
    // Index 1023 is local (31, 31): chunk (-32 + 31, 64 + 31).
    std::fs::write(dir.path().join("c.-1.95.mcc"), zlib(&chunk(Some(5023)))).unwrap();

    let counts = region::chunk_versions(dir.path()).unwrap();
    assert_eq!(counts.by_version.get(&5023), Some(&2));
    assert_eq!(counts.by_version.get(&4671), Some(&1));
    assert_eq!(counts.unversioned, 1);
    assert_eq!(counts.unreadable, 2);
    assert_eq!(counts.total(), 6);
    assert_eq!(counts.older_than(5023), 2);
    assert_eq!(region::region_files(dir.path()), [path]);
    assert_eq!(
        region::chunk_versions(&dir.path().join("none"))
            .unwrap()
            .total(),
        0
    );
}

#[test]
fn region_edge_cases_are_unreadable_not_errors() {
    let dir = tempfile::tempdir().unwrap();
    // Shorter than a header: no chunks.
    std::fs::write(dir.path().join("r.5.5.mca"), [0_u8; 100]).unwrap();
    assert_eq!(
        region::file_versions(&dir.path().join("r.5.5.mca"))
            .unwrap()
            .total(),
        0
    );

    let mut file = vec![0_u8; 8192];
    let mut entry = |index: usize, sector: u32, sectors: u32| {
        let e = (sector << 8) | sectors;
        file[index * 4..index * 4 + 4].copy_from_slice(&e.to_be_bytes());
    };
    entry(0, 2, 1); // length past its one sector
    entry(1, 3, 1); // length 0
    entry(2, 4, 1); // external, but no .mcc file
    entry(3, 5000, 1); // offset past the end of the file
    // A zlib bomb: valid NBT, a 40 MiB byte array of zeros, in about 40 KiB.
    let mut e = ZlibEncoder::new(Vec::new(), Compression::default());
    e.write_all(&[7, 0, 0]).unwrap();
    e.write_all(&(40_i32 << 20).to_be_bytes()).unwrap();
    e.write_all(&vec![0; 40 << 20]).unwrap();
    let bomb = e.finish().unwrap();
    let bomb_sectors = (bomb.len() + 5).div_ceil(4096);
    entry(4, 5, bomb_sectors as u32);
    let mut sector = |data: &[u8], sectors: usize| {
        let mut s = data.to_vec();
        s.resize(sectors * 4096, 0);
        file.extend(s);
    };
    sector(&[0, 0, 0x20, 0, 2], 1);
    sector(&[0, 0, 0, 0, 2], 1);
    sector(&[0, 0, 0, 1, 2 | 128], 1);
    let mut body = ((bomb.len() + 1) as u32).to_be_bytes().to_vec();
    body.push(2);
    body.extend(bomb);
    sector(&body, bomb_sectors);
    let path = dir.path().join("r.0.0.mca");
    std::fs::write(&path, &file).unwrap();

    let counts = region::file_versions(&path).unwrap();
    assert_eq!(counts.unreadable, 5);
    assert_eq!(counts.total(), 5);
}
