//! Map images (`docs/specs/render.md`, ADR 0017): biomes on a world-aligned grid, with
//! structure, slime-chunk and spawn overlays from the existing queries. The viewer and
//! `spyglass render` both draw through here, so they make the same image.

mod palette;

use std::collections::BTreeMap;
use std::io;

use spyglass_biome::{BiomeId, Lookup};

pub use palette::{biome_colour, structure_colour};

use crate::{
    BiomeQuery, Error, Result, Session, SpawnOrigin, StructureQuery, StructureStart, World,
};

/// The height biomes are sampled at unless asked otherwise.
pub const DEFAULT_MAP_Y: i32 = 64;
/// The largest scale, in blocks per pixel.
pub const MAX_SCALE: u32 = 256;
/// The largest width or height, in pixels.
pub const MAX_SIDE: u32 = 16_384;
/// The most pixels in one area.
pub const MAX_PIXELS: u64 = 1 << 26;
/// The largest scale slime chunks are drawn at: a chunk is at least a pixel.
pub const MAX_SLIME_SCALE: u32 = 16;
/// How many pixels a marker reaches from its centre. An area includes the features whose
/// centre is within this of it.
pub const MARKER_REACH: u32 = 5;

const BACKGROUND: [u8; 3] = [0x20, 0x20, 0x20];
const SLIME: [u8; 3] = [0x40, 0xe0, 0x40];
const SLIME_ALPHA: u32 = 128;
const BLACK: [u8; 3] = [0, 0, 0];
const WHITE: [u8; 3] = [0xff, 0xff, 0xff];

/// A rectangle of the map: which blocks, at what scale, and the height biomes are sampled at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MapArea {
    /// The block `(x, z)` at the top-left pixel's corner; a multiple of `scale`.
    pub min: [i32; 2],
    /// Width and height in pixels.
    pub size: [u32; 2],
    /// Blocks per pixel: a power of two up to [`MAX_SCALE`].
    pub scale: u32,
    /// The block height biomes are sampled at.
    pub y: i32,
}

fn invalid(message: impl Into<String>) -> Error {
    Error::InvalidMap(message.into())
}

fn check_scale(scale: u32) -> Result<()> {
    if scale.is_power_of_two() && scale <= MAX_SCALE {
        Ok(())
    } else {
        Err(invalid(format!(
            "scale {scale} isn't a power of two from 1 to {MAX_SCALE}"
        )))
    }
}

impl MapArea {
    /// The area of `size` pixels with block `centre` in pixel `size / 2`, on the scale's grid.
    pub fn centred_on(centre: [i32; 2], size: [u32; 2], scale: u32, y: i32) -> Result<MapArea> {
        check_scale(scale)?;
        let s = i64::from(scale);
        let min = |d: usize| {
            let corner = i64::from(centre[d]) - i64::from(size[d] / 2) * s;
            i32::try_from(corner.div_euclid(s) * s)
                .map_err(|_| invalid("the area goes past the edge of the world's coordinates"))
        };
        let area = MapArea {
            min: [min(0)?, min(1)?],
            size,
            scale,
            y,
        };
        area.check()?;
        Ok(area)
    }

    /// Fails unless the area is valid (`docs/specs/render.md#map-areas`).
    pub fn check(&self) -> Result<()> {
        check_scale(self.scale)?;
        let [w, h] = self.size;
        if !(1..=MAX_SIDE).contains(&w) || !(1..=MAX_SIDE).contains(&h) {
            return Err(invalid(format!(
                "{w} × {h} pixels: each side must be 1 to {MAX_SIDE}"
            )));
        }
        if u64::from(w) * u64::from(h) > MAX_PIXELS {
            return Err(invalid(format!(
                "{w} × {h} pixels is more than {MAX_PIXELS}"
            )));
        }
        let s = i64::from(self.scale);
        for d in 0..2 {
            if i64::from(self.min[d]) % s != 0 {
                return Err(invalid(format!(
                    "the corner {:?} isn't a multiple of the scale {s}",
                    self.min
                )));
            }
        }
        if self.end().iter().any(|&e| e - 1 > i64::from(i32::MAX)) {
            return Err(invalid(
                "the area goes past the edge of the world's coordinates",
            ));
        }
        Ok(())
    }

    /// The block just past the area, per axis.
    fn end(&self) -> [i64; 2] {
        [0, 1].map(|d| i64::from(self.min[d]) + i64::from(self.size[d]) * i64::from(self.scale))
    }

