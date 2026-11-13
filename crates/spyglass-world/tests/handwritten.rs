//! Saves and region files built in the test: every layout, the single player, and each chunk
//! encoding. No game files (ADR 0008).

use std::io::Write;
use std::path::Path;

use flate2::Compression;
use flate2::write::{GzEncoder, ZlibEncoder};
use spyglass_data::nbt::{self, Compound, Tag};
use spyglass_world::chunk::{self, SavedPiece, SavedStack, SavedStart, SavedStarts};
use spyglass_world::region::{ChunkFields, ChunkRecord};
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
        save.region_dir("minecraft:the_nether").unwrap(),
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
        save.region_dir("minecraft:the_end").unwrap(),
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
        save.region_dir("minecraft:overworld").unwrap(),
        dir.path().join("region")
    );
    // A data pack's dimension is under dimensions/, and an id that could lead out isn't one.
    assert_eq!(
        save.region_dir("pack:deep/sky").unwrap(),
        dir.path().join("dimensions/pack/deep/sky/region")
    );
    for bad in ["x:../../..", "x:a/./b", "X:y", "x:", ":y"] {
        assert!(
            matches!(save.region_dir(bad), Err(WorldError::DimensionId(_))),
            "{bad}"
        );
    }
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

const MCMETA: &str = r#"{"pack": {"description": "", "min_format": 121, "max_format": 121}}"#;

#[test]
fn data_pack_contents() {
    let dir = tempfile::tempdir().unwrap();
    let packs = dir.path().join("datapacks");
    touch(&packs.join("terrain/data/x/worldgen/noise/a.json"));
    touch(&packs.join("terrain/overlay/data/x/tags/worldgen/biome/b.json"));
    touch(&packs.join("terrain/data/x/function/c.mcfunction"));
    std::fs::write(
        packs.join("terrain/pack.mcmeta"),
        r#"{"pack": {"description": "", "min_format": 121, "max_format": 121},
            "overlays": {"entries": [{"directory": "overlay", "min_format": 121, "max_format": 121}]}}"#,
    )
    .unwrap();
    touch(&packs.join("loot/data/x/loot_table/d.json"));
    std::fs::write(packs.join("loot/pack.mcmeta"), MCMETA).unwrap();
    touch(&packs.join("bare/data/x/worldgen/noise/a.json"));
    touch(&packs.join("notzip.jar"));
    touch(&packs.join("nometa/pack.mcmeta"));
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
            "file/bare",
            "file/notzip.jar",
            "file/nometa",
        ],
    );
    let contents: Vec<PackContents> = save.packs().into_iter().map(|(_, c)| c).collect();
    let PackContents::Files { pack } = &contents[1] else {
        panic!("{:?}", contents[1]);
    };
    assert_eq!((pack.name(), pack.worldgen_files()), ("file/terrain", 2));
    let PackContents::Files { pack } = &contents[2] else {
        panic!("{:?}", contents[2]);
    };
    assert_eq!((pack.name(), pack.worldgen_files()), ("file/loot", 0));
    assert_eq!(contents[0], PackContents::Vanilla);
    assert_eq!(
        contents[3..6],
        [
            PackContents::Missing {
                path: packs.join("gone.zip")
            },
            PackContents::Missing {
                path: packs.join("../escape")
            },
            PackContents::Other,
        ]
    );
    for skipped in &contents[6..] {
        assert!(
            matches!(skipped, PackContents::Skipped { .. }),
            "{skipped:?}"
        );
    }
}

