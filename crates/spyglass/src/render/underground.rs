//! The underground layers (`docs/specs/render.md#underground`): ore veins, noise caves and
//! carvers at the area's height, each pixel from its corner block alone.

use std::collections::HashMap;

use spyglass_blocks::carver;
use spyglass_blocks::material::VeinPart as BlockVeinPart;
use spyglass_terrain::Block;

use super::{Image, Layers, MapArea, invalid};
use crate::{Error, Result, Session};

/// The largest scale the underground layers are drawn at: each pixel needs its column down to
/// the area's height, and carvers its chunk's carving.
pub const MAX_UNDERGROUND_SCALE: u32 = 4;

/// Copper veins' ore, iron veins' ore, and the tints drawn over the map.
const COPPER: [u8; 3] = [0xe8, 0x74, 0x3c];
const IRON: [u8; 3] = [0xb8, 0xc4, 0xff];
const VEIN_ORE_ALPHA: u32 = 255;
const VEIN_FILLER_ALPHA: u32 = 128;
const NOISE_CAVE: [u8; 3] = [0x00, 0x00, 0x00];
const NOISE_CAVE_ALPHA: u32 = 160;
const CARVER_CAVE: [u8; 3] = [0xff, 0xd8, 0x30];
const CANYON: [u8; 3] = [0xff, 0x40, 0xc8];
const CARVER_ALPHA: u32 = 200;
/// The base with the biome layer and relief off, when an underground layer is on.
pub(super) const UNDERGROUND_BACKGROUND: [u8; 3] = [0x80, 0x80, 0x80];

/// What a carver cuts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CarverKind {
    /// Caves: the Overworld's `cave` and `cave_extra_underground`, and the Nether's
    /// `nether_cave`.
    Cave,
    /// Canyons: ravines.
    Canyon,
}

impl From<carver::CarverKind> for CarverKind {
    fn from(kind: carver::CarverKind) -> Self {
        match kind {
            carver::CarverKind::Cave => CarverKind::Cave,
            carver::CarverKind::Canyon => CarverKind::Canyon,
        }
    }
}

/// Which of an ore vein's blocks a block is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum VeinPart {
    /// The ore, such as `minecraft:copper_ore`.
    Ore,
    /// The raw-ore block, such as `minecraft:raw_copper_block`.
    RawOre,
    /// The filler stone, such as `minecraft:granite`.
    Filler,
}

/// An ore vein's block under a pixel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Vein<'m> {
    /// The vein's ore block, which names it: `minecraft:copper_ore` or
    /// `minecraft:deepslate_iron_ore`.
    pub ore: &'m str,
    /// Which of its blocks this is.
    pub part: VeinPart,
}

/// The positions carvers carve under each pixel of an area, at every height, from
/// [`Session::carving_map`]. It doesn't depend on the area's `y`, so a front end can keep it
/// while the height changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CarvingMap {
    /// The world it was carved in.
    world: u64,
    /// The area it was made for, at any height.
    area: MapArea,
    /// Each pixel's first run in `runs`, and one past the last pixel's last.
    offsets: Vec<u32>,
    /// Runs of positions one kind carved, `(min_y, max_y, kind)`, pixel by pixel, bottom up.
    runs: Vec<(i32, i32, CarverKind)>,
}

impl CarvingMap {
    /// The area it was made for; any `y` uses it.
    pub fn area(&self) -> &MapArea {
        &self.area
    }

    /// The kind of carver that carved the block at height `y` of a pixel's corner column, the
    /// first to carve it. Panics outside the area.
    pub fn carver(&self, [i, j]: [u32; 2], y: i32) -> Option<CarverKind> {
        let [w, h] = self.area.size;
        assert!(i < w && j < h, "pixel ({i}, {j}) outside the area");
        let pixel = j as usize * w as usize + i as usize;
        let runs = &self.runs[self.offsets[pixel] as usize..self.offsets[pixel + 1] as usize];
        runs.iter()
            .find(|(min, max, _)| (*min..=*max).contains(&y))
            .map(|r| r.2)
    }

    /// Whether `area` is this map's, at any height.
    fn covers(&self, area: &MapArea) -> bool {
        let at = |a: &MapArea| (a.min, a.size, a.scale);
        at(&self.area) == at(area)
    }
}

