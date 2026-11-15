//! The flower bone meal grows on grass at a block (`docs/specs/flowers.md`): the biome there as
//! the game's level gives it, that biome's bone meal features, and each flower's chance.

use spyglass_biome::{BiomeId, Lookup};
use spyglass_blocks::zoom::BiomeZoom;
use spyglass_feature::FeatureSettings;
use spyglass_feature::flowers::{Chance, FlowerOdds, Flowers};

use crate::{BiomeAt, Cause, Error, Result, Session, World};

/// What bone meal grows on grass at a block.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FlowerAt {
    /// The block the flower would stand in, one above the grass.
    pub pos: [i32; 3],
    /// One answer for each biome the game may give at the block: one, or each of a tied
    /// cell's, spyglass's own first.
    pub biomes: Vec<FlowerBiome>,
}

impl FlowerAt {
    /// The answer for spyglass's biome, the first.
    pub fn first(&self) -> &FlowerBiome {
        &self.biomes[0]
    }

    /// The one flower that certainly grows here, whatever the biome lookup gives: `None` where
    /// it's down to chance, or there's no flower.
    pub fn certain(&self) -> Option<&str> {
        let first = self.first().certain()?;
        self.biomes
            .iter()
            .all(|b| b.certain() == Some(first))
            .then_some(first)
    }
}

/// What bone meal grows at a block in one biome.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FlowerBiome {
    /// The biome, e.g. `minecraft:flower_forest`.
    pub biome: String,
    /// Its bone meal features, in the game's order with its repeats: bone meal picks one of
    /// them, each as likely.
    pub features: Vec<BoneMealFeature>,
    /// The flowers it can grow, each with its chance given that bone meal makes a flower here,
    /// most likely first. Empty where the biome has no bone meal features.
    pub flowers: Vec<FlowerChance>,
}

impl FlowerBiome {
    /// The flower that's certain here, if one is.
    pub fn certain(&self) -> Option<&str> {
        match &self.flowers[..] {
            [only] if only.chance.is_certain() => Some(&only.flower),
            _ => None,
        }
    }

    /// The chance of `flower` (a block id, every state of it together), 0 if it can't grow.
    pub fn chance_of(&self, flower: &str) -> Chance {
        self.flowers
            .iter()
            .filter(|f| block_id(&f.flower) == flower)
            .fold(Chance::new_zero(), |sum, f| sum.plus(f.chance))
    }
}

/// A bone meal feature a biome can pick.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct BoneMealFeature {
    /// Its id, e.g. `minecraft:flower_plain`.
    pub id: String,
    /// Its block state provider's type, e.g. `minecraft:noise_threshold`.
    pub provider: String,
    /// Whether what it places depends on where (a noise), not only on chance.
    pub by_position: bool,
}

/// A flower and its chance.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct FlowerChance {
    /// The block state, its id then any non-default properties: `minecraft:poppy`,
    /// `minecraft:pink_petals[flower_amount=2]`.
    pub flower: String,
    /// Its chance, exact.
    pub chance: Chance,
}

/// A state's block id: its name without properties.
pub(crate) fn block_id(state: &str) -> &str {
    state.split_once('[').map_or(state, |(id, _)| id)
}

impl World {
    /// Every biome's bone meal features, loaded the first time they're asked for.
    pub(crate) fn flowers(&self) -> Result<&Flowers> {
        if let Some(flowers) = self.flowers.get() {
            return Ok(flowers);
        }
        let v1_21 = self
            .bundle
            .resource("worldgen/configured_feature", "minecraft:monster_room")
            .is_ok();
        let settings = FeatureSettings::load(&self.bundle, self.source.biome_names(), v1_21)?;
        let flowers = Flowers::load(&self.bundle, &settings)?;
        Ok(self.flowers.get_or_init(|| flowers))
    }
}

impl World {
    /// Fails unless `flower` (a block id) is a flower some bone meal feature can place.
    pub(crate) fn check_flower(&self, flower: &str) -> Result<()> {
        let flowers = self.flowers()?;
        if flowers.states().any(|(_, name)| block_id(name) == flower) {
            Ok(())
        } else {
            Err(Error::Unknown {
                what: "flower",
                id: flower.to_string(),
            })
        }
    }

