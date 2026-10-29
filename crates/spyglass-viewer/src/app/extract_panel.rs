//! Getting game data from the viewer (ADR 0042, ADR 0043): where bundles go, the game version
//! picker and the extraction's progress.

use super::*;

impl App {
    /// Choosing where bundles are: beside spyglass, the per-user folder, or another one. Not
    /// while an extraction is writing to one.
    pub(super) fn bundles_folder_choice(&mut self, ui: &mut egui::Ui) {
        let current = self.bundles_dir();
        let places = [
            (spyglass::app_bundles_dir(), "Next to spyglass"),
            (spyglass::user_bundles_dir(), "In your user folder"),
        ];
        let path = |ui: &mut egui::Ui, dir: &Path| {
            ui.indent(dir, |ui| {
                ui.add(
                    egui::Label::new(egui::RichText::new(dir.display().to_string()).weak())
                        .truncate(),
                );
            });
        };
        let mut chosen = None;
        let mut elsewhere = false;
        ui.add_enabled_ui(self.extracting.is_none(), |ui| {
            let mut known = false;
            for (dir, label) in places {
                let Some(dir) = dir else {
                    continue;
                };
                known |= dir == current;
                if ui.radio(dir == current, label).clicked() && dir != current {
                    chosen = Some(dir.clone());
                }
                path(ui, &dir);
            }
            elsewhere = ui.radio(!known, "Somewhere else…").clicked();
            if !known {
                path(ui, &current);
            }
        });
        if let Some(dir) = chosen {
            self.set_bundles(ui.ctx(), dir);
        } else if elsewhere {
            self.choose_bundles(ui.ctx());
        }
    }

    /// Uses the bundles in `dir` from now on.
    pub(super) fn set_bundles(&mut self, ctx: &egui::Context, dir: PathBuf) {
        self.settings.bundles = dir;
        self.extract_note = None;
        self.offer_user_folder = false;
        self.scan(ctx);
    }

    /// The game version picker and the Extract button, or the extraction under way.
    pub(super) fn extract_section(&mut self, ui: &mut egui::Ui) {
        if self.offers.is_none() && self.offering.is_none() {
            self.offering = Some(Pending::spawn(ui.ctx(), extract::fetch_offers));
        }
        if let Some(running) = &self.extracting {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(format!("{}: {}", running.version, running.step()));
            });
            ui.weak("This takes a minute or two.");
            return;
        }
        match &self.offers {
            None => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Getting the list of game versions…");
                });
            }
            Some(Err(e)) => {
                ui.colored_label(ui.visuals().error_fg_color, e);
                if ui.button("Try again").clicked() {
                    self.offers = None;
                }
            }
            Some(Ok(offers)) => self.version_picker(ui, &offers.clone()),
        }
        match &self.extract_note {
            Some(Ok(note)) => {
                ui.label(note);
            }
            Some(Err(e)) => {
                ui.colored_label(ui.visuals().error_fg_color, e);
            }
            None => {}
        }
        if self.offer_user_folder
            && let Some(user) = spyglass::user_bundles_dir()
            && ui.button("Use your user folder instead").clicked()
        {
            self.set_bundles(ui.ctx(), user);
        }
    }

    fn version_picker(&mut self, ui: &mut egui::Ui, offers: &[Offer]) {
        let have: Vec<String> = self
            .bundles
            .as_ref()
            .map(|found| found.iter().map(|b| b.version.clone()).collect())
            .unwrap_or_default();
        let label = |o: &Offer| {
            if have.contains(&o.id) {
                format!("{} (have it)", o.id)
            } else if o.checked {
                o.id.clone()
            } else {
                format!("{} (not checked)", o.id)
            }
        };
        if self
            .extract_version
            .as_ref()
            .is_none_or(|v| !offers.iter().any(|o| o.id == *v))
        {
            self.extract_version = extract::preferred(offers).map(|o| o.id.clone());
        }
        let chosen = offers
            .iter()
            .find(|o| Some(&o.id) == self.extract_version.as_ref());
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("extract version")
                .selected_text(chosen.map_or_else(String::new, label))
                .width(180.0)
                .show_ui(ui, |ui| {
                    for offer in offers {
                        let text = egui::RichText::new(label(offer));
                        let text = if offer.release { text } else { text.weak() };
                        ui.selectable_value(
                            &mut self.extract_version,
                            Some(offer.id.clone()),
                            text,
                        );
                    }
                });
            let ready = chosen.is_some_and(|o| !have.contains(&o.id));
            if ui
                .add_enabled(ready, egui::Button::new("Extract"))
                .clicked()
                && let Some(offer) = chosen
            {
                self.start_extract(ui.ctx(), offer.id.clone());
            }
        });
        if let Some(offer) = chosen
            && !offer.checked
        {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                format!(
                    "This build of spyglass wasn't checked against {}. Its maps will show, but \
                     may differ from the game's.",
                    offer.id
                ),
            );
        }
        ui.weak("Downloads the version's server from Mojang, then runs it with Java.");
    }

    /// Extracts `version` into the bundles folder, creating it first.
    fn start_extract(&mut self, ctx: &egui::Context, version: String) {
        let out = self.bundles_dir();
        self.extract_note = None;
        self.offer_user_folder = false;
        if let Err(e) = std::fs::create_dir_all(&out) {
            self.extract_note = Some(Err(format!("Can't create {}: {e}", out.display())));
            // ADR 0043: the folder beside spyglass may not be writable, as in Program Files.
            self.offer_user_folder = spyglass::user_bundles_dir().is_some_and(|user| user != out);
            return;
        }
        let ctx = ctx.clone();
        self.extracting = Some(extract::Running::start(version, out, move || {
            ctx.request_repaint();
        }));
    }

    /// Picks up the version list and a finished extraction, opening the new bundle.
    pub(super) fn poll_extract(&mut self, ctx: &egui::Context) {
        if let Some(result) = self.offering.as_ref().and_then(Pending::poll) {
            self.offering = None;
            self.offers = Some(
                result.unwrap_or_else(|()| Err("getting the list of game versions failed".into())),
            );
        }
        let Some(running) = &self.extracting else {
            return;
        };
        let Some(result) = running.poll() else {
            return;
        };
        let version = running.version.clone();
        self.extracting = None;
        match result {
            Ok(()) => {
                self.extract_note = Some(Ok(format!("Extracted {version}.")));
                self.settings.version = Some(version);
                self.scan(ctx);
            }
            Err(e) => self.extract_note = Some(Err(e)),
        }
    }
}
