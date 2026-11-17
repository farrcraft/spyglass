//! The side panel: its tabs, the Layers tab, the Locate tab, and exporting the view. The
//! Markers tab is `markers_panel`'s.

use super::*;
use crate::style;

/// The side panel's tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Tab {
    /// What the map shows.
    #[default]
    Layers,
    /// Seed searches and finds in the seed shown.
    Search,
    /// Locate's hits.
    Locate,
    /// The user's markers and done places.
    Markers,
    /// What spyglass read from the save shown, while one is.
    World,
}

impl App {
    pub(super) fn side_panel(&mut self, ui: &mut egui::Ui) {
        let hits = match &self.found {
            Some(Ok(found)) => Some(found.hits.len()),
            _ => None,
        };
        let searching = self
            .search_run
            .as_ref()
            .is_some_and(|r| r.running.is_some());
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            let tab = |ui: &mut egui::Ui, current: &mut Tab, tab: Tab, text: String| {
                let selected = *current == tab;
                let text = egui::RichText::new(text).size(13.5);
                let text = if selected {
                    text.strong().color(style::TEXT)
                } else {
                    text.color(style::MUTED)
                };
                let response = ui.add(egui::Button::new(text).frame(false));
                if selected {
                    let tab_rect = response.rect;
                    ui.painter().line_segment(
                        [
                            tab_rect.left_bottom() + egui::vec2(0.0, 3.0),
                            tab_rect.right_bottom() + egui::vec2(0.0, 3.0),
                        ],
                        egui::Stroke::new(2.0, style::ACCENT),
                    );
                }
                if response.clicked() {
                    *current = tab;
                }
            };
            tab(ui, &mut self.tab, Tab::Layers, "Layers".to_string());
            tab(
                ui,
                &mut self.tab,
                Tab::Search,
                if searching { "Search •" } else { "Search" }.to_string(),
            );
            let locate = match (self.locating.is_some(), hits) {
                (true, _) => "Locate •".to_string(),
                (false, Some(n)) => format!("Locate ({n})"),
                (false, None) => "Locate".to_string(),
            };
            tab(ui, &mut self.tab, Tab::Locate, locate);
            let dimension = self.world.map(|w| w.dimension_kind().id());
            let markers = self.here().map_or(0, |h| {
                h.live_markers()
                    .filter(|m| Some(m.dimension.as_str()) == dimension)
                    .count()
            });
            let label = if markers > 0 {
                format!("Markers ({markers})")
            } else {
                "Markers".to_string()
            };
            tab(ui, &mut self.tab, Tab::Markers, label);
            if let Some(details) = self.save_report().and(self.save_details.as_ref()) {
                let warnings = details.warnings.iter().filter(|w| !w.is_note()).count();
                let label = if warnings > 0 {
                    format!("World ({warnings})")
                } else {
                    "World".to_string()
                };
                tab(ui, &mut self.tab, Tab::World, label);
            }
        });
        if self.tab == Tab::World && self.save_report().is_none() {
            self.tab = Tab::Layers;
        }
        ui.add_space(4.0);
        ui.separator();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(4.0);
                match self.tab {
                    Tab::Layers => self.layers_tab(ui),
                    Tab::Search => self.search_section(ui),
                    Tab::Locate => self.locate_section(ui),
                    Tab::Markers => self.markers_tab(ui),
                    Tab::World => self.world_tab(ui),
                }
                ui.add_space(8.0);
            });
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
        let area = visible_area(&self.settings.view, self.map_size)?;
        Some(Export {
            area,
            source,
            dimension: world.dimension_kind(),
            bundles: (self.settings.bundles != Settings::default().bundles)
                .then(|| self.bundles_dir()),
            structures: if self.structures_generate() {
                self.settings.structures.clone()
            } else {
                Vec::new()
            },
            features: self.settings.features.clone(),
            loot: self
                .selection
                .loot
                .as_ref()
                .map_or(String::new(), |q| q.argument().to_string()),
            highlight: self.settings.highlight.clone(),
            ties: self.settings.ties,
            slime_chunks: self.settings.slime_chunks && self.slime_allowed(),
            generated: self.settings.generated && self.generated_allowed(),
            inhabited: self.settings.inhabited,
            spawn: self.settings.spawn && self.spawn_allowed(),
            confirm: self.settings.confirm,
            relief: self.settings.relief && terrain,
            surface: self.settings.surface && terrain,
            veins: self.underground().veins,
            noise_caves: self.underground().noise_caves,
            carvers: self.underground().carvers,
            flowers: self.settings.flowers,
            flower: self
                .settings
                .flower
                .clone()
                .filter(|_| self.settings.flowers),
            pins: if self.settings.export_markers {
                self.export_pins(world.dimension_kind(), &area)
            } else {
                Vec::new()
            },
        })
    }

    /// The user's markers an image of `area` in `dimension` shows: those whose pin reaches it,
    /// and not done ones while done things are hidden.
    fn export_pins(&self, dimension: Dimension, area: &spyglass::MapArea) -> Vec<spyglass::Pin> {
        let Some(here) = self.here() else {
            return Vec::new();
        };
        let reach = i64::from(spyglass::MARKER_REACH);
        let inside = |[px, pz]: [i64; 2]| {
            (-reach..i64::from(area.size[0]) + reach).contains(&px)
                && (-reach..i64::from(area.size[1]) + reach).contains(&pz)
        };
        here.live_markers()
            .filter(|m| m.dimension == dimension.id() && (self.settings.show_done || !m.done))
            .map(|m| spyglass::Pin {
                position: [m.x, m.z],
                colour: m.colour,
            })
            .filter(|p| inside(area.pixel(p.position)))
            .collect()
    }

    /// Draws `export` with the library and writes it to `path`, in the background.
    pub(super) fn start_export(&mut self, ctx: &egui::Context, export: Export, path: PathBuf) {
        let (Some(shown), Some(world)) = (&self.shown, self.world) else {
            return;
        };
        let session = shown.session.clone();
        self.export_note = None;
        self.exporting = Some(Pending::spawn(ctx, move || {
            let layers = Layers {
                loot: if export.loot.is_empty() || export.area.scale > spyglass::MAX_FEATURE_SCALE {
                    None
                } else {
                    Some(
                        world
                            .item_query(&export.loot, None)
                            .map_err(|e| e.to_string())?,
                    )
                },
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
                features: if export.area.scale <= spyglass::MAX_FEATURE_SCALE {
                    export
                        .features
                        .iter()
                        .filter_map(|f| world.feature_query(f).ok())
                        .collect()
                } else {
                    Vec::new()
                },
                slime_chunks: export.slime_chunks && export.area.scale <= spyglass::MAX_SLIME_SCALE,
                generated: export.generated
                    && export.area.scale <= spyglass::MAX_SLIME_SCALE
                    && session.save().is_some(),
                inhabited: export.inhabited,
                spawn: export.spawn,
                confirm: export.confirm,
                relief: export.relief,
                surface: export.surface,
                veins: export.veins && export.area.scale <= MAX_UNDERGROUND_SCALE,
                noise_caves: export.noise_caves && export.area.scale <= MAX_UNDERGROUND_SCALE,
                carvers: export.carvers && export.area.scale <= MAX_UNDERGROUND_SCALE,
                flowers: export.flowers,
                flower: export.flower.clone(),
                pins: export.pins.clone(),
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

    /// The last locate's answer: click a hit to go there. Locates start from the map's
    /// right-click menu.
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
                ui.label(egui::RichText::new(format!("from {x} {y} {z}")).color(style::MUTED));
                ui.add_space(4.0);
                if found.nearest.is_empty() {
                    if let Some(answer) = found.hits.iter().find_map(|h| h.answer) {
                        ui.label(egui::RichText::new(answer).color(style::MUTED));
                    }
                    go = go.or(hit_table(ui, "locate hits", &found.hits));
                } else {
                    style::heading(ui, "Nearest");
                    go = go.or(hit_table(ui, "nearest hits", &found.nearest));
                    ui.add_space(6.0);
                    style::heading(ui, "/locate gives");
                    go = go.or(hit_table(ui, "locate hits", &found.hits));
                }
                if let Some(note) = &found.note {
                    style::hint(ui, note);
                }
                ui.add_space(4.0);
                if ui.button("Clear").clicked() {
                    close = true;
                }
            }
            Some(Err(e)) => {
                ui.colored_label(style::ERROR, e);
            }
            None => {
                ui.label("Right-click the map to find the nearest structure, feature or biome from there.");
            }
        }
        if let Some([x, z]) = go {
            self.settings.view.centre = [f64::from(x) + 0.5, f64::from(z) + 0.5];
        }
        if close {
            self.found = None;
        }
    }

    /// The height biomes and the underground layers are drawn at: a slider over the level, and
    /// presets for the surface at the view's centre, y 0, and just above the bottom.
    fn height_row(&mut self, ui: &mut egui::Ui) {
        let [min_y, height] = self.world.map_or([-64, 384], |w| w.level());
        let top = min_y + height - 1;
        ui.horizontal(|ui| {
            ui.label("Height");
            ui.add(
                egui::Slider::new(&mut self.settings.view.y, min_y..=top)
                    .clamping(egui::SliderClamping::Always),
            )
            .on_hover_text(
                "Biomes, the underground layers and flowers are drawn at this y: a slice.",
            );
        });
        ui.horizontal(|ui| {
            let centre = self.settings.view.centre.map(|c| c.floor() as i32);
            if let Some(shown) = &self.shown
                && ui
                    .small_button("Surface")
                    .on_hover_text("The terrain's top block at the centre of the view.")
                    .clicked()
            {
                let height = shown.session.height_at(centre, Heightmap::WorldSurfaceWg);
                self.settings.view.y = (height - 1).clamp(min_y, top);
            }
            if ui
                .small_button("Underground")
                .on_hover_text("y 0.")
                .clicked()
            {
                self.settings.view.y = 0.clamp(min_y, top);
            }
            if ui
                .small_button("Bottom")
                .on_hover_text("8 above the level's bottom.")
                .clicked()
            {
                self.settings.view.y = (min_y + 8).min(top);
            }
        });
    }

    /// The underground layers: ore veins, noise caves and carvers at the view's height.
    fn underground_section(&mut self, ui: &mut egui::Ui) {
        style::heading(ui, "Underground");
        let close = self.settings.view.scale <= MAX_UNDERGROUND_SCALE;
        let veins_here = self.veins_allowed();
        ui.add_enabled(
            veins_here,
            egui::Checkbox::new(&mut self.settings.veins, "Ore veins"),
        )
        .on_hover_text("Copper and iron veins: bright where ore is, faint where filler is.");
        if !veins_here {
            style::hint(ui, "Only where the dimension has ore veins.");
        }
        ui.checkbox(&mut self.settings.noise_caves, "Noise caves")
            .on_hover_text("Caves in the terrain itself: air and fluid with ground above.");
        ui.checkbox(&mut self.settings.carvers, "Carver caves and ravines")
            .on_hover_text("What carvers cut: caves in yellow, ravines in magenta.");
        if self.underground().any() {
            if close {
                style::hint(ui, "At the height above. Slow to draw.");
            } else {
                style::hint(ui, "Zoom to 1:4 or closer to see them.");
            }
        }
    }

    /// Flowers: what bone meal grows at each block at the view's height, the key of those in
    /// view (click one to show only it), and the heights where the one shown covers most of
    /// the view.
    fn flowers_section(&mut self, ui: &mut egui::Ui) {
        style::heading(ui, "Flowers");
        ui.checkbox(&mut self.settings.flowers, "Bone meal flowers")
            .on_hover_text(
                "The flower bone meal grows on grass at each block, at the height above: the \
                 flower's own block, one above the grass.",
            );
        if !self.settings.flowers {
            return;
        }
        style::hint(
            ui,
            "Opaque where the flower is certain, fainter as it's less likely; dimmed where no \
             flower grows. Hover the map for the answer at a block.",
        );
        if let Some(error) = &self.flower_error {
            ui.colored_label(style::ERROR, error);
        }
        let mut toggle = None;
        ui.add_space(2.0);
        if let Some(flower) = &self.settings.flower
            && !self.flowers_in_view.iter().any(|f| f.flower == *flower)
            && flower_row(ui, flower, true, "not in view").clicked()
        {
            toggle = Some(flower.clone());
        }
        let pixels = self.flowers_in_view_pixels.max(1);
        for count in self.flowers_in_view.iter().take(KEY_ROWS) {
            let selected = self.settings.flower.as_ref() == Some(&count.flower);
            let note = if count.certain > 0 {
                format!(
                    "{} · certain {}",
                    percent(count.possible, pixels),
                    percent(count.certain, pixels)
                )
            } else {
                percent(count.possible, pixels)
            };
            if flower_row(ui, &count.flower, selected, &note)
                .on_hover_text(
                    "The share of the view it can grow in, and is certain in. Click to show \
                     only it.",
                )
                .clicked()
            {
                toggle = Some(count.flower.clone());
            }
        }
        if let Some(flower) = toggle {
            self.settings.flower = if self.settings.flower.as_ref() == Some(&flower) {
                None
            } else {
                Some(flower)
            };
            self.best_heights = None;
        }
        ui.add_space(4.0);
        self.best_height(ui);
    }

    /// Best height: the heights where the flower shown covers most of the view, worked out in
    /// the background over the view at a coarser scale. Clicking a height moves the view to it.
    fn best_height(&mut self, ui: &mut egui::Ui) {
        let Some(flower) = self.settings.flower.clone() else {
            style::hint(
                ui,
                "Click a flower to show only it and find its best height.",
            );
            return;
        };
        ui.horizontal(|ui| {
            let running = self.flower_heights.is_some();
            if ui
                .add_enabled(!running, egui::Button::new("Best height"))
                .on_hover_text(
                    "The heights where this flower covers most of the view: certain first, \
                     then possible.",
                )
                .clicked()
            {
                self.start_best_height(ui.ctx(), flower.clone());
            }
            if running {
                ui.spinner();
            }
        });
        let mut go = None;
        match &self.best_heights {
            Some(Ok(best)) if best.flower == flower => {
                let rows = best.best();
                if rows.is_empty() {
                    style::hint(ui, "It doesn't grow anywhere in view, at any height.");
                } else {
                    style::hint(ui, &best.over);
                    egui::Grid::new("best heights")
                        .num_columns(3)
                        .striped(true)
                        .show(ui, |ui| {
                            for share in rows {
                                let percent = |n: u64| percent(n, share.pixels);
                                if ui
                                    .selectable_label(
                                        self.settings.view.y == share.y,
                                        style::mono(format!("y {}", share.y)),
                                    )
                                    .on_hover_text("Show the map at this height")
                                    .clicked()
                                {
                                    go = Some(share.y);
                                }
                                ui.label(format!("certain {}", percent(share.certain)));
                                ui.label(
                                    egui::RichText::new(format!(
                                        "possible {}",
                                        percent(share.possible)
                                    ))
                                    .color(style::MUTED),
                                );
                                ui.end_row();
                            }
                        });
                }
            }
            Some(Err(e)) => {
                ui.colored_label(style::ERROR, e);
            }
            _ => {}
        }
        if let Some(y) = go {
            self.settings.view.y = y;
        }
    }

    /// Starts working out `flower`'s best heights over the view, every
    /// [`BEST_HEIGHT_STEP`] blocks through the level.
    fn start_best_height(&mut self, ctx: &egui::Context, flower: String) {
        let (Some(shown), Some(world)) = (&self.shown, self.world) else {
            return;
        };
        let Some(view) = visible_area(&self.settings.view, self.map_size) else {
            return;
        };
        let area = coarser(view, BEST_HEIGHT_PIXELS);
        let over = format!(
            "Over the {} × {} blocks in view, every {BEST_HEIGHT_STEP} blocks of height.",
            u64::from(view.size[0]) * u64::from(view.scale),
            u64::from(view.size[1]) * u64::from(view.scale),
        );
        let [min_y, height] = world.level();
        let range = [min_y, min_y + height - 1];
        let session = shown.session.clone();
        self.best_heights = None;
        self.flower_heights = Some(Pending::spawn(ctx, move || {
            let shares = session
                .flower_heights(&area, &flower, range, BEST_HEIGHT_STEP)
                .map_err(|e| e.to_string())?;
            Ok(BestHeights {
                flower,
                over,
                shares,
            })
        }));
    }

    /// Features to mark: dungeons, geodes, wells, fossils and lava lakes, up close.
    fn features_section(&mut self, ui: &mut egui::Ui) {
        style::heading(ui, "Features");
        let mut changed = false;
        for (name, placed) in icons::FEATURE_KINDS {
            let mut on = placed
                .iter()
                .all(|p| self.settings.features.iter().any(|f| f == p));
            if ui.checkbox(&mut on, *name).changed() {
                self.settings
                    .features
                    .retain(|f| !placed.contains(&f.as_str()));
                if on {
                    self.settings
                        .features
                        .extend(placed.iter().map(|p| p.to_string()));
                }
                changed = true;
            }
        }
        if changed {
            self.select();
        }
        if !self.settings.features.is_empty() {
            if self.settings.view.scale <= spyglass::MAX_FEATURE_SCALE {
                style::hint(
                    ui,
                    "Certain ones filled, likely ones outlined. Slow to draw: each chunk is placed.",
                );
            } else {
                style::hint(ui, "Zoom to 1:2 or closer to see them.");
            }
        }
    }

    /// Loot containers, up close: every one, and rings around those holding an item.
    fn loot_section(&mut self, ui: &mut egui::Ui) {
        style::heading(ui, "Loot");
        let mut changed = ui
            .checkbox(&mut self.settings.containers, "Containers")
            .on_hover_text(
                "Every chest, barrel and other loot container structures and dungeons place.",
            )
            .changed();
        ui.horizontal(|ui| {
            ui.label("Ring those holding");
            let edit = ui.add(
                style::text_box(&mut self.settings.loot)
                    .hint_text("minecraft:diamond")
                    .desired_width(150.0),
            );
            if edit.lost_focus() || (edit.changed() && self.settings.loot.is_empty()) {
                changed = true;
            }
        });
        if changed {
            self.select();
        }
        if let Some(error) = &self.selection.loot_error {
            ui.colored_label(style::ERROR, error);
        }
        if self.settings.containers || self.selection.loot.is_some() {
            if self.settings.view.scale <= spyglass::MAX_FEATURE_SCALE {
                style::hint(
                    ui,
                    "Gold rings: a full one where it certainly holds the item, a thin one where it \
                     might. Click a container for its contents. Slow to draw: each chunk is placed.",
                );
            } else {
                style::hint(ui, "Zoom to 1:2 or closer to see them.");
            }
        }
    }

    /// What's drawn on the map: biomes, overlays, structures, and whether to confirm with
    /// terrain.
    pub(super) fn layers_tab(&mut self, ui: &mut egui::Ui) {
        self.biomes_section(ui);

        ui.add_space(6.0);
        self.overlays_section(ui);

        ui.add_space(6.0);
        self.underground_section(ui);

        ui.add_space(6.0);
        self.flowers_section(ui);

        ui.add_space(6.0);
        self.features_section(ui);

        ui.add_space(6.0);
        self.loot_section(ui);

        ui.add_space(6.0);
        self.structure_list(ui);

        ui.add_space(6.0);
        ui.separator();
        ui.checkbox(&mut self.settings.confirm, "Confirm with terrain");
        style::hint(
            ui,
            "Decides structure starts and locates with the seed's terrain, as the game does. Off, \
             most surface structures are candidates. Jigsaw and mineshaft bounds are drawn when \
             confirmed, at 1:4 or closer.",
        );
    }

    /// The Biomes section: the height, biomes at the surface, the End's void, tied cells and
    /// the highlighted biomes.
    fn biomes_section(&mut self, ui: &mut egui::Ui) {
        let terrain = self.terrain_allowed();
        let ceiling = "Not under a ceiling: the surface there is the ceiling.";
        style::heading(ui, "Biomes");
        // The height is the underground layers' and the flowers' too, so it stays usable for
        // them.
        let deep = self.underground().any() || self.settings.flowers;
        ui.add_enabled_ui(!(self.settings.surface && terrain) || deep, |ui| {
            self.height_row(ui);
        });
        ui.add_enabled(
            terrain,
            egui::Checkbox::new(&mut self.settings.surface, "At the surface"),
        )
        .on_hover_text("Draw each column's biome at its terrain height instead of at one y.");
        if !terrain {
            style::hint(ui, ceiling);
        } else if self.settings.surface {
            style::hint(
                ui,
                "Slow to draw: biomes at the height show until it's done.",
            );
        }
        let void_shown = terrain && (self.settings.surface || self.settings.relief);
        if self
            .world
            .is_some_and(|w| w.dimension_kind() == Dimension::End)
            && !void_shown
        {
            style::hint(
                ui,
                "The void between the End's islands shows with biomes at the surface or relief: the biome alone is the same over it.",
            );
        }
        ui.checkbox(&mut self.settings.ties, "Tied biome cells").on_hover_text(
            "Where the game's biome depends on lookup history, show a checker of the biomes it may give.",
        );
        self.highlight_chips(ui);
    }

    /// The Overlays section: the spawn, slime chunks, a save's generated chunks, done things
    /// and relief shading.
    fn overlays_section(&mut self, ui: &mut egui::Ui) {
        let terrain = self.terrain_allowed();
        let ceiling = "Not under a ceiling: the surface there is the ceiling.";
        style::heading(ui, "Overlays");
        let overworld_only = "Only in the Overworld.";
        let spawn_here = self.spawn_allowed();
        ui.add_enabled(
            spawn_here,
            egui::Checkbox::new(&mut self.settings.spawn, "World spawn"),
        )
        .on_hover_text("Where the game's spawn search starts.");
        if !spawn_here {
            style::hint(ui, overworld_only);
        }
        let slime_here = self.slime_allowed();
        let slime_visible = self.settings.view.scale <= spyglass::MAX_SLIME_SCALE;
        ui.add_enabled(
            slime_here && slime_visible,
            egui::Checkbox::new(&mut self.settings.slime_chunks, "Slime chunks"),
        );
        if !slime_here {
            style::hint(ui, overworld_only);
        } else if !slime_visible {
            style::hint(ui, "Zoom to 1:16 or closer to see them.");
        }
        let generated_here = self.generated_allowed();
        ui.add_enabled(
            generated_here && slime_visible,
            egui::Checkbox::new(&mut self.settings.generated, "Generated chunks"),
        )
        .on_hover_text(
            "The chunks your world has generated: magenta, fainter where generated partway, \
             amber where an older version generated them.",
        );
        ui.indent("inhabited", |ui| {
            ui.add_enabled(
                generated_here && slime_visible && self.settings.generated,
                egui::Checkbox::new(&mut self.settings.inhabited, "By time played"),
            )
            .on_hover_text(
                "Shade each chunk by the time players spent near it: faint where they passed \
                 through, strong where they lived.",
            );
        });
        if !generated_here {
            style::hint(ui, "Open a world to see its chunks.");
        } else if !slime_visible {
            style::hint(ui, "Zoom to 1:16 or closer to see them.");
        }
        ui.checkbox(&mut self.settings.show_done, "Done things")
            .on_hover_text("Structures, pins and markers you've marked done, dimmed.");
        ui.add_enabled(
            terrain,
            egui::Checkbox::new(&mut self.settings.relief, "Relief shading"),
        )
        .on_hover_text("Shade the map by terrain height, lit from the north-west.");
        if terrain {
            style::hint(ui, "Slow to draw.");
        } else {
            style::hint(ui, ceiling);
        }
    }

    /// The highlighted biomes as removable chips, and a menu to add one.
    fn highlight_chips(&mut self, ui: &mut egui::Ui) {
        let Some(world) = self.world else {
            return;
        };
        let mut remove = None;
        ui.horizontal_wrapped(|ui| {
            for id in &self.settings.highlight {
                let [r, g, b] = spyglass::biome_colour(id);
                egui::Frame::new()
                    .fill(style::SURFACE)
                    .corner_radius(egui::CornerRadius::same(10))
                    .inner_margin(egui::Margin {
                        left: 8,
                        right: 2,
                        top: 1,
                        bottom: 1,
                    })
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        let (swatch, _) =
                            ui.allocate_exact_size(egui::vec2(9.0, 9.0), Sense::hover());
                        ui.painter()
                            .rect_filled(swatch, 2.0, Color32::from_rgb(r, g, b));
                        ui.label(spyglass::display_name(id));
                        if ui
                            .add(egui::Button::new("×").frame(false))
                            .on_hover_text("Stop highlighting it")
                            .clicked()
                        {
                            remove = Some(id.clone());
                        }
                    });
            }
            let mut biomes: Vec<&str> = world.biomes().iter().map(String::as_str).collect();
            biomes.sort_unstable();
            ui.menu_button("+ Highlight a biome", |ui| {
                ui.add(
                    style::text_box(&mut self.biome_filter)
                        .hint_text("Filter biomes")
                        .desired_width(200.0),
                )
                .request_focus();
                let needle = self.biome_filter.to_lowercase();
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .show(ui, |ui| {
                        for id in biomes {
                            let name = spyglass::display_name(id);
                            if !name.to_lowercase().contains(&needle) && !id.contains(&needle) {
                                continue;
                            }
                            let mut on = self.settings.highlight.iter().any(|h| h == id);
                            if style::menu_checkbox(ui, &mut on, name).changed() {
                                if on {
                                    self.settings.highlight.push(id.to_string());
                                    self.settings.highlight.sort();
                                } else {
                                    self.settings.highlight.retain(|h| h != id);
                                }
                            }
                        }
                    });
            });
        });
        if let Some(id) = remove {
            self.settings.highlight.retain(|h| *h != id);
        }
    }

    /// The structures to mark: a filter and a checkable row per family, with its icon and how
    /// many are in view, opening to its variants.
    fn structure_list(&mut self, ui: &mut egui::Ui) {
        let Some(world) = self.world else {
            return;
        };
        ui.horizontal(|ui| {
            style::heading(ui, "Structures");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !self.settings.structures.is_empty() && ui.small_button("Clear").clicked() {
                    self.settings.structures.clear();
                    self.select();
                }
            });
        });
        if !self.structures_generate() {
            style::hint(ui, "Structures don't generate in this save.");
        }
        ui.add(
            style::text_box(&mut self.structure_filter)
                .hint_text("Filter structures")
                .desired_width(f32::INFINITY),
        );
        // One row per family, its variants under it when opened.
        let mut groups: BTreeMap<String, Vec<&str>> = BTreeMap::new();
        // Only what can generate here: the sets are every dimension's.
        for id in world
            .structures()
            .filter(|id| world.structure_generates(id))
        {
            groups.entry(icons::family_name(id)).or_default().push(id);
        }
        let needle = self.structure_filter.to_lowercase();
        let mut changed = false;
        for (name, ids) in groups {
            changed |= self.structure_family(ui, world, &name, ids, &needle);
        }
        if changed {
            self.select();
        }
    }

    /// One structure family's row in the list, `ids` its structures, and its variants under it
    /// when opened or when `needle` (the filter, lower-case) matches only them. Hidden if
    /// nothing in it matches. Whether the structures chosen changed.
    fn structure_family(
        &mut self,
        ui: &mut egui::Ui,
        world: &World,
        name: &str,
        mut ids: Vec<&str>,
        needle: &str,
    ) -> bool {
        let mut changed = false;
        let matches = |id: &str| {
            spyglass::display_name(id).to_lowercase().contains(needle) || id.contains(needle)
        };
        if !name.to_lowercase().contains(needle) && !ids.iter().any(|id| matches(id)) {
            return false;
        }
        ids.sort_unstable_by_key(|id| spyglass::display_name(id));
        let on = ids
            .iter()
            .filter(|id| self.settings.structures.iter().any(|s| s == **id))
            .count();
        let open = self.expanded.contains(name);
        let row = self.family_row(ui, world, name, &ids, on, open);
        if let Some(all) = row.set_all {
            self.settings
                .structures
                .retain(|s| !ids.contains(&s.as_str()));
            if all {
                self.settings
                    .structures
                    .extend(ids.iter().map(|id| id.to_string()));
            }
            self.settings.structures.sort();
            changed = true;
        }
        if row.toggle_open {
            if open {
                self.expanded.remove(name);
            } else {
                self.expanded.insert(name.to_string());
            }
        }
        if open || (!needle.is_empty() && !name.to_lowercase().contains(needle)) {
            ui.indent(name, |ui| {
                for id in ids.iter().filter(|id| open || matches(id)) {
                    changed |= self.variant_checkbox(ui, id);
                }
            });
        }
        changed
    }

    /// A structure family's own row: its checkbox (mixed when `on` of `ids` are chosen), badge
    /// and name, and on the right the button that opens its variants and how many start in
    /// view.
    fn family_row(
        &self,
        ui: &mut egui::Ui,
        world: &World,
        name: &str,
        ids: &[&str],
        on: usize,
        open: bool,
    ) -> FamilyRow {
        let mut all = on == ids.len();
        let mut flip = false;
        let mut toggle_open = false;
        ui.horizontal(|ui| {
            flip = ui
                .add(egui::Checkbox::new(&mut all, "").indeterminate(on > 0 && on < ids.len()))
                .changed();
            let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), Sense::hover());
            if let Some(badge) = world.structure_set(ids[0]).map(icons::badge) {
                let dim = if on > 0 { 1.0 } else { 0.45 };
                ui.painter().rect(
                    rect,
                    egui::CornerRadius::same(5),
                    badge.fill.gamma_multiply(dim),
                    egui::Stroke::new(1.0, style::GROUND),
                    egui::StrokeKind::Inside,
                );
                icons::glyph(ids[0])
                    .tint(badge.ink.gamma_multiply(dim))
                    .paint_at(ui, rect.shrink(3.0));
            }
            ui.label(name);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ids.len() > 1 {
                    let text = if open {
                        "Hide".to_string()
                    } else {
                        format!("{} kinds", ids.len())
                    };
                    toggle_open = ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(text).size(11.5).color(style::ACCENT),
                            )
                            .frame(false),
                        )
                        .on_hover_text("Choose among its variants")
                        .clicked();
                }
                let seen: usize = ids.iter().filter_map(|id| self.in_view.get(*id)).sum();
                if seen > 0 {
                    ui.label(style::mono(seen.to_string()).color(style::MUTED))
                        .on_hover_text("Starts in view");
                }
            });
        });
        FamilyRow {
            set_all: flip.then_some(all),
            toggle_open,
        }
    }

    /// One variant's checkbox under its open family. Whether the structures chosen changed.
    fn variant_checkbox(&mut self, ui: &mut egui::Ui, id: &str) -> bool {
        let mut variant = self.settings.structures.iter().any(|s| s == id);
        let label = spyglass::display_name(id);
        if !ui.checkbox(&mut variant, label).changed() {
            return false;
        }
        if variant {
            self.settings.structures.push(id.to_string());
            self.settings.structures.sort();
        } else {
            self.settings.structures.retain(|s| s != id);
        }
        true
    }
}