#[cfg(unix)]
#[test]
fn data_pack_symlink_cycles_end() {
    let dir = tempfile::tempdir().unwrap();
    let pack = dir.path().join("datapacks/loop");
    touch(&pack.join("data/x/worldgen/noise/a.json"));
    std::fs::write(pack.join("pack.mcmeta"), MCMETA).unwrap();
    std::os::unix::fs::symlink(&pack, pack.join("data/x/again")).unwrap();
    let save = save_with_packs(dir.path(), &["file/loop"]);
    let PackContents::Files { pack } = &save.packs()[0].1 else {
        panic!("not read");
    };
    assert_eq!(pack.worldgen_files(), 1);
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

    // The index keeps each chunk, by position: region (-1, 2) starts at chunk (-32, 64).
    let index = region::index(dir.path()).unwrap();
    assert_eq!(index.versions(), counts);
    let full = |version| {
        ChunkRecord::Read(ChunkFields {
            data_version: Some(version),
            status: Some("full".to_string()),
            inhabited_time: None,
        })
    };
    assert_eq!(index.chunk([-32, 64]), Some(&full(5023)));
    assert_eq!(index.chunk([-31, 64]), Some(&full(4671)));
    assert_eq!(
        index.chunk([-31, 65]),
        Some(&ChunkRecord::Read(ChunkFields::default()))
    );
    assert_eq!(index.chunk([-24, 65]), Some(&ChunkRecord::Unreadable));
    assert_eq!(index.chunk([-1, 95]), Some(&full(5023)));
    assert_eq!(index.chunk([-30, 64]), None);
    assert_eq!(index.chunk([0, 0]), None);
    assert_eq!(index.chunks().count(), 6);
    assert!(matches!(index.chunk([-32, 64]), Some(ChunkRecord::Read(f)) if f.is_full()));
    // A chunk can be read again, whole.
    assert!(index.region_of([-32, 64]).is_some());
    let raw = region::read_chunk(dir.path(), [-32, 64]).unwrap();
    assert_eq!(
        nbt::read(&raw).unwrap().1.get("DataVersion"),
        Some(&Tag::Int(5023))
    );
    assert_eq!(region::read_chunk(dir.path(), [-30, 64]), None);
    assert_eq!(region::read_chunk(dir.path(), [-24, 65]), None);
    assert_eq!(region::read_chunk(dir.path(), [0, 0]), None);

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

/// Uncompressed chunk NBT of exactly `size` bytes, with data version `version`.
fn chunk_of_size(version: i32, size: usize) -> Vec<u8> {
    let padding = |n: usize| {
        nbt::write(
            "",
            &Tag::Compound(Compound::from([
                ("DataVersion".to_string(), Tag::Int(version)),
                ("p".to_string(), Tag::ByteArray(vec![0; n])),
            ])),
        )
    };
    let bytes = padding(size - padding(0).len());
    assert_eq!(bytes.len(), size);
    bytes
}

#[test]
fn region_chunks_are_cut_off_where_the_games_are() {
    // RegionFile reads a chunk's sectors, and its data (after the 4-byte length and the
    // compression byte) must fit in what's left: 4,091 bytes in one sector, not 4,092, even when
    // the file goes on.
    let dir = tempfile::tempdir().unwrap();
    let mut file = vec![0_u8; 8192];
    for (index, sector) in [(0, 2_u32), (1, 4), (2, 7)] {
        file[index * 4..index * 4 + 4].copy_from_slice(&((sector << 8) | 1).to_be_bytes());
    }
    for (version, size) in [(100, 4091), (200, 4092)] {
        let nbt = chunk_of_size(version, size);
        file.extend(((nbt.len() + 1) as u32).to_be_bytes());
        file.push(3);
        file.extend(&nbt);
        file.resize(file.len().next_multiple_of(4096) + 4096, 0);
    }
    // An external chunk with a length of 0: missing, though its .mcc file is there.
    file.extend([0, 0, 0, 0, 3 | 128]);
    file.resize(file.len().next_multiple_of(4096), 0);
    let path = dir.path().join("r.0.0.mca");
    std::fs::write(&path, &file).unwrap();
    std::fs::write(dir.path().join("c.2.0.mcc"), chunk_of_size(300, 100)).unwrap();

    let counts = region::file_versions(&path).unwrap();
    assert_eq!(
        counts.by_version.into_iter().collect::<Vec<_>>(),
        [(100, 1)]
    );
    assert_eq!(counts.unreadable, 2);
}

#[test]
fn region_files_that_cant_be_read_are_counted_not_errors() {
    let dir = tempfile::tempdir().unwrap();
    // A folder named like a region file can't be read as one, on any platform.
    std::fs::create_dir(dir.path().join("r.0.0.mca")).unwrap();
    let counts = region::chunk_versions(dir.path()).unwrap();
    assert_eq!(counts.unreadable_files, 1);
    assert_eq!(counts.total(), 0);
}

#[test]
fn chunks_give_their_starts_and_containers() {
    let piece = |id: &str, bb: [i32; 6]| c([("id", s(id)), ("BB", Tag::IntArray(bb.to_vec()))]);
    let start = |id: &str, x: i32, z: i32, references: i32| {
        c([
            ("id", s(id)),
            ("ChunkX", Tag::Int(x)),
            ("ChunkZ", Tag::Int(z)),
            ("references", Tag::Int(references)),
            (
                "Children",
                Tag::List(vec![piece("minecraft:jigsaw", [0, 60, 0, 15, 70, 15])]),
            ),
        ])
    };
    let item = |slot: i8, id: &str, count: i32| {
        c([
            ("Slot", Tag::Byte(slot)),
            ("id", s(id)),
            ("count", Tag::Int(count)),
        ])
    };
    let chunk = c([
        ("DataVersion", Tag::Int(4671)),
        (
            "structures",
            c([(
                "starts",
                c([
                    (
                        "minecraft:village_plains",
                        start("minecraft:village_plains", 3, -2, 1),
                    ),
                    ("minecraft:mineshaft", c([("id", s("INVALID"))])),
                ]),
            )]),
        ),
        (
            "block_entities",
            Tag::List(vec![
                // Unopened: its table, and a seed.
                c([
                    ("id", s("minecraft:chest")),
                    ("x", Tag::Int(50)),
                    ("y", Tag::Int(64)),
                    ("z", Tag::Int(-30)),
                    (
                        "LootTable",
                        s("minecraft:chests/village/village_plains_house"),
                    ),
                    ("LootTableSeed", Tag::Long(-7)),
                ]),
                // Opened: its items, no table.
                c([
                    ("id", s("minecraft:barrel")),
                    ("x", Tag::Int(51)),
                    ("y", Tag::Int(64)),
                    ("z", Tag::Int(-30)),
                    ("Items", Tag::List(vec![item(4, "minecraft:bread", 3)])),
                ]),
                // A seed of 0 is drawn when filled, so it isn't one.
                c([
                    ("id", s("minecraft:chest")),
                    ("x", Tag::Int(52)),
                    ("y", Tag::Int(64)),
                    ("z", Tag::Int(-30)),
                    ("LootTable", s("minecraft:chests/simple_dungeon")),
                    ("LootTableSeed", Tag::Long(0)),
                ]),
                // No inventory, but still the block entity at its position.
                c([
                    ("id", s("minecraft:sign")),
                    ("x", Tag::Int(53)),
                    ("y", Tag::Int(64)),
                    ("z", Tag::Int(-30)),
                ]),
            ]),
        ),
    ]);
    let contents = chunk::read(&raw(&chunk)).unwrap();
    assert_eq!(contents.data_version, Some(4671));
    assert_eq!(contents.status, None);
    assert!(!contents.has_features());
    for (status, placed) in [
        ("minecraft:full", true),
        ("features", true),
        ("minecraft:light", true),
        ("minecraft:carvers", false),
        ("minecraft:structure_starts", false),
    ] {
        let tag = c([("Status", s(status))]);
        assert_eq!(
            chunk::read(&raw(&tag)).unwrap().has_features(),
            placed,
            "{status}"
        );
    }
    assert_eq!(
        contents.starts,
        SavedStarts::Starts(vec![SavedStart {
            structure: "minecraft:village_plains".to_string(),
            chunk: [3, -2],
            references: 1,
            pieces: vec![SavedPiece {
                id: "minecraft:jigsaw".to_string(),
                bounds: [0, 60, 0, 15, 70, 15],
            }],
        }])
    );
    let containers = contents.containers.unwrap();
    // Every block entity, the sign included.
    assert_eq!(containers.len(), 4);
    assert_eq!(containers[3].id, "minecraft:sign");
    assert!(containers[3].items.is_empty() && containers[3].loot_table.is_none());
    assert_eq!(containers[0].position, [50, 64, -30]);
    assert_eq!(containers[0].loot_table_seed, Some(-7));
    assert!(containers[0].items.is_empty());
    assert_eq!(containers[1].loot_table, None);
    assert_eq!(
        containers[1].items,
        [SavedStack {
            slot: 4,
            id: "minecraft:bread".to_string(),
            count: 3,
            components: None,
        }]
    );
    assert_eq!(containers[2].loot_table_seed, None);

    // No starts compound: predicted. An empty one: none here.
    let bare = chunk::read(&raw(&c([("DataVersion", Tag::Int(4671))]))).unwrap();
    assert_eq!(bare.starts, SavedStarts::Missing);
    assert_eq!(bare.containers, Some(Vec::new()));
    let empty = c([("structures", c([("starts", c([]))]))]);
    assert_eq!(
        chunk::read(&raw(&empty)).unwrap().starts,
        SavedStarts::Starts(Vec::new())
    );
    // Before 1.18, everything is under Level.
    let legacy = chunk::read(&raw(&c([("Level", c([]))]))).unwrap();
    assert_eq!(legacy.starts, SavedStarts::Legacy);
    assert_eq!(legacy.containers, None);
    assert!(chunk::read(b"not nbt").is_err());
}

#[test]
fn entity_chunks_give_their_containers() {
    let pos = Tag::List(vec![Tag::Double(-0.5), Tag::Double(40.0), Tag::Double(7.9)]);
    let entities = c([(
        "Entities",
        Tag::List(vec![
            c([
                ("id", s("minecraft:chest_minecart")),
                ("Pos", pos.clone()),
                ("LootTable", s("minecraft:chests/abandoned_mineshaft")),
                ("LootTableSeed", Tag::Long(12)),
            ]),
            c([("id", s("minecraft:zombie")), ("Pos", pos)]),
        ]),
    )]);
    // A decorated pot holds one stack, as `item`.
    let pot = c([(
        "block_entities",
        Tag::List(vec![c([
            ("id", s("minecraft:decorated_pot")),
            ("x", Tag::Int(1)),
            ("y", Tag::Int(2)),
            ("z", Tag::Int(3)),
            (
                "item",
                c([("id", s("minecraft:emerald")), ("count", Tag::Int(2))]),
            ),
        ])]),
    )]);
    let pots = chunk::read(&raw(&pot)).unwrap().containers.unwrap();
    assert_eq!(pots[0].items[0].id, "minecraft:emerald");
    assert_eq!(pots[0].items[0].count, 2);
    let found = chunk::read_entities(&raw(&entities)).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, "minecraft:chest_minecart");
    assert_eq!(found[0].position, [-1, 40, 7]);
    assert_eq!(found[0].loot_table_seed, Some(12));
}

#[test]
fn entities_are_beside_regions() {
    let dir = tempfile::tempdir().unwrap();
    let save = save_with_packs(dir.path(), &[]);
    for dimension in ["minecraft:overworld", "minecraft:the_nether"] {
        let region = save.region_dir(dimension).unwrap();
        assert_eq!(
            save.entities_dir(dimension).unwrap(),
            region.with_file_name("entities")
        );
    }
}
