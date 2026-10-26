//! Exporting the view: the map area on screen, drawn by the library's `Session::render`, and the
//! `spyglass render` command that draws the same image (`docs/specs/render.md`).

use std::path::PathBuf;

use spyglass::{Dimension, MAX_SLIME_SCALE, MAX_TERRAIN_SCALE, MapArea};

use crate::view::View;

/// The map area on screen: `size` pixels, with the view's centre block in pixel `size / 2`, as
/// the map draws it.
pub fn visible_area(view: &View, size: [u32; 2]) -> Option<MapArea> {
    let [x, z] = view.block_at([0.0, 0.0]).map(|b| b as i32);
    MapArea::centred_on([x, z], size, view.scale, view.y).ok()
}

/// Where the world and seed come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// A seed, with the game version of its bundle.
    Seed { seed: i64, version: String },
    /// A world save's folder.
    Save(PathBuf),
}

/// An image of the map: what to draw, as `spyglass render` takes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Export {
    pub area: MapArea,
    pub source: Source,
    /// The dimension drawn.
    pub dimension: Dimension,
    /// The bundles folder, if it isn't `spyglass render`'s default.
    pub bundles: Option<PathBuf>,
    pub structures: Vec<String>,
    pub highlight: Vec<String>,
    pub ties: bool,
    pub slime_chunks: bool,
    pub spawn: bool,
    pub confirm: bool,
    pub relief: bool,
    pub surface: bool,
}

/// `text` as one shell argument: quoted if it has spaces or quotes.
fn quote(text: &str) -> String {
    if !text.is_empty() && !text.contains([' ', '"', '\'', '#', '&', ';', '|', '$']) {
        text.to_string()
    } else {
        format!("\"{}\"", text.replace('"', "\\\""))
    }
}

impl Export {
    /// The `spyglass render` arguments for this image, writing `output`.
    pub fn args(&self, output: &str) -> Vec<String> {
        let mut args = vec!["render".to_string()];
        let mut add = |flag: &str, value: String| {
            args.push(flag.to_string());
            args.push(value);
        };
        match &self.source {
            Source::Seed { seed, version } => {
                add("--seed", seed.to_string());
                add("--game-version", version.clone());
            }
            Source::Save(dir) => add("--world", dir.display().to_string()),
        }
        if let Some(dir) = &self.bundles {
            add("--bundles", dir.display().to_string());
        }
        // The Overworld is the default whenever --centre is given.
        match self.dimension {
            Dimension::Overworld => {}
            Dimension::Nether => add("--dimension", "nether".to_string()),
            Dimension::End => add("--dimension", "end".to_string()),
        }
        let [w, h] = self.area.size;
        let s = i64::from(self.area.scale);
        let centre =
            [0, 1].map(|d| i64::from(self.area.min[d]) + i64::from(self.area.size[d] / 2) * s);
        add("--centre", format!("{},{}", centre[0], centre[1]));
        add("--size", format!("{w}x{h}"));
        add("--scale", self.area.scale.to_string());
        add("--y", self.area.y.to_string());
        if !self.structures.is_empty() {
            add("--structures", self.structures.join(","));
        }
        if !self.highlight.is_empty() {
            add("--highlight", self.highlight.join(","));
        }
        if self.ties {
            args.push("--ties".to_string());
        }
        if self.slime_chunks && self.area.scale <= MAX_SLIME_SCALE {
            args.push("--slime".to_string());
        }
        if self.spawn {
            args.push("--spawn".to_string());
        }
        if self.confirm && !self.structures.is_empty() {
            args.push("--confirm".to_string());
        }
        let terrain = self.area.scale <= MAX_TERRAIN_SCALE;
        if self.relief && terrain {
            args.push("--relief".to_string());
        }
        if self.surface && terrain {
            args.push("--surface".to_string());
        }
        args.push("-o".to_string());
        args.push(output.to_string());
        args
    }

    /// The command line, for pasting into a shell.
    pub fn command(&self, output: &str) -> String {
        let args: Vec<String> = self.args(output).iter().map(|a| quote(a)).collect();
        format!("spyglass {}", args.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_export_is_the_area_on_screen() {
        // The map puts the view's centre block in pixel size / 2, as `centred_on` does.
        let view = View {
            centre: [1000.7, -333.2],
            scale: 8,
            y: 70,
        };
        let area = visible_area(&view, [1013, 775]).unwrap();
        let [x, z] = view.block_at([0.0, 0.0]);
        assert_eq!(area.pixel([x as i32, z as i32]), [506, 387]);
        assert_eq!(area.size, [1013, 775]);
        assert_eq!((area.scale, area.y), (8, 70));
        assert_eq!(area.min, [1000 - 506 * 8, -336 - 387 * 8]);
    }

    #[test]
    fn the_command_says_what_the_export_draws() {
        let area = MapArea::centred_on([1000, -336], [1013, 775], 8, 70).unwrap();
        let export = Export {
            area,
            source: Source::Seed {
                seed: -5,
                version: "26.3".into(),
            },
            dimension: Dimension::Overworld,
            bundles: None,
            structures: vec![
                "minecraft:village_plains".into(),
                "minecraft:stronghold".into(),
            ],
            highlight: vec!["minecraft:river".into()],
            ties: true,
            slime_chunks: true,
            spawn: true,
            confirm: true,
            relief: true,
            surface: false,
        };
        // Terrain layers aren't drawn past scale 4, so the command doesn't ask for them.
        assert_eq!(
            export.command("map.png"),
            "spyglass render --seed -5 --game-version 26.3 --centre 1000,-336 --size 1013x775 \
             --scale 8 --y 70 --structures minecraft:village_plains,minecraft:stronghold \
             --highlight minecraft:river --ties --slime --spawn --confirm -o map.png"
        );
        let close = Export {
            area: MapArea::centred_on([0, 0], [10, 10], 4, 64).unwrap(),
            structures: Vec::new(),
            surface: true,
            ..export.clone()
        };
        assert!(
            close
                .command("map.png")
                .ends_with("--ties --slime --spawn --relief --surface -o map.png")
        );
        // The CLI centres --centre with `centred_on`, which gives back the same area.
        assert_eq!(
            MapArea::centred_on([1000, -336], area.size, 8, 70).unwrap(),
            area
        );

        // Slime chunks aren't drawn past scale 16, so the command doesn't ask for them; paths
        // with spaces are quoted.
        let export = Export {
            area: MapArea::centred_on([0, 0], [10, 10], 32, 64).unwrap(),
            source: Source::Save(PathBuf::from("C:/saves/My World")),
            dimension: Dimension::Overworld,
            bundles: Some(PathBuf::from("D:/bundles")),
            structures: Vec::new(),
            highlight: Vec::new(),
            ties: false,
            slime_chunks: true,
            spawn: false,
            confirm: false,
            relief: false,
            surface: false,
        };
        assert_eq!(
            export.command("out dir/map.png"),
            "spyglass render --world \"C:/saves/My World\" --bundles D:/bundles --centre 0,0 \
             --size 10x10 --scale 32 --y 64 -o \"out dir/map.png\""
        );
        let nether = Export {
            dimension: Dimension::Nether,
            ..export
        };
        assert!(
            nether
                .command("m.png")
                .contains("--bundles D:/bundles --dimension nether --centre 0,0")
        );
    }
}