/// What a click on a structure family's row asked for.
struct FamilyRow {
    /// Choose every structure in the family (`true`) or none, when its checkbox was clicked.
    set_all: Option<bool>,
    /// Open or close its list of variants.
    toggle_open: bool,
}

/// A locate answer's hits as a table: where, how far, and whether certain. Returns the block
/// of the hit clicked, to go to.
fn hit_table(ui: &mut egui::Ui, id: &str, hits: &[locate::Hit]) -> Option<[i32; 2]> {
    let mut go = None;
    egui::Grid::new(id)
        .num_columns(3)
        .spacing([12.0, 4.0])
        .striped(true)
        .show(ui, |ui| {
            ui.label(egui::RichText::new("Where").size(11.0).color(style::MUTED));
            ui.label(egui::RichText::new("Away").size(11.0).color(style::MUTED));
            ui.label("");
            ui.end_row();
            for hit in hits {
                let y = hit.y.map_or("~".to_string(), |y| y.to_string());
                if ui
                    .selectable_label(false, style::mono(format!("{} {y} {}", hit.x, hit.z)))
                    .on_hover_text(&hit.what)
                    .clicked()
                {
                    go = Some([hit.x, hit.z]);
                }
                ui.label(style::mono(hit.blocks_away.to_string()));
                if hit.saved {
                    ui.colored_label(style::GOOD, "Saved")
                        .on_hover_text("Your world has generated it: it's there.");
                } else if hit.certain {
                    ui.colored_label(style::GOOD, "Certain");
                } else {
                    ui.colored_label(style::MUTED, "Candidate");
                }
                ui.end_row();
            }
        });
    go
}

