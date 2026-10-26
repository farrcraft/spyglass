//! Ruined portals (`RuinedPortalStructure`): the setup, template and placement drawn from the
//! context random, and the check's height from terrain (`findSuitableY`). See
//! `docs/specs/structure-placement.md`.

use serde_json::{Map, Value};
use spyglass_data::codec::{At, LoadErrorKind, LoadResult, field, float, object};
use spyglass_rng::{LegacyRandomSource, RandomSource};
use spyglass_terrain::{Heightmap, Terrain};

use crate::jigsaw::{JigsawLoader, Rotation};

/// `RuinedPortalPiece.VerticalPlacement`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerticalPlacement {
    /// `on_land_surface`
    OnLandSurface,
    /// `partly_buried`
    PartlyBuried,
    /// `on_ocean_floor`
    OnOceanFloor,
    /// `in_mountain`
    InMountain,
    /// `underground`
    Underground,
    /// `in_nether`
    InNether,
}

/// `RuinedPortalStructure.Setup`, the fields the check uses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Setup {
    /// `placement`.
    pub placement: VerticalPlacement,
    /// `air_pocket_probability`, 0 to 1.
    pub air_pocket_probability: f32,
    /// `weight`.
    pub weight: f32,
}

/// A ruined portal structure's setups, and the sizes of the templates it draws from.
#[derive(Debug, Clone, PartialEq)]
pub struct RuinedPortal {
    /// `setups`, in file order, which the weighted pick walks.
    pub setups: Vec<Setup>,
    /// `STRUCTURE_LOCATION_PORTALS`' sizes: `ruined_portal/portal_1` to `portal_10`.
    portals: Vec<[i32; 3]>,
    /// `STRUCTURE_LOCATION_GIANT_PORTALS`' sizes: `ruined_portal/giant_portal_1` to `_3`.
    giants: Vec<[i32; 3]>,
}

impl RuinedPortal {
    /// `RuinedPortalStructure.CODEC`'s `setups`, and the templates the structure draws from.
    pub(crate) fn load(
        obj: &Map<String, Value>,
        at: &At<'_>,
        templates: &mut JigsawLoader<'_>,
    ) -> LoadResult<Self> {
        let setups_at = at.child("setups");
        let list = field(obj, "setups", at)?
            .as_array()
            .ok_or_else(|| setups_at.error(LoadErrorKind::Shape("a list of setups")))?;
        let setups = list
            .iter()
            .enumerate()
            .map(|(i, json)| setup(json, &setups_at.child(&format!("[{i}]"))))
            .collect::<LoadResult<Vec<_>>>()?;
        if setups.is_empty() {
            return Err(setups_at.error(LoadErrorKind::Invalid("no setups")));
        }
        let mut sizes = |prefix: &str, n: usize| {
            (1..=n)
                .map(|i| {
                    templates
                        .template(&format!("minecraft:ruined_portal/{prefix}_{i}"))
                        .map(|t| t.size)
                })
                .collect::<LoadResult<Vec<_>>>()
        };
        Ok(RuinedPortal {
            setups,
            portals: sizes("portal", 10)?,
            giants: sizes("giant_portal", 3)?,
        })
    }

