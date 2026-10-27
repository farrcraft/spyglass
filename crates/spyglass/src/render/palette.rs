//! spyglass's own map colours (ADR 0018): a table per kind of id, and a hashed colour for ids
//! the tables don't have (`docs/specs/render.md#colours`).

/// Biome colours, by id without the `minecraft:` namespace.
const BIOMES: &[(&str, [u8; 3])] = &[
    // Oceans and rivers: blues, deeper is darker, colder is greyer.
    ("ocean", [0x1f, 0x4e, 0xa8]),
    ("deep_ocean", [0x13, 0x33, 0x7a]),
    ("warm_ocean", [0x1b, 0x8f, 0xc4]),
    ("lukewarm_ocean", [0x22, 0x6c, 0xb8]),
    ("deep_lukewarm_ocean", [0x17, 0x4f, 0x8f]),
    ("cold_ocean", [0x3a, 0x5c, 0x9c]),
    ("deep_cold_ocean", [0x27, 0x41, 0x75]),
    ("frozen_ocean", [0x76, 0x92, 0xc8]),
    ("deep_frozen_ocean", [0x51, 0x6b, 0xa0]),
    ("river", [0x3b, 0x78, 0xd8]),
    ("frozen_river", [0x9c, 0xb4, 0xe0]),
    // Shores.
    ("beach", [0xe6, 0xd9, 0x8f]),
    ("snowy_beach", [0xe8, 0xe6, 0xc8]),
    ("stony_shore", [0x8a, 0x8a, 0x82]),
    // Plains, forests and swamps: greens.
    ("plains", [0x8d, 0xb3, 0x60]),
    ("sunflower_plains", [0xa8, 0xc0, 0x4c]),
    ("meadow", [0x7f, 0xc0, 0x7a]),
    ("forest", [0x3c, 0x7a, 0x34]),
    ("flower_forest", [0x5e, 0x8c, 0x3e]),
    ("birch_forest", [0x5a, 0x94, 0x4c]),
    ("old_growth_birch_forest", [0x4a, 0x84, 0x40]),
    ("dark_forest", [0x2a, 0x52, 0x24]),
    ("pale_garden", [0x9a, 0xa0, 0x94]),
    ("dappled_forest", [0x4c, 0x6e, 0x2e]),
    ("cherry_grove", [0xe8, 0xa8, 0xc8]),
    ("swamp", [0x4c, 0x62, 0x3c]),
    ("mangrove_swamp", [0x3e, 0x5a, 0x2a]),
    ("mushroom_fields", [0x9a, 0x6e, 0x9a]),
    // Jungles.
    ("jungle", [0x2c, 0x8c, 0x1c]),
    ("sparse_jungle", [0x56, 0x9c, 0x34]),
    ("bamboo_jungle", [0x5c, 0xa8, 0x20]),
    // Taigas and cold land.
    ("taiga", [0x3e, 0x66, 0x54]),
    ("old_growth_pine_taiga", [0x4e, 0x5e, 0x44]),
    ("old_growth_spruce_taiga", [0x46, 0x58, 0x40]),
    ("snowy_taiga", [0x6a, 0x84, 0x7c]),
    ("snowy_plains", [0xe4, 0xec, 0xf0]),
    ("ice_spikes", [0xb4, 0xdc, 0xf0]),
    ("grove", [0xa4, 0xc4, 0xb4]),
    ("snowy_slopes", [0xd0, 0xd8, 0xe4]),
    // Peaks and hills: greys and whites.
    ("jagged_peaks", [0xf4, 0xf8, 0xfc]),
    ("frozen_peaks", [0xc4, 0xdc, 0xf4]),
    ("stony_peaks", [0x9c, 0x98, 0x90]),
    ("windswept_hills", [0x70, 0x7c, 0x6c]),
    ("windswept_gravelly_hills", [0x84, 0x84, 0x80]),
    ("windswept_forest", [0x4e, 0x6c, 0x50]),
    // Dry land: sands, ochres and reds.
    ("desert", [0xe8, 0xcc, 0x7c]),
    ("savanna", [0xb4, 0xa8, 0x50]),
    ("savanna_plateau", [0xa0, 0x94, 0x48]),
    ("windswept_savanna", [0x94, 0x8c, 0x54]),
    ("badlands", [0xc8, 0x5c, 0x28]),
    ("eroded_badlands", [0xd8, 0x70, 0x34]),
    ("wooded_badlands", [0xa8, 0x6c, 0x34]),
    // Caves.
    ("dripstone_caves", [0x8c, 0x6c, 0x50]),
    ("lush_caves", [0x6c, 0x9c, 0x2c]),
    ("deep_dark", [0x10, 0x24, 0x2c]),
    ("sulfur_caves", [0xc8, 0xc0, 0x3c]),
    ("ice_caves", [0x78, 0xc0, 0xdc]),
    // The Nether.
    ("nether_wastes", [0x80, 0x30, 0x2c]),
    ("crimson_forest", [0xa8, 0x1c, 0x24]),
    ("warped_forest", [0x1c, 0x80, 0x78]),
    ("soul_sand_valley", [0x5c, 0x48, 0x38]),
    ("basalt_deltas", [0x50, 0x4c, 0x50]),
    // The End.
    ("the_end", [0xd8, 0xd8, 0x9c]),
    ("end_highlands", [0xc4, 0xc8, 0x84]),
    ("end_midlands", [0xb0, 0xb4, 0x78]),
    ("end_barrens", [0x9c, 0x9c, 0x70]),
    ("small_end_islands", [0x60, 0x5c, 0x78]),
    ("the_void", [0x00, 0x00, 0x00]),
];