    /// The block at pixel `(i, j)`'s corner, which its biome is sampled at.
    pub fn block(&self, [i, j]: [u32; 2]) -> [i32; 2] {
        let s = i64::from(self.scale);
        [
            (i64::from(self.min[0]) + i64::from(i) * s) as i32,
            (i64::from(self.min[1]) + i64::from(j) * s) as i32,
        ]
    }

    /// The pixel containing block `(x, z)`, which may be outside the area.
    pub fn pixel(&self, [x, z]: [i32; 2]) -> [i64; 2] {
        let s = i64::from(self.scale);
        [
            (i64::from(x) - i64::from(self.min[0])).div_euclid(s),
            (i64::from(z) - i64::from(self.min[1])).div_euclid(s),
        ]
    }

    /// Whether a marker centred on block `(x, z)` is drawn in this area.
    fn reaches(&self, block: [i32; 2]) -> bool {
        let reach = i64::from(MARKER_REACH);
        let [px, pz] = self.pixel(block);
        (-reach..i64::from(self.size[0]) + reach).contains(&px)
            && (-reach..i64::from(self.size[1]) + reach).contains(&pz)
    }

    /// The blocks within marker reach of the area, inclusive, per axis.
    fn reach_blocks(&self) -> [[i64; 2]; 2] {
        let margin = i64::from(MARKER_REACH) * i64::from(self.scale);
        let end = self.end();
        [0, 1].map(|d| [i64::from(self.min[d]) - margin, end[d] - 1 + margin])
    }
}

/// What to draw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layers {
    /// Biomes; otherwise a flat background.
    pub biomes: bool,
    /// Show tied cells as a checker of their candidates.
    pub ties: bool,
    /// Biomes to keep bright; every other one is dimmed. Empty: nothing is dimmed.
    pub highlight: Vec<BiomeQuery>,
    /// Structures to mark, by id or tag.
    pub structures: Vec<StructureQuery>,
    /// Slime chunks, at scales up to [`MAX_SLIME_SCALE`].
    pub slime_chunks: bool,
    /// The world spawn's origin.
    pub spawn: bool,
}

impl Default for Layers {
    /// Biomes only.
    fn default() -> Self {
        Layers {
            biomes: true,
            ties: false,
            highlight: Vec::new(),
            structures: Vec::new(),
            slime_chunks: false,
            spawn: false,
        }
    }
}

/// The features an area draws, from [`Session::map_features`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MapFeatures {
    /// Structure starts and candidates, in drawing order: by set id, then chunk z, then x.
    pub structures: Vec<StructureStart>,
    /// Slime chunks overlapping the area, by z, then x.
    pub slime_chunks: Vec<[i32; 2]>,
    /// The spawn's origin, if its marker reaches the area.
    pub spawn: Option<SpawnOrigin>,
}

impl SpawnOrigin {
    /// The block a map's spawn marker is centred on: the climate search's position, or the
    /// centre of the origin chunk when there's none.
    pub fn map_block(&self) -> [i32; 2] {
        match self.position {
            Some([x, _, z]) => [x, z],
            None => self.chunk.map(|c| c.wrapping_shl(4).wrapping_add(8)),
        }
    }
}

/// The biome under each pixel of an area, from [`Session::biome_map`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BiomeMap {
    world: u64,
    area: MapArea,
    /// Each pixel column's grid column, and each pixel row's grid row.
    columns: Vec<u32>,
    rows: Vec<u32>,
    grid_width: usize,
    /// Each sampled quart's biome (a tied cell's first), x fastest.
    cells: Vec<BiomeId>,
    /// Tied cells' sets, by cell index.
    ties: BTreeMap<usize, Box<[BiomeId]>>,
}

impl BiomeMap {
    /// The area.
    pub fn area(&self) -> &MapArea {
        &self.area
    }

    /// A pixel's candidates: one biome, or a tie.
    fn cell(&self, [i, j]: [u32; 2]) -> &[BiomeId] {
        let row = self.rows[j as usize] as usize;
        let index = row * self.grid_width + self.columns[i as usize] as usize;
        match self.ties.get(&index) {
            Some(set) => set,
            None => std::slice::from_ref(&self.cells[index]),
        }
    }

    /// The biome at a pixel, as an index into [`World::biomes`]: where the cell is tied, the
    /// first of the tie. Panics outside the area.
    pub fn biome(&self, pixel: [u32; 2]) -> usize {
        usize::from(self.cell(pixel)[0].0)
    }

