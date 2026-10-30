//! The side panel: its tabs, the Layers tab, the Locate tab, and exporting the view.

use super::*;
use crate::style;

/// The side panel's tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Tab {
    #[default]
    Layers,
    Search,
    Locate,
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
                    let r = response.rect;
                    ui.painter().line_segment(
                        [
                            r.left_bottom() + egui::vec2(0.0, 3.0),
                            r.right_bottom() + egui::vec2(0.0, 3.0),
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
        });
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
                ui.label("Right-click the map to find the nearest structure or biome from there.");
            }
        }
        if let Some([x, z]) = go {
            self.settings.view.centre = [f64::from(x) + 0.5, f64::from(z) + 0.5];
        }
        if close {
            self.found = None;
        }
    }

    /// What's drawn on the map: biomes, overlays, structures, and whether to confirm with
    /// terrain.
    pub(super) fn layers_tab(&mut self, ui: &mut egui::Ui) {
        let terrain_here = self.terrain_allowed();
        let terrain = terrain_here && self.settings.view.scale <= MAX_TERRAIN_SCALE;
        let far = if terrain_here {
            "Zoom to 1:4 or closer to draw terrain."
        } else {
            "Not under a ceiling: the surface there is the ceiling."
        };

        style::heading(ui, "Biomes");
        ui.add_enabled_ui(!(self.settings.surface && terrain), |ui| {
            ui.horizontal(|ui| {
                ui.label("Height");
                ui.add(
                    egui::Slider::new(&mut self.settings.view.y, -64..=319)
                        .clamping(egui::SliderClamping::Always),
                )
                .on_hover_text("Biomes are drawn at this y: a slice, not the surface.");
            });
        });
        ui.add_enabled(
            terrain,
            egui::Checkbox::new(&mut self.settings.surface, "At the surface"),
        )
        .on_hover_text("Draw each column's biome at its terrain height instead of at one y.");
        if !terrain {
            style::hint(ui, far);
        }
        let void_shown = terrain && (self.settings.surface || self.settings.relief);
        if self
            .world
            .is_some_and(|w| w.dimension_kind() == Dimension::End)
            && !void_shown
        {
            style::hint(
                ui,
                "The void between the End's islands shows at 1:4 or closer, with biomes at the                  surface or relief: the biome alone is the same over it.",
            );
        }
        ui.checkbox(&mut self.settings.ties, "Tied biome cells").on_hover_text(
            "Where the game's biome depends on lookup history, show a checker of the biomes it may give.",
        );
        self.highlight_chips(ui);

        ui.add_space(6.0);
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
        ui.add_enabled(
            terrain,
            egui::Checkbox::new(&mut self.settings.relief, "Relief shading"),
        )
        .on_hover_text("Shade the map by terrain height, lit from the north-west.");
        if terrain {
            style::hint(ui, "Slow to draw.");
        } else {
            style::hint(ui, far);
        }

        ui.add_space(6.0);
        self.structure_list(ui);

        ui.add_space(6.0);
        ui.separator();
        ui.checkbox(&mut self.settings.confirm, "Confirm with terrain");
        style::hint(
            ui,
            "Decides structure starts and locates with the seed's terrain, as the game does. Off, \
             most surface structures are candidates. Jigsaw bounds are drawn when confirmed, at \
             1:4 or closer.",
        );
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
                            ui.allocate_exact_size(egui::vec2(9.0, 9.0), egui::Sense::hover());
                        ui.painter()
                            .rect_filled(swatch, 2.0, egui::Color32::from_rgb(r, g, b));
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
                    egui::TextEdit::singleline(&mut self.biome_filter)
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
                            if ui.checkbox(&mut on, name).changed() {
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
            egui::TextEdit::singleline(&mut self.structure_filter)
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
        for (name, mut ids) in groups {
            let matches = |id: &str| {
                spyglass::display_name(id).to_lowercase().contains(&needle) || id.contains(&needle)
            };
            if !name.to_lowercase().contains(&needle) && !ids.iter().any(|id| matches(id)) {
                continue;
            }
            ids.sort_unstable_by_key(|id| spyglass::display_name(id));
            let on = ids
                .iter()
                .filter(|id| self.settings.structures.iter().any(|s| s == **id))
                .count();
            let mut all = on == ids.len();
            let open = self.expanded.contains(&name);
            let mut toggle_open = false;
            let mut flip = false;
            ui.horizontal(|ui| {
                flip = ui
                    .add(egui::Checkbox::new(&mut all, "").indeterminate(on > 0 && on < ids.len()))
                    .changed();
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
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
                ui.label(&name);
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
            if flip {
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
            if toggle_open {
                if open {
                    self.expanded.remove(&name);
                } else {
                    self.expanded.insert(name.clone());
                }
            }
            if open || (!needle.is_empty() && !name.to_lowercase().contains(&needle)) {
                ui.indent(&name, |ui| {
                    for id in ids.iter().filter(|id| open || matches(id)) {
                        let mut variant = self.settings.structures.iter().any(|s| s == id);
                        let label = spyglass::display_name(id);
                        if ui.checkbox(&mut variant, label).changed() {
                            if variant {
                                self.settings.structures.push(id.to_string());
                                self.settings.structures.sort();
                            } else {
                                self.settings.structures.retain(|s| s != id);
                            }
                            changed = true;
                        }
                    }
                });
            }
        }
        if changed {
            self.select();
        }
    }
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
                if hit.certain {
                    ui.colored_label(style::GOOD, "Certain");
                } else {
                    ui.colored_label(style::MUTED, "Candidate");
                }
                ui.end_row();
            }
        });
    go
}