/// Structure-set marker colours, by id without the `minecraft:` namespace.
const STRUCTURE_SETS: &[(&str, [u8; 3])] = &[
    ("villages", [0xf0, 0x9c, 0x30]),
    ("pillager_outposts", [0x6c, 0x6c, 0x6c]),
    ("woodland_mansions", [0x7c, 0x48, 0x20]),
    ("desert_pyramids", [0xf8, 0xe0, 0x58]),
    ("jungle_temples", [0x40, 0xa0, 0x30]),
    ("igloos", [0xe8, 0xf4, 0xfc]),
    ("swamp_huts", [0x5c, 0x40, 0x5c]),
    ("ocean_monuments", [0x30, 0xd0, 0xc0]),
    ("ocean_ruins", [0x58, 0x88, 0x98]),
    ("shipwrecks", [0xa0, 0x70, 0x40]),
    ("buried_treasures", [0xf8, 0xd0, 0x00]),
    ("ruined_portals", [0x9c, 0x30, 0xd0]),
    ("strongholds", [0x40, 0xd0, 0x60]),
    ("mineshafts", [0xb0, 0x88, 0x60]),
    ("ancient_cities", [0x00, 0x68, 0x78]),
    ("trail_ruins", [0xc8, 0x78, 0x58]),
    ("trial_chambers", [0xd0, 0x80, 0x20]),
    ("abandoned_camp", [0x88, 0x60, 0x3c]),
    ("nether_complexes", [0x68, 0x10, 0x18]),
    ("nether_fossils", [0xe0, 0xd8, 0xc0]),
    ("end_cities", [0xd0, 0xa0, 0xe0]),
];

fn find(table: &[(&str, [u8; 3])], id: &str) -> Option<[u8; 3]> {
    let name = id.strip_prefix("minecraft:")?;
    table.iter().find(|(n, _)| *n == name).map(|(_, c)| *c)
}

/// The colour for an id no table has: bytes of its FNV-1a hash, kept away from black and white.
fn hashed(id: &str) -> [u8; 3] {
    let hash = id.bytes().fold(0x811c_9dc5_u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    });
    let [r, g, b, _] = hash.to_le_bytes();
    [r, g, b].map(|c| (48 + u32::from(c) * 176 / 255) as u8)
}

/// A biome's map colour, e.g. for `minecraft:plains`.
pub fn biome_colour(id: &str) -> [u8; 3] {
    find(BIOMES, id).unwrap_or_else(|| hashed(id))
}

/// A structure set's marker colour, e.g. for `minecraft:villages`.
pub fn structure_colour(set: &str) -> [u8; 3] {
    find(STRUCTURE_SETS, set).unwrap_or_else(|| hashed(set))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_ids_use_the_table_and_others_hash() {
        assert_eq!(biome_colour("minecraft:plains"), [0x8d, 0xb3, 0x60]);
        assert_eq!(structure_colour("minecraft:villages"), [0xf0, 0x9c, 0x30]);
        // FNV-1a of "" is its offset basis, 0x811c9dc5: bytes c5, 9d, 1c.
        assert_eq!(hashed(""), [183, 156, 67]);
        // Another namespace isn't the table's.
        assert_eq!(biome_colour("mod:plains"), hashed("mod:plains"));
        assert_ne!(
            biome_colour("minecraft:new_biome"),
            biome_colour("minecraft:other_new_biome")
        );
    }

    #[test]
    fn tables_have_no_duplicates() {
        for table in [BIOMES, STRUCTURE_SETS] {
            for (i, (name, _)) in table.iter().enumerate() {
                assert!(table[..i].iter().all(|(n, _)| n != name), "{name}");
            }
        }
    }
}