    /// Every biome the game may give at a pixel, as indices into [`World::biomes`]: one, or
    /// a tie. Panics outside the area.
    pub fn candidates(&self, pixel: [u32; 2]) -> impl Iterator<Item = usize> + '_ {
        self.cell(pixel).iter().map(|b| usize::from(b.0))
    }

    /// The image of these biomes (`docs/specs/render.md#layers`): tied cells as a checker of
    /// their candidates with `ties`, and biomes not in `highlight` dimmed unless it's empty.
    pub fn colorize(&self, world: &World, ties: bool, highlight: &[BiomeQuery]) -> Result<Image> {
        if self.world != world.id {
            return Err(Error::OtherWorld("a biome map".to_string()));
        }
        let mut bright = vec![false; world.palette.len()];
        for query in highlight {
            if query.world != world.id {
                return Err(Error::OtherWorld(query.id.clone()));
            }
            if let Some(BiomeId(b)) = query.biome {
                bright[usize::from(b)] = true;
            }
        }
        let dim = !highlight.is_empty();
        let [w, h] = self.area.size;
        let s = i64::from(self.area.scale);
        let [gx0, gz0] = self.area.min.map(|m| i64::from(m) / s);
        let mut image = Image::filled(self.area.size, BACKGROUND);
        for j in 0..h {
            for i in 0..w {
                let candidates = self.cell([i, j]);
                let shown = if ties && candidates.len() > 1 {
                    let parity = (gx0 + i64::from(i) + gz0 + i64::from(j))
                        .rem_euclid(candidates.len() as i64);
                    candidates[parity as usize]
                } else {
                    candidates[0]
                };
                let mut colour = world.palette[usize::from(shown.0)];
                if dim && !candidates.iter().any(|b| bright[usize::from(b.0)]) {
                    colour = dimmed(colour);
                }
                image.set([i, j], colour);
            }
        }
        Ok(image)
    }
}

/// A non-highlighted colour: towards its grey, and darker.
fn dimmed([r, g, b]: [u8; 3]) -> [u8; 3] {
    let grey = (77 * u32::from(r) + 150 * u32::from(g) + 29 * u32::from(b)) / 256;
    [r, g, b].map(|c| ((u32::from(c) + 3 * grey) / 8) as u8)
}

/// An 8-bit RGBA image, rows top to bottom, always opaque.
#[derive(Clone, PartialEq, Eq)]
pub struct Image {
    size: [u32; 2],
    rgba: Vec<u8>,
}

impl std::fmt::Debug for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Image")
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}

impl Image {
    fn filled(size: [u32; 2], [r, g, b]: [u8; 3]) -> Image {
        let pixels = size[0] as usize * size[1] as usize;
        Image {
            size,
            rgba: [r, g, b, 0xff].repeat(pixels),
        }
    }

    /// Width and height in pixels.
    pub fn size(&self) -> [u32; 2] {
        self.size
    }

    /// The pixels, four bytes each.
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    /// One pixel's colour. Panics outside the image.
    pub fn pixel(&self, [i, j]: [u32; 2]) -> [u8; 3] {
        let at = self.index(i, j);
        [self.rgba[at], self.rgba[at + 1], self.rgba[at + 2]]
    }

    fn index(&self, i: u32, j: u32) -> usize {
        assert!(
            i < self.size[0] && j < self.size[1],
            "pixel ({i}, {j}) outside the image"
        );
        (j as usize * self.size[0] as usize + i as usize) * 4
    }

    fn set(&mut self, [i, j]: [u32; 2], [r, g, b]: [u8; 3]) {
        let at = self.index(i, j);
        self.rgba[at..at + 3].copy_from_slice(&[r, g, b]);
    }

    /// The pixel, if `(x, y)` is inside the image.
    fn inside(&self, [x, y]: [i64; 2]) -> Option<[u32; 2]> {
        let fits = |v: i64, n: u32| (0..i64::from(n)).contains(&v);
        (fits(x, self.size[0]) && fits(y, self.size[1])).then_some([x as u32, y as u32])
    }

