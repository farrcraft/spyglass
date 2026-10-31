//! The viewer's map icons (ADRs 0044 and 0045): a pictogram per structure family, kept as SVG
//! files in `icons/`, and the colours of the badge it sits on. Glyphs are white strokes that
//! are tinted to the ink their badge needs.

use eframe::egui::{self, Color32, ImageSource};
use spyglass::structure_colour;

use crate::style;

/// An icon's file, embedded.
macro_rules! icon {
    ($name:literal) => {
        (
            concat!("bytes://icons/", $name, ".svg"),
            include_bytes!(concat!("../icons/", $name, ".svg")),
        )
    };
}

/// A structure family: the structure ids it covers, as prefixes of the id without the
/// `minecraft:` namespace, its name, and its icon. Variants share one: `village_` covers the
/// five villages.
struct Family {
    prefix: &'static str,
    name: &'static str,
    icon: (&'static str, &'static [u8]),
}

const FAMILIES: &[Family] = &[
    Family {
        prefix: "village_",
        name: "Village",
        icon: icon!("village"),
    },
    Family {
        prefix: "pillager_outpost",
        name: "Pillager outpost",
        icon: icon!("pillager_outpost"),
    },
    Family {
        prefix: "mansion",
        name: "Woodland mansion",
        icon: icon!("woodland_mansion"),
    },
    Family {
        prefix: "desert_pyramid",
        name: "Desert pyramid",
        icon: icon!("desert_pyramid"),
    },
    Family {
        prefix: "jungle_pyramid",
        name: "Jungle temple",
        icon: icon!("jungle_temple"),
    },
    Family {
        prefix: "igloo",
        name: "Igloo",
        icon: icon!("igloo"),
    },
    Family {
        prefix: "swamp_hut",
        name: "Swamp hut",
        icon: icon!("swamp_hut"),
    },
    Family {
        prefix: "monument",
        name: "Ocean monument",
        icon: icon!("ocean_monument"),
    },
    Family {
        prefix: "ocean_ruin_",
        name: "Ocean ruins",
        icon: icon!("ocean_ruins"),
    },
    Family {
        prefix: "shipwreck",
        name: "Shipwreck",
        icon: icon!("shipwreck"),
    },
    Family {
        prefix: "buried_treasure",
        name: "Buried treasure",
        icon: icon!("buried_treasure"),
    },
    Family {
        prefix: "ruined_portal",
        name: "Ruined portal",
        icon: icon!("ruined_portal"),
    },
    Family {
        prefix: "stronghold",
        name: "Stronghold",
        icon: icon!("stronghold"),
    },
    Family {
        prefix: "mineshaft",
        name: "Mineshaft",
        icon: icon!("mineshaft"),
    },
    Family {
        prefix: "ancient_city",
        name: "Ancient city",
        icon: icon!("ancient_city"),
    },
    Family {
        prefix: "trail_ruins",
        name: "Trail ruins",
        icon: icon!("trail_ruins"),
    },
    Family {
        prefix: "trial_chambers",
        name: "Trial chambers",
        icon: icon!("trial_chambers"),
    },
    Family {
        prefix: "abandoned_camp_",
        name: "Abandoned camp",
        icon: icon!("abandoned_camp"),
    },
    Family {
        prefix: "fortress",
        name: "Nether fortress",
        icon: icon!("nether_fortress"),
    },
    Family {
        prefix: "bastion_remnant",
        name: "Bastion remnant",
        icon: icon!("bastion_remnant"),
    },
    Family {
        prefix: "nether_fossil",
        name: "Nether fossil",
        icon: icon!("nether_fossil"),
    },
    Family {
        prefix: "end_city",
        name: "End city",
        icon: icon!("end_city"),
    },
];

/// For a structure no family covers: another namespace's, or one a new version adds.
const UNKNOWN: (&str, &[u8]) = icon!("unknown");

fn find(id: &str) -> Option<&'static Family> {
    let name = id.strip_prefix("minecraft:")?;
    FAMILIES.iter().find(|f| name.starts_with(f.prefix))
}

/// The family icon of structure `id`, e.g. `minecraft:village_taiga`.
fn family(id: &str) -> (&'static str, &'static [u8]) {
    find(id).map_or(UNKNOWN, |f| f.icon)
}

/// The features the Layers tab offers, each a name and the placed features it marks.
pub const FEATURE_KINDS: &[(&str, &[&str])] = &[
    (
        "Dungeons",
        &["minecraft:monster_room", "minecraft:monster_room_deep"],
    ),
    ("Amethyst geodes", &["minecraft:amethyst_geode"]),
    ("Desert wells", &["minecraft:desert_well"]),
    (
        "Fossils",
        &["minecraft:fossil_upper", "minecraft:fossil_lower"],
    ),
    (
        "Lava lakes",
        &[
            "minecraft:lake_lava_underground",
            "minecraft:lake_lava_surface",
        ],
    ),
];

