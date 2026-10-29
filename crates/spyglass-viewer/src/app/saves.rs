//! World saves and bundles: reading a save, showing it, and the screen shown until there's a
//! bundle.

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

    pub(super) fn choose_bundles(&mut self, ctx: &egui::Context) {
        if let Some(dir) = rfd::FileDialog::new()
            .set_directory(&self.settings.bundles)
            .pick_folder()
        {
            self.set_bundles(ctx, dir);
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
            "spyglass reads world generation data from your own copy of the game's server, one \
             folder (a bundle) per game version. It can download the server from Mojang and \
             extract a bundle for you. That needs Java.",
        );
        ui.add_space(12.0);
        ui.strong("Where to keep game data");
        self.bundles_folder_choice(ui);
        if let Err(e) = &self.bundles {
            ui.colored_label(ui.visuals().error_fg_color, e);
        }
        ui.add_space(12.0);
        ui.strong("Game version");
        self.extract_section(ui);
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.weak("Or extract with the spyglass-extract command, then");
            if ui.small_button("Look again").clicked() {
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