    /// Sets each pixel `(dx, dy)` from `centre` that `shape` gives a colour for, clipped.
    fn stamp(&mut self, centre: [i64; 2], reach: i64, shape: impl Fn(i64, i64) -> Option<[u8; 3]>) {
        for dy in -reach..=reach {
            for dx in -reach..=reach {
                if let (Some(colour), Some(pixel)) =
                    (shape(dx, dy), self.inside([centre[0] + dx, centre[1] + dy]))
                {
                    self.set(pixel, colour);
                }
            }
        }
    }

    /// Blends `colour` at `alpha` over pixels `min..=max`, clipped.
    fn blend_rect(&mut self, min: [i64; 2], max: [i64; 2], colour: [u8; 3], alpha: u32) {
        let clip = |v: i64, n: u32| v.clamp(0, i64::from(n) - 1) as u32;
        if max[0] < 0
            || max[1] < 0
            || min[0] >= i64::from(self.size[0])
            || min[1] >= i64::from(self.size[1])
        {
            return;
        }
        for j in clip(min[1], self.size[1])..=clip(max[1], self.size[1]) {
            for i in clip(min[0], self.size[0])..=clip(max[0], self.size[0]) {
                let below = self.pixel([i, j]);
                let mixed = [0, 1, 2].map(|c| {
                    ((u32::from(colour[c]) * alpha + u32::from(below[c]) * (255 - alpha) + 127)
                        / 255) as u8
                });
                self.set([i, j], mixed);
            }
        }
    }

    /// Writes the image as an 8-bit RGB PNG with no ancillary chunks.
    pub fn write_png(&self, out: impl io::Write) -> io::Result<()> {
        let mut encoder = png::Encoder::new(out, self.size[0], self.size[1]);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let (pixels, _) = self.rgba.as_chunks::<4>();
        let rgb: Vec<u8> = pixels.iter().flat_map(|&[r, g, b, _]| [r, g, b]).collect();
        let mut writer = encoder.write_header().map_err(io::Error::other)?;
        writer.write_image_data(&rgb).map_err(io::Error::other)?;
        writer.finish().map_err(io::Error::other)
    }
}

/// A structure marker: a 9 × 9 square with a black border, hollow for a candidate.
fn structure_marker(colour: [u8; 3], certain: bool) -> impl Fn(i64, i64) -> Option<[u8; 3]> {
    move |dx, dy| match dx.abs().max(dy.abs()) {
        4 => Some(BLACK),
        2 | 3 => Some(colour),
        _ if certain => Some(colour),
        _ => None,
    }
}

/// The spawn marker: a white cross over a wider black one.
fn spawn_marker(dx: i64, dy: i64) -> Option<[u8; 3]> {
    let (ax, ay) = (dx.abs(), dy.abs());
    if (ax == 0 && ay <= 4) || (ay == 0 && ax <= 4) {
        Some(WHITE)
    } else if (ax <= 1 && ay <= 5) || (ay <= 1 && ax <= 5) {
        Some(BLACK)
    } else {
        None
    }
}