/// A placed feature's name, e.g. "Dungeon" for `minecraft:monster_room`; one not listed by
/// its id.
pub fn feature_name(placed: &str) -> String {
    let name = match placed.strip_prefix("minecraft:").unwrap_or(placed) {
        "monster_room" => "Dungeon",
        "monster_room_deep" => "Deep dungeon",
        "amethyst_geode" => "Amethyst geode",
        "desert_well" => "Desert well",
        "fossil_upper" => "Fossil",
        "fossil_lower" => "Deep fossil",
        "lake_lava_underground" => "Underground lava lake",
        "lake_lava_surface" => "Surface lava lake",
        _ => return spyglass::display_name(placed),
    };
    name.to_string()
}

/// The name of structure `id`'s family, e.g. "Village" for `minecraft:village_taiga`, or its own
/// name if no family covers it.
pub fn family_name(id: &str) -> String {
    find(id).map_or_else(|| spyglass::display_name(id), |f| f.name.to_string())
}

/// The glyph for structure `id`, to tint and draw.
pub fn glyph(id: &str) -> egui::Image<'static> {
    let (uri, bytes) = family(id);
    egui::Image::new(ImageSource::Bytes {
        uri: uri.into(),
        bytes: bytes.into(),
    })
}

/// How light a colour looks, 0 to 255 (ITU-R BT.601 weights).
fn lightness([r, g, b]: [u8; 3]) -> u32 {
    (299 * u32::from(r) + 587 * u32::from(g) + 114 * u32::from(b)) / 1000
}

/// Below this, a badge's glyph is white and its hollow tint is lightened.
const DARK: u32 = 112;

/// A structure set's badge: its palette colour, the glyph's ink on it, and the lighter tint a
/// hollow (candidate) badge is drawn in, so dark sets still show on dark biomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Badge {
    pub fill: Color32,
    pub ink: Color32,
    pub tint: Color32,
}

/// The badge of structure set `set`, e.g. `minecraft:villages`.
pub fn badge(set: &str) -> Badge {
    let rgb = structure_colour(set);
    let [r, g, b] = rgb;
    let fill = Color32::from_rgb(r, g, b);
    if lightness(rgb) < DARK {
        let lighten = |c: u8| (u32::from(c) + (255 - u32::from(c)) * 45 / 100) as u8;
        Badge {
            fill,
            ink: Color32::WHITE,
            tint: Color32::from_rgb(lighten(r), lighten(g), lighten(b)),
        }
    } else {
        Badge {
            fill,
            ink: style::GROUND,
            tint: fill,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variants_share_their_family() {
        assert_eq!(
            family("minecraft:village_taiga").0,
            family("minecraft:village_plains").0
        );
        assert_eq!(
            family("minecraft:ruined_portal_nether").0,
            "bytes://icons/ruined_portal.svg"
        );
        assert_eq!(
            family("minecraft:ocean_ruin_warm").0,
            "bytes://icons/ocean_ruins.svg"
        );
        assert_eq!(
            family("minecraft:fortress").0,
            "bytes://icons/nether_fortress.svg"
        );
        assert_eq!(
            family("minecraft:bastion_remnant").0,
            "bytes://icons/bastion_remnant.svg"
        );
    }

    #[test]
    fn unknown_structures_get_the_generic_icon() {
        assert_eq!(family("minecraft:new_thing"), UNKNOWN);
        assert_eq!(family("mod:village_plains"), UNKNOWN);
        assert_eq!(family("village_plains"), UNKNOWN);
    }

    #[test]
    fn families_have_distinct_prefixes_and_files() {
        for (i, family) in FAMILIES.iter().enumerate() {
            let (uri, bytes) = family.icon;
            assert!(!bytes.is_empty(), "{uri}");
            for other in &FAMILIES[..i] {
                assert!(
                    !family.prefix.starts_with(other.prefix)
                        && !other.prefix.starts_with(family.prefix),
                    "{} and {} overlap",
                    family.prefix,
                    other.prefix
                );
                assert_ne!(uri, other.icon.0);
                assert_ne!(family.name, other.name);
            }
        }
        assert_eq!(family_name("minecraft:village_snowy"), "Village");
        assert_eq!(family_name("mod:tower"), "Tower (mod)");
    }

    #[test]
    fn glyphs_contrast_with_their_badge() {
        // Light sets get dark ink and keep their colour hollow; dark ones the reverse.
        let villages = badge("minecraft:villages");
        assert_eq!(
            (villages.ink, villages.tint),
            (style::GROUND, villages.fill)
        );
        let huts = badge("minecraft:swamp_huts");
        assert_eq!(huts.ink, Color32::WHITE);
        assert!(lightness([huts.tint.r(), huts.tint.g(), huts.tint.b()]) >= DARK);
    }

    /// Every game structure in the oracle's bundle has a family of its own; a data pack's, in
    /// another namespace, gets the generic icon by design. Bundles are made locally, so this is
    /// ignored unless asked for (`cargo xtask oracle-test`).
    #[test]
    #[ignore = "needs bundles/<version> (SPYGLASS_ORACLE_VERSION, else the newest release)"]
    fn every_bundled_structure_has_a_family() {
        let game = spyglass::Game::open(spyglass_oracle::bundle_dir()).expect("a bundle");
        for dimension in spyglass::Dimension::ALL {
            let world = game.world(dimension).expect("a world");
            for id in world.structures().filter(|id| id.starts_with("minecraft:")) {
                assert_ne!(family(id).0, UNKNOWN.0, "{id} in {}", world.version());
            }
        }
    }
}
