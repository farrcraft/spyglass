//! Points of interest (`poi/r.<x>.<z>.mca`): the Nether portal records the game's portal search
//! reads (`PoiManager`, `SectionStorage`). See `docs/specs/world-saves.md#points-of-interest`
//! and `docs/specs/portals.md#points-of-interest`.
//!
//! Each chunk is an NBT compound with `DataVersion` and `Sections`, keyed by section y as a
//! decimal string. Each section has `Valid` and `Records`, a list of `{pos, type,
//! free_tickets}`. They're read as the game's codecs read them: leniently where the codec is
//! lenient, and a record the codec rejects is skipped while the rest of its section is kept.

use std::collections::BTreeMap;
use std::ops::RangeInclusive;
use std::path::Path;

use spyglass_data::nbt::{self, Tag};

use crate::region;

/// The most memory one chunk's tree may take, as for chunks: a chunk is at most 32 MiB
/// uncompressed (`region`'s limit), and a tree takes several times its bytes.
const MAX_CHUNK_TREE: u64 = 256 << 20;

/// The schema of `ForcePoiRebuild` (1.16): the game rebuilds every section of a chunk saved
/// before it from the chunk's blocks. A fixed historical version, the same for every game
/// version.
const FORCE_REBUILD: i32 = 2218;

/// The data version the game assumes for a chunk without one
/// (`SectionStorage.PackedChunk.parse`).
const UNVERSIONED: i32 = 1945;

/// The point-of-interest type of a Nether portal block.
const NETHER_PORTAL: &str = "minecraft:nether_portal";

/// One section's records, as saved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoiSection {
    /// The section's y, in sections (blocks `16 · y` to `16 · y + 15`).
    pub y: i32,
    /// `Valid`: whether the game uses the records as saved. A section that isn't valid is
    /// rebuilt from the chunk's blocks when the game loads the chunk.
    pub valid: bool,
    /// The positions of its `minecraft:nether_portal` records, in the order saved.
    pub portals: Vec<[i32; 3]>,
}

/// One chunk's sections, as saved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoiChunk {
    /// `DataVersion`: the version that last saved it.
    pub data_version: Option<i32>,
    /// The sections it has that the game reads, lowest first. A section the game wouldn't read
    /// (outside the dimension's height, or not a compound with a `Records` list) is left out:
    /// it counts as missing.
    pub sections: Vec<PoiSection>,
}

impl PoiChunk {
    /// Section `y`, if the chunk has it.
    pub fn section(&self, y: i32) -> Option<&PoiSection> {
        self.sections.iter().find(|s| s.y == y)
    }
}

/// Every chunk in one dimension's `poi/` folder.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PoiIndex {
    /// Each readable chunk, by chunk position.
    pub chunks: BTreeMap<[i32; 2], PoiChunk>,
    /// Chunks a region file lists that couldn't be read: their sections count as missing.
    pub unreadable_chunks: usize,
    /// Region files that couldn't be opened or read, or whose names don't parse.
    pub unreadable_files: usize,
}

impl PoiIndex {
    /// Every Nether portal record, with whether its section is valid, chunk by chunk.
    pub fn portals(&self) -> impl Iterator<Item = ([i32; 3], bool)> + '_ {
        self.chunks.values().flat_map(|chunk| {
            chunk
                .sections
                .iter()
                .flat_map(|s| s.portals.iter().map(move |&pos| (pos, s.valid)))
        })
    }
}

/// Reads the `poi/` folder `dir` of a dimension whose sections are `sections` (in section y;
/// the Overworld's are −4 to 19). A folder that doesn't exist has no chunks; a file or chunk
/// that can't be read is counted, not an error.
pub fn read(dir: &Path, sections: RangeInclusive<i32>) -> PoiIndex {
    let mut index = PoiIndex::default();
    for path in region::region_files(dir) {
        let mut chunks = Vec::new();
        let read = region::visit_chunks(&path, |at, raw| chunks.push((at, raw)));
        if read.is_err() || region::chunk_pos_in(&path, 0).is_none() {
            index.unreadable_files += 1;
            continue;
        }
        for (at, raw) in chunks {
            let pos = region::chunk_pos_in(&path, at).expect("the name parses");
            match raw.and_then(|raw| read_chunk(&raw, sections.clone())) {
                Some(chunk) => {
                    index.chunks.insert(pos, chunk);
                }
                None => index.unreadable_chunks += 1,
            }
        }
    }
    index
}

