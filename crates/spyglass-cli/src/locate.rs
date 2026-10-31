//! `spyglass locate`: structures, biomes, slime chunks and the spawn, as text or JSON.

use anyhow::Result;
use spyglass::{BlockBox, LocateOptions, StructureLayout};

use crate::json::{self, Xyz, Xz};
use crate::{Out, SHOWN};

pub fn structure(
    out: &Out<'_>,
    target: &str,
    from: [i32; 3],
    options: &LocateOptions,
    bounds: bool,
    nearest: bool,
) -> Result<()> {
    let session = out.session;
    let query = session.world().structure_query(target)?;
    let found = if nearest {
        session.nearest_structure(&query, from, options)?
    } else {
        session.locate_structure(&query, from, options)?
    };
    // Each hit's layout, where asked for and known.
    let layouts = found
        .hits
        .iter()
        .map(|h| match h.start.starting() {
            Some(structure) if bounds => session.structure_layout(structure, h.start.chunk),
            _ => Ok(None),
        })
        .collect::<spyglass::Result<Vec<Option<StructureLayout>>>>()?;
    if out.json {
        let hits = found
            .hits
            .iter()
            .zip(&layouts)
            .map(|(h, layout)| json::StructureHit {
                position: Xz {
                    x: h.start.position[0],
                    z: h.start.position[2],
                },
                chunk: h.start.chunk,
                set: &h.start.set,
                structures: &h.start.structures,
                certain: h.start.certain,
                blocks_away: h.blocks_away,
                bounds: layout.as_ref().map(json::Bounds::new),
            })
            .collect();
        return out.print(
            "locate structure",
            json::StructureAnswer {
                target,
                mode: if nearest { "nearest" } else { "locate" },
                from,
                radius: options.radius,
                confirmed: options.confirm,
                complete: found.complete,
                certain_answer: found.certain_answer,
                hits,
            },
        );
    }
    let mode = if nearest { "nearest " } else { "" };
    out!("{mode}{target}{}", out.context(Some(from)));
    if found.hits.is_empty() {
        out!("  none within {} rings", options.radius);
        return Ok(());
    }
    for (h, layout) in found.hits.iter().zip(&layouts).take(SHOWN) {
        let [x, _, z] = h.start.position;
        out!(
            "  {:>9} ~ {:<9} {:>7} blocks  {:<10} {}",
            x,
            z,
            h.blocks_away,
            if h.start.certain {
                "certain"
            } else {
                "candidate"
            },
            h.start.structures.join(" or "),
        );
        if let Some(layout) = layout {
            let BlockBox { min, max } = layout.bounds;
            out!(
                "  {:>9}   bounds {} {} {} to {} {} {}, {} pieces",
                "",
                min[0],
                min[1],
                min[2],
                max[0],
                max[1],
                max[2],
                layout.pieces.len()
            );
        }
    }
    if found.hits.len() > SHOWN {
        out!(
            "  ... and {} more (--json lists all)",
            found.hits.len() - SHOWN
        );
    }
    if found.hits.iter().any(|h| !h.start.certain) {
        let rest = if found.complete && found.certain_answer.is_some() {
            "."
        } else {
            ", or further out."
        };
        let why = if options.confirm {
            "Candidates are behind a tied biome or a platform-dependent ring position."
        } else {
            "Candidates depend on terrain, which --unconfirmed leaves undecided, or a tied biome."
        };
        if nearest {
            out!("{why} The nearest is the first of these that generates{rest}");
        } else {
            out!(
                "{why} The game's answer is the first of these it generates, usually the first \
                 listed{rest}"
            );
        }
    }
    Ok(())
}

pub fn feature(
    out: &Out<'_>,
    target: &str,
    from: [i32; 3],
    radius: i32,
    all: bool,
    neighbours: bool,
) -> Result<()> {
    let session = out.session;
    let query = session.world().feature_query(target)?;
    let options = spyglass::FeatureOptions {
        neighbours,
        unlikely: all,
    };
    let found = session.nearest_feature(&query, from, radius, options)?;
    let hits: Vec<&spyglass::FoundFeature> = found
        .hits
        .iter()
        .filter(|f| all || f.certain || f.likely)
        .collect();
    if out.json {
        return out.print(
            "locate feature",
            json::FeatureLocateAnswer {
                target,
                from,
                radius,
                complete: found.complete,
                hits: hits
                    .iter()
                    .map(|f| json::FeatureHit::new(f, Some(from)))
                    .collect(),
            },
        );
    }
    out!("nearest {target}{}", out.context(Some(from)));
    if hits.is_empty() {
        out!("  none within {radius} chunks");
        return Ok(());
    }
    for f in hits.iter().take(SHOWN) {
        feature_line(f, Some(from));
    }
    if hits.len() > SHOWN {
        out!("  ... and {} more (--json lists all)", hits.len() - SHOWN);
    }
    feature_notes(&hits, all);
    Ok(())
}