/// The most flowers the key lists.
const KEY_ROWS: usize = 12;
/// The most pixels a side Best height samples the view at: it coarsens the scale until the
/// view fits.
const BEST_HEIGHT_PIXELS: u32 = 128;
/// Blocks between the heights Best height tries.
const BEST_HEIGHT_STEP: u32 = 2;
/// The heights Best height lists.
const BEST_HEIGHT_ROWS: usize = 6;

/// Best height's answer for a flower.
pub(super) struct BestHeights {
    /// The flower, by block id.
    pub flower: String,
    /// What it was worked out over, for the panel.
    pub over: String,
    /// Each height's share, lowest first.
    pub shares: Vec<spyglass::FlowerShare>,
}

impl BestHeights {
    /// The heights to list: where the flower is certain in the most pixels, then where it can
    /// grow in the most, then the lowest; only heights where it can grow.
    pub fn best(&self) -> Vec<spyglass::FlowerShare> {
        let mut best: Vec<spyglass::FlowerShare> = self
            .shares
            .iter()
            .copied()
            .filter(|s| s.possible > 0)
            .collect();
        best.sort_by_key(|s| (std::cmp::Reverse((s.certain, s.possible)), s.y));
        best.truncate(BEST_HEIGHT_ROWS);
        best
    }
}