    /// `findGenerationPoint` up to the stub, drawing from `random` (the context random) in the
    /// game's order: `(minX, y, minZ)` with `y` from `findSuitableY` over `terrain`.
    pub(crate) fn check_position(
        &self,
        chunk: [i32; 2],
        terrain: &Terrain,
        level_min_y: i32,
        random: &mut LegacyRandomSource,
    ) -> [i32; 3] {
        let setup = self.choose_setup(random);
        let air_pocket = sample(random, setup.air_pocket_probability);
        let size = if random.next_float() < 0.05 {
            self.giants[random.next_int_bounded(self.giants.len() as i32) as usize]
        } else {
            self.portals[random.next_int_bounded(self.portals.len() as i32) as usize]
        };
        let rotation = Rotation::random(random);
        let front_back = random.next_float() >= 0.5;
        // StructureTemplate.getBoundingBox(chunk origin, rotation, pivot, mirror).
        let (min_x, min_z) = (chunk[0].wrapping_shl(4), chunk[1].wrapping_shl(4));
        let pivot = [size[0] / 2, size[2] / 2];
        let a = transform([0, 0], front_back, rotation, pivot);
        let b = transform([size[0] - 1, size[2] - 1], front_back, rotation, pivot);
        let box_min = [min_x + a[0].min(b[0]), min_z + a[1].min(b[1])];
        let box_max = [min_x + a[0].max(b[0]), min_z + a[1].max(b[1])];
        // BoundingBox.getCenter.
        let centre = [
            box_min[0] + (box_max[0] - box_min[0] + 1) / 2,
            box_min[1] + (box_max[1] - box_min[1] + 1) / 2,
        ];
        let heightmap = if setup.placement == VerticalPlacement::OnOceanFloor {
            Heightmap::OceanFloorWg
        } else {
            Heightmap::WorldSurfaceWg
        };
        let surface = terrain.first_occupied_height(centre[0], centre[1], heightmap);
        let y = find_suitable_y(
            random,
            terrain,
            setup.placement,
            air_pocket,
            surface,
            size[1],
            [box_min, box_max],
            level_min_y,
        );
        [min_x, y, min_z]
    }

    /// With several setups, `f = nextFloat()`, minus each `weight / total` until it's
    /// negative; with one, that one, without a draw.
    fn choose_setup(&self, random: &mut LegacyRandomSource) -> Setup {
        if let [only] = self.setups.as_slice() {
            return *only;
        }
        let total: f32 = self.setups.iter().fold(0.0, |t, s| t + s.weight);
        let mut f = random.next_float();
        for s in &self.setups {
            f -= s.weight / total;
            if f < 0.0 {
                return *s;
            }
        }
        // The game throws here; weights that sum to their total can't get here.
        *self.setups.last().expect("setups aren't empty")
    }
}

/// `RuinedPortalStructure.sample`: never at 0, always at 1, else `nextFloat() < p`.
fn sample(random: &mut LegacyRandomSource, p: f32) -> bool {
    if p == 0.0 {
        false
    } else if p == 1.0 {
        true
    } else {
        random.next_float() < p
    }
}

/// `Mth.randomBetweenInclusive`: `nextInt(max − min + 1) + min`.
fn between(random: &mut LegacyRandomSource, min: i32, max: i32) -> i32 {
    random.next_int_bounded(max - min + 1) + min
}

/// `getRandomWithinInterval`: between `min` and `max` if `min < max`, else `max`, no draw.
fn within(random: &mut LegacyRandomSource, min: i32, max: i32) -> i32 {
    if min < max {
        between(random, min, max)
    } else {
        max
    }
}

/// `findSuitableY`: a start height by placement, then down from it to just above
/// `minY + 15`, the first y where three of the box's four corner columns hold a block the
/// placement's heightmap counts.
#[allow(
    clippy::too_many_arguments,
    reason = "mirrors the game's findSuitableY"
)]
fn find_suitable_y(
    random: &mut LegacyRandomSource,
    terrain: &Terrain,
    placement: VerticalPlacement,
    air_pocket: bool,
    surface: i32,
    y_span: i32,
    [box_min, box_max]: [[i32; 2]; 2],
    level_min_y: i32,
) -> i32 {
    let bottom = level_min_y + 15;
    let start = match placement {
        VerticalPlacement::InNether if air_pocket => between(random, 32, 100),
        VerticalPlacement::InNether if random.next_float() < 0.5 => between(random, 27, 29),
        VerticalPlacement::InNether => between(random, 29, 100),
        VerticalPlacement::InMountain => within(random, 70, surface - y_span),
        VerticalPlacement::Underground => within(random, bottom, surface - y_span),
        VerticalPlacement::PartlyBuried => surface - y_span + between(random, 2, 8),
        _ => surface,
    };
    let columns = [
        [box_min[0], box_min[1]],
        [box_max[0], box_min[1]],
        [box_min[0], box_max[1]],
        [box_max[0], box_max[1]],
    ]
    .map(|[x, z]| terrain.base_column(x, z));
    let heightmap = if placement == VerticalPlacement::OnOceanFloor {
        Heightmap::OceanFloorWg
    } else {
        Heightmap::WorldSurfaceWg
    };
    let mut y = start;
    while y > bottom {
        let solid = columns
            .iter()
            .filter(|c| terrain.counts(heightmap, c.at(y)))
            .count();
        if solid >= 3 {
            break;
        }
        y -= 1;
    }
    y
}