/// One feature, as text.
pub fn feature_line(f: &spyglass::FoundFeature, from: Option<[i32; 3]>) {
    let [x, y, z] = f.position;
    let away = from.map_or(String::new(), |o| {
        let (dx, dz) = (f64::from(x - o[0]), f64::from(z - o[2]));
        format!(" {:>7} blocks", (dx * dx + dz * dz).sqrt() as i32)
    });
    let kind = match (f.certain, f.likely) {
        (true, _) => "certain",
        (false, true) => "candidate",
        (false, false) => "unlikely",
    };
    let why = f
        .reason
        .as_deref()
        .map_or(String::new(), |r| format!(" ({r})"));
    out!(
        "  {x:>9} {y:>4} {z:<9}{away}  {kind:<10} {}{why}",
        f.feature
    );
}

/// What the answers mean, after them.
pub fn feature_notes(hits: &[&spyglass::FoundFeature], all: bool) {
    if hits
        .iter()
        .all(|f| f.neighbours == spyglass::Neighbours::NotChecked)
        && !hits.is_empty()
    {
        out!(
            "Certain is for the chunk decorated before its neighbours (--neighbours checks whether each holds whatever they do)."
        );
    }
    if hits.iter().any(|f| !f.certain) {
        out!(
            "Candidates are where something spyglass can't know could change the answer: listed likely if they placed on spyglass's blocks{}.",
            if all {
                ", unlikely if not"
            } else {
                " (--all lists the unlikely ones too)"
            }
        );
    }
    if hits
        .iter()
        .any(|f| matches!(f.neighbours, spyglass::Neighbours::Could(_)))
    {
        out!(
            "Certain is for the chunk decorated before its neighbours; neighbours decorated first could change these."
        );
    }
}

pub fn biome(out: &Out<'_>, biome: &str, from: [i32; 3], radius: i32) -> Result<()> {
    let session = out.session;
    let query = session.world().biome_query(biome)?;
    let found = session.locate_biome(&query, from, radius)?;
    if out.json {
        let hits = found
            .hits
            .iter()
            .map(|h| json::BiomeHit {
                position: Xyz {
                    x: h.position[0],
                    y: h.position[1],
                    z: h.position[2],
                },
                biome: &h.biome.biome,
                tied: &h.biome.tied,
                certain: h.certain,
                blocks_away: h.blocks_away,
            })
            .collect();
        return out.print(
            "locate biome",
            json::BiomeAnswer {
                target: biome,
                from,
                radius,
                hits,
            },
        );
    }
    out!("{biome}{}", out.context(Some(from)));
    if found.hits.is_empty() {
        out!("  none within {radius} blocks");
        return Ok(());
    }
    for h in found.hits.iter().take(SHOWN) {
        let [x, y, z] = h.position;
        let tie = if h.biome.tied.is_empty() {
            String::new()
        } else {
            format!(" (tied: {})", h.biome.tied.join(", "))
        };
        out!(
            "  {x:>9} {y:>4} {z:<9} {:>7} blocks  {}{tie}",
            h.blocks_away,
            if h.certain { "certain" } else { "candidate" },
        );
    }
    if found.hits.iter().any(|h| !h.certain) {
        out!(
            "Candidates are tied cells, where the game's biome depends on lookup history; the \
             game's answer is the first of these where it finds the biome."
        );
    }
    Ok(())
}

pub fn slime(out: &Out<'_>, from: [i32; 3], radius: i32) -> Result<()> {
    let centre = [from[0] >> 4, from[2] >> 4];
    out.overworld_only("slime chunks are")?;
    let chunks = out.session.slime_chunks_near(centre, radius)?;
    if out.json {
        return out.print(
            "locate slime",
            json::SlimeAnswer {
                from,
                radius,
                chunks,
            },
        );
    }
    out!(
        "Slime chunks within {radius} chunks of chunk {} {}{}",
        centre[0],
        centre[1],
        out.context(None)
    );
    for &[x, z] in chunks.iter().take(SHOWN * 2) {
        out!(
            "  chunk {x:>6} {z:<6}  blocks {}..{}, {}..{}",
            x << 4,
            (x << 4) + 15,
            z << 4,
            (z << 4) + 15
        );
    }
    if chunks.len() > SHOWN * 2 {
        out!(
            "  ... and {} more (--json lists all)",
            chunks.len() - SHOWN * 2
        );
    }
    Ok(())
}

pub fn spawn(out: &Out<'_>) -> Result<()> {
    out.overworld_only("the world spawn is")?;
    let origin = out.session.spawn_origin();
    if out.json {
        return out.print("locate spawn", json::SpawnAnswer::new(&origin));
    }
    let [cx, cz] = origin.chunk;
    out!("World spawn{}", out.context(None));
    out!(
        "  the spawn search starts in chunk {cx} {cz} (blocks {}..{}, {}..{})",
        cx << 4,
        (cx << 4) + 15,
        cz << 4,
        (cz << 4) + 15
    );
    if let Some([x, _, z]) = origin.position {
        out!("  the climate search's best point is {x} ~ {z}");
    }
    if origin.platform_dependent {
        out!("  this may differ on other platforms (Java's sin and cos aren't exact)");
    }
    out!(
        "The spawn itself is in the 11 x 11 chunks around it, chosen by generated blocks \
         (surface and features), which spyglass doesn't compute yet."
    );
    Ok(())
}
