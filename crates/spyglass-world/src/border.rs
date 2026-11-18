//! A dimension's world border as a save holds it: the `minecraft:world_border` saved data in
//! 26.2 and later (`WorldBorder.Settings`), or `level.dat`'s `Border*` fields before, which the
//! game moves into each dimension when it opens the save (`LegacyWorldBorderFix`,
//! `LevelDatToSavedDataFileFix`). See `docs/specs/world-saves.md#world-borders` and
//! `docs/specs/portals.md#the-world-border`.

use std::path::Path;

use spyglass_data::nbt::{self, Tag};

use crate::{Layout, MAX_NBT_FILE, Save, SaveSource, WorldError, read_capped, read_nbt};

/// The `level.dat` fields a border is read from before the current layout.
const LEGACY_FIELDS: &str =
    "Data.BorderCenterX, BorderCenterZ, BorderSize, BorderSizeLerpTime, BorderSizeLerpTarget";

/// The default border's side (`WorldBorder.Settings.DEFAULT`'s size).
const DEFAULT_SIZE: f64 = 5.999_996_8E7;

/// How far from 0 a saved centre may be (`Codec.doubleRange` in `WorldBorder.Settings`).
const MAX_CENTER: f64 = 2.999_998_4E7;

/// The border settings that decide its bounds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SavedBorder {
    /// `center_x` and `center_z`.
    pub center: [f64; 2],
    /// `size`: the side's length now.
    pub size: f64,
    /// `lerp_time`: milliseconds left of a move to `lerp_target`; 0 or less when not moving.
    pub lerp_time: i64,
    /// `lerp_target`: the size it's moving to.
    pub lerp_target: f64,
}

impl Default for SavedBorder {
    /// The border of a dimension that has never had one set.
    fn default() -> Self {
        SavedBorder {
            center: [0.0, 0.0],
            size: DEFAULT_SIZE,
            lerp_time: 0,
            lerp_target: 0.0,
        }
    }
}

impl SavedBorder {
    /// Whether it's moving: the game applies a positive `lerp_time` as a move.
    pub fn is_moving(&self) -> bool {
        self.lerp_time > 0
    }

    /// The size it ends at: `lerp_target` while moving, `size` otherwise.
    pub fn final_size(&self) -> f64 {
        if self.is_moving() {
            self.lerp_target
        } else {
            self.size
        }
    }
}

/// What a save says about one dimension's border.
#[derive(Debug, Clone, PartialEq)]
pub enum BorderRead {
    /// Its saved settings.
    Saved(SavedBorder),
    /// None saved, so the game uses the default.
    Missing,
    /// A file that doesn't decode, which the game replaces with the default, and why.
    Unreadable(String),
}

impl BorderRead {
    /// The settings the game runs with: the saved ones, or the default.
    pub fn settings(&self) -> SavedBorder {
        match self {
            BorderRead::Saved(border) => *border,
            BorderRead::Missing | BorderRead::Unreadable(_) => SavedBorder::default(),
        }
    }
}

impl Save {
    /// `dimension`'s world border (e.g. `minecraft:the_nether`), as the game reads it on
    /// opening this save. In the current layout it's
    /// `dimensions/<namespace>/<path>/data/minecraft/world_border.dat`. Before, it's `level.dat`'s
    /// `Data.Border*` fields, which the game copies to the three vanilla dimensions, the
    /// Nether's centre divided by 8; other dimensions get none.
    ///
    /// Fails only for a `level.dat` that can't be read or a dimension id that names no folder.
    pub fn world_border(&self, dimension: &str) -> Result<BorderRead, WorldError> {
        self.world_border_with_source(dimension)
            .map(|(border, _)| border)
    }

    /// [`world_border`](Self::world_border), and where it's read from.
    pub fn world_border_with_source(
        &self,
        dimension: &str,
    ) -> Result<(BorderRead, SaveSource), WorldError> {
        let data = self.region_dir(dimension)?.with_file_name("data");
        match self.layout {
            Layout::Current => {
                let path = data.join("minecraft").join("world_border.dat");
                let file = path
                    .strip_prefix(&self.dir)
                    .unwrap_or(&path)
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/");
                let source = SaveSource {
                    file,
                    field: "data".into(),
                    present: path.is_file(),
                };
                Ok((read_file(&path), source))
            }
            Layout::Legacy | Layout::Before116 => {
                let mut source = SaveSource {
                    file: "level.dat".into(),
                    field: LEGACY_FIELDS.into(),
                    present: false,
                };
                let divisor = match dimension {
                    "minecraft:overworld" | "minecraft:the_end" => 1.0,
                    "minecraft:the_nether" => 8.0,
                    _ => return Ok((BorderRead::Missing, source)),
                };
                let level = read_nbt(&self.dir.join("level.dat"))?;
                let data = level.get("Data");
                source.present = data.and_then(|d| d.get("BorderSize")).is_some();
                let border = data.map_or(BorderRead::Missing, |data| legacy(data, divisor));
                Ok((border, source))
            }
        }
    }
}