/// `area` at a coarser scale, covering the same blocks, until neither side is over `pixels`
/// (or the scale is [`spyglass::MAX_SCALE`]). Its corner moves back to a multiple of the new
/// scale, as a map area's must.
fn coarser(area: spyglass::MapArea, pixels: u32) -> spyglass::MapArea {
    let mut out = area;
    while (out.size[0] > pixels || out.size[1] > pixels) && out.scale < spyglass::MAX_SCALE {
        let scale = out.scale * 2;
        let step = i64::from(scale);
        for d in 0..2 {
            let min = i64::from(area.min[d]);
            let end = min + i64::from(area.size[d]) * i64::from(area.scale);
            let start = min.div_euclid(step) * step;
            out.min[d] = i32::try_from(start).expect("within the area's blocks");
            out.size[d] = u32::try_from((end - start + step - 1) / step).expect("fewer pixels");
        }
        out.scale = scale;
    }
    out
}

/// `part` of `whole` as a percentage, with a decimal below 10% so a small field doesn't read
/// as none: `0%` only for none, `<0.1%` for a sliver.
fn percent(part: u64, whole: u64) -> String {
    if part == 0 {
        return "0%".to_string();
    }
    let share = 100.0 * part as f64 / whole.max(1) as f64;
    if share < 0.1 {
        "<0.1%".to_string()
    } else if share < 10.0 {
        format!("{share:.1}%")
    } else {
        format!("{share:.0}%")
    }
}