/// What's under one pixel at the area's height.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Cell {
    /// An index into [`UndergroundMap::ores`] and the part, if an ore vein's block is there.
    vein: Option<(u16, VeinPart)>,
    /// Whether it's a noise cave.
    noise_cave: bool,
    /// The kind of carver that carved it first, if one did.
    carver: Option<CarverKind>,
}

/// What's under each pixel of an area at its height, from [`Session::underground_map`]: only
/// what the layers asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndergroundMap {
    /// The world it was drawn in.
    world: u64,
    /// The area it covers.
    area: MapArea,
    /// Each pixel's cell, x fastest.
    cells: Vec<Cell>,
    /// The ore blocks naming the veins found, in order of first appearance.
    ores: Vec<String>,
}

impl UndergroundMap {
    /// The area.
    pub fn area(&self) -> &MapArea {
        &self.area
    }

    fn cell(&self, [i, j]: [u32; 2]) -> &Cell {
        let [w, h] = self.area.size;
        assert!(i < w && j < h, "pixel ({i}, {j}) outside the area");
        &self.cells[j as usize * w as usize + i as usize]
    }

    /// The ore vein's block at the pixel's corner block, if there's one and veins were asked
    /// for. Panics outside the area.
    pub fn vein(&self, pixel: [u32; 2]) -> Option<Vein<'_>> {
        self.cell(pixel).vein.map(|(ore, part)| Vein {
            ore: &self.ores[usize::from(ore)],
            part,
        })
    }

    /// Whether the pixel's corner block is a noise cave: air or a fluid in the noise fill, with
    /// a solid block above it, if noise caves were asked for. Panics outside the area.
    pub fn is_noise_cave(&self, pixel: [u32; 2]) -> bool {
        self.cell(pixel).noise_cave
    }

    /// The kind of carver that carved the pixel's corner block first, if carvers were asked
    /// for. Panics outside the area.
    pub fn carver(&self, pixel: [u32; 2]) -> Option<CarverKind> {
        self.cell(pixel).carver
    }
}

/// An ore vein's colour, by its ore block: copper's and iron's, and a hashed colour for others.
pub fn vein_colour(ore: &str) -> [u8; 3] {
    match ore {
        "minecraft:copper_ore" | "minecraft:deepslate_copper_ore" => COPPER,
        "minecraft:iron_ore" | "minecraft:deepslate_iron_ore" => IRON,
        _ => super::palette::hashed(ore),
    }
}

/// Fails unless the underground layers are drawn at `area`'s scale.
fn check_scale(area: &MapArea) -> Result<()> {
    if area.scale > MAX_UNDERGROUND_SCALE {
        return Err(invalid(format!(
            "underground layers are drawn at scales up to {MAX_UNDERGROUND_SCALE}, not {}",
            area.scale
        )));
    }
    Ok(())
}

impl Layers {
    /// Whether any underground layer is on.
    pub fn need_underground(&self) -> bool {
        self.veins || self.noise_caves || self.carvers
    }
}