/// `i64` to `i32`, saturating.
fn saturate(v: i64) -> i32 {
    v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

impl Session<'_> {
    /// The biome under each pixel of `area` (`docs/specs/render.md#biomes`).
    pub fn biome_map(&self, area: &MapArea) -> Result<BiomeMap> {
        area.check()?;
        let s = i64::from(area.scale);
        let step = (area.scale / 4).max(1);
        let origin = area.min.map(|m| m >> 2);
        // Each pixel's sample quart, as a grid index along one axis.
        let axis = |d: usize| -> Vec<u32> {
            (0..area.size[d])
                .map(|i| {
                    let block = i64::from(area.min[d]) + i64::from(i) * s;
                    (((block >> 2) - i64::from(origin[d])) / i64::from(step)) as u32
                })
                .collect()
        };
        let (columns, rows) = (axis(0), axis(1));
        let grid = [columns[columns.len() - 1] + 1, rows[rows.len() - 1] + 1];
        let lookups = self
            .biomes
            .biome_grid(origin, area.y >> 2, step as i32, grid);
        let mut ties = BTreeMap::new();
        let cells = lookups
            .into_iter()
            .enumerate()
            .map(|(i, lookup)| match lookup {
                Lookup::Unique(b) => b,
                Lookup::Tied(set) => {
                    let first = set[0];
                    ties.insert(i, set);
                    first
                }
            })
            .collect();
        Ok(BiomeMap {
            world: self.world.id,
            area: *area,
            columns,
            rows,
            grid_width: grid[0] as usize,
            cells,
            ties,
        })
    }

    /// The structure starts, slime chunks and spawn that `area` draws with `layers`
    /// (`docs/specs/render.md#layers`): structures and the spawn whose marker reaches the
    /// area, and slime chunks that overlap it.
    pub fn map_features(&self, area: &MapArea, layers: &Layers) -> Result<MapFeatures> {
        area.check()?;
        let mut features = MapFeatures::default();
        if !layers.structures.is_empty() {
            let mut targets = Vec::new();
            for query in &layers.structures {
                self.check_world(query.world, &query.argument)?;
                for &t in &query.targets {
                    if !targets.contains(&t) {
                        targets.push(t);
                    }
                }
            }
            let [[x0, x1], [z0, z1]] = area.reach_blocks();
            let mut starts = self.starts_by_set(&targets, |offset| {
                // The chunks whose position, chunk·16 + offset, is in reach.
                let chunk = |b: i64, o: i32| saturate((b - i64::from(o)).div_euclid(16));
                (
                    [chunk(x0, offset[0]), chunk(z0, offset[2])],
                    [chunk(x1, offset[0]), chunk(z1, offset[2])],
                )
            });
            starts.retain(|s| area.reaches([s.position[0], s.position[2]]));
            starts.sort_by(|a, b| {
                (&a.set, a.chunk[1], a.chunk[0]).cmp(&(&b.set, b.chunk[1], b.chunk[0]))
            });
            features.structures = starts;
        }
        if layers.slime_chunks {
            if area.scale > MAX_SLIME_SCALE {
                return Err(invalid(format!(
                    "slime chunks are drawn at scales up to {MAX_SLIME_SCALE}, not {}",
                    area.scale
                )));
            }
            let end = area.end();
            let min = area.min.map(|m| m >> 4);
            let max = [0, 1].map(|d| saturate((end[d] - 1) >> 4));
            features.slime_chunks = self.slime_chunks(min, max);
        }
        if layers.spawn {
            let origin = self.spawn_origin();
            if area.reaches(origin.map_block()) {
                features.spawn = Some(origin);
            }
        }
        Ok(features)
    }

    /// `area` drawn with `layers` (`docs/specs/render.md`).
    pub fn render(&self, area: &MapArea, layers: &Layers) -> Result<Image> {
        let features = self.map_features(area, layers)?;
        let mut image = if layers.biomes {
            self.biome_map(area)?
                .colorize(self.world, layers.ties, &layers.highlight)?
        } else {
            Image::background(area.size)
        };
        image.draw_features(area, &features);
        Ok(image)
    }
}

impl Image {
    /// The background drawn when the biome layer is off.
    pub fn background(size: [u32; 2]) -> Image {
        Image::filled(size, BACKGROUND)
    }

