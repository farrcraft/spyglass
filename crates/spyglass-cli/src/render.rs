//! `spyglass render`: a map image of a seed or world save (`docs/specs/render.md`).

use std::fs::File;
use std::io::BufWriter;

use anyhow::{Context, Result};
use spyglass::{Layers, MapArea, SaveLocation};

use crate::args::{Centre, Render};
use crate::json::{self, Xz};
use crate::{Origin, Out};

pub fn run(args: &Render) -> Result<()> {
    // Centred on the player: by default with --world, or when asked for.
    let player = match args.centre {
        Some(Centre::Player) => true,
        Some(Centre::At(_)) => false,
        None => args.min.is_none() && args.common.world.is_some(),
    };
    let (world, seed, report) = crate::load(&args.common, player)?;
    let session = world.session(seed)?;
    let out = Out {
        session: &session,
        json: args.common.json,
        report: report.as_ref(),
    };
    let area = match args.min {
        Some(min) => {
            let area = MapArea {
                min,
                size: args.size,
                scale: args.scale,
                y: args.y,
            };
            area.check()?;
            area
        }
        None => MapArea::centred_on(out.centre(args.centre)?, args.size, args.scale, args.y)?,
    };
    let layers = Layers {
        biomes: !args.no_biomes,
        ties: args.ties,
        highlight: args
            .highlight
            .iter()
            .map(|b| world.biome_query(b))
            .collect::<spyglass::Result<_>>()?,
        structures: args
            .structures
            .iter()
            .map(|s| world.structure_query(s))
            .collect::<spyglass::Result<_>>()?,
        slime_chunks: args.slime,
        spawn: args.spawn,
        confirm: args.confirm,
        relief: args.relief,
        surface: args.surface,
    };
    let features = session.map_features(&area, &layers)?;
    let image = session.render(&area, &layers)?;
    let path = &args.output;
    let file = File::create(path).with_context(|| format!("creating {}", path.display()))?;
    image
        .write_png(BufWriter::new(file))
        .with_context(|| format!("writing {}", path.display()))?;

    if out.json {
        let structures = features
            .structures
            .iter()
            .map(|s| json::StructureMark {
                position: Xz {
                    x: s.position[0],
                    z: s.position[2],
                },
                chunk: s.chunk,
                set: &s.set,
                structures: &s.structures,
                certain: s.certain,
            })
            .collect();
        return out.print(
            "render",
            json::RenderAnswer {
                output: json::path(path),
                area: json::Area {
                    min: area.min,
                    size: area.size,
                    scale: area.scale,
                    y: area.y,
                },
                layers: json::RenderLayers {
                    biomes: layers.biomes,
                    ties: layers.ties,
                    highlight: &args.highlight,
                    structures: &args.structures,
                    slime_chunks: layers.slime_chunks,
                    spawn: layers.spawn,
                    confirm: layers.confirm,
                    relief: layers.relief,
                    surface: layers.surface,
                },
                structures,
                slime_chunks: features.slime_chunks,
                spawn: features.spawn.map(|s| json::SpawnAnswer::new(&s)),
            },
        );
    }
    let [w, h] = area.size;
    let s = i64::from(area.scale);
    let [x0, z0] = area.min.map(i64::from);
    let (x1, z1) = (x0 + i64::from(w) * s - 1, z0 + i64::from(h) * s - 1);
    let biomes = match (layers.biomes, layers.surface) {
        (false, _) => String::new(),
        (true, true) => ", biomes at the surface".to_string(),
        (true, false) => format!(", biomes at y {}", area.y),
    };
    println!(
        "wrote {}: {w} × {h} pixels, {} blocks per pixel, x {x0} to {x1}, z {z0} to {z1}\
         {biomes}{}",
        path.display(),
        area.scale,
        out.context(None),
    );
    if layers.spawn || !layers.structures.is_empty() || layers.slime_chunks {
        let certain = features.structures.iter().filter(|s| s.certain).count();
        println!(
            "  {} structures ({certain} certain, {} candidates), {} slime chunks, spawn {}",
            features.structures.len(),
            features.structures.len() - certain,
            features.slime_chunks.len(),
            if features.spawn.is_some() {
                "shown"
            } else {
                "not in view"
            },
        );
    }
    out.warn();
    Ok(())
}

impl Out<'_> {
    /// Resolves `--centre`, or its default: the player, or else the spawn, of `--world`'s save
    /// when in the map's dimension; otherwise 0, 0.
    fn centre(&self, centre: Option<Centre>) -> Result<[i32; 2]> {
        let xz = |[x, _, z]: [i32; 3]| [x, z];
        match centre {
            Some(Centre::At(at)) => Ok(at),
            Some(Centre::Player) => Ok(xz(self.origin(Origin::Player)?)),
            None => {
                let save = self.report.map(|r| &r.save);
                let here = self.session.world().dimension_kind();
                let in_view = |l: &&SaveLocation| l.dimension_kind() == Some(here);
                let place = save.and_then(|s| {
                    s.player
                        .as_ref()
                        .filter(in_view)
                        .or(s.spawn.as_ref().filter(in_view))
                });
                Ok(place.map_or([0, 0], |l| xz(l.block())))
            }
        }
    }
}