/// One chunk's uncompressed NBT, as `SectionStorage.PackedChunk.parse` reads it: each section y
/// in `sections` looked up by its decimal key, and each section read with `PoiSection`'s
/// codec. A chunk saved before 1.16 (data version 2218, or none) has every section rebuilt
/// by the game, so none is valid. `None` if the NBT is malformed or its root isn't a compound.
pub fn read_chunk(raw: &[u8], sections: RangeInclusive<i32>) -> Option<PoiChunk> {
    let (_, root) = nbt::read_limited(raw, MAX_CHUNK_TREE).ok()?;
    let root = root.as_compound()?;
    let data_version = root.get("DataVersion").and_then(Tag::as_int);
    let saved = root.get("Sections").and_then(Tag::as_compound);
    let mut sections: Vec<PoiSection> = saved
        .map(|saved| {
            sections
                .filter_map(|y| read_section(y, saved.get(&y.to_string())?))
                .collect()
        })
        .unwrap_or_default();
    // ForcePoiRebuild: a chunk this old is rebuilt whatever its sections say.
    if data_version.unwrap_or(UNVERSIONED) < FORCE_REBUILD {
        for section in &mut sections {
            section.valid = false;
        }
    }
    Some(PoiChunk {
        data_version,
        sections,
    })
}

/// A section, by `PoiSection.Packed`'s codec: `Valid` a lenient boolean, false when missing or
/// not a number; `Records` a required list. `None` if it's not a compound or has no list.
fn read_section(y: i32, tag: &Tag) -> Option<PoiSection> {
    let section = tag.as_compound()?;
    let records = section.get("Records")?.as_list()?;
    let valid = section
        .get("Valid")
        .and_then(byte_value)
        .is_some_and(|b| b != 0);
    let portals = records
        .iter()
        .filter_map(read_record)
        .filter(|(_, kind)| kind == NETHER_PORTAL)
        .map(|(pos, _)| pos)
        .collect();
    Some(PoiSection { y, valid, portals })
}

/// A record, by `PoiRecord.Packed`'s codec: its position and type, the type with the
/// `minecraft` namespace when it has none. `None` if the codec rejects it: `pos` not three
/// numbers, `type` not a string, or `free_tickets` present but not a number. Whether the type
/// is registered isn't checked, since only the portal's is kept.
fn read_record(tag: &Tag) -> Option<([i32; 3], String)> {
    let record = tag.as_compound()?;
    let pos = int_stream(record.get("pos")?)?;
    let pos: [i32; 3] = pos.try_into().ok()?;
    let kind = record.get("type")?.as_str()?;
    if record
        .get("free_tickets")
        .is_some_and(|t| byte_value(t).is_none())
    {
        return None;
    }
    let kind = if kind.contains(':') {
        kind.to_string()
    } else {
        format!("minecraft:{kind}")
    };
    Some((pos, kind))
}

/// `NbtOps.getIntStream`: an int array as it is, or a list or array of numbers by each one's
/// `intValue()`.
fn int_stream(tag: &Tag) -> Option<Vec<i32>> {
    match tag {
        Tag::IntArray(ints) => Some(ints.clone()),
        Tag::ByteArray(bytes) => Some(bytes.iter().map(|&b| i32::from(b)).collect()),
        Tag::LongArray(longs) => Some(longs.iter().map(|&l| java_int(l)).collect()),
        Tag::List(items) => items.iter().map(int_value).collect(),
        _ => None,
    }
}

/// A number's `intValue()`: longs narrowed, floats and doubles cast as Java casts them
/// (towards zero, saturating, NaN as 0). `None` if it isn't a number.
fn int_value(tag: &Tag) -> Option<i32> {
    match *tag {
        Tag::Long(v) => Some(java_int(v)),
        #[expect(clippy::cast_possible_truncation, reason = "Java's (int) cast")]
        Tag::Float(v) => Some(v as i32),
        #[expect(clippy::cast_possible_truncation, reason = "Java's (int) cast")]
        Tag::Double(v) => Some(v as i32),
        _ => tag.as_int(),
    }
}

