//! `pack.mcmeta`, parsed and validated as `Pack.readPackMetadata` reads it.

use serde_json::Value;

use super::{BlockedPattern, FormatRange, Overlay, PackFormat, PackMeta};

/// The newest data pack format without a minor part (`PackFormat.lastPreMinorVersion` for
/// `SERVER_DATA`): packs and overlays that reach it or older must also give the old fields.
const LAST_PRE_MINOR: u32 = 81;

/// Parses `pack.mcmeta` as `Pack.readPackMetadata` reads it. An error is a pack the game skips:
/// not JSON, no `pack` section or description, or broken `overlays` or `filter`. A `pack`
/// section whose formats don't validate isn't an error: the pack loads, of unknown format.
pub(super) fn parse_meta(bytes: &[u8]) -> Result<PackMeta, String> {
    let json: Value = serde_json::from_slice(bytes).map_err(|e| format!("isn't JSON: {e}"))?;
    let pack = json
        .get("pack")
        .and_then(Value::as_object)
        .ok_or("has no \"pack\" section")?;
    let description = match pack.get("description") {
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
        None => return Err("has no pack description".into()),
    };
    let (formats, formats_error) = match FormatFields::read(pack, "supported_formats")
        .and_then(|f| f.validate("Pack", "supported_formats", true, false))
    {
        Ok(range) => (Some(range), None),
        Err(e) => (None, Some(e)),
    };
    let overlays = match json.get("overlays") {
        None => Vec::new(),
        Some(section) => overlays(section).map_err(|e| format!("overlays: {e}"))?,
    };
    let filter = match json.get("filter") {
        None => Vec::new(),
        Some(section) => filter(section).map_err(|e| format!("filter: {e}"))?,
    };
    Ok(PackMeta {
        description,
        formats,
        formats_error,
        filter,
        overlays,
    })
}

/// The format fields of a pack section or an overlay entry (`IntermediaryFormat`), decoded but
/// not yet validated.
struct FormatFields {
    /// `min_format`.
    min: Option<PackFormat>,
    /// `max_format`.
    max: Option<PackFormat>,
    /// `pack_format`, read only for a pack section.
    pack_format: Option<u32>,
    /// `supported_formats`, or an overlay's `formats`: the range in major formats only.
    old: Option<(u32, u32)>,
}

impl FormatFields {
    fn read(object: &serde_json::Map<String, Value>, old_key: &str) -> Result<Self, String> {
        Ok(FormatFields {
            min: object
                .get("min_format")
                .map(|v| format_value(v, 0))
                .transpose()?,
            max: object
                .get("max_format")
                .map(|v| format_value(v, i32::MAX.unsigned_abs()))
                .transpose()?,
            pack_format: object
                .get("pack_format")
                .map(|v| non_negative(v).ok_or("pack_format isn't a non-negative int"))
                .transpose()?,
            old: object.get(old_key).map(int_range).transpose()?,
        })
    }

    fn is_empty(&self) -> bool {
        self.min.is_none() && self.max.is_none() && self.old.is_none()
    }

    /// `IntermediaryFormat.validate`: which fields a pack or overlay must give, and the range
    /// they make. `has_pack_format` says whether `pack_format` is read (packs, not overlays);
    /// `require_old` whether the old range field must be present (overlays, when any entry
    /// reaches format 81 or older).
    fn validate(
        &self,
        what: &str,
        old_key: &str,
        has_pack_format: bool,
        require_old: bool,
    ) -> Result<FormatRange, String> {
        let holder = Holder {
            what,
            old_key,
            has_pack_format,
            require_old,
        };
        match (self.min, self.max) {
            (Some(_), None) | (None, Some(_)) => Err(format!(
                "{what} missing field, must declare both min_format and max_format"
            )),
            (Some(min), Some(max)) => self.validate_new(&holder, min, max),
            (None, None) => self.validate_old(&holder),
        }
    }

