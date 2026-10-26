//! `spyglass locate`: structures, biomes, slime chunks and the spawn, as text or JSON.

use anyhow::Result;

use crate::json::{self, Xyz, Xz};
use crate::{Out, SHOWN};

pub fn structure(out: &Out<'_>, target: &str, from: [i32; 3], radius: i32) -> Result<()> {
    let session = out.session;
    let query = session.world().structure_query(target)?;
    let found = session.locate_structure(
        &query,
        from,
        &spyglass::LocateOptions {
            radius,
            confirm: false,
        },
    )?;
    if out.json {
        let hits = found
            .hits
            .iter()
            .map(|h| json::StructureHit {
                position: Xz {
                    x: h.start.position[0],
                    z: h.start.position[2],
                },
                chunk: h.start.chunk,
                set: &h.start.set,
                structures: &h.start.structures,
                certain: h.start.certain,
                blocks_away: h.blocks_away,
            })
            .collect();
        return out.print(
            "locate structure",
            json::StructureAnswer {
                target,
                from,
                radius,
                complete: found.complete,
                certain_answer: found.certain_answer,
                hits,
            },
        );
    }
    println!("{target}{}", out.context(Some(from)));
    if found.hits.is_empty() {
        println!("  none within {radius} rings");
        return Ok(());
    }
    for h in found.hits.iter().take(SHOWN) {
        let [x, _, z] = h.start.position;
        println!(
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
    }
    if found.hits.len() > SHOWN {
        println!(
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
        println!(
            "Candidates depend on terrain (or a tied biome), which spyglass doesn't compute yet. \
             The game's answer is the first of these it generates, usually the first listed{rest}"
        );
    }
    Ok(())
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
    println!("{biome}{}", out.context(Some(from)));
    if found.hits.is_empty() {
        println!("  none within {radius} blocks");
        return Ok(());
    }
    for h in found.hits.iter().take(SHOWN) {
        let [x, y, z] = h.position;
        let tie = if h.biome.tied.is_empty() {
            String::new()
        } else {
            format!(" (tied: {})", h.biome.tied.join(", "))
        };
        println!(
            "  {x:>9} {y:>4} {z:<9} {:>7} blocks  {}{tie}",
            h.blocks_away,
            if h.certain { "certain" } else { "candidate" },
        );
    }
    if found.hits.iter().any(|h| !h.certain) {
        println!(
            "Candidates are tied cells, where the game's biome depends on lookup history; the \
             game's answer is the first of these where it finds the biome."
        );
    }
    Ok(())
}

pub fn slime(out: &Out<'_>, from: [i32; 3], radius: i32) -> Result<()> {
    let centre = [from[0] >> 4, from[2] >> 4];
    let chunks = out.session.slime_chunks_near(centre, radius);
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
    println!(
        "Slime chunks within {radius} chunks of chunk {} {}{}",
        centre[0],
        centre[1],
        out.context(None)
    );
    for &[x, z] in chunks.iter().take(SHOWN * 2) {
        println!(
            "  chunk {x:>6} {z:<6}  blocks {}..{}, {}..{}",
            x << 4,
            (x << 4) + 15,
            z << 4,
            (z << 4) + 15
        );
    }
    if chunks.len() > SHOWN * 2 {
        println!(
            "  ... and {} more (--json lists all)",
            chunks.len() - SHOWN * 2
        );
    }
    Ok(())
}

pub fn spawn(out: &Out<'_>) -> Result<()> {
    let origin = out.session.spawn_origin();
    if out.json {
        return out.print("locate spawn", json::SpawnAnswer::new(&origin));
    }
    let [cx, cz] = origin.chunk;
    println!("World spawn{}", out.context(None));
    println!(
        "  the spawn search starts in chunk {cx} {cz} (blocks {}..{}, {}..{})",
        cx << 4,
        (cx << 4) + 15,
        cz << 4,
        (cz << 4) + 15
    );
    if let Some([x, _, z]) = origin.position {
        println!("  the climate search's best point is {x} ~ {z}");
    }
    if origin.platform_dependent {
        println!("  this may differ on other platforms (Java's sin and cos aren't exact)");
    }
    println!(
        "The spawn itself is in the 11 x 11 chunks around it, chosen by terrain, which spyglass \
         doesn't compute yet."
    );
    Ok(())
}
