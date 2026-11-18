//! `SaveReport::portals` and `Game::save_portal_links` on a save built in the test, against
//! the fixture bundle: portal records read from `poi/` in both dimensions, a border, and the
//! flags (`docs/specs/portals.md#a-saves-portals`). No game data (ADR 0008).

use std::path::Path;

use spyglass::{Border, Dimension, Game, PortalFlag, PortalNote, Scan};
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

/// A current-layout save with the fixture's data version and Default generators.
fn save(dir: &Path) {
    write(
        &dir.join("level.dat"),
        &c([(
            "Data",
            c([
                ("LevelName", s("Portals")),
                ("DataVersion", Tag::Int(5023)),
                ("Version", c([("Name", s("test")), ("Id", Tag::Int(1))])),
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
    let nether = c([
        ("type", s("minecraft:noise")),
        ("settings", s("minecraft:nether")),
        (
            "biome_source",
            c([
                ("type", s("minecraft:multi_noise")),
                ("preset", s("minecraft:nether")),
            ]),
        ),
    ]);
    write(
        &dir.join("data/minecraft/world_gen_settings.dat"),
        &c([
            (
                "data",
                c([
                    ("seed", Tag::Long(42)),
                    ("generate_structures", Tag::Byte(1)),
                    (
                        "dimensions",
                        c([
                            (
                                "minecraft:overworld",
                                c([("type", s("minecraft:overworld")), ("generator", overworld)]),
                            ),
                            (
                                "minecraft:the_nether",
                                c([("type", s("minecraft:the_nether")), ("generator", nether)]),
                            ),
                        ]),
                    ),
                ]),
            ),
            ("DataVersion", Tag::Int(5023)),
        ]),
    );
}

/// The blocks of a 2 × 3 portal along x from `corner`.
fn portal([x, y, z]: [i32; 3]) -> Vec<[i32; 3]> {
    (0..3)
        .flat_map(|up| (0..2).map(move |along| [x + along, y + up, z]))
        .collect()
}

/// Writes `blocks` as portal records into `poi/` beside dimension `path`'s region folder,
/// one region file per region, each chunk's records in its sections, valid as `valid` says.
fn write_poi(save: &Path, path: &str, blocks: &[[i32; 3]], valid: bool) {
    use std::collections::BTreeMap;
    let dir = save.join("dimensions/minecraft").join(path).join("poi");
    std::fs::create_dir_all(&dir).unwrap();
    // Records by chunk, then by section.
    let mut chunks: BTreeMap<[i32; 2], BTreeMap<i32, Vec<Tag>>> = BTreeMap::new();
    for &[x, y, z] in blocks {
        chunks
            .entry([x >> 4, z >> 4])
            .or_default()
            .entry(y >> 4)
            .or_default()
            .push(c([
                ("pos", Tag::IntArray(vec![x, y, z])),
                ("type", s("minecraft:nether_portal")),
                ("free_tickets", Tag::Int(0)),
            ]));
    }
    // Each region's chunks: position and NBT.
    type Chunks = Vec<([i32; 2], Vec<u8>)>;
    let mut regions: BTreeMap<[i32; 2], Chunks> = BTreeMap::new();
    for (pos @ [cx, cz], sections) in chunks {
        let sections: Compound = sections
            .into_iter()
            .map(|(y, records)| {
                let section = c([
                    ("Valid", Tag::Byte(i8::from(valid))),
                    ("Records", Tag::List(records)),
                ]);
                (y.to_string(), section)
            })
            .collect();
        let raw = nbt::write(
            "",
            &c([
                ("DataVersion", Tag::Int(5023)),
                ("Sections", Tag::Compound(sections)),
            ]),
        );
        regions
            .entry([cx >> 5, cz >> 5])
            .or_default()
            .push((pos, raw));
    }
    for ([rx, rz], chunks) in regions {
        let mut file = vec![0u8; 8192];
        for ([cx, cz], raw) in chunks {
            let index = usize::try_from((cx & 31) + (cz & 31) * 32).unwrap();
            let sector = u32::try_from(file.len() / 4096).unwrap();
            let mut payload = (u32::try_from(raw.len()).unwrap() + 1)
                .to_be_bytes()
                .to_vec();
            // Uncompressed.
            payload.push(3);
            payload.extend(&raw);
            payload.resize(payload.len().next_multiple_of(4096), 0);
            let sectors = u32::try_from(payload.len() / 4096).unwrap();
            file[index * 4..index * 4 + 4]
                .copy_from_slice(&((sector << 8) | sectors).to_be_bytes());
            file.extend(payload);
        }
        std::fs::write(dir.join(format!("r.{rx}.{rz}.mca")), file).unwrap();
    }
}

#[test]
fn a_save_s_portals_link_and_a_mislinked_one_is_flagged() {
    let bundle = fixtures::overworld_and_nether();
    let dir = tempfile::tempdir().unwrap();
    save(dir.path());
    // A, in the Overworld, and B, in the Nether, lead to each other. C, 7 blocks from B,
    // leads to A, but A's trips land at B. D, far from everything, leads nowhere.
    let overworld = [portal([800, 64, 800]), portal([-5000, 70, 300])];
    write_poi(dir.path(), "overworld", &overworld.concat(), true);
    let nether = [portal([100, 64, 100]), portal([99, 64, 107])];
    write_poi(dir.path(), "the_nether", &nether.concat(), true);
    // A Nether border 1000 across, centred on 0, 0: every Nether portal and exit is inside.
    write(
        &dir.path()
            .join("dimensions/minecraft/the_nether/data/minecraft/world_border.dat"),
        &c([(
            "data",
            c([
                ("center_x", Tag::Double(0.0)),
                ("center_z", Tag::Double(0.0)),
                ("size", Tag::Double(1000.0)),
                ("lerp_time", Tag::Long(0)),
                ("lerp_target", Tag::Double(0.0)),
                ("damage_per_block", Tag::Double(0.2)),
                ("safe_zone", Tag::Double(5.0)),
                ("warning_blocks", Tag::Int(5)),
                ("warning_time", Tag::Int(15)),
            ]),
        )]),
    );

    let report = spyglass::inspect_save(dir.path(), bundle.bundles(), Scan::Quick).unwrap();
    let game: Game = report.open_game().unwrap();
    let portals = report.portals(&game).unwrap();
    assert_eq!(portals.notes, Vec::<PortalNote>::new());
    assert_eq!(
        portals.borders[Dimension::Nether.index()],
        Border {
            size: 1000.0,
            ..Border::default()
        }
    );
    // The Overworld's by lowest block, then the Nether's: A (y 64), D (y 70), B, C.
    let corners: Vec<(Dimension, [i32; 3])> = portals
        .portals
        .iter()
        .map(|p| (p.dimension, p.corner()))
        .collect();
    assert_eq!(
        corners,
        [
            (Dimension::Overworld, [800, 64, 800]),
            (Dimension::Overworld, [-5000, 70, 300]),
            (Dimension::Nether, [100, 64, 100]),
            (Dimension::Nether, [99, 64, 107]),
        ]
    );

    let links = game.save_portal_links(&portals).unwrap();
    let flags: Vec<Vec<PortalFlag>> = links.iter().map(|l| l.flags.clone()).collect();
    assert_eq!(
        flags,
        [
            vec![],
            vec![PortalFlag::Unlinked],
            vec![],
            vec![PortalFlag::Mislinked {
                to: 0,
                back: vec![2]
            }],
        ]
    );
    assert_eq!(links[0].outcomes[0].portals, [2]);
    assert_eq!(links[2].outcomes[0].portals, [0]);
}

#[test]
fn old_chunks_and_invalid_sections_are_noted() {
    let bundle = fixtures::overworld_and_nether();
    let dir = tempfile::tempdir().unwrap();
    save(dir.path());
    write_poi(dir.path(), "overworld", &portal([0, 64, 0]), false);
    let report = spyglass::inspect_save(dir.path(), bundle.bundles(), Scan::Quick).unwrap();
    let game = report.open_game().unwrap();
    let portals = report.portals(&game).unwrap();
    assert_eq!(
        portals.notes,
        [PortalNote::InvalidSections {
            dimension: Dimension::Overworld,
            sections: 1
        }]
    );
    // The records are still used.
    assert_eq!(portals.portals.len(), 1);
}

/// The real saves on this machine with a bundle: every portal grouped and linked, with the
/// flags printed. A check on files the game wrote, ignored by default. Run with
/// `cargo test --release -p spyglass --test save_portals -- --ignored --nocapture`.
#[test]
#[ignore = "reads the saves on this machine and needs bundles/"]
fn every_local_save_s_portals_link() {
    let bundles = spyglass_oracle::bundle_dir()
        .parent()
        .unwrap()
        .to_path_buf();
    let inspector = spyglass::SaveInspector::new(&bundles).unwrap();
    for found in spyglass::launcher_saves() {
        let Ok(report) = inspector.inspect(&found.dir, Scan::Quick) else {
            continue;
        };
        let Ok(game) = report.open_game() else {
            continue;
        };
        let start = std::time::Instant::now();
        let portals = report.portals(&game).unwrap();
        let links = game.save_portal_links(&portals).unwrap();
        let mut counts = std::collections::BTreeMap::<&str, usize>::new();
        for link in &links {
            for flag in &link.flags {
                *counts.entry(flag.kind()).or_default() += 1;
            }
        }
        println!(
            "{} ({}): {} portals, flags {counts:?}, {:.2?}",
            found.dir.display(),
            game.version(),
            portals.portals.len(),
            start.elapsed()
        );
        for note in &portals.notes {
            println!("  note: {note}");
        }
        let all = std::env::var_os("SPYGLASS_ALL_PORTALS").is_some();
        for link in links.iter().filter(|l| all || !l.flags.is_empty()) {
            let portal = &portals.portals[link.portal];
            println!(
                "  {} {:?}: {:?} -> {:?}",
                portal.dimension,
                portal.corner(),
                link.flags,
                link.outcomes
            );
        }
    }
}