    /// Every flower some bone meal feature can place, by block id, sorted.
    pub fn bone_meal_flowers(&self) -> Result<Vec<String>> {
        let mut ids: Vec<String> = self
            .flowers()?
            .states()
            .map(|(_, name)| block_id(name).to_string())
            .collect();
        ids.sort();
        ids.dedup();
        Ok(ids)
    }
}

impl Session<'_> {
    /// The biome block `pos` has in the world, as the game's level gives it
    /// (`ServerLevel.getBiome`): `BiomeManager`'s zoom picks one of the nearby quarts, with a
    /// seed-dependent offset, and the quart's y is clamped to the level. This is the biome the
    /// F3 screen shows, and the one bone meal reads. [`Session::biome_at`] gives the quart's.
    pub fn biome_at_block(&self, pos: [i32; 3]) -> BiomeAt {
        self.biome_names(&self.block_lookup(&BiomeZoom::new(self.seed), pos))
    }

    /// The biome lookup at block `pos`, through `zoom` (the seed's).
    pub(crate) fn block_lookup(&self, zoom: &BiomeZoom, pos: [i32; 3]) -> Lookup {
        let [qx, qy, qz] = self.block_quart(zoom, pos);
        self.biomes.biome_at(qx, qy, qz)
    }

    /// The quart block `pos` takes its biome from, through `zoom`, with its y clamped to the
    /// level as the level's chunks clamp it.
    pub(crate) fn block_quart(&self, zoom: &BiomeZoom, pos: [i32; 3]) -> [i32; 3] {
        let [qx, qy, qz] = zoom.quart(pos);
        let terrain = &self.world.terrain;
        let min_qy = terrain.level_min_y >> 2;
        [
            qx,
            qy.clamp(min_qy, min_qy + (terrain.level_height >> 2) - 1),
            qz,
        ]
    }

    /// What bone meal grows on grass at `pos`, the block above the grass: for each biome the
    /// game may give there, its bone meal features and each flower's chance
    /// (`docs/specs/flowers.md`). The flower bone meal makes depends on the block and its
    /// biome, not on where the player clicks: a click grows flowers on the grass around it.
    ///
    /// # Errors
    ///
    /// When the bundle's flower features can't be read, or a biome there picks a feature
    /// spyglass can't answer for.
    pub fn flower_at(&self, pos: [i32; 3]) -> Result<FlowerAt> {
        let flowers = self.world.flowers()?;
        let lookup = self.block_lookup(&BiomeZoom::new(self.seed), pos);
        let biomes = lookup
            .candidates()
            .iter()
            .map(|&biome| self.flower_biome(flowers, biome, pos))
            .collect::<Result<_>>()?;
        Ok(FlowerAt { pos, biomes })
    }

    /// The answer at `pos` in `biome`.
    fn flower_biome(
        &self,
        flowers: &Flowers,
        biome: BiomeId,
        pos: [i32; 3],
    ) -> Result<FlowerBiome> {
        let features = flowers
            .biome_features(biome)
            .iter()
            .map(|&f| {
                let feature = &flowers.features()[f];
                BoneMealFeature {
                    id: feature.id.clone(),
                    provider: feature.provider_type.clone(),
                    by_position: feature.picks_by_position(),
                }
            })
            .collect();
        let odds = flowers
            .odds(biome, pos)
            .map_err(|e| Error::Data(Cause(Box::new(e))))?;
        Ok(FlowerBiome {
            biome: self.world.source.name(biome).to_string(),
            features,
            flowers: flower_chances(flowers, &odds),
        })
    }
}

/// Odds as named flowers, most likely first, then by name.
pub(crate) fn flower_chances(flowers: &Flowers, odds: &[FlowerOdds]) -> Vec<FlowerChance> {
    sorted(
        odds.iter()
            .map(|o| FlowerChance {
                flower: flowers.state_name(o.state).to_string(),
                chance: o.chance,
            })
            .collect(),
    )
}

/// Flowers, most likely first, then by name.
pub(crate) fn sorted(mut flowers: Vec<FlowerChance>) -> Vec<FlowerChance> {
    flowers.sort_by(|a, b| {
        // Exact: a/b > c/d when a·d > c·b, both far inside u128 (ADR 0075).
        let cross = |x: Chance, y: Chance| {
            x.numerator
                .checked_mul(y.denominator)
                .expect("chances' cross products fit u128")
        };
        cross(b.chance, a.chance)
            .cmp(&cross(a.chance, b.chance))
            .then_with(|| a.flower.cmp(&b.flower))
    });
    flowers
}
