//! The side panel's sections: export, locate, the view and the layers.

use super::map::{parse_goto, pick_list};
use super::*;

impl App {
    pub(super) fn side_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("spyglass");
        ui.add_space(4.0);
        egui::CollapsingHeader::new("World")
            .default_open(true)
            .show(ui, |ui| self.world_section(ui));
        egui::CollapsingHeader::new("View")
            .default_open(true)
            .show(ui, |ui| self.view_section(ui));
        egui::CollapsingHeader::new("Layers")
            .default_open(true)
            .show(ui, |ui| self.layers_section(ui));
        egui::CollapsingHeader::new("Search")
            .default_open(false)
            .show(ui, |ui| self.search_section(ui));
        egui::CollapsingHeader::new("Export")
            .default_open(true)
            .show(ui, |ui| self.export_section(ui));
        if self.locating.is_some() || self.found.is_some() {
            egui::CollapsingHeader::new("Locate")
                .default_open(true)
                .show(ui, |ui| self.locate_section(ui));
        }
    }

    /// What exporting the view now would draw.
    pub(super) fn export(&self) -> Option<Export> {
        let (world, shown) = (self.world?, self.shown.as_ref()?);
        let source = match &self.save {
            Some(report) => Source::Save(report.save.dir.clone()),
            None => Source::Seed {
                seed: shown.session.seed(),
                version: world.version().to_string(),
            },
        };
        let terrain = self.terrain_allowed();
        Some(Export {
            area: visible_area(&self.settings.view, self.map_size)?,
            source,
            dimension: world.dimension_kind(),
            bundles: (self.settings.bundles != Settings::default().bundles)
                .then(|| self.bundles_dir()),
            structures: if self.structures_generate() {
                self.settings.structures.clone()
            } else {
                Vec::new()
            },
            highlight: self.settings.highlight.clone(),
            ties: self.settings.ties,
            slime_chunks: self.settings.slime_chunks && self.slime_allowed(),
            spawn: self.settings.spawn && self.spawn_allowed(),
            confirm: self.settings.confirm,
            relief: self.settings.relief && terrain,
            surface: self.settings.surface && terrain,
        })
    }

    /// Saving the view as an image, or as the command that draws it.
    pub(super) fn export_section(&mut self, ui: &mut egui::Ui) {
        let export = self.export();
        ui.horizontal(|ui| {
            let ready = export.is_some() && self.exporting.is_none();
            if ui
                .add_enabled(ready, egui::Button::new("Export image…"))
                .clicked()
                && let Some(export) = &export
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("PNG image", &["png"])
                    .set_file_name("map.png")
                    .save_file()
            {
                self.start_export(ui.ctx(), export.clone(), path);
            }
            if ui
                .add_enabled(export.is_some(), egui::Button::new("Copy command"))
                .on_hover_text("The spyglass render command that draws this view")
                .clicked()
                && let Some(export) = &export
            {
                let command = export.command("map.png");
                ui.ctx().copy_text(command.clone());
                self.export_note = Some(Ok(format!("Copied: {command}")));
            }
        });
        if self.exporting.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Drawing…");
            });
        }
        match &self.export_note {
            Some(Ok(note)) => {
                ui.weak(note);
            }
            Some(Err(e)) => {
                ui.colored_label(ui.visuals().error_fg_color, e);
            }
            None => {}
        }
    }

    /// Draws `export` with the library and writes it to `path`, in the background.
    pub(super) fn start_export(&mut self, ctx: &egui::Context, export: Export, path: PathBuf) {
        let (Some(shown), Some(world)) = (&self.shown, self.world) else {
            return;
        };
        let session = shown.session.clone();
        self.export_note = None;
        self.exporting = Some(Pending::spawn(ctx, move || {
            let layers = spyglass::Layers {
                biomes: true,
                ties: export.ties,
                highlight: export
                    .highlight
                    .iter()
                    .map(|b| world.biome_query(b))
                    .collect::<spyglass::Result<_>>()
                    .map_err(|e| e.to_string())?,
                structures: export
                    .structures
                    .iter()
                    .map(|s| world.structure_query(s))
                    .collect::<spyglass::Result<_>>()
                    .map_err(|e| e.to_string())?,
                slime_chunks: export.slime_chunks && export.area.scale <= spyglass::MAX_SLIME_SCALE,
                spawn: export.spawn,
                confirm: export.confirm,
                relief: export.relief && export.area.scale <= MAX_TERRAIN_SCALE,
                surface: export.surface && export.area.scale <= MAX_TERRAIN_SCALE,
            };
            let image = session
                .render(&export.area, &layers)
                .map_err(|e| e.to_string())?;
            let file = std::fs::File::create(&path)
                .map_err(|e| format!("creating {}: {e}", path.display()))?;
            image
                .write_png(std::io::BufWriter::new(file))
                .map_err(|e| format!("writing {}: {e}", path.display()))?;
            Ok(path)
        }));
    }

    /// The last locate's answer: click a hit to go there.
    pub(super) fn locate_section(&mut self, ui: &mut egui::Ui) {
        if self.locating.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Searching…");
            });
            return;
        }
        let mut go = None;
        let mut close = false;
        match &self.found {
            Some(Ok(found)) => {
                let [x, y, z] = found.from;
                ui.strong(found.target.name());
                ui.weak(format!("from {x} {y} {z}"));
                for hit in &found.hits {
                    let y = hit.y.map_or("~".to_string(), |y| y.to_string());
                    let label = format!(
                        "{} {y} {}  {} blocks  {}",
                        hit.x,
                        hit.z,
                        hit.blocks_away,
                        if hit.certain { "certain" } else { "candidate" }
                    );
                    if ui
                        .selectable_label(false, label)
                        .on_hover_text(&hit.what)
                        .clicked()
                    {
                        go = Some([hit.x, hit.z]);
                    }
                }
                if let Some(note) = &found.note {
                    ui.weak(note);
                }
            }
            Some(Err(e)) => {
                ui.colored_label(ui.visuals().error_fg_color, e);
            }
            None => {}
        }
        if ui.small_button("Clear").clicked() {
            close = true;
        }
        if let Some([x, z]) = go {
            self.settings.view.centre = [f64::from(x) + 0.5, f64::from(z) + 0.5];
        }
        if close {
            self.found = None;
        }
    }

    pub(super) fn view_section(&mut self, ui: &mut egui::Ui) {
        let view = &mut self.settings.view;
        ui.horizontal(|ui| {
            ui.label(format!(
                "1 pixel = {} block{}",
                view.scale,
                if view.scale == 1 { "" } else { "s" }
            ));
            if ui.small_button("+").on_hover_text("Zoom in").clicked() {
                view.zoom(true, [0.0, 0.0]);
            }
            if ui.small_button("−").on_hover_text("Zoom out").clicked() {
                view.zoom(false, [0.0, 0.0]);
            }
        });
        ui.horizontal(|ui| {
            ui.label("Height");
            ui.add(egui::DragValue::new(&mut view.y).range(-64..=319))
                .on_hover_text(
                    "Biomes are drawn at this y: a slice, not the surface, unless biomes at the \
                     surface are shown.",
                );
        });
        ui.horizontal(|ui| {
            ui.label("Go to");
            let edit = ui.add(
                egui::TextEdit::singleline(&mut self.goto)
                    .hint_text("x z or x y z")
                    .desired_width(110.0),
            );
            if edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                match parse_goto(&self.goto) {
                    Some((x, y, z)) => {
                        view.centre = [f64::from(x) + 0.5, f64::from(z) + 0.5];
                        if let Some(y) = y {
                            view.y = y;
                        }
                        self.goto_error = false;
                    }
                    None => self.goto_error = true,
                }
            }
        });
        if self.goto_error {
            ui.colored_label(ui.visuals().error_fg_color, "Type x z or x y z, in blocks.");
        }
    }

    pub(super) fn layers_section(&mut self, ui: &mut egui::Ui) {
        ui.checkbox(&mut self.settings.ties, "Tied biome cells")
            .on_hover_text("Where the game's biome depends on lookup history, show a checker of the biomes it may give.");
        let overworld_only = "Only in the Overworld.";
        let slime_here = self.slime_allowed();
        let slime_visible = self.settings.view.scale <= spyglass::MAX_SLIME_SCALE;
        ui.add_enabled(
            slime_here && slime_visible,
            egui::Checkbox::new(&mut self.settings.slime_chunks, "Slime chunks"),
        )
        .on_disabled_hover_text(if slime_here {
            "Zoom in to 16 blocks per pixel or closer to see slime chunks."
        } else {
            overworld_only
        });
        let spawn_here = self.spawn_allowed();
        ui.add_enabled(
            spawn_here,
            egui::Checkbox::new(&mut self.settings.spawn, "World spawn"),
        )
        .on_hover_text("Where the game's spawn search starts.")
        .on_disabled_hover_text(overworld_only);
        let terrain_here = self.terrain_allowed();
        let terrain = terrain_here && self.settings.view.scale <= MAX_TERRAIN_SCALE;
        let far = if terrain_here {
            "Zoom in to 4 blocks per pixel or closer to see terrain."
        } else {
            "Terrain isn't drawn under a ceiling: the surface heights are the ceiling's."
        };
        ui.add_enabled(
            terrain,
            egui::Checkbox::new(&mut self.settings.relief, "Relief"),
        )
        .on_hover_text("Shade the map by terrain height, lit from the north-west. Slow to draw.")
        .on_disabled_hover_text(far);
        ui.add_enabled(
            terrain,
            egui::Checkbox::new(&mut self.settings.surface, "Biomes at the surface"),
        )
        .on_hover_text("Draw each column's biome at its terrain height instead of at one y.")
        .on_disabled_hover_text(far);
        ui.checkbox(&mut self.settings.confirm, "Confirm with terrain")
            .on_hover_text(
                "Decide structure starts and locates with the seed's terrain, as the game does. \
                 Off, most surface structures are candidates. Jigsaw structures' bounds are \
                 drawn when confirmed, at 4 blocks per pixel or closer.",
            );
        let Some(world) = self.world else {
            return;
        };
        if !self.structures_generate() {
            ui.weak("Structures don't generate in this save.");
        }
        let mut structures: Vec<&str> = world.structures().collect();
        structures.sort_unstable();
        let header = match self.settings.structures.len() {
            0 => "Structures".to_string(),
            n => format!("Structures ({n})"),
        };
        let changed = egui::CollapsingHeader::new(header)
            .id_salt("structures")
            .show(ui, |ui| {
                pick_list(
                    ui,
                    "structure list",
                    &structures,
                    &mut self.settings.structures,
                    &mut self.structure_filter,
                )
            })
            .body_returned
            .unwrap_or(false);
        if changed {
            self.select();
        }
        let mut biomes: Vec<&str> = world.biomes().iter().map(String::as_str).collect();
        biomes.sort_unstable();
        let header = match self.settings.highlight.len() {
            0 => "Highlight biomes".to_string(),
            n => format!("Highlight biomes ({n})"),
        };
        egui::CollapsingHeader::new(header)
            .id_salt("highlight")
            .show(ui, |ui| {
                pick_list(
                    ui,
                    "biome list",
                    &biomes,
                    &mut self.settings.highlight,
                    &mut self.biome_filter,
                )
            });
    }
}
