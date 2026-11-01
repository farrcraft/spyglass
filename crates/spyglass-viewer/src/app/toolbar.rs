//! The toolbar: the world on the map (seed, saved worlds, version, dimension, save), sharing,
//! export, and the settings window with where game data and user data are kept. Under it, a row for anything that needs
//! saying: errors, warnings, the save shown, the last export.

use std::hash::{BuildHasher, Hasher};

use super::saves::listed_saves;
use super::*;
use crate::style;

impl App {
    pub(super) fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("spyglass")
                    .size(15.0)
                    .strong()
                    .color(style::TEXT),
            )
            .on_hover_text(spyglass::build::version());
            ui.separator();
            self.seed_field(ui);
            self.worlds_menu(ui);
            self.version_picker_bar(ui);
            self.dimension_toggles(ui);
            self.save_menu(ui);
            if self.loading.is_some() || self.inspecting.is_some() {
                ui.spinner();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .selectable_label(self.settings_open, "Settings")
                    .on_hover_text("Settings: where game data is, and adding game versions")
                    .clicked()
                {
                    self.settings_open = !self.settings_open;
                }
                self.export_menu(ui);
                self.share_menu(ui);
            });
        });
        ui.add_space(2.0);
        self.notices(ui);
    }

    /// The seed, shown on Enter or when the field is left with a new one.
    fn seed_field(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new("Seed").color(style::MUTED));
        let edit = ui.add(
            egui::TextEdit::singleline(&mut self.settings.seed)
                .font(egui::TextStyle::Monospace)
                .desired_width(180.0)
                .hint_text("a number or text"),
        );
        let entered = edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        let changed = edit.lost_focus()
            && spyglass::parse_seed(&self.settings.seed).ok()
                != self.shown.as_ref().map(|s| s.session.seed());
        if entered || changed {
            self.save = None;
            self.apply_seed();
        }
        if ui
            .small_button("Copy")
            .on_hover_text("Copy the seed")
            .clicked()
            && let Some(shown) = &self.shown
        {
            ui.ctx().copy_text(shown.session.seed().to_string());
        }
        if ui
            .small_button("Random")
            .on_hover_text("Show a random seed")
            .clicked()
        {
            let seed = std::collections::hash_map::RandomState::new()
                .build_hasher()
                .finish() as i64;
            self.settings.seed = seed.to_string();
            self.save = None;
            self.apply_seed();
        }
    }

    fn version_picker_bar(&mut self, ui: &mut egui::Ui) {
        let versions: Vec<BundleInfo> = self.bundles.clone().unwrap_or_default();
        let current = match self.settings.version.clone() {
            Some(v) if versions.iter().any(|b| b.version == v) => v,
            _ => "none".to_string(),
        };
        let mut chosen = None;
        ui.label(egui::RichText::new("Version").color(style::MUTED));
        egui::ComboBox::from_id_salt("version")
            .selected_text(current.as_str())
            .width(120.0)
            .show_ui(ui, |ui| {
                for bundle in &versions {
                    if ui
                        .selectable_label(bundle.version == current, &bundle.version)
                        .clicked()
                    {
                        chosen = Some(bundle.clone());
                    }
                }
            });
        if let Some(bundle) = chosen {
            self.save = None;
            self.open(ui.ctx(), &bundle);
        }
    }

    fn dimension_toggles(&mut self, ui: &mut egui::Ui) {
        let here = self.settings.dimension();
        let mut picked = None;
        egui::Frame::new()
            .fill(style::GROUND)
            .stroke(egui::Stroke::new(1.0, style::BORDER))
            .corner_radius(egui::CornerRadius::same(7))
            .inner_margin(egui::Margin::same(2))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                ui.horizontal(|ui| {
                    for dimension in Dimension::ALL {
                        if ui
                            .selectable_label(dimension == here, dimension_label(dimension))
                            .clicked()
                            && dimension != here
                        {
                            picked = Some(dimension);
                        }
                    }
                });
            });
        if let Some(dimension) = picked {
            self.switch_dimension(ui.ctx(), dimension);
        }
    }

    /// Opening a world save: the launchers' saves, most recently played first, or a folder.
    fn save_menu(&mut self, ui: &mut egui::Ui) {
        let title = self.save.as_ref().map_or("Open save…".to_string(), |r| {
            format!("Save: {}", r.save.name)
        });
        let mut chosen = None;
        let mut relist = false;
        ui.menu_button(title, |ui| {
            let saves = self.saves.get_or_insert_with(listed_saves);
            if saves.is_empty() {
                ui.weak("No saves in the launchers' folders.");
            } else {
                ui.add(
                    egui::TextEdit::singleline(&mut self.save_filter)
                        .hint_text("Filter saves")
                        .desired_width(220.0),
                )
                .request_focus();
                let needle = self.save_filter.to_lowercase();
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .show(ui, |ui| {
                        for save in saves.iter() {
                            let name = save
                                .dir
                                .file_name()
                                .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                            if !name.to_lowercase().contains(&needle) {
                                continue;
                            }
                            if ui
                                .button(name)
                                .on_hover_text(format!("{}\n{}", save.launcher, save.dir.display()))
                                .clicked()
                            {
                                chosen = Some(save.dir.clone());
                                ui.close();
                            }
                        }
                    });
            }
            ui.separator();
            if ui.button("Open a save folder…").clicked() {
                ui.close();
                if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                    chosen = Some(dir);
                }
            }
            if ui.button("List the launchers' saves again").clicked() {
                relist = true;
            }
        });
        if relist {
            self.saves = None;
        }
        if let Some(dir) = chosen {
            self.open_save(ui.ctx(), dir);
        }
    }

    /// Saving the view as an image, or as the command that draws it.
    fn export_menu(&mut self, ui: &mut egui::Ui) {
        let export = self.export();
        let ready = export.is_some() && self.exporting.is_none();
        ui.add_enabled_ui(export.is_some(), |ui| {
            ui.menu_button("Export", |ui| {
                if ui
                    .add_enabled(ready, egui::Button::new("Image of this view…"))
                    .clicked()
                {
                    ui.close();
                    if let Some(export) = &export
                        && let Some(path) = rfd::FileDialog::new()
                            .add_filter("PNG image", &["png"])
                            .set_file_name("map.png")
                            .save_file()
                    {
                        self.start_export(ui.ctx(), export.clone(), path);
                    }
                }
                ui.checkbox(&mut self.settings.export_markers, "Include markers")
                    .on_hover_text("Your markers in view, as pins in the image");
                if ui
                    .button("Copy the command that draws it")
                    .on_hover_text("The spyglass render command for this view")
                    .clicked()
                    && let Some(export) = &export
                {
                    let command = export.command("map.png");
                    ui.ctx().copy_text(command.clone());
                    self.export_note = Some(Ok(format!("Copied: {command}")));
                    ui.close();
                }
            });
        });
        if self.exporting.is_some() {
            ui.spinner();
        }
    }

    /// Errors, warnings and notes from the toolbar's work, in one row under it.
    fn notices(&mut self, ui: &mut egui::Ui) {
        let mut lines: Vec<(egui::Color32, String)> = Vec::new();
        if let Some(e) = &self.load_error {
            lines.push((style::ERROR, e.clone()));
        }
        if let Some(e) = &self.seed_error {
            lines.push((style::ERROR, e.clone()));
        }
        if let Some(unchecked) = self.current_bundle().and_then(|b| b.unchecked_version()) {
            lines.push((style::WARN, unchecked.to_string()));
        }
        if self.inspecting.is_some() {
            lines.push((style::MUTED, "Reading the save…".to_string()));
        }
        if let Some(e) = &self.save_error {
            lines.push((style::ERROR, e.clone()));
        }
        if let Some(report) = &self.save {
            lines.push((
                style::MUTED,
                format!(
                    "Showing the save {}, made in Minecraft {}.",
                    report.save.name,
                    report.save.version_name.as_deref().unwrap_or("?")
                ),
            ));
            for warning in &report.warnings {
                lines.push((style::WARN, warning.to_string()));
            }
        }
        if self.exporting.is_some() {
            lines.push((style::MUTED, "Drawing the image…".to_string()));
        }
        match &self.export_note {
            Some(Ok(note)) => lines.push((style::MUTED, note.clone())),
            Some(Err(e)) => lines.push((style::ERROR, e.clone())),
            None => {}
        }
        self.data_notices(&mut lines);
        let data_problems = !self.store.problems.is_empty() && !self.data_problems_seen;
        if lines.is_empty() {
            return;
        }
        ui.horizontal_wrapped(|ui| {
            for (colour, text) in lines {
                ui.colored_label(colour, text);
            }
            if data_problems {
                ui.small_button("Details").on_hover_ui(|ui| {
                    for problem in self.store.problems.iter().take(30) {
                        ui.label(style::mono(problem.clone()).size(11.5));
                    }
                    if self.store.problems.len() > 30 {
                        ui.label(format!("and {} more", self.store.problems.len() - 30));
                    }
                });
            }
            if (self.export_note.is_some()
                || self.save_error.is_some()
                || self.data_note.is_some()
                || data_problems)
                && ui.small_button("Dismiss").clicked()
            {
                self.export_note = None;
                self.save_error = None;
                self.data_note = None;
                self.data_problems_seen = true;
            }
        });
        ui.add_space(2.0);
    }

    /// Where game data is kept, and adding game versions.
    pub(super) fn settings_window(&mut self, ctx: &egui::Context) {
        let mut open = self.settings_open || self.extracting.is_some();
        egui::Window::new("Settings")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(420.0)
            .show(ctx, |ui| {
                style::heading(ui, "Game data");
                self.bundles_folder_choice(ui);
                if let Err(e) = &self.bundles {
                    ui.colored_label(style::ERROR, e);
                }
                if ui.button("Look for bundles again").clicked() {
                    self.scan(ui.ctx());
                }
                ui.add_space(8.0);
                style::heading(ui, "Add a game version");
                self.extract_section(ui);
                ui.add_space(8.0);
                style::heading(ui, "Your markers and worlds");
                self.data_section(ui);
                ui.add_space(8.0);
                style::heading(ui, "About");
                ui.label(format!("spyglass {}", spyglass::build::version()));
            });
        self.settings_open = open;
    }
}
