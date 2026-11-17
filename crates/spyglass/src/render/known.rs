//! Pixels already known for an area, from maps at other scales
//! (`docs/specs/render.md#reuse-across-scales`). Every map pixel is its corner block's answer
//! alone, so a pixel at one scale is the same pixel at any scale where its corner block is a
//! pixel corner: a 1:2 map's pixels are every other pixel, on both axes, of the 1:1 maps under
//! it. A front end that has maps at one scale can hand their pixels to a map call at another,
//! which then computes only the rest.

use std::fmt;

use super::{MapArea, invalid};
use crate::{Error, Result};

pub(crate) mod sealed {
    use std::fmt;

    use super::MapArea;

    /// A map whose pixels can be reused: one answer per pixel, from its corner block alone.
    pub trait Pixels {
        /// One pixel's answer.
        type Pixel: Clone + fmt::Debug;
        /// What two maps must share for their pixels to be the same, besides the world: the
        /// height sampled where it matters, whether biomes are at the surface, which
        /// underground layers.
        type Kind: Copy + PartialEq + fmt::Debug;
        /// Pixels the map holds beyond its west and north edges: a height map's edge row.
        const EDGE: u32;
        /// The world it was made in.
        fn world(&self) -> u64;
        /// Its area.
        fn map_area(&self) -> &MapArea;
        /// What its pixels are of.
        fn kind(&self) -> Self::Kind;
        /// The answer at pixel `p`, from `-EDGE` on each axis.
        fn pixel_at(&self, p: [i64; 2]) -> Self::Pixel;
    }
}

use sealed::Pixels;

/// The pixels of an area already known from other maps of the same kind
/// (`docs/specs/render.md#reuse-across-scales`): biome maps sampled at the same height, surface
/// biome maps, height maps (the edge row too), carving maps, underground maps with the same
/// layers at the same height, or flower maps at the same height. Hand it to a `…_with` map call
/// ([`Session::biome_map_with`](crate::Session::biome_map_with) and the rest), which computes
/// only the pixels it lacks; with every pixel known, it computes nothing.
pub struct Known<M: Pixels> {
    /// The area the pixels are for.
    area: MapArea,
    /// The world and kind of the maps added, once one has been.
    source: Option<(u64, M::Kind)>,
    /// Each pixel's answer, if known: `(width + EDGE) × (height + EDGE)` from pixel `-EDGE` on
    /// each axis, x fastest.
    pixels: Vec<Option<M::Pixel>>,
    /// How many pixels aren't known.
    missing: usize,
}

impl<M: Pixels> fmt::Debug for Known<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Known")
            .field("area", &self.area)
            .field("source", &self.source)
            .field("missing", &self.missing)
            .finish_non_exhaustive()
    }
}

impl<M: Pixels> Clone for Known<M> {
    fn clone(&self) -> Self {
        Known {
            area: self.area,
            source: self.source,
            pixels: self.pixels.clone(),
            missing: self.missing,
        }
    }
}

impl<M: Pixels> Known<M> {
    /// Nothing known yet about `area`'s pixels.
    pub fn new(area: &MapArea) -> Result<Known<M>> {
        area.check()?;
        let [w, h] = Self::side(area);
        let count = w * h;
        Ok(Known {
            area: *area,
            source: None,
            pixels: vec![None; count],
            missing: count,
        })
    }

    /// Pixels on a side, the edge included.
    fn side(area: &MapArea) -> [usize; 2] {
        area.size.map(|s| (s + M::EDGE) as usize)
    }

    /// The area the pixels are for.
    pub fn area(&self) -> &MapArea {
        &self.area
    }

    /// How many of the area's pixels aren't known, a height map's edge row included.
    pub fn missing(&self) -> usize {
        self.missing
    }

    /// Whether every pixel is known, so a map call computes nothing.
    pub fn is_complete(&self) -> bool {
        self.missing == 0
    }

    /// Adds the pixels `map` has: each pixel of the area whose corner block is one of `map`'s
    /// pixel corners, at any scale. A map of another world is [`Error::OtherWorld`]; one of
    /// another kind (sampled at another height, or with other underground layers) than those
    /// added before is [`Error::InvalidMap`].
    pub fn add(&mut self, map: &M) -> Result<()> {
        match self.source {
            Some((world, _)) if world != map.world() => {
                return Err(Error::OtherWorld("a map".to_string()));
            }
            Some((_, kind)) if kind != map.kind() => {
                return Err(invalid(
                    "a map of another height or other layers than those known",
                ));
            }
            _ => self.source = Some((map.world(), map.kind())),
        }
        let from = map.map_area();
        let edge = i64::from(M::EDGE);
        // Along one axis, each of our pixels' position in `map`, if its corner is one of
        // `map`'s.
        let along = |d: usize| -> Vec<Option<i64>> {
            (-edge..i64::from(self.area.size[d]))
                .map(|p| {
                    let block = i64::from(self.area.min[d]) + p * i64::from(self.area.scale);
                    let offset = block - i64::from(from.min[d]);
                    let scale = i64::from(from.scale);
                    let q = offset.div_euclid(scale);
                    (offset.rem_euclid(scale) == 0 && (-edge..i64::from(from.size[d])).contains(&q))
                        .then_some(q)
                })
                .collect()
        };
        let (xs, zs) = (along(0), along(1));
        let width = xs.len();
        for (j, qz) in zs.iter().enumerate() {
            let Some(qz) = *qz else { continue };
            for (i, qx) in xs.iter().enumerate() {
                let Some(qx) = *qx else { continue };
                let slot = &mut self.pixels[j * width + i];
                if slot.is_none() {
                    *slot = Some(map.pixel_at([qx, qz]));
                    self.missing -= 1;
                }
            }
        }
        Ok(())
    }

    /// The answer at pixel `p`, from `-EDGE` on each axis, if it's known.
    pub(crate) fn get(&self, [i, j]: [i64; 2]) -> Option<&M::Pixel> {
        let edge = i64::from(M::EDGE);
        let width = i64::from(self.area.size[0]) + edge;
        self.pixels[((j + edge) * width + i + edge) as usize].as_ref()
    }

    /// Fails unless these pixels can go into a map of `area` and `kind` in `world`.
    pub(crate) fn check_for(&self, world: u64, area: &MapArea, kind: M::Kind) -> Result<()> {
        let place = |a: &MapArea| (a.min, a.size, a.scale);
        if place(&self.area) != place(area) {
            return Err(invalid("the known pixels are for another area"));
        }
        match self.source {
            Some((w, _)) if w != world => Err(Error::OtherWorld("known pixels".to_string())),
            Some((_, k)) if k != kind => Err(invalid(
                "the known pixels are of another height or other layers",
            )),
            _ => Ok(()),
        }
    }
}

/// The answer at pixel `[i, j]` (from 0) in `known`, if there is one.
pub(crate) fn known_at<M: Pixels>(known: Option<&Known<M>>, [i, j]: [u32; 2]) -> Option<&M::Pixel> {
    known.and_then(|k| k.get([i64::from(i), i64::from(j)]))
}
