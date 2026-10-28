//! Behavior switches: what changes between game versions without showing in the data (ADR 0026).
//!
//! Each switch is a named field of [`Behavior`], on for the data versions in its ranges. Code reads
//! the field, never a data version. Lower crates, which don't depend on this one, take the field
//! they need as a plain parameter. Each switch's doc names the spec section that describes it, and
//! that section's _Versions_ note names the switch.
//!
//! Ranges are stated for the versions spyglass supports (ADR 0028): a range starting at 5125 means
//! the first supported version with the change is data version 5125, not that the game changed
//! there. Adding an older version means checking each switch for it.

use std::fmt;

/// A range of data versions, `from` inclusive to `until` exclusive (open-ended when `None`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// The first data version in the range.
    pub from: u32,
    /// The first data version after the range, if it ends.
    pub until: Option<u32>,
}

impl Span {
    /// Whether `data_version` is in the range.
    pub fn contains(&self, data_version: u32) -> bool {
        data_version >= self.from && self.until.is_none_or(|until| data_version < until)
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.until {
            Some(until) => write!(f, "{}..{until}", self.from),
            None => write!(f, "{}..", self.from),
        }
    }
}

/// One switch, for listing: its name, where it's specified and when it's on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Switch {
    /// The [`Behavior`] field's name.
    pub name: &'static str,
    /// The spec section, relative to `docs/specs/`.
    pub spec: &'static str,
    /// The data versions it's on for.
    pub spans: &'static [Span],
}

impl Switch {
    /// Whether the switch is on for `data_version`.
    pub fn is_on(&self, data_version: u32) -> bool {
        self.spans.iter().any(|span| span.contains(data_version))
    }
}

macro_rules! switches {
    ($(
        $(#[doc = $doc:literal])*
        $name:ident, $spec:literal, [$($from:literal .. $($until:literal)?),+ $(,)?];
    )*) => {
        /// Every behavior switch, on or off for one data version. See the module docs.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
        #[non_exhaustive]
        pub struct Behavior {
            $(
                $(#[doc = $doc])*
                ///
                #[doc = concat!("Specified in `docs/specs/", $spec, "`.")]
                pub $name: bool,
            )*
        }

        /// Every switch, in declaration order.
        pub const SWITCHES: &[Switch] = &[$(
            Switch {
                name: stringify!($name),
                spec: $spec,
                spans: &[$(Span { from: $from, until: switches!(@until $($until)?) }),+],
            },
        )*];

        impl Behavior {
            /// The switches for `data_version`, a bundle's or a save's.
            pub fn of(data_version: u32) -> Self {
                let mut on = SWITCHES.iter().map(|s| s.is_on(data_version));
                Self {
                    $($name: on.next().expect("one switch per field"),)*
                }
            }
        }
    };
    (@until) => { None };
    (@until $until:literal) => { Some($until) };
}

/// A game version this build's answers were compared with the game's for: one job in CI's oracle
/// matrix (ADR 0028).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Checked {
    /// The version id, e.g. `26.3`.
    pub id: &'static str,
    /// Its data version.
    pub data_version: u32,
}

/// Every version CI checks this build against, oldest first. It changes in the same commit as
/// the matrix in `.github/workflows/ci.yml`, which a test checks. Other versions still load, with
/// a warning (ADR 0039).
pub const CHECKED: &[Checked] = &[
    Checked {
        id: "26.2",
        data_version: 4903,
    },
    Checked {
        id: "26.3",
        data_version: 5023,
    },
    Checked {
        id: "26.4-snapshot-5",
        data_version: 5125,
    },
    Checked {
        id: "26.4-snapshot-6",
        data_version: 5126,
    },
];

/// How a data version relates to [`CHECKED`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Checking {
    /// One of [`CHECKED`].
    Checked,
    /// Newer than every checked version: a later spyglass may support it.
    Newer,
    /// Not checked, but not newer either: older than them all, or between two.
    Unchecked,
}

impl Checking {
    /// How `data_version` relates to [`CHECKED`].
    pub fn of(data_version: u32) -> Self {
        let newest = CHECKED.iter().map(|c| c.data_version).max().unwrap_or(0);
        if CHECKED.iter().any(|c| c.data_version == data_version) {
            Checking::Checked
        } else if data_version > newest {
            Checking::Newer
        } else {
            Checking::Unchecked
        }
    }
}

/// Whether the version with id `id` is one of [`CHECKED`], for when only the id is known yet.
pub fn is_checked_id(id: &str) -> bool {
    CHECKED.iter().any(|c| c.id == id)
}

