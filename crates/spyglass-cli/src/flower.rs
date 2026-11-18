//! `spyglass flower`: what bone meal grows on grass at a block, and the best height for a
//! farm of one flower (`docs/specs/flowers.md`).

use anyhow::{Result, bail};
use spyglass::{FlowerAt, MapArea, display_name};

use crate::args::Flower;
use crate::{Out, json};

pub fn run(what: &Flower) -> Result<()> {
    match what {
        Flower::At { position, common } => {
            let (world, seed, report) = crate::load(common, false)?;
            let session = crate::session(&world, seed, report.as_ref())?;
            let out = Out {
                session: &session,
                json: common.json,
                report: report.as_ref(),
            };
            let at = session.flower_at(*position)?;
            if out.json {
                return out.print("flower", at_json(&at));
            }
            print_at(&out, &at);
            out.warn();
            Ok(())
        }
        Flower::Heights {
            flower,
            min,
            max,
            y,
            step,
            scale,
            common,
        } => {
            let flower = if flower.contains(':') {
                flower.clone()
            } else {
                format!("minecraft:{flower}")
            };
            let (world, seed, report) = crate::load(common, false)?;
            let session = crate::session(&world, seed, report.as_ref())?;
            let out = Out {
                session: &session,
                json: common.json,
                report: report.as_ref(),
            };
            let area = area(*min, *max, *scale, y[0])?;
            let shares = session.flower_heights(&area, &flower, *y, *step)?;
            if out.json {
                return out.print(
                    "flower_heights",
                    json::FlowerHeightsAnswer {
                        flower: &flower,
                        min: *min,
                        max: *max,
                        scale: *scale,
                        heights: shares
                            .iter()
                            .map(|s| json::FlowerShareJson {
                                y: s.y,
                                certain: s.certain,
                                possible: s.possible,
                                pixels: s.pixels,
                            })
                            .collect(),
                    },
                );
            }
            out!(
                "{} from {} {} to {} {}, by the height of its block{}",
                display_name(&flower),
                min[0],
                min[1],
                max[0],
                max[1],
                out.context(None)
            );
            out!("  {:>5}  {:>8}  {:>8}", "y", "certain", "possible");
            let best = shares.iter().map(|s| s.certain).max().unwrap_or(0);
            for s in &shares {
                out!(
                    "  {:>5}  {:>7.1}%  {:>7.1}%{}",
                    s.y,
                    percent(s.certain, s.pixels),
                    percent(s.possible, s.pixels),
                    if s.certain == best && best > 0 {
                        "  best"
                    } else {
                        ""
                    }
                );
            }
            out!(
                "Certain: bone meal always grows it there. Possible: it can grow there, by chance."
            );
            out.warn();
            Ok(())
        }
    }
}

/// `part` of `whole` as a percentage, for display.
#[expect(
    clippy::cast_precision_loss,
    reason = "pixel counts, shown to a tenth of a percent"
)]
fn percent(part: u64, whole: u64) -> f64 {
    100.0 * part as f64 / whole as f64
}

/// The area from `min` to `max`, both included, sampled every `scale` blocks.
fn area(min: [i32; 2], max: [i32; 2], scale: u32, y: i32) -> Result<MapArea> {
    if max[0] < min[0] || max[1] < min[1] {
        bail!("--max must be at least --min on both axes");
    }
    if !scale.is_power_of_two() {
        bail!("--scale must be a power of two");
    }
    let s = i64::from(scale);
    let low = |v: i32| i64::from(v).div_euclid(s) * s;
    let size = |d: usize| {
        let pixels = (i64::from(max[d]) - low(min[d])) / s + 1;
        u32::try_from(pixels).map_err(|_| anyhow::anyhow!("the area is too large"))
    };
    let area = MapArea {
        min: [0, 1]
            .map(|d| i32::try_from(low(min[d])).expect("a multiple of the scale below an i32")),
        size: [size(0)?, size(1)?],
        scale,
        y,
    };
    area.check()?;
    Ok(area)
}

fn at_json(at: &FlowerAt) -> json::FlowerAnswer<'_> {
    json::FlowerAnswer {
        position: at.pos,
        certain: at.certain(),
        biomes: at
            .biomes
            .iter()
            .map(|b| json::FlowerBiomeJson {
                biome: &b.biome,
                features: b
                    .features
                    .iter()
                    .map(|f| json::BoneMealFeatureJson {
                        id: &f.id,
                        provider: &f.provider,
                        by_position: f.by_position,
                    })
                    .collect(),
                flowers: b
                    .flowers
                    .iter()
                    .map(|f| json::FlowerChanceJson {
                        flower: &f.flower,
                        chance: f.chance.into(),
                    })
                    .collect(),
            })
            .collect(),
    }
}

fn print_at(out: &Out<'_>, at: &FlowerAt) {
    let [x, y, z] = at.pos;
    out!(
        "Bone meal on grass at {x} {} {z}{}",
        y - 1,
        out.context(None)
    );
    for (i, biome) in at.biomes.iter().enumerate() {
        if i == 1 {
            out!("The biome here is tied; the game may give this one instead:");
        }
        out!("  biome: {}", display_name(&biome.biome));
        if biome.features.is_empty() {
            out!("  no flowers: the biome has no flower features");
            continue;
        }
        let features: Vec<String> = biome
            .features
            .iter()
            .map(|f| {
                let how = if f.by_position { ", by position" } else { "" };
                format!("{} ({}{how})", f.id, f.provider)
            })
            .collect();
        let label = if features.len() == 1 {
            "feature"
        } else {
            "features, one at random"
        };
        out!("  {label}: {}", features.join(", "));
        for f in &biome.flowers {
            let chance = f.chance;
            let exact = if chance.is_certain() {
                String::new()
            } else {
                format!("  ({}/{})", chance.numerator, chance.denominator)
            };
            out!("  {:>6.2}%  {}{exact}", 100.0 * chance.as_f64(), f.flower);
        }
    }
    out!(
        "Chances are for the flower at {x} {y} {z} once bone meal makes a flower there; each \
         try near the click makes one 1 time in 8."
    );
}