    /// [`validate`](Self::validate) with `min_format` and `max_format` given: past format 81
    /// the old field is forbidden or required as `require_old` says, and at 81 or below it must
    /// agree with them.
    fn validate_new(
        &self,
        holder: &Holder<'_>,
        min: PackFormat,
        max: PackFormat,
    ) -> Result<FormatRange, String> {
        let Holder {
            what,
            old_key,
            require_old,
            ..
        } = *holder;
        if min > max {
            return Err(format!(
                "{what} min_format ({min}) is greater than max_format ({max})"
            ));
        }
        if min.major > LAST_PRE_MINOR {
            if require_old && self.old.is_none() {
                return Err(format!("{what} missing required field {old_key}"));
            }
            if self.old.is_some() && !require_old {
                return Err(format!(
                    "{what} key {old_key} is deprecated starting from pack format {}",
                    LAST_PRE_MINOR + 1
                ));
            }
            if self.pack_format.is_some() {
                self.check_pack_format(holder, min.major, max.major)?;
            }
        } else {
            let Some((old_min, old_max)) = self.old else {
                return Err(format!(
                    "{what} declares format {} and so requires a {old_key} field",
                    min.major
                ));
            };
            if old_min != min.major || (old_max != max.major && old_max != LAST_PRE_MINOR) {
                return Err(format!(
                    "{what} version declaration mismatch between {old_key} and min_format/max_format"
                ));
            }
            self.check_pack_format(holder, min.major, max.major)?;
        }
        Ok(FormatRange { min, max })
    }

    /// [`validate`](Self::validate) with neither `min_format` nor `max_format`: the old range,
    /// or a pack's `pack_format` alone, at format 81 or below.
    fn validate_old(&self, holder: &Holder<'_>) -> Result<FormatRange, String> {
        let what = holder.what;
        let newer_without_fields = || {
            format!(
                "{what} declares support for version newer than {LAST_PRE_MINOR}, but is missing mandatory fields min_format and max_format"
            )
        };
        match (self.old, holder.has_pack_format, self.pack_format) {
            (Some((_, max)), ..) if max > LAST_PRE_MINOR => Err(newer_without_fields()),
            (Some((min, max)), ..) => {
                self.check_pack_format(holder, min, max)?;
                Ok(FormatRange {
                    min: PackFormat::from([min, 0]),
                    max: PackFormat::from([max, 0]),
                })
            }
            (None, true, Some(f)) if f > LAST_PRE_MINOR => Err(newer_without_fields()),
            (None, true, Some(f)) => Ok(FormatRange {
                min: PackFormat::from([f, 0]),
                max: PackFormat::from([f, 0]),
            }),
            _ => Err(format!(
                "{what} could not be parsed, missing format version information"
            )),
        }
    }

    /// A pack's `pack_format` against its major range `min..=max`: required, inside it, and
    /// a range below 15 only of one format. Overlays have none to check.
    fn check_pack_format(&self, holder: &Holder<'_>, min: u32, max: u32) -> Result<(), String> {
        let what = holder.what;
        if !holder.has_pack_format {
            return Ok(());
        }
        let Some(main) = self.pack_format else {
            return Err(format!(
                "{what} declares formats {min} to {max} and so requires a pack_format field"
            ));
        };
        if main < min || main > max {
            return Err(format!(
                "{what} declared support for versions {min} to {max} but declared main format is {main}"
            ));
        }
        if min < 15 && min != max {
            return Err(format!(
                "Multi-version packs cannot support minimum version of less than 15 ({what} declares {min})"
            ));
        }
        Ok(())
    }
}

/// What [`FormatFields::validate`] checks the fields for: a pack section or an overlay entry.
#[derive(Clone, Copy)]
struct Holder<'a> {
    /// How errors name it, e.g. `Pack` or `Overlay "x"`.
    what: &'a str,
    /// The old range field's key: `supported_formats` or `formats`.
    old_key: &'a str,
    /// Whether `pack_format` is read: for packs, not overlays.
    has_pack_format: bool,
    /// Whether the old range field must be present (overlays, when any entry reaches format
    /// 81 or older).
    require_old: bool,
}

/// `min_format` or `max_format` (`PackFormat.fullCodec`): `N` or `[N]` is `N.<minor>`, with
/// `minor` 0 for a minimum and the largest for a maximum; `[N, M]` is `N.M`, and later elements
/// are ignored. A list has 1 to 256 elements, as the game's `NON_NEGATIVE_INT.listOf(1, 256)`
/// reads it.
fn format_value(value: &Value, minor: u32) -> Result<PackFormat, String> {
    let bad = || format!("{value} isn't a pack format");
    match value {
        Value::Array(parts) => {
            if parts.is_empty() || parts.len() > 256 {
                return Err(bad());
            }
            let parts = parts
                .iter()
                .map(|v| non_negative(v).ok_or_else(bad))
                .collect::<Result<Vec<u32>, _>>()?;
            Ok(PackFormat {
                major: parts[0],
                minor: parts.get(1).copied().unwrap_or(minor),
            })
        }
        other => Ok(PackFormat {
            major: non_negative(other).ok_or_else(bad)?,
            minor,
        }),
    }
}

