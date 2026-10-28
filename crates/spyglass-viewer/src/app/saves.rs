//! World saves and bundles: the save list, a save's report, and choosing where bundles are.

use super::*;

impl App {
    /// Reads a save in the background, then shows it.
    pub(super) fn open_save(&mut self, ctx: &egui::Context, dir: PathBuf) {
        let bundles = self.settings.bundles.clone();
        self.save_error = None;
        self.inspecting = Some(Pending::spawn(ctx, move || {
            spyglass::inspect_save(&dir, &bundles, Scan::Chunks).map_err(|e| e.to_string())
        }));
    }

    /// Shows an inspected save: its world and seed, in its player's dimension if spyglass
    /// answers there (otherwise the Overworld), centred on its player or else its spawn.
    pub(super) fn show_save(&mut self, ctx: &egui::Context, report: SaveReport) {
        let save = &report.save;
        let dimension = save
            .player
            .as_ref()
            .and_then(spyglass::SaveLocation::dimension_kind)
            .filter(|&d| report.answerable_in(d))
            .unwrap_or(Dimension::Overworld);
        let bundle = match (&report.bundle, report.answerable_in(dimension)) {
            (Some(bundle), true) => bundle.clone(),
            _ => {
                let reasons: Vec<String> = report
                    .warnings
                    .iter()
                    .filter(|w| w.blocks(dimension))
                    .map(ToString::to_string)
                    .collect();
                self.save_error = Some(format!(
                    "spyglass can't show {}: {}",
                    report.save.name,
                    reasons.join("; ")
                ));
                return;
            }
        };
        if dimension != self.settings.dimension() {
            self.set_dimension(dimension);
            self.world = None;
            self.shown = None;
        }
        let save = &report.save;
        let here = |l: &&spyglass::SaveLocation| l.dimension_kind() == Some(dimension);
        if let Some(place) = save
            .player
            .as_ref()
            .filter(here)
            .or(save.spawn.as_ref().filter(here))
        {
            let [x, _, z] = place.block();
            self.settings.view.centre = [f64::from(x) + 0.5, f64::from(z) + 0.5];
        }
        self.settings.seed = save.seed.to_string();
        self.save = Some(report);
        self.found = None;
        self.open(ctx, &bundle);
        // If that world is already loaded, the load finishes at once and applies the seed.
    }