/// A number's `byteValue()`, as `NbtOps.getBooleanValue` and `getNumberValue` read it: `None` if
/// it isn't a number.
fn byte_value(tag: &Tag) -> Option<i8> {
    #[expect(clippy::cast_possible_truncation, reason = "Java's (byte) narrowing")]
    int_value(tag).map(|v| v as i8)
}

/// A long narrowed to an int, as Java's `(int)` does: its low 32 bits.
fn java_int(value: i64) -> i32 {
    #[expect(clippy::cast_possible_truncation, reason = "Java's (int) narrowing")]
    let low = value as i32;
    low
}

#[cfg(test)]
mod tests {
    use super::*;
    use spyglass_data::nbt::Compound;

    fn compound(fields: Vec<(&str, Tag)>) -> Tag {
        Tag::Compound(
            fields
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect::<Compound>(),
        )
    }

    fn record(pos: Tag, kind: &str) -> Tag {
        compound(vec![
            ("pos", pos),
            ("type", Tag::String(kind.into())),
            ("free_tickets", Tag::Int(0)),
        ])
    }

    fn chunk(sections: Vec<(&str, Tag)>) -> Vec<u8> {
        nbt::write(
            "",
            &compound(vec![
                ("DataVersion", Tag::Int(4671)),
                ("Sections", compound(sections)),
            ]),
        )
    }

    fn section(valid: Tag, records: Vec<Tag>) -> Tag {
        compound(vec![("Valid", valid), ("Records", Tag::List(records))])
    }

    #[test]
    fn portal_records_are_read_by_section() {
        let raw = chunk(vec![
            (
                "4",
                section(
                    Tag::Byte(1),
                    vec![
                        record(Tag::IntArray(vec![1, 70, 2]), "minecraft:nether_portal"),
                        record(Tag::IntArray(vec![1, 71, 2]), "nether_portal"),
                        record(Tag::IntArray(vec![5, 70, 5]), "minecraft:home"),
                    ],
                ),
            ),
            (
                "-1",
                section(
                    Tag::Byte(0),
                    vec![record(
                        Tag::IntArray(vec![0, -10, 0]),
                        "minecraft:nether_portal",
                    )],
                ),
            ),
        ]);
        let chunk = read_chunk(&raw, -4..=19).unwrap();
        assert_eq!(chunk.data_version, Some(4671));
        assert_eq!(
            chunk.sections,
            vec![
                PoiSection {
                    y: -1,
                    valid: false,
                    portals: vec![[0, -10, 0]],
                },
                PoiSection {
                    y: 4,
                    valid: true,
                    portals: vec![[1, 70, 2], [1, 71, 2]],
                },
            ]
        );
    }

    #[test]
    fn sections_the_game_wouldn_t_read_are_missing() {
        let portal = || record(Tag::IntArray(vec![0, 0, 0]), "minecraft:nether_portal");
        let raw = chunk(vec![
            // Outside the dimension's height.
            ("20", section(Tag::Byte(1), vec![portal()])),
            // Not the decimal key the game looks up.
            ("+3", section(Tag::Byte(1), vec![portal()])),
            // No records list.
            ("2", compound(vec![("Valid", Tag::Byte(1))])),
            ("1", Tag::Int(3)),
        ]);
        assert_eq!(read_chunk(&raw, -4..=19).unwrap().sections, vec![]);
    }

    #[test]
    fn valid_is_lenient_and_bad_records_are_skipped() {
        let raw = chunk(vec![(
            "0",
            compound(vec![
                ("Valid", Tag::String("yes".into())),
                (
                    "Records",
                    Tag::List(vec![
                        record(Tag::IntArray(vec![1, 2]), "minecraft:nether_portal"),
                        record(
                            Tag::List(vec![Tag::Long(1 << 32 | 3), Tag::Long(4), Tag::Long(-1)]),
                            "minecraft:nether_portal",
                        ),
                        compound(vec![
                            ("pos", Tag::IntArray(vec![7, 7, 7])),
                            ("type", Tag::String("minecraft:nether_portal".into())),
                            ("free_tickets", Tag::String("1".into())),
                        ]),
                        compound(vec![
                            ("pos", Tag::IntArray(vec![8, 8, 8])),
                            ("type", Tag::String("minecraft:nether_portal".into())),
                        ]),
                    ]),
                ),
            ]),
        )]);
        let chunk = read_chunk(&raw, -4..=19).unwrap();
        assert_eq!(
            chunk.sections,
            vec![PoiSection {
                y: 0,
                valid: false,
                portals: vec![[3, 4, -1], [8, 8, 8]],
            }]
        );
        // Any number other than 0 is true, by its low byte.
        let raw = chunk_with_valid(Tag::Int(2));
        assert!(read_chunk(&raw, 0..=0).unwrap().sections[0].valid);
        let raw = chunk_with_valid(Tag::Int(256));
        assert!(!read_chunk(&raw, 0..=0).unwrap().sections[0].valid);
    }