/// A `world_border.dat`: gzip-compressed NBT whose `data` is `WorldBorder.Settings`.
fn read_file(path: &Path) -> BorderRead {
    if !path.is_file() {
        return BorderRead::Missing;
    }
    let tag = read_capped(path, MAX_NBT_FILE)
        .map_err(|e| e.to_string())
        .and_then(|bytes| {
            nbt::read_compressed_limited(&bytes, MAX_NBT_FILE).map_err(|e| e.to_string())
        });
    match tag {
        Ok((_, root)) => match root.get("data").map(settings) {
            Some(Ok(border)) => BorderRead::Saved(border),
            Some(Err(why)) => BorderRead::Unreadable(why),
            None => BorderRead::Unreadable("no data compound".into()),
        },
        Err(why) => BorderRead::Unreadable(why),
    }
}

/// `WorldBorder.Settings.CODEC`: every field required and a number, the centre within
/// ±29,999,984.
fn settings(data: &Tag) -> Result<SavedBorder, String> {
    let number = |field: &str| {
        data.get(field)
            .and_then(double_value)
            .ok_or_else(|| format!("{field} isn't a number"))
    };
    for field in [
        "damage_per_block",
        "safe_zone",
        "warning_blocks",
        "warning_time",
    ] {
        number(field)?;
    }
    let center = [number("center_x")?, number("center_z")?];
    if center
        .iter()
        .any(|c| !(-MAX_CENTER..=MAX_CENTER).contains(c))
    {
        return Err(format!("its centre {center:?} is past ±{MAX_CENTER}"));
    }
    Ok(SavedBorder {
        center,
        size: number("size")?,
        lerp_time: data
            .get("lerp_time")
            .and_then(long_value)
            .ok_or("lerp_time isn't a number")?,
        lerp_target: number("lerp_target")?,
    })
}

/// `LegacyWorldBorderFix` on `level.dat`'s `Data`, each field defaulted when missing or not a
/// number, then `LevelDatToSavedDataFileFix`'s centre divided by `divisor`.
fn legacy(data: &Tag, divisor: f64) -> BorderRead {
    let number =
        |field: &str, default: f64| data.get(field).and_then(double_value).unwrap_or(default);
    let center = [
        number("BorderCenterX", 0.0) / divisor,
        number("BorderCenterZ", 0.0) / divisor,
    ];
    let border = SavedBorder {
        center,
        size: number("BorderSize", DEFAULT_SIZE),
        lerp_time: data
            .get("BorderSizeLerpTime")
            .and_then(long_value)
            .unwrap_or(0),
        lerp_target: number("BorderSizeLerpTarget", 0.0),
    };
    // The migrated file must decode as the current one does.
    if center
        .iter()
        .any(|c| !(-MAX_CENTER..=MAX_CENTER).contains(c))
    {
        return BorderRead::Unreadable(format!("its centre {center:?} is past ±{MAX_CENTER}"));
    }
    BorderRead::Saved(border)
}

/// A number's `doubleValue()`.
fn double_value(tag: &Tag) -> Option<f64> {
    match *tag {
        #[expect(clippy::cast_precision_loss, reason = "Java's long-to-double widening")]
        Tag::Long(v) => Some(v as f64),
        _ => tag.as_double().or_else(|| tag.as_int().map(f64::from)),
    }
}