    /// Draws `features` over the image of `area`, which must be the image's size: slime chunks,
    /// then structure markers, then the spawn. [`Session::render`] is
    /// [`BiomeMap::colorize`] (or [`background`](Self::background)), then this, with
    /// [`Session::map_features`]; front ends that cache biome maps can do the steps
    /// themselves, and leave out features they draw another way.
    pub fn draw_features(&mut self, area: &MapArea, features: &MapFeatures) {
        assert_eq!(self.size, area.size, "an image of the area");
        for &[x, z] in &features.slime_chunks {
            let min = area.pixel([x.wrapping_shl(4), z.wrapping_shl(4)]);
            let max = area.pixel([
                x.wrapping_shl(4).wrapping_add(15),
                z.wrapping_shl(4).wrapping_add(15),
            ]);
            self.blend_rect(min, max, SLIME, SLIME_ALPHA);
        }
        for start in &features.structures {
            let centre = area.pixel([start.position[0], start.position[2]]);
            let marker = structure_marker(structure_colour(&start.set), start.certain);
            self.stamp(centre, 4, marker);
        }
        if let Some(origin) = &features.spawn {
            self.stamp(area.pixel(origin.map_block()), 5, spawn_marker);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn areas_are_checked() {
        let area = |min, size, scale| MapArea {
            min,
            size,
            scale,
            y: 64,
        };
        assert!(area([0, 0], [256, 256], 4).check().is_ok());
        assert!(area([-256, 512], [1, 1], 256).check().is_ok());
        assert!(area([0, 0], [256, 256], 3).check().is_err());
        assert!(area([0, 0], [256, 256], 512).check().is_err());
        assert!(area([2, 0], [256, 256], 4).check().is_err());
        assert!(area([0, 0], [0, 256], 1).check().is_err());
        assert!(area([0, 0], [MAX_SIDE + 1, 1], 1).check().is_err());
        assert!(area([0, 0], [MAX_SIDE, MAX_SIDE], 1).check().is_err());
        assert!(area([i32::MAX - 255, 0], [256, 1], 1).check().is_ok());
        assert!(area([i32::MAX - 255, 0], [257, 1], 1).check().is_err());
    }

    #[test]
    fn centred_areas_put_the_centre_in_the_middle_pixel() {
        for (centre, size, scale) in [
            ([0, 0], [256, 256], 4),
            ([-1, 7], [101, 3], 16),
            ([1000, -1000], [1, 1], 256),
        ] {
            let area = MapArea::centred_on(centre, size, scale, 64).unwrap();
            assert_eq!(
                area.pixel(centre),
                size.map(|s| i64::from(s / 2)),
                "{area:?}"
            );
            assert!(area.check().is_ok());
        }
        assert!(MapArea::centred_on([i32::MIN, 0], [256, 256], 256, 64).is_err());
    }

    #[test]
    fn pixels_and_blocks_round_trip() {
        let area = MapArea {
            min: [-64, 32],
            size: [10, 10],
            scale: 16,
            y: 64,
        };
        assert_eq!(area.block([0, 0]), [-64, 32]);
        assert_eq!(area.block([3, 1]), [-16, 48]);
        assert_eq!(area.pixel([-16, 48]), [3, 1]);
        assert_eq!(area.pixel([-1, 47]), [3, 0]);
        assert_eq!(area.pixel([-65, 32]), [-1, 0]);
    }

    #[test]
    fn dimming_and_blending_are_integer() {
        assert_eq!(dimmed([255, 255, 255]), [127, 127, 127]);
        assert_eq!(dimmed([0, 0, 0]), [0, 0, 0]);
        let mut image = Image::filled([2, 1], [0, 0, 0]);
        image.blend_rect([0, 0], [0, 0], [255, 255, 255], 128);
        assert_eq!(image.pixel([0, 0]), [128, 128, 128]);
        assert_eq!(image.pixel([1, 0]), [0, 0, 0]);
        // Entirely outside: nothing changes.
        image.blend_rect([5, 0], [9, 0], [255, 255, 255], 255);
        image.blend_rect([-9, 0], [-1, 0], [255, 255, 255], 255);
        assert_eq!(image.pixel([1, 0]), [0, 0, 0]);
    }

    #[test]
    fn markers_have_the_specified_shapes() {
        let mut image = Image::filled([9, 9], [1, 1, 1]);
        image.stamp([4, 4], 4, structure_marker([9, 9, 9], false));
        assert_eq!(image.pixel([0, 0]), BLACK);
        assert_eq!(image.pixel([1, 4]), [9, 9, 9]);
        assert_eq!(image.pixel([4, 4]), [1, 1, 1]);
        image.stamp([4, 4], 4, structure_marker([9, 9, 9], true));
        assert_eq!(image.pixel([4, 4]), [9, 9, 9]);
        // Clipped at the edges without panicking.
        image.stamp([-3, 10], 5, spawn_marker);
        let mut image = Image::filled([11, 11], [1, 1, 1]);
        image.stamp([5, 5], 5, spawn_marker);
        assert_eq!(image.pixel([5, 5]), WHITE);
        assert_eq!(image.pixel([1, 5]), WHITE);
        assert_eq!(image.pixel([0, 5]), BLACK);
        assert_eq!(image.pixel([5, 6]), WHITE);
        assert_eq!(image.pixel([4, 6]), BLACK);
        assert_eq!(image.pixel([0, 0]), [1, 1, 1]);
    }

    #[test]
    fn png_has_the_image() {
        let mut image = Image::filled([3, 2], [10, 20, 30]);
        image.set([2, 1], [200, 100, 50]);
        let mut bytes = Vec::new();
        image.write_png(&mut bytes).unwrap();
        let decoder = png::Decoder::new(io::Cursor::new(bytes));
        let mut reader = decoder.read_info().unwrap();
        let mut buffer = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buffer).unwrap();
        assert_eq!((info.width, info.height), (3, 2));
        assert_eq!(info.color_type, png::ColorType::Rgb);
        assert_eq!(&buffer[..3], &[10, 20, 30]);
        assert_eq!(&buffer[15..18], &[200, 100, 50]);
    }
}