/// An int range as `InclusiveRange.codec(INT)` reads it: `N`, `[min, max]` or
/// `{"min_inclusive": min, "max_inclusive": max}`, with `min <= max`. Negative ends are clamped
/// to 0, below every format.
fn int_range(value: &Value) -> Result<(u32, u32), String> {
    let (min, max) = match value {
        Value::Array(ends) if ends.len() == 2 => (int(&ends[0]), int(&ends[1])),
        Value::Object(ends) => (
            ends.get("min_inclusive").and_then(int),
            ends.get("max_inclusive").and_then(int),
        ),
        other => (int(other), int(other)),
    };
    let (Some(min), Some(max)) = (min, max) else {
        return Err(format!("{value} isn't a format range"));
    };
    if min > max {
        return Err(format!(
            "range {min} to {max} has its ends the wrong way round"
        ));
    }
    Ok((min.max(0).unsigned_abs(), max.max(0).unsigned_abs()))
}

/// A JSON number as the game's `Codec.INT` reads it: `Number.intValue()`, so `121.0` is 121, and
/// a number past the int range keeps its low 32 bits.
fn int(value: &Value) -> Option<i32> {
    crate::codec::int_of(value)
}

/// A JSON int in `0..=i32::MAX` (`ExtraCodecs.NON_NEGATIVE_INT`).
fn non_negative(value: &Value) -> Option<u32> {
    int(value).and_then(|v| u32::try_from(v).ok())
}

/// `overlays.entries` (`OverlayMetadataSection`): each a `directory` matching
/// `[-_a-zA-Z0-9.]+` and its formats, validated together (`PackFormat.validateHolderList`). An
/// entry without any format field is skipped, as the game skips it; any other bad entry fails
/// the list, and so the pack.
fn overlays(section: &Value) -> Result<Vec<Overlay>, String> {
    let entries = section
        .get("entries")
        .and_then(Value::as_array)
        .ok_or("has no \"entries\" list")?;
    let mut read = Vec::new();
    for entry in entries {
        let object = entry.as_object().ok_or("an entry isn't an object")?;
        let directory = object
            .get("directory")
            .and_then(Value::as_str)
            .ok_or("an entry has no directory")?;
        let valid = !directory.is_empty()
            && directory
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
        if !valid {
            return Err(format!("{directory} is not accepted directory name"));
        }
        let fields = FormatFields::read(object, "formats")?;
        if !fields.is_empty() {
            read.push((directory.to_string(), fields));
        }
    }
    // The oldest major format any entry reaches: at 81 or below, every entry needs `formats`.
    let min_major = read
        .iter()
        .filter_map(|(_, f)| f.min.map(|m| m.major).or(f.old.map(|o| o.0)))
        .min();
    let require_old = min_major.is_some_and(|m| m <= LAST_PRE_MINOR);
    read.into_iter()
        .map(|(directory, fields)| {
            let what = format!("Overlay \"{directory}\"");
            let formats = fields.validate(&what, "formats", false, require_old)?;
            Ok(Overlay { directory, formats })
        })
        .collect()
}

