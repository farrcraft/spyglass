//! The flower layer (`docs/specs/render.md#flowers`): the flower bone meal grows at each pixel's
//! corner block, at the area's height.

use std::collections::HashMap;

use spyglass_biome::Lookup;
use spyglass_blocks::zoom::BiomeZoom;
use spyglass_feature::flowers::{Chance, FlowerOdds, Flowers};

use super::palette::hashed;
use super::{Image, MapArea, dimmed};
use crate::flowers::block_id;
use crate::{Error, FlowerChance, Halt, Result, Session};

/// Flower colours by block id; any other block has the hashed colour of its id.
const FLOWERS: &[(&str, [u8; 3])] = &[
    ("minecraft:allium", [0xb5, 0x5f, 0xd9]),
    ("minecraft:azure_bluet", [0xd6, 0xe4, 0xee]),
    ("minecraft:blue_orchid", [0x2f, 0xa8, 0xe0]),
    ("minecraft:closed_eyeblossom", [0x8a, 0x6d, 0x8f]),
    ("minecraft:cornflower", [0x46, 0x64, 0xe0]),
    ("minecraft:dandelion", [0xf5, 0xd0, 0x3b]),
    ("minecraft:lily_of_the_valley", [0xf2, 0xf6, 0xe6]),
    ("minecraft:orange_tulip", [0xf0, 0x8a, 0x2a]),
    ("minecraft:oxeye_daisy", [0xe8, 0xe0, 0xa8]),
    ("minecraft:pink_petals", [0xf4, 0xa6, 0xc8]),
    ("minecraft:pink_tulip", [0xf0, 0x90, 0xc0]),
    ("minecraft:poppy", [0xd8, 0x35, 0x2a]),
    ("minecraft:red_tulip", [0xe0, 0x41, 0x2f]),
    ("minecraft:short_grass", [0x6f, 0xa6, 0x4a]),
    ("minecraft:tall_grass", [0x5c, 0x9a, 0x3c]),
    ("minecraft:white_tulip", [0xf4, 0xf4, 0xf4]),
    ("minecraft:wildflowers", [0xf2, 0xe0, 0x4a]),
];

/// A flower's map colour, by block id, e.g. `minecraft:cornflower`.
pub fn flower_colour(id: &str) -> [u8; 3] {
    FLOWERS
        .iter()
        .find(|(i, _)| *i == id)
        .map_or_else(|| hashed(id), |(_, c)| *c)
}

/// What bone meal grows at one pixel's block.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct FlowerPixel {
    /// The flowers it can grow in spyglass's biome there, by block id (a flower's states
    /// together), with their chances given that bone meal makes a flower, most likely first.
    /// Empty where the biome has no bone meal features.
    pub flowers: Vec<FlowerChance>,
    /// Whether the block's biome cell is tied and another biome of the tie would grow other
    /// flowers: then nothing here is certain.
    pub tied: bool,
}

impl FlowerPixel {
    /// The flower that certainly grows here, if one does.
    pub fn certain(&self) -> Option<&str> {
        match &self.flowers[..] {
            [only] if only.chance.is_certain() && !self.tied => Some(&only.flower),
            _ => None,
        }
    }

    /// `flower`'s chance here (a block id), 0 if it can't grow.
    pub fn chance_of(&self, flower: &str) -> Chance {
        self.flowers
            .iter()
            .find(|f| f.flower == flower)
            .map_or(Chance::new_zero(), |f| f.chance)
    }
}

/// What bone meal grows at each pixel of an area, from [`Session::flower_map`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowerMap {
    /// The area.
    area: MapArea,
    /// The answers the pixels have, each once.
    answers: Vec<FlowerPixel>,
    /// Each pixel's answer, rows top to bottom.
    cells: Vec<u32>,
}

impl FlowerMap {
    /// The area it's of.
    pub fn area(&self) -> &MapArea {
        &self.area
    }

    /// The answer at `pixel`.
    pub fn at(&self, [i, j]: [u32; 2]) -> &FlowerPixel {
        let index = j as usize * self.area.size[0] as usize + i as usize;
        &self.answers[self.cells[index] as usize]
    }