impl Session<'_> {
    /// The positions carvers carve under each pixel of `area`, at every height
    /// (`docs/specs/render.md#underground`). Scales up to [`MAX_UNDERGROUND_SCALE`].
    pub fn carving_map(&self, area: &MapArea) -> Result<CarvingMap> {
        area.check()?;
        check_scale(area)?;
        let generator = self.chunks()?;
        let [w, h] = area.size;
        let mut offsets = Vec::with_capacity(w as usize * h as usize + 1);
        let mut runs = Vec::new();
        // Rows go down the chunks in order, so each chunk is carved once.
        let mut row: Option<(i32, HashMap<i32, carver::Carvings>)> = None;
        for j in 0..h {
            for i in 0..w {
                let [x, z] = area.block([i, j]);
                let (cx, cz) = (x >> 4, z >> 4);
                if row.as_ref().is_none_or(|(at, _)| *at != cz) {
                    row = Some((cz, HashMap::new()));
                }
                let (_, chunks) = row.as_mut().expect("a row");
                let carvings = chunks
                    .entry(cx)
                    .or_insert_with(|| generator.carvings(cx, cz));
                offsets.push(runs.len() as u32);
                runs.extend(
                    carvings
                        .column_runs(x & 15, z & 15)
                        .into_iter()
                        .map(|(min, max, kind)| (min, max, kind.into())),
                );
            }
        }
        offsets.push(runs.len() as u32);
        Ok(CarvingMap {
            world: self.world.id,
            area: *area,
            offsets,
            runs,
        })
    }

    /// What's under each pixel of `area` at its `y`, for the underground layers in `layers`
    /// (`docs/specs/render.md#underground`). Carvers come from `carvings` if given, which must
    /// be for the same blocks and scale, and are carved here otherwise. Scales up to
    /// [`MAX_UNDERGROUND_SCALE`].
    pub fn underground_map(
        &self,
        area: &MapArea,
        layers: &Layers,
        carvings: Option<&CarvingMap>,
    ) -> Result<UndergroundMap> {
        area.check()?;
        check_scale(area)?;
        layers.check(self.world)?;
        let [w, h] = area.size;
        let mut cells = vec![Cell::default(); w as usize * h as usize];
        let mut ores: Vec<String> = Vec::new();
        if layers.veins || layers.noise_caves {
            let generator = self.chunks()?;
            let palette = self.world.block_settings()?.palette();
            let xs: Vec<i32> = (0..w).map(|i| area.block([i, 0])[0]).collect();
            let zs: Vec<i32> = (0..h).map(|j| area.block([0, j])[1]).collect();
            let probes = generator.probes_on(&xs, &zs, area.y);
            for (pixel, probe) in probes.iter().enumerate() {
                let cell = &mut cells[pixel];
                if layers.noise_caves {
                    let open = matches!(probe.block, Block::Air | Block::Water | Block::Lava);
                    cell.noise_cave = open && probe.covered;
                }
                if layers.veins {
                    let pos = [xs[pixel % w as usize], area.y, zs[pixel / w as usize]];
                    if let Some(vein) = generator.vein_at(pos, probe.block) {
                        let ore = palette.name(vein.ore);
                        let index = match ores.iter().position(|o| o == ore) {
                            Some(i) => i,
                            None => {
                                ores.push(ore.to_string());
                                ores.len() - 1
                            }
                        };
                        let part = match vein.part {
                            BlockVeinPart::Ore => VeinPart::Ore,
                            BlockVeinPart::RawOre => VeinPart::RawOre,
                            BlockVeinPart::Filler => VeinPart::Filler,
                        };
                        cell.vein = Some((index as u16, part));
                    }
                }
            }
        }
        if layers.carvers {
            let made;
            let carvings = match carvings {
                Some(given) => {
                    if given.world != self.world.id {
                        return Err(Error::OtherWorld("a carving map".to_string()));
                    }
                    if !given.covers(area) {
                        return Err(invalid("the carving map is for another area"));
                    }
                    given
                }
                None => {
                    made = self.carving_map(area)?;
                    &made
                }
            };
            for j in 0..h {
                for i in 0..w {
                    cells[j as usize * w as usize + i as usize].carver =
                        carvings.carver([i, j], area.y);
                }
            }
        }
        Ok(UndergroundMap {
            world: self.world.id,
            area: *area,
            cells,
            ores,
        })
    }
}

impl Image {
    /// Draws the underground layers of `map` that `layers` has on, over the image of its area,
    /// which must be the image's size: ore veins, then noise caves, then carvers
    /// (`docs/specs/render.md#underground`).
    pub fn draw_underground(&mut self, map: &UndergroundMap, layers: &Layers) {
        assert_eq!(self.size, map.area.size, "an image of the area");
        for j in 0..self.size[1] {
            for i in 0..self.size[0] {
                let cell = map.cell([i, j]);
                if let (true, Some(vein)) = (layers.veins, map.vein([i, j])) {
                    let alpha = match vein.part {
                        VeinPart::Ore | VeinPart::RawOre => VEIN_ORE_ALPHA,
                        VeinPart::Filler => VEIN_FILLER_ALPHA,
                    };
                    self.blend([i, j], vein_colour(vein.ore), alpha);
                }
                if layers.noise_caves && cell.noise_cave {
                    self.blend([i, j], NOISE_CAVE, NOISE_CAVE_ALPHA);
                }
                if let (true, Some(kind)) = (layers.carvers, cell.carver) {
                    let colour = match kind {
                        CarverKind::Cave => CARVER_CAVE,
                        CarverKind::Canyon => CANYON,
                    };
                    self.blend([i, j], colour, CARVER_ALPHA);
                }
            }
        }
    }
}
