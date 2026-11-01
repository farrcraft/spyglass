//! A chunk's terrain in outline, before its blocks are built: where a block could be air, and
//! where it surely isn't solid, from the noise fill's density, every position a carver could
//! carve, and the biomes where the material rule gives air or its extensions add blocks.
//! Enough to rule out a feature without placing it (`docs/specs/features.md#ruling-out`).

use std::collections::HashMap;

use spyglass_biome::BiomeId;
use spyglass_blocks::ChunkGenerator;
use spyglass_blocks::carver::CarvingMask;

/// One seed's terrain in outline, chunk by chunk as it's asked about.
pub struct Outline<'g> {
    generator: &'g ChunkGenerator<'g>,
    /// The level's lowest y and the y above its top.
    min_y: i32,
    top: i32,
    /// The biomes where the material rule can give air.
    air: Vec<BiomeId>,
    /// The biomes whose extensions add blocks.
    extension: Vec<BiomeId>,
    /// By chunk: every position a carver could carve.
    carvable: HashMap<[i32; 2], CarvingMask>,
    /// By chunk: whether a biome it can store has the rule's air, and whether one extends the
    /// terrain.
    biomes: HashMap<[i32; 2], (bool, bool)>,
}

/// The outline of a box of blocks, from [`Outline::shape`].
pub struct Shape {
    min: [i32; 3],
    size: [i32; 3],
    /// By position, x outer, then z, then y: whether it could be air.
    air: Vec<bool>,
    /// By position: whether it surely isn't solid.
    open: Vec<bool>,
}

impl Shape {
    fn index(&self, [x, y, z]: [i32; 3]) -> Option<usize> {
        let offset = [x - self.min[0], y - self.min[1], z - self.min[2]];
        (0..3)
            .all(|i| (0..self.size[i]).contains(&offset[i]))
            .then(|| ((offset[0] * self.size[2] + offset[2]) * self.size[1] + offset[1]) as usize)
    }

    /// Whether the block at `pos` could be air before features: outside the level, where the
    /// noise fill leaves no default block, where a carver could carve, or in a chunk where the
    /// material rule could give air. Outside the box, it could.
    pub fn maybe_air(&self, pos: [i32; 3]) -> bool {
        self.index(pos).is_none_or(|i| self.air[i])
    }

    /// Whether the block at `pos` surely isn't solid before features: outside the level, or
    /// where the noise fill leaves air or a fluid and no extension can add a block. Outside the
    /// box, it isn't sure.
    pub fn never_solid(&self, pos: [i32; 3]) -> bool {
        self.index(pos).is_some_and(|i| self.open[i])
    }
}

impl<'g> Outline<'g> {
    /// The outline of `generator`'s terrain, or `None` where it can't be drawn: 26.2's terrain,
    /// or a material rule that can give air in any biome.
    pub fn new(generator: &'g ChunkGenerator<'g>) -> Option<Self> {
        let (min_y, top) = generator.level();
        generator.densities([0, min_y, 0], [1, 2, 1])?;
        Some(Outline {
            generator,
            min_y,
            top,
            air: generator.rule_air_biomes()?,
            extension: generator.extension_biomes(),
            carvable: HashMap::new(),
            biomes: HashMap::new(),
        })
    }

    /// The outline of the box from `min` to `max`, inclusive.
    pub fn shape(&mut self, min: [i32; 3], max: [i32; 3]) -> Shape {
        let size = [0, 1, 2].map(|i| max[i] - min[i] + 1);
        let len = size.iter().map(|&s| s.max(0) as usize).product();
        let mut shape = Shape {
            min,
            size,
            air: vec![true; len],
            open: vec![true; len],
        };
        if len == 0 {
            return shape;
        }
        // The noise fill, within the level.
        let (lo, hi) = (min[1].max(self.min_y), max[1].min(self.top - 1));
        if lo <= hi {
            let part = [size[0], hi - lo + 1, size[2]];
            let densities = self
                .generator
                .densities([min[0], lo, min[2]], part)
                .expect("the outline's terrain has densities");
            // Volume order: y fastest, then x, then z.
            for (i, density) in densities.iter().enumerate() {
                let (dy, dx, dz) = (
                    i as i32 % part[1],
                    (i as i32 / part[1]) % part[0],
                    i as i32 / (part[1] * part[0]),
                );
                let pos = [min[0] + dx, lo + dy, min[2] + dz];
                let at = shape.index(pos).expect("within the box");
                let (rule_air, extended) = self.biomes([pos[0] >> 4, pos[2] >> 4]);
                // AquiferSubstance: the default block where the density is positive.
                let filled = f64::from(*density) > 0.0;
                shape.air[at] = !filled || rule_air || self.carved(pos);
                shape.open[at] = !filled && !extended;
            }
        }
        shape
    }

    /// Whether a carver could carve `pos`.
    fn carved(&mut self, [x, y, z]: [i32; 3]) -> bool {
        let chunk = [x >> 4, z >> 4];
        let generator = self.generator;
        self.carvable
            .entry(chunk)
            .or_insert_with(|| generator.carvable(chunk[0], chunk[1]))
            .is_carved(x & 15, y, z & 15)
    }

    /// Whether a biome chunk `chunk` can store has the rule's air, and whether one extends its
    /// terrain.
    fn biomes(&mut self, chunk: [i32; 2]) -> (bool, bool) {
        let (generator, air, extension) = (self.generator, &self.air, &self.extension);
        *self.biomes.entry(chunk).or_insert_with(|| {
            if air.is_empty() && extension.is_empty() {
                return (false, false);
            }
            let biomes = generator.chunk_biomes(chunk[0], chunk[1]);
            let any = |set: &[BiomeId]| biomes.iter().any(|b| set.contains(b));
            (any(air), any(extension))
        })
    }
}