    /// The flowers the map shows, by block id, each with the pixels it can grow at and those
    /// it's certain at, most pixels first: the map's key.
    pub fn key(&self) -> Vec<FlowerCount> {
        let mut uses = vec![0u64; self.answers.len()];
        for &c in &self.cells {
            uses[c as usize] += 1;
        }
        let mut counts: HashMap<&str, FlowerCount> = HashMap::new();
        for (answer, &n) in self.answers.iter().zip(&uses) {
            let certain = answer.certain();
            for f in &answer.flowers {
                let entry = counts.entry(&f.flower).or_insert_with(|| FlowerCount {
                    flower: f.flower.clone(),
                    possible: 0,
                    certain: 0,
                });
                entry.possible += n;
                if certain == Some(f.flower.as_str()) {
                    entry.certain += n;
                }
            }
        }
        let mut key: Vec<FlowerCount> = counts.into_values().collect();
        key.sort_by(|a, b| {
            b.possible
                .cmp(&a.possible)
                .then_with(|| a.flower.cmp(&b.flower))
        });
        key
    }
}

/// A flower in a [`FlowerMap`]'s key.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FlowerCount {
    /// Its block id.
    pub flower: String,
    /// The pixels it can grow at.
    pub possible: u64,
    /// The pixels it certainly grows at.
    pub certain: u64,
}

/// The least alpha a flower that's down to chance is drawn with, and how much its chance adds:
/// a certain flower is opaque, and one that's only likely never quite is.
const CHANCE_ALPHA: u32 = 64;
const CHANCE_ALPHA_RANGE: u128 = 159;

/// The alpha a flower with `chance` is drawn with: 255 if it's certain, else
/// `64 + ⌊159 · chance⌋`.
fn flower_alpha(chance: Chance, certain: bool) -> u32 {
    if certain {
        return 255;
    }
    let extra = CHANCE_ALPHA_RANGE * chance.numerator / chance.denominator;
    CHANCE_ALPHA + u32::try_from(extra).expect("at most 159")
}

impl Image {
    /// Draws `map` over the image of its area (`docs/specs/render.md#flowers`): each block's
    /// most likely flower, opaque where it's certain and fainter as it's less likely, and
    /// blocks without a flower dimmed. With `highlight` (a block id), only that flower is
    /// drawn, by its own chance, and blocks that can't grow it are dimmed.
    pub fn draw_flowers(&mut self, map: &FlowerMap, highlight: Option<&str>) {
        let [width, height] = map.area.size;
        for j in 0..height {
            for i in 0..width {
                let pixel = map.at([i, j]);
                let shown = match highlight {
                    Some(flower) => {
                        let chance = pixel.chance_of(flower);
                        (chance.numerator > 0).then(|| {
                            (
                                flower,
                                flower_alpha(chance, pixel.certain() == Some(flower)),
                            )
                        })
                    }
                    None => pixel.flowers.first().map(|top| {
                        let certain = pixel.certain().is_some();
                        (top.flower.as_str(), flower_alpha(top.chance, certain))
                    }),
                };
                match shown {
                    Some((flower, alpha)) => self.blend([i, j], flower_colour(flower), alpha),
                    None => {
                        let dim = dimmed(self.pixel([i, j]));
                        self.set([i, j], dim);
                    }
                }
            }
        }
    }
}

impl Session<'_> {
    /// What bone meal grows at each pixel's corner block of `area`, at the area's `y`, the
    /// block above the grass (`docs/specs/render.md#flowers`). Each pixel is that block's
    /// answer alone, as [`Session::flower_at`] gives it, so a view equals its tiles.
    pub fn flower_map(&self, area: &MapArea) -> Result<FlowerMap> {
        self.flower_map_halting(area, Halt::NEVER)
    }

    /// [`flower_map`](Self::flower_map), checking `halt` before each row.
    pub(crate) fn flower_map_halting(&self, area: &MapArea, halt: Halt<'_>) -> Result<FlowerMap> {
        area.check()?;
        let flowers = self.world.flowers()?;
        let zoom = BiomeZoom::new(self.seed);
        let mut quarts: HashMap<[i32; 3], Lookup> = HashMap::new();
        let mut answers: Vec<FlowerPixel> = Vec::new();
        let mut index: HashMap<FlowerPixel, u32> = HashMap::new();
        let [width, height] = area.size;
        let mut cells = Vec::with_capacity(width as usize * height as usize);
        for j in 0..height {
            halt.check()?;
            for i in 0..width {
                let [x, z] = area.block([i, j]);
                let pos = [x, area.y, z];
                let quart = self.block_quart(&zoom, pos);
                let lookup = quarts
                    .entry(quart)
                    .or_insert_with(|| self.biomes.biome_at(quart[0], quart[1], quart[2]));
                let pixel = pixel_answer(flowers, lookup, pos)?;
                let next = u32::try_from(answers.len()).expect("fewer answers than pixels");
                let cell = *index.entry(pixel.clone()).or_insert_with(|| {
                    answers.push(pixel);
                    next
                });
                cells.push(cell);
            }
        }
        Ok(FlowerMap {
            area: *area,
            answers,
            cells,
        })
    }
}