    #[test]
    fn a_chunk_saved_before_1_16_is_rebuilt() {
        let portal = record(Tag::IntArray(vec![0, 0, 0]), "minecraft:nether_portal");
        for version in [None, Some(2217)] {
            let mut root = vec![(
                "Sections",
                compound(vec![("0", section(Tag::Byte(1), vec![portal.clone()]))]),
            )];
            if let Some(v) = version {
                root.push(("DataVersion", Tag::Int(v)));
            }
            let chunk = read_chunk(&nbt::write("", &compound(root)), 0..=0).unwrap();
            assert!(!chunk.sections[0].valid, "{version:?}");
            assert_eq!(chunk.sections[0].portals, vec![[0, 0, 0]]);
        }
    }

    #[test]
    fn a_position_of_doubles_is_truncated() {
        let pos = Tag::List(vec![Tag::Double(4.9), Tag::Double(-0.5), Tag::Double(-3.7)]);
        let raw = chunk(vec![(
            "0",
            section(Tag::Byte(1), vec![record(pos, "minecraft:nether_portal")]),
        )]);
        let chunk = read_chunk(&raw, 0..=0).unwrap();
        assert_eq!(chunk.sections[0].portals, vec![[4, 0, -3]]);
    }

    fn chunk_with_valid(valid: Tag) -> Vec<u8> {
        chunk(vec![("0", section(valid, vec![]))])
    }

    #[test]
    fn a_malformed_chunk_is_none_and_a_chunk_without_sections_is_empty() {
        assert_eq!(read_chunk(b"\x0a\x00", -4..=19), None);
        assert_eq!(read_chunk(&nbt::write("", &Tag::Int(1)), -4..=19), None);
        let empty = read_chunk(&nbt::write("", &compound(vec![])), -4..=19).unwrap();
        assert_eq!(
            empty,
            PoiChunk {
                data_version: None,
                sections: vec![]
            }
        );
    }

    #[test]
    fn a_poi_folder_is_read_by_chunk() {
        let dir = tempfile::tempdir().unwrap();
        let raw = chunk(vec![(
            "4",
            section(
                Tag::Byte(1),
                vec![record(
                    Tag::IntArray(vec![-20, 70, 40]),
                    "minecraft:nether_portal",
                )],
            ),
        )]);
        // Region -1, 1: chunk (-2, 34) is local (30, 2), header index 30 + 2 · 32.
        let mut file = vec![0u8; 8192];
        let index = 30 + 2 * 32;
        file[index * 4..index * 4 + 4].copy_from_slice(&((2u32 << 8) | 1).to_be_bytes());
        // A chunk the header lists that isn't there.
        file[4..8].copy_from_slice(&((9u32 << 8) | 1).to_be_bytes());
        let mut payload = (u32::try_from(raw.len()).unwrap() + 1)
            .to_be_bytes()
            .to_vec();
        payload.push(3);
        payload.extend(&raw);
        payload.resize(payload.len().next_multiple_of(4096), 0);
        file.extend(payload);
        std::fs::write(dir.path().join("r.-1.1.mca"), &file).unwrap();
        std::fs::write(dir.path().join("r.x.0.mca"), &file).unwrap();
        let index = read(dir.path(), -4..=19);
        assert_eq!(index.unreadable_chunks, 1);
        assert_eq!(index.unreadable_files, 1);
        assert_eq!(
            index.portals().collect::<Vec<_>>(),
            vec![([-20, 70, 40], true)]
        );
        assert!(index.chunks.contains_key(&[-2, 34]));
        assert_eq!(
            read(&dir.path().join("missing"), -4..=19),
            PoiIndex::default()
        );
    }
}