    pub(super) fn world_section(&mut self, ui: &mut egui::Ui) {
        let here = self.settings.dimension();
        let mut picked = None;
        egui::ComboBox::from_label("Dimension")
            .selected_text(dimension_label(here))
            .show_ui(ui, |ui| {
                for d in Dimension::ALL {
                    if ui.selectable_label(d == here, dimension_label(d)).clicked() {
                        picked = Some(d);
                    }
                }
            });
        if let Some(d) = picked {
            self.switch_dimension(ui.ctx(), d);
        }
        let versions: Vec<BundleInfo> = self.bundles.clone().unwrap_or_default();
        let current = match self.settings.version.clone() {
            Some(v) if versions.iter().any(|b| b.version == v) => v,
            _ => "none".to_string(),
        };
        let mut chosen = None;
        egui::ComboBox::from_label("Version")
            .selected_text(current.as_str())
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
        if self.loading.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Loading game data…");
            });
        }
        if let Some(e) = &self.load_error {
            ui.colored_label(ui.visuals().error_fg_color, e);
        }
        if let Some(unchecked) = self.current_bundle().and_then(|b| b.unchecked_version()) {
            ui.colored_label(ui.visuals().warn_fg_color, unchecked.to_string());
        }
        ui.horizontal(|ui| {
            ui.label("Seed");
            let edit =
                ui.add(egui::TextEdit::singleline(&mut self.settings.seed).desired_width(140.0));
            if edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter))
                || ui.button("Show").clicked()
            {
                self.save = None;
                self.apply_seed();
            }
        });
        if let Some(e) = &self.seed_error {
            ui.colored_label(ui.visuals().error_fg_color, e);
        } else if let Some(shown) = &self.shown {
            ui.weak(format!("seed {}", shown.session.seed()));
        }
        self.save_section(ui);
        let dir = self.bundles_dir().display().to_string();
        ui.add(
            egui::Label::new(egui::RichText::new(format!("Bundles in {dir}")).weak()).truncate(),
        )
        .on_hover_text(dir);
        ui.horizontal(|ui| {
            if ui.button("Choose folder…").clicked() {
                self.choose_bundles(ui.ctx());
            }
            if ui.button("Look again").clicked() {
                self.scan(ui.ctx());
            }
        });
    }

    /// Opening a world save, and what spyglass makes of the one shown.
    pub(super) fn save_section(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        let saves = self.saves.get_or_insert_with(listed_saves);
        let filter = &mut self.save_filter;
        let mut chosen = None;
        let title = self
            .save
            .as_ref()
            .map_or("Open a save…".to_string(), |r| r.save.name.clone());
        egui::ComboBox::from_id_salt("saves")
            .selected_text(title)
            .width(200.0)
            .show_ui(ui, |ui| {
                if saves.is_empty() {
                    ui.weak("No saves in the launchers' folders.");
                    return;
                }
                ui.add(
                    egui::TextEdit::singleline(filter)
                        .hint_text("filter")
                        .desired_width(180.0),
                )
                .request_focus();
                let needle = filter.to_lowercase();
                for save in saves.iter() {
                    let name = save
                        .dir
                        .file_name()
                        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                    if !name.to_lowercase().contains(&needle) {
                        continue;
                    }
                    if ui
                        .selectable_label(false, name)
                        .on_hover_text(format!("{}\n{}", save.launcher, save.dir.display()))
                        .clicked()
                    {
                        chosen = Some(save.dir.clone());
                    }
                }
            });
        ui.horizontal(|ui| {
            if ui.button("Open save folder…").clicked()
                && let Some(dir) = rfd::FileDialog::new().pick_folder()
            {
                chosen = Some(dir);
            }
            if ui
                .small_button("⟳")
                .on_hover_text("List the launchers' saves again")
                .clicked()
            {
                self.saves = None;
            }
        });
        if let Some(dir) = chosen {
            self.open_save(ui.ctx(), dir);
        }
        if self.inspecting.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Reading the save…");
            });
        }
        if let Some(e) = &self.save_error {
            ui.colored_label(ui.visuals().error_fg_color, e);
        }
        if let Some(report) = &self.save {
            ui.weak(format!(
                "{} · Minecraft {}",
                report.save.name,
                report.save.version_name.as_deref().unwrap_or("?")
            ));
            for warning in &report.warnings {
                ui.colored_label(ui.visuals().warn_fg_color, warning.to_string());
            }
        }
    }

    pub(super) fn choose_bundles(&mut self, ctx: &egui::Context) {
        if let Some(dir) = rfd::FileDialog::new()
            .set_directory(&self.settings.bundles)
            .pick_folder()
        {
            self.settings.bundles = dir;
            self.scan(ctx);
        }
    }

    /// The screen shown until there's a bundle to read.
    pub(super) fn no_bundle(&mut self, ui: &mut egui::Ui) {
        let width = ui.available_width().min(560.0);
        let margin = (ui.available_width() - width) / 2.0;
        ui.add_space(ui.available_height() / 5.0);
        ui.horizontal(|ui| {
            ui.add_space(margin);
            ui.vertical(|ui| {
                ui.set_width(width);
                self.no_bundle_text(ui);
            });
        });
    }

    pub(super) fn no_bundle_text(&mut self, ui: &mut egui::Ui) {
        ui.heading("No game data yet");
        ui.add_space(8.0);
        ui.label(
            "spyglass reads world generation data extracted from your own copy of the game's \
             server, one folder (a bundle) per game version. Extract one from the spyglass \
             repository with:",
        );
        ui.add_space(4.0);
        let command = "cargo run -p spyglass-extract -- 26.3";
        ui.horizontal(|ui| {
            ui.code(command);
            if ui.small_button("Copy").clicked() {
                ui.ctx().copy_text(command.to_string());
            }
        });
        ui.add_space(4.0);
        ui.label("Use latest for the newest release. It needs Java and downloads the server jar from Mojang.");
        ui.add_space(12.0);
        match &self.bundles {
            Ok(_) => ui.label(format!("No bundles in {}.", self.bundles_dir().display())),
            Err(e) => ui.colored_label(ui.visuals().error_fg_color, e),
        };
        ui.horizontal(|ui| {
            if ui.button("Choose bundles folder…").clicked() {
                self.choose_bundles(ui.ctx());
            }
            if ui.button("Look again").clicked() {
                self.scan(ui.ctx());
            }
        });
    }
}

/// The launchers' saves, most recently played first.
pub(super) fn listed_saves() -> Vec<LauncherSave> {
    let played = |s: &LauncherSave| {
        std::fs::metadata(s.dir.join("level.dat"))
            .and_then(|m| m.modified())
            .ok()
    };
    let mut saves: Vec<(Option<std::time::SystemTime>, LauncherSave)> = spyglass::launcher_saves()
        .into_iter()
        .map(|s| (played(&s), s))
        .collect();
    saves.sort_by_key(|(played, _)| std::cmp::Reverse(*played));
    saves.into_iter().map(|(_, s)| s).collect()
}