/// A block's answer in spyglass's biome, by block id, and whether a tie could change it.
fn pixel_answer(flowers: &Flowers, lookup: &Lookup, pos: [i32; 3]) -> Result<FlowerPixel> {
    let odds = |biome| {
        flowers
            .odds(biome, pos)
            .map_err(|e| Error::Data(crate::Cause(Box::new(e))))
    };
    let first = odds(lookup.biome())?;
    let mut tied = false;
    for &other in &lookup.candidates()[1..] {
        if odds(other)? != first {
            tied = true;
            break;
        }
    }
    Ok(FlowerPixel {
        flowers: by_block(flowers, &first),
        tied,
    })
}

/// Odds by block id, a flower's states summed, most likely first, then by id.
fn by_block(flowers: &Flowers, odds: &[FlowerOdds]) -> Vec<FlowerChance> {
    let mut summed: Vec<(String, Chance)> = Vec::new();
    for o in odds {
        let id = block_id(flowers.state_name(o.state));
        match summed.iter_mut().find(|(i, _)| i == id) {
            Some((_, chance)) => *chance = chance.plus(o.chance),
            None => summed.push((id.to_string(), o.chance)),
        }
    }
    let as_odds: Vec<FlowerChance> = summed
        .into_iter()
        .map(|(flower, chance)| FlowerChance { flower, chance })
        .collect();
    crate::flowers::sorted(as_odds)
}

/// How much of an area one flower covers at one height, from [`Session::flower_heights`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct FlowerShare {
    /// The height: the flower's block, one above the grass.
    pub y: i32,
    /// The pixels where the flower is certain.
    pub certain: u64,
    /// The pixels where it can grow.
    pub possible: u64,
    /// The area's pixels.
    pub pixels: u64,
}

impl Session<'_> {
    /// How much of `area` `flower` (a block id) covers at each height from `min_y` to `max_y`,
    /// every `step` blocks: the pixels where it's certain and where it can grow, as
    /// [`Session::flower_map`] gives them at that height. For choosing a flower farm's height
    /// (`docs/specs/render.md#flowers`).
    ///
    /// # Errors
    ///
    /// For an unknown flower, an invalid area or range, or flower features spyglass can't read.
    pub fn flower_heights(
        &self,
        area: &MapArea,
        flower: &str,
        [min_y, max_y]: [i32; 2],
        step: u32,
    ) -> Result<Vec<FlowerShare>> {
        self.world.check_flower(flower)?;
        if min_y > max_y || step == 0 {
            return Err(super::invalid(
                "a height range needs min ≤ max and a step above 0",
            ));
        }
        let pixels = u64::from(area.size[0]) * u64::from(area.size[1]);
        let mut out = Vec::new();
        let mut y = i64::from(min_y);
        while y <= i64::from(max_y) {
            let at = MapArea {
                y: i32::try_from(y).expect("within the range"),
                ..*area
            };
            let map = self.flower_map(&at)?;
            let (certain, possible) = map
                .key()
                .into_iter()
                .find(|c| c.flower == flower)
                .map_or((0, 0), |c| (c.certain, c.possible));
            out.push(FlowerShare {
                y: at.y,
                certain,
                possible,
                pixels,
            });
            y += i64::from(step);
        }
        Ok(out)
    }
}