/// A number's `longValue()`: floats and doubles cast as Java casts them.
fn long_value(tag: &Tag) -> Option<i64> {
    match *tag {
        #[expect(clippy::cast_possible_truncation, reason = "Java's (long) cast")]
        Tag::Float(v) => Some(v as i64),
        #[expect(clippy::cast_possible_truncation, reason = "Java's (long) cast")]
        Tag::Double(v) => Some(v as i64),
        _ => tag.as_long(),
    }
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

    fn full(center_x: f64, size: f64, lerp_time: i64) -> Tag {
        compound(vec![
            ("center_x", Tag::Double(center_x)),
            ("center_z", Tag::Int(-3)),
            ("size", Tag::Double(size)),
            ("lerp_time", Tag::Long(lerp_time)),
            ("lerp_target", Tag::Double(100.0)),
            ("damage_per_block", Tag::Double(0.2)),
            ("safe_zone", Tag::Double(5.0)),
            ("warning_blocks", Tag::Int(5)),
            ("warning_time", Tag::Int(300)),
        ])
    }

    fn file(dir: &Path, data: Tag) {
        let path = dir.join("dimensions/minecraft/the_nether/data/minecraft");
        std::fs::create_dir_all(&path).unwrap();
        let root = compound(vec![("data", data), ("DataVersion", Tag::Int(4671))]);
        std::fs::write(
            path.join("world_border.dat"),
            nbt::write_compressed("", &root),
        )
        .unwrap();
    }

    fn save(dir: &Path, layout: Layout) -> Save {
        Save {
            dir: dir.to_path_buf(),
            layout,
            name: String::new(),
            data_version: None,
            version_name: None,
            seed: 0,
            generate_structures: true,
            generators: Default::default(),
            enabled_packs: Vec::new(),
            disabled_packs: Vec::new(),
            was_modded: false,
            spawn: None,
            player: None,
            sources: Default::default(),
        }
    }

    #[test]
    fn a_dimension_s_own_file_is_read() {
        let dir = tempfile::tempdir().unwrap();
        file(dir.path(), full(12.5, 64.0, 0));
        let save = save(dir.path(), Layout::Current);
        let read = save.world_border("minecraft:the_nether").unwrap();
        assert_eq!(
            read,
            BorderRead::Saved(SavedBorder {
                center: [12.5, -3.0],
                size: 64.0,
                lerp_time: 0,
                lerp_target: 100.0,
            })
        );
        assert_eq!(read.settings().final_size(), 64.0);
        assert_eq!(
            save.world_border("minecraft:overworld").unwrap(),
            BorderRead::Missing
        );
        assert_eq!(
            save.world_border("minecraft:overworld").unwrap().settings(),
            SavedBorder::default()
        );
        let (_, source) = save
            .world_border_with_source("minecraft:the_nether")
            .unwrap();
        assert_eq!(
            source,
            SaveSource {
                file: "dimensions/minecraft/the_nether/data/minecraft/world_border.dat".into(),
                field: "data".into(),
                present: true,
            }
        );
        let (_, source) = save
            .world_border_with_source("minecraft:overworld")
            .unwrap();
        assert!(!source.present);
    }

    #[test]
    fn a_moving_border_ends_at_its_target() {
        let dir = tempfile::tempdir().unwrap();
        file(dir.path(), full(0.0, 64.0, 2000));
        let border = save(dir.path(), Layout::Current)
            .world_border("minecraft:the_nether")
            .unwrap()
            .settings();
        assert!(border.is_moving());
        assert_eq!(border.final_size(), 100.0);
    }

    #[test]
    fn a_file_that_doesn_t_decode_is_unreadable_and_the_default() {
        let dir = tempfile::tempdir().unwrap();
        let save = save(dir.path(), Layout::Current);
        let mut missing = full(0.0, 64.0, 0);
        if let Tag::Compound(c) = &mut missing {
            c.remove("warning_time");
        }
        file(dir.path(), missing);
        let read = save.world_border("minecraft:the_nether").unwrap();
        assert!(matches!(read, BorderRead::Unreadable(_)), "{read:?}");
        assert_eq!(read.settings(), SavedBorder::default());
        file(dir.path(), full(3.0e7, 64.0, 0));
        assert!(matches!(
            save.world_border("minecraft:the_nether").unwrap(),
            BorderRead::Unreadable(_)
        ));
    }

    #[test]
    fn an_older_save_s_border_is_copied_with_the_nether_s_centre_scaled() {
        let dir = tempfile::tempdir().unwrap();
        let level = compound(vec![(
            "Data",
            compound(vec![
                ("BorderCenterX", Tag::Double(800.0)),
                ("BorderCenterZ", Tag::Double(-80.0)),
                ("BorderSize", Tag::Double(1000.0)),
                ("BorderSizeLerpTime", Tag::String("x".into())),
            ]),
        )]);
        std::fs::write(
            dir.path().join("level.dat"),
            nbt::write_compressed("", &level),
        )
        .unwrap();
        let save = save(dir.path(), Layout::Legacy);
        let border = |d| save.world_border(d).unwrap();
        let expected = |center| {
            BorderRead::Saved(SavedBorder {
                center,
                size: 1000.0,
                lerp_time: 0,
                lerp_target: 0.0,
            })
        };
        assert_eq!(border("minecraft:overworld"), expected([800.0, -80.0]));
        assert_eq!(border("minecraft:the_end"), expected([800.0, -80.0]));
        assert_eq!(border("minecraft:the_nether"), expected([100.0, -10.0]));
        assert_eq!(border("example:mine"), BorderRead::Missing);
    }
}