switches! {
    /// `Climate.Parameter.distance` is half-open: a value equal to `max` is at distance 1, not 0.
    /// Biome fitness, the search's box distances and spawn fitness all follow.
    climate_half_open, "biome-lookup.md#fitness", [5125..];

    /// `getFirstFreeHeight` counts only solid blocks for every heightmap but `WORLD_SURFACE` and
    /// `WORLD_SURFACE_WG`, so fluids don't count for `MOTION_BLOCKING` and the like. An empty
    /// column's height is the noise minimum y.
    solid_only_heights, "terrain.md#queries", [5125..];

    /// A spline evaluated at a NaN coordinate returns NaN, instead of the last point's value.
    spline_nan_is_nan, "density-functions.md#types", [5125..];

    /// `clamp` is `Math.clamp`, so a `-0.0` input clamped below by `+0.0` gives `+0.0`, where
    /// `Mth.clamp` gave `-0.0`.
    clamp_positive_zero, "density-functions.md#types", [5125..];

    /// `upscale` plans a stepped volume whose offset from its first cell is a multiple of the
    /// step, where 26.4-snapshot-5 required a multiple of the cell size and otherwise fell back to
    /// upscaling every block.
    upscale_offset_by_step, "density-functions.md#upscale", [5126..];

    /// A feature pool element's jigsaw is named `minecraft:bottom`, and attaches only to a parent
    /// jigsaw targeting that name exactly. Later versions leave the name empty, matching any target.
    feature_jigsaw_named_bottom, "structure-placement.md#jigsaw-assembly", [0..5023];

    /// `OCEAN_FLOOR_WG` counts blocks whose `blocksMotion()` is true, a block property, not the
    /// `#minecraft:blocks_motion_in_heightmap` tag, which doesn't exist.
    heightmap_blocks_motion, "terrain.md#queries", [0..5023];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans() {
        let open = Span {
            from: 10,
            until: None,
        };
        let closed = Span {
            from: 10,
            until: Some(20),
        };
        assert!(!open.contains(9) && open.contains(10) && open.contains(u32::MAX));
        assert!(closed.contains(10) && closed.contains(19) && !closed.contains(20));
        assert_eq!(open.to_string(), "10..");
        assert_eq!(closed.to_string(), "10..20");
    }

    #[test]
    fn behavior_by_version() {
        // 26.2, 26.3 and 26.4-snapshot-5.
        let (old, mid, new) = (Behavior::of(4903), Behavior::of(5023), Behavior::of(5125));
        assert!(old.feature_jigsaw_named_bottom && !mid.feature_jigsaw_named_bottom);
        assert!(!mid.climate_half_open && new.climate_half_open);
        assert!(!old.climate_half_open);
        assert_eq!(
            mid,
            Behavior::default(),
            "26.3 is the baseline: every switch off"
        );
    }

    #[test]
    fn checking() {
        assert_eq!(Checking::of(5023), Checking::Checked);
        assert_eq!(Checking::of(9999), Checking::Newer);
        assert_eq!(Checking::of(5000), Checking::Unchecked);
        assert_eq!(Checking::of(100), Checking::Unchecked);
        assert!(is_checked_id("26.3") && !is_checked_id("26.1"));
        assert!(CHECKED.is_sorted_by_key(|c| c.data_version));
    }

    /// CHECKED names the versions in CI's oracle matrix, no more and no fewer.
    #[test]
    fn checked_is_the_ci_matrix() {
        let ci =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.github/workflows/ci.yml");
        let ci = std::fs::read_to_string(&ci).unwrap();
        let mut ids: Vec<&str> = ci
            .lines()
            .skip_while(|line| line.trim() != "version:")
            .skip(1)
            .map_while(|line| line.trim().strip_prefix("- "))
            .map(|id| id.trim_matches('"'))
            .collect();
        assert!(!ids.is_empty(), "no oracle matrix in ci.yml");
        let mut checked: Vec<&str> = CHECKED.iter().map(|c| c.id).collect();
        ids.sort_unstable();
        checked.sort_unstable();
        assert_eq!(
            checked, ids,
            "versions::CHECKED and ci.yml's oracle matrix differ"
        );
    }

    #[test]
    fn table_names_fields_and_specs() {
        let names: Vec<&str> = SWITCHES.iter().map(|s| s.name).collect();
        assert!(names.contains(&"climate_half_open"));
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/specs");
        for switch in SWITCHES {
            let (file, anchor) = switch.spec.split_once('#').unwrap_or((switch.spec, ""));
            let text = std::fs::read_to_string(repo.join(file))
                .unwrap_or_else(|e| panic!("{}: {}: {e}", switch.name, switch.spec));
            let anchors: Vec<String> = text
                .lines()
                .filter_map(|line| {
                    line.trim_start_matches('#')
                        .strip_prefix(' ')
                        .filter(|_| line.starts_with('#'))
                })
                .map(slug)
                .collect();
            assert!(
                anchor.is_empty() || anchors.iter().any(|a| a == anchor),
                "{}: no heading for {}",
                switch.name,
                switch.spec
            );
            assert!(!switch.spans.is_empty());
        }
    }

    /// A heading's anchor, as GitHub makes it.
    fn slug(heading: &str) -> String {
        heading
            .to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_'))
            .map(|c| if c == ' ' { '-' } else { c })
            .collect()
    }
}