/// `filter.block` (`ResourceFilterSection`): a list of `{"namespace": regex, "path": regex}`,
/// either optional. The game's patterns are Java's; one `regex_lite` can't read is an error,
/// since spyglass couldn't apply the pack as the game does.
fn filter(section: &Value) -> Result<Vec<BlockedPattern>, String> {
    let block = section
        .get("block")
        .and_then(Value::as_array)
        .ok_or("has no \"block\" list")?;
    let pattern = |entry: &Value, key: &str| -> Result<Option<regex_lite::Regex>, String> {
        match entry.get(key) {
            None => Ok(None),
            Some(Value::String(re)) => regex_lite::Regex::new(re)
                .map(Some)
                .map_err(|e| format!("pattern {re:?} can't be read: {e}")),
            Some(_) => Err(format!("{key} isn't a string")),
        }
    };
    block
        .iter()
        .map(|entry| {
            if !entry.is_object() {
                return Err("an entry isn't an object".to_string());
            }
            Ok(BlockedPattern {
                namespace: pattern(entry, "namespace")?,
                path: pattern(entry, "path")?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::{Pack, PackError};

    fn meta(json: &str) -> Result<PackMeta, String> {
        parse_meta(json.as_bytes())
    }

    fn formats(pack: &str) -> Option<FormatRange> {
        meta(&format!(r#"{{"pack": {{"description": "", {pack}}}}}"#))
            .unwrap()
            .formats
    }

    fn range(min: [u32; 2], max: [u32; 2]) -> Option<FormatRange> {
        Some(FormatRange {
            min: min.into(),
            max: max.into(),
        })
    }

    #[test]
    fn format_fields_as_the_game_reads_them() {
        let max = i32::MAX.unsigned_abs();
        assert_eq!(
            formats(r#""min_format": 121, "max_format": 121"#),
            range([121, 0], [121, max])
        );
        assert_eq!(
            formats(r#""min_format": [120, 3], "max_format": [121, 1, 9]"#),
            range([120, 3], [121, 1])
        );
        assert_eq!(formats(r#""pack_format": 48"#), range([48, 0], [48, 0]));
        assert_eq!(
            formats(r#""pack_format": 48, "supported_formats": [48, 57]"#),
            range([48, 0], [57, 0])
        );
        assert_eq!(
            formats(
                r#""pack_format": 71, "supported_formats": {"min_inclusive": 71, "max_inclusive": 81},
                "min_format": 71, "max_format": 121"#
            ),
            range([71, 0], [121, max])
        );
        // Numbers as `Number.intValue()` reads them: a fraction is cut off, and past the int
        // range only the low 32 bits count (4294967417 is 2^32 + 121).
        assert_eq!(
            formats(r#""min_format": 121.0, "max_format": [4294967417, 0.5]"#),
            range([121, 0], [121, 0])
        );
        assert_eq!(formats(r#""pack_format": 4.8e1"#), range([48, 0], [48, 0]));
        // One part to 256, as `listOf(1, 256)` reads them.
        let parts = vec!["121"; 256].join(", ");
        assert_eq!(
            formats(&format!(r#""min_format": [{parts}], "max_format": 121"#)),
            range([121, 121], [121, max])
        );
        // Invalid: the pack still loads, of unknown format.
        for bad in [
            r#""min_format": 121"#,
            r#""min_format": 122, "max_format": 121"#,
            r#""pack_format": 121"#,
            r#""supported_formats": [48, 121], "pack_format": 48"#,
            r#""min_format": 121, "max_format": 121, "supported_formats": [121, 121]"#,
            r#""min_format": 71, "max_format": 121"#,
            r#""pack_format": 60, "supported_formats": [48, 57]"#,
            r#""pack_format": 4, "supported_formats": [4, 10]"#,
            r#""min_format": -1, "max_format": 121"#,
            r#""min_format": [], "max_format": 121"#,
            r#""min_format": "121", "max_format": 121"#,
            r#""note": 1"#,
        ] {
            let read = meta(&format!(r#"{{"pack": {{"description": "", {bad}}}}}"#)).unwrap();
            assert!(read.formats.is_none(), "{bad}");
            assert!(read.formats_error.is_some(), "{bad}");
        }
    }

    #[test]
    fn packs_the_game_skips() {
        for bad in [
            "not json",
            "{}",
            r#"{"pack": {"min_format": 121, "max_format": 121}}"#,
            r#"{"pack": {"description": ""}, "overlays": {}}"#,
            r#"{"pack": {"description": ""}, "overlays": {"entries": [{"directory": "a/b", "formats": 1}]}}"#,
            r#"{"pack": {"description": ""}, "overlays": {"entries": [{"directory": "a", "min_format": 121}]}}"#,
            r#"{"pack": {"description": ""}, "filter": {"block": [{"path": "("}]}}"#,
        ] {
            assert!(meta(bad).is_err(), "{bad}");
        }
        assert!(matches!(
            Pack::from_files("x", [("data/a/b.json".to_string(), b"{}".to_vec())]),
            Err(PackError::Meta { .. })
        ));
    }
}