/// A row of the flower key: its swatch, its name and `note`, selected if it's the flower shown.
fn flower_row(ui: &mut egui::Ui, flower: &str, selected: bool, note: &str) -> egui::Response {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let [r, g, b] = spyglass::flower_colour(flower);
        let (swatch, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), Sense::hover());
        ui.painter()
            .rect_filled(swatch, 2.0, Color32::from_rgb(r, g, b));
        let response = ui.selectable_label(selected, spyglass::display_name(flower));
        ui.label(egui::RichText::new(note).color(style::MUTED));
        response
    })
    .inner
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_shares_keep_a_decimal() {
        assert_eq!(percent(0, 100), "0%");
        assert_eq!(percent(1, 10_000), "<0.1%");
        assert_eq!(percent(25, 1_000), "2.5%");
        assert_eq!(percent(1, 3), "33%");
        assert_eq!(percent(5, 5), "100%");
    }

    #[test]
    fn best_height_samples_the_same_blocks_coarser() {
        let view = spyglass::MapArea::centred_on([100, -40], [1000, 300], 2, 64).unwrap();
        let area = coarser(view, BEST_HEIGHT_PIXELS);
        assert_eq!(area.scale, 16);
        assert!(area.size[0] <= BEST_HEIGHT_PIXELS && area.size[1] <= BEST_HEIGHT_PIXELS);
        area.check().unwrap();
        // It covers every block the view does.
        let end = |a: &spyglass::MapArea, d: usize| {
            i64::from(a.min[d]) + i64::from(a.size[d]) * i64::from(a.scale)
        };
        for d in 0..2 {
            assert!(area.min[d] <= view.min[d] && end(&area, d) >= end(&view, d));
        }
    }
}