/// `StructureTemplate.transform` in x and z: the mirror (front-back negates x), then the
/// rotation about `pivot`.
fn transform(
    [x, z]: [i32; 2],
    front_back: bool,
    rotation: Rotation,
    [px, pz]: [i32; 2],
) -> [i32; 2] {
    let x = if front_back { -x } else { x };
    match rotation {
        Rotation::Counterclockwise90 => [px - pz + z, px + pz - x],
        Rotation::Clockwise90 => [px + pz - z, pz - px + x],
        Rotation::Clockwise180 => [px + px - x, pz + pz - z],
        Rotation::None => [x, z],
    }
}

/// `RuinedPortalStructure.Setup.CODEC`'s fields the check uses.
fn setup(json: &Value, at: &At<'_>) -> LoadResult<Setup> {
    let obj = object(json, at)?;
    let placement_at = at.child("placement");
    let placement = match field(obj, "placement", at)?.as_str() {
        Some("on_land_surface") => VerticalPlacement::OnLandSurface,
        Some("partly_buried") => VerticalPlacement::PartlyBuried,
        Some("on_ocean_floor") => VerticalPlacement::OnOceanFloor,
        Some("in_mountain") => VerticalPlacement::InMountain,
        Some("underground") => VerticalPlacement::Underground,
        Some("in_nether") => VerticalPlacement::InNether,
        _ => return Err(placement_at.error(LoadErrorKind::Shape("a vertical placement"))),
    };
    let air_pocket_probability = float(
        field(obj, "air_pocket_probability", at)?,
        &at.child("air_pocket_probability"),
    )?;
    let weight = float(field(obj, "weight", at)?, &at.child("weight"))?;
    Ok(Setup {
        placement,
        air_pocket_probability,
        weight,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transforms_mirror_then_rotate_about_the_pivot() {
        let pivot = [3, 2];
        let none = Rotation::None;
        assert_eq!(transform([5, 7], false, none, pivot), [5, 7]);
        assert_eq!(transform([5, 7], true, none, pivot), [-5, 7]);
        // Clockwise 90: (px + pz − z, pz − px + x).
        assert_eq!(
            transform([5, 7], false, Rotation::Clockwise90, pivot),
            [-2, 4]
        );
        // Counterclockwise 90: (px − pz + z, px + pz − x).
        assert_eq!(
            transform([5, 7], false, Rotation::Counterclockwise90, pivot),
            [8, 0]
        );
        // 180: (2px − x, 2pz − z), here after the mirror.
        assert_eq!(
            transform([5, 7], true, Rotation::Clockwise180, pivot),
            [11, -3]
        );
    }

    fn portal(weights: &[f32]) -> RuinedPortal {
        RuinedPortal {
            setups: weights
                .iter()
                .map(|&weight| Setup {
                    placement: VerticalPlacement::OnLandSurface,
                    air_pocket_probability: 0.0,
                    weight,
                })
                .collect(),
            portals: vec![[1, 1, 1]; 10],
            giants: vec![[1, 1, 1]; 3],
        }
    }

    #[test]
    fn setups_are_picked_by_weight_and_one_needs_no_draw() {
        let mut random = LegacyRandomSource::new(1);
        let mut copy = LegacyRandomSource::new(1);
        portal(&[1.0]).choose_setup(&mut random);
        assert_eq!(random.next_int(), copy.next_int());

        let two = portal(&[0.5, 1.5]);
        for seed in 0..50 {
            let mut random = LegacyRandomSource::new(seed);
            let f = LegacyRandomSource::new(seed).next_float();
            let want = if f - 0.25 < 0.0 { 0.5 } else { 1.5 };
            assert_eq!(two.choose_setup(&mut random).weight, want);
        }
    }

    #[test]
    fn air_pockets_draw_only_between_0_and_1() {
        let mut random = LegacyRandomSource::new(9);
        assert!(!sample(&mut random, 0.0));
        assert!(sample(&mut random, 1.0));
        let next = LegacyRandomSource::new(9).next_float();
        assert_eq!(sample(&mut random, 0.5), next < 0.5);
    }
}
