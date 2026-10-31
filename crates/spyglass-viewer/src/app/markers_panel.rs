//! The user's own data on screen (`docs/specs/user-data.md`): the Markers tab, the marker
//! editor, the worlds menu, sharing a view as text, and importing and exporting the data file.
//! Also keeping the data in step with the world shown.

use super::*;
use crate::share::{Flags, SharedView};
use crate::style;
use crate::userdata::{COLOURS, Kind, Place, WorldData, WorldKey};

/// What the map's context menu was opened on, besides a block.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum MenuTarget {
    Marker(String),
    Place(Place),
}

impl App {
    /// The world shown, as the user data keys it.
    fn shown_key(&self) -> Option<WorldKey> {
        let shown = self.shown.as_ref()?;
        Some(WorldKey {
            seed: shown.session.seed(),
            save: self.save.as_ref().map(|r| r.save.name.clone()),
        })
    }

    /// The user data of the world shown.
    pub(super) fn here(&self) -> Option<&WorldData> {
        self.store.data.world(self.world_key.as_ref()?)
    }

    /// The user data of the world shown, to change; the change is written soon.
    pub(super) fn here_mut(&mut self) -> Option<&mut WorldData> {
        let key = self.world_key.clone()?;
        self.store.changed();
        Some(self.store.data.world_entry(&key))
    }

    /// Notes that the world shown was opened now: keeps the views of the world before it, if
    /// this is another, and the version it's shown in.
    pub(super) fn remember_world(&mut self) {
        let Some(key) = self.shown_key() else {
            return;
        };
        if self.world_key.as_ref() != Some(&key) {
            self.keep_views();
        }
        let version = self.world.map(|w| w.version().to_string());
        let dir = self.save.as_ref().map(|r| r.save.dir.clone());
        let world = self.store.data.world_entry(&key);
        world.touch(version.as_deref());
        if let Some(dir) = dir {
            world.set_save_dir(&dir);
        }
        self.world_key = Some(key);
        self.store.changed();
    }

    /// Keeps the views of the world shown in its user data, to go back to.
    pub(super) fn keep_views(&mut self) {
        let mut views = self.settings.views.clone();
        views.insert(self.settings.dimension.clone(), self.settings.view);
        let dimension = self.settings.dimension.clone();
        let Some(world) = self
            .world_key
            .as_ref()
            .and_then(|k| self.store.data.world_mut(k))
        else {
            return;
        };
        if world.views != views || world.dimension != dimension {
            world.views = views;
            world.dimension = dimension;
            self.store.changed();
        }
    }

    /// Shows `seed` in `version` (else the version shown), in `dimension` with `view`.
    fn show(
        &mut self,
        ctx: &egui::Context,
        seed: i64,
        version: Option<&str>,
        dimension: Dimension,
        view: View,
        views: BTreeMap<String, View>,
    ) {
        self.keep_views();
        self.settings.seed = seed.to_string();
        self.save = None;
        self.settings.dimension = dimension.id().to_string();
        self.settings.view = view.sanitized();
        self.settings.views = views;
        self.found = None;
        self.menu_block = None;
        let wanted = version.and_then(|v| {
            self.bundles
                .as_ref()
                .ok()?
                .iter()
                .find(|b| b.version == v)
                .cloned()
        });
        let bundle = match (wanted, version) {
            (Some(bundle), _) => Some(bundle),
            (None, Some(v)) => {
                let here = self.current_bundle();
                self.data_note = Some(Err(format!(
                    "There's no bundle for {v}; showing it in {}.",
                    here.as_ref().map_or("no version", |b| b.version.as_str())
                )));
                here
            }
            (None, None) => self.current_bundle(),
        };
        self.world = None;
        self.shown = None;
        if let Some(bundle) = bundle {
            self.open(ctx, &bundle);
        }
    }

    /// Opens a world from the user data: its seed, or its save, where it was last left.
    pub(super) fn open_world_entry(&mut self, ctx: &egui::Context, key: &WorldKey) {
        let Some(world) = self.store.data.world(key).cloned() else {
            return;
        };
        if let Some(name) = &key.save {
            match world.save_dir.as_deref().map(PathBuf::from) {
                Some(dir) if dir.join("level.dat").is_file() => {
                    self.keep_views();
                    self.open_save(ctx, dir);
                }
                _ => {
                    self.data_note = Some(Err(format!(
                        "The save {name} isn't where it was{}; open it with Open save….",
                        world
                            .save_dir
                            .as_deref()
                            .map_or(String::new(), |d| format!(" ({d})"))
                    )));
                }
            }
            return;
        }
        let dimension = Dimension::from_id(&world.dimension).unwrap_or(Dimension::Overworld);
        let mut views = world.views.clone();
        let view = views.remove(dimension.id()).unwrap_or_default();
        self.show(
            ctx,
            key.seed,
            world.version.as_deref(),
            dimension,
            view,
            views,
        );
    }

    /// The view shown, as a view text carries it.
    pub(super) fn shared_view(&self) -> Option<SharedView> {
        let (shown, world) = (self.shown.as_ref()?, self.world?);
        let s = &self.settings;
        Some(SharedView {
            seed: shown.session.seed(),
            version: Some(world.version().to_string()),
            dimension: world.dimension_kind(),
            centre: s.view.centre.map(|c| c.floor() as i32),
            scale: s.view.scale,
            y: s.view.y,
            structures: s.structures.clone(),
            highlight: s.highlight.clone(),
            features: s.features.clone(),
            flags: Flags {
                ties: s.ties,
                slime: s.slime_chunks,
                spawn: s.spawn,
                confirm: s.confirm,
                relief: s.relief,
                surface: s.surface,
                veins: s.veins,
                caves: s.noise_caves,
                carvers: s.carvers,
            },
        })
    }

    /// Copies the view text of the view shown.
    pub(super) fn copy_view(&mut self, ctx: &egui::Context) {
        if let Some(view) = self.shared_view() {
            ctx.copy_text(view.encode());
            self.data_note = Some(Ok(
                "Copied the view text: paste it into another spyglass to open this view."
                    .to_string(),
            ));
        }
    }

    /// Opens the view in `text`, or says why it can't.
    pub(super) fn open_view_text(&mut self, ctx: &egui::Context, text: &str) -> bool {
        match SharedView::decode(text) {
            Ok(view) => {
                let s = &mut self.settings;
                s.structures = view.structures.clone();
                s.highlight = view.highlight.clone();
                s.features = view.features.clone();
                s.ties = view.flags.ties;
                s.slime_chunks = view.flags.slime;
                s.spawn = view.flags.spawn;
                s.confirm = view.flags.confirm;
                s.relief = view.flags.relief;
                s.surface = view.flags.surface;
                s.veins = view.flags.veins;
                s.noise_caves = view.flags.caves;
                s.carvers = view.flags.carvers;
                let centre = view.centre.map(|c| f64::from(c) + 0.5);
                let shown = View {
                    centre,
                    scale: view.scale,
                    y: view.y,
                };
                self.data_note = None;
                self.show(
                    ctx,
                    view.seed,
                    view.version.as_deref(),
                    view.dimension,
                    shown,
                    BTreeMap::new(),
                );
                true
            }
            Err(e) => {
                self.data_note = Some(Err(e));
                false
            }
        }
    }

    /// Adds a marker at `block` of the dimension shown, and opens it to edit.
    pub(super) fn add_marker(&mut self, block: [i32; 2]) {
        let Some(world) = self.world else {
            return;
        };
        let dimension = world.dimension_kind().id();
        if let Some(here) = self.here_mut() {
            let id = here.add_marker(dimension, block, String::new());
            self.editing = Some(id);
        }
    }

    /// Marks `place` in the dimension shown done or not.
    pub(super) fn set_done(&mut self, place: &Place, done: bool) {
        let Some(world) = self.world else {
            return;
        };
        let dimension = world.dimension_kind().id();
        if let Some(here) = self.here_mut() {
            here.set_done(dimension, place, done);
        }
    }

    /// Changes the marker `id` with `change`, and notes it.
    pub(super) fn change_marker(&mut self, id: &str, change: impl FnOnce(&mut Marker)) {
        if let Some(marker) = self.here_mut().and_then(|h| h.marker_mut(id)) {
            change(marker);
            marker.changed();
        }
    }

    /// The marker being edited, in a window over the map.
    pub(super) fn marker_editor(&mut self, ctx: &egui::Context) {
        let Some(id) = self.editing.clone() else {
            return;
        };
        let Some(mut marker) = self
            .here()
            .and_then(|h| h.live_markers().find(|m| m.id == id))
            .cloned()
        else {
            self.editing = None;
            return;
        };
        let before = marker.clone();
        let mut open = true;
        let mut delete = false;
        let mut go = false;
        egui::Window::new("Marker")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(260.0)
            .show(ctx, |ui| {
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut marker.name)
                        .hint_text("Name")
                        .char_limit(200)
                        .desired_width(f32::INFINITY),
                );
                if marker.name.is_empty() && before.modified == marker.modified {
                    edit.request_focus();
                }
                if edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    self.editing = None;
                }
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    for (name, colour) in COLOURS {
                        let [r, g, b] = colour;
                        let (rect, response) =
                            ui.allocate_exact_size(egui::vec2(20.0, 20.0), Sense::click());
                        ui.painter()
                            .circle_filled(rect.center(), 8.0, Color32::from_rgb(r, g, b));
                        if marker.colour == colour {
                            ui.painter().circle_stroke(
                                rect.center(),
                                9.5,
                                egui::Stroke::new(2.0, style::TEXT),
                            );
                        }
                        if response.on_hover_text(name).clicked() {
                            marker.colour = colour;
                        }
                    }
                });
                ui.add_space(4.0);
                ui.label(style::mono(format!("{}, {}", marker.x, marker.z)).color(style::MUTED));
                ui.checkbox(&mut marker.done, "Done");
                ui.horizontal(|ui| {
                    go = ui.button("Go to it").clicked();
                    delete = ui.button("Delete").clicked();
                });
            });
        if go {
            self.settings.view.centre = [f64::from(marker.x) + 0.5, f64::from(marker.z) + 0.5];
        }
        if delete {
            self.change_marker(&id, Marker::delete);
            self.editing = None;
            return;
        }
        if (marker.name.as_str(), marker.colour, marker.done)
            != (before.name.as_str(), before.colour, before.done)
        {
            self.change_marker(&id, |m| {
                m.name = marker.name;
                m.colour = marker.colour;
                m.done = marker.done;
            });
        }
        if !open {
            self.editing = None;
        }
    }

    /// The Markers tab: the world shown, its markers in this dimension, and what's done.
    pub(super) fn markers_tab(&mut self, ui: &mut egui::Ui) {
        let (Some(world), Some(here)) = (self.world, self.here().cloned()) else {
            ui.label("Open a seed or a save to keep markers in it.");
            return;
        };
        let dimension = world.dimension_kind().id();

        style::heading(ui, "This world");
        // Edited in a buffer of its own, so spaces typed at its end stay while the stored name is
        // trimmed; it's the stored name again whenever the world changes.
        if self.world_name.0 != self.world_key {
            self.world_name = (
                self.world_key.clone(),
                here.name.clone().unwrap_or_default(),
            );
        }
        let name = &mut self.world_name.1;
        let edit = ui
            .add(
                egui::TextEdit::singleline(name)
                    .id_salt("world name")
                    .hint_text("Name it to keep it in Worlds")
                    .char_limit(200)
                    .desired_width(f32::INFINITY),
            )
            .on_hover_text("Named worlds stay in the Worlds menu; unnamed ones are recent.");
        if edit.changed() {
            let name = Some(name.clone()).filter(|n| !n.trim().is_empty());
            if let Some(here) = self.here_mut() {
                here.set_name(name);
            }
        }
        if here.name.is_some()
            && ui
                .small_button("Forget this world")
                .on_hover_text("Remove its name, markers and done places from your data")
                .clicked()
        {
            if let Some(key) = self.world_key.clone() {
                self.store.data.forget(&key);
                self.store.changed();
                self.world_key = None;
                self.world_name = (None, String::new());
                self.remember_world();
            }
            return;
        }

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            style::heading(ui, "Markers");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button("Add at centre")
                    .on_hover_text("Or right-click the map, or press M over it")
                    .clicked()
                {
                    let centre = self.settings.view.centre.map(|c| c.floor() as i32);
                    self.add_marker(centre);
                }
            });
        });
        let markers: Vec<Marker> = here
            .live_markers()
            .filter(|m| m.dimension == dimension)
            .cloned()
            .collect();
        if markers.is_empty() {
            style::hint(
                ui,
                "No markers here. Right-click the map, or press M over it.",
            );
        }
        let mut go = None;
        for m in &markers {
            ui.horizontal(|ui| {
                let [r, g, b] = m.colour;
                let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), Sense::hover());
                ui.painter()
                    .circle_filled(rect.center(), 5.5, Color32::from_rgb(r, g, b));
                let name = if m.name.is_empty() {
                    egui::RichText::new("Unnamed").italics().color(style::MUTED)
                } else if m.done {
                    egui::RichText::new(&m.name)
                        .strikethrough()
                        .color(style::MUTED)
                } else {
                    egui::RichText::new(&m.name)
                };
                if ui
                    .add(egui::Button::new(name).frame(false))
                    .on_hover_text(format!("{}, {} · click to go there", m.x, m.z))
                    .clicked()
                {
                    go = Some([m.x, m.z]);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Edit").clicked() {
                        self.editing = Some(m.id.clone());
                    }
                    ui.label(style::mono(format!("{}, {}", m.x, m.z)).color(style::MUTED));
                });
            });
        }

        let done: Vec<Place> = here
            .done
            .iter()
            .filter(|d| d.done && d.dimension == dimension)
            .map(|d| d.place.clone())
            .collect();
        ui.add_space(6.0);
        style::heading(ui, "Done");
        ui.checkbox(&mut self.settings.show_done, "Show done things on the map")
            .on_hover_text("Done things are drawn dimmed; off, they're hidden.");
        if done.is_empty() {
            style::hint(
                ui,
                "Nothing done here. Right-click a structure, locate pin or search match to mark \
                 it done.",
            );
        }
        let mut undo = None;
        for place in &done {
            ui.horizontal(|ui| {
                let what = match place.kind {
                    Kind::Structure => icons::family_name(&set_member(world, &place.id)),
                    Kind::Biome => spyglass::display_name(&place.id),
                    Kind::Place => "A place".to_string(),
                };
                if ui
                    .add(egui::Button::new(what).frame(false))
                    .on_hover_text(format!("{} · click to go there", place.id))
                    .clicked()
                {
                    go = Some(place.at);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Not done").clicked() {
                        undo = Some(place.clone());
                    }
                    ui.label(
                        style::mono(format!("{}, {}", place.at[0], place.at[1]))
                            .color(style::MUTED),
                    );
                });
            });
        }
        if let Some(place) = undo {
            self.set_done(&place, false);
        }
        if let Some([x, z]) = go {
            self.settings.view.centre = [f64::from(x) + 0.5, f64::from(z) + 0.5];
        }
    }

    /// The worlds menu: saved worlds by name, then recent ones, to open where they were left.
    pub(super) fn worlds_menu(&mut self, ui: &mut egui::Ui) {
        let mut chosen = None;
        ui.menu_button("Worlds", |ui| {
            let mut worlds: Vec<&WorldData> = self.store.data.worlds.iter().collect();
            worlds.sort_by_key(|w| std::cmp::Reverse(w.opened));
            let (saved, recent): (Vec<&WorldData>, Vec<&WorldData>) =
                worlds.into_iter().partition(|w| w.name.is_some());
            let row = |ui: &mut egui::Ui, w: &WorldData| {
                let title = match (&w.name, &w.key.save) {
                    (Some(name), _) => name.clone(),
                    (None, Some(save)) => format!("Save: {save}"),
                    (None, None) => w.key.seed.to_string(),
                };
                let markers = w.live_markers().count();
                let mut detail = vec![w.version.clone().unwrap_or_default()];
                if markers > 0 {
                    detail.push(format!(
                        "{markers} marker{}",
                        if markers == 1 { "" } else { "s" }
                    ));
                }
                let current = self.world_key.as_ref() == Some(&w.key);
                let text = format!("{title}   {}", detail.join(" · "));
                ui.add(egui::Button::new(text).selected(current))
                    .on_hover_text(format!("Seed {}", w.key.seed))
                    .clicked()
            };
            egui::ScrollArea::vertical()
                .max_height(420.0)
                .show(ui, |ui| {
                    style::heading(ui, "Saved");
                    if saved.is_empty() {
                        style::hint(ui, "Name a world in the Markers tab to keep it here.");
                    }
                    for w in &saved {
                        if row(ui, w) {
                            chosen = Some(w.key.clone());
                            ui.close();
                        }
                    }
                    ui.add_space(4.0);
                    style::heading(ui, "Recent");
                    for w in &recent {
                        if row(ui, w) {
                            chosen = Some(w.key.clone());
                            ui.close();
                        }
                    }
                });
        });
        if let Some(key) = chosen {
            self.open_world_entry(ui.ctx(), &key);
        }
    }

    /// Sharing the view as text, and opening a text someone shared.
    pub(super) fn share_menu(&mut self, ui: &mut egui::Ui) {
        let mut open = None;
        ui.menu_button("Share", |ui| {
            if ui
                .add_enabled(
                    self.shown.is_some(),
                    egui::Button::new("Copy the view text"),
                )
                .on_hover_text("A line that opens this view in another spyglass")
                .clicked()
            {
                self.copy_view(ui.ctx());
                ui.close();
            }
            ui.separator();
            ui.label("Open a view text");
            let edit = ui.add(
                egui::TextEdit::singleline(&mut self.share_text)
                    .hint_text("spyglass:1;s=…")
                    .desired_width(280.0),
            );
            let entered = edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            if (ui.button("Open").clicked() || entered) && !self.share_text.trim().is_empty() {
                open = Some(self.share_text.clone());
                ui.close();
            }
            style::hint(ui, "Or paste one onto the map.");
        });
        if let Some(text) = open
            && self.open_view_text(ui.ctx(), &text)
        {
            self.share_text.clear();
        }
    }

    /// The data file: where it is, what went wrong reading it, and import and export.
    pub(super) fn data_section(&mut self, ui: &mut egui::Ui) {
        ui.label(
            egui::RichText::new(self.store.path().display().to_string())
                .monospace()
                .size(11.5)
                .color(style::MUTED),
        )
        .on_hover_text("Markers, done places and saved worlds are kept here.");
        if self.store.read_only {
            ui.colored_label(
                style::WARN,
                "Not saving changes: the file was written by a newer spyglass, or couldn't be \
                 read.",
            );
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button("Export all…").clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("spyglass data", &["json"])
                    .set_file_name("spyglass-data.json")
                    .save_file()
            {
                self.data_note = Some(
                    self.store
                        .export(&path, None)
                        .map(|()| format!("Exported to {}", path.display())),
                );
            }
            let key = self.world_key.clone();
            if ui
                .add_enabled(key.is_some(), egui::Button::new("Export this world…"))
                .clicked()
                && let Some(key) = key
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("spyglass data", &["json"])
                    .set_file_name(format!("spyglass-{}.json", key.seed))
                    .save_file()
            {
                self.data_note = Some(
                    self.store
                        .export(&path, Some(&key))
                        .map(|()| format!("Exported to {}", path.display())),
                );
            }
            if ui
                .add_enabled(!self.store.read_only, egui::Button::new("Import…"))
                .on_hover_text("Merge a file exported from spyglass into yours")
                .clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("spyglass data", &["json"])
                    .pick_file()
            {
                self.data_note = Some(self.store.import(&path).map(|(worlds, skipped)| {
                    let mut note = format!(
                        "Imported {worlds} world{} from {}.",
                        if worlds == 1 { "" } else { "s" },
                        path.display()
                    );
                    if !skipped.is_empty() {
                        note.push_str(&format!(" {} parts couldn't be read.", skipped.len()));
                    }
                    note
                }));
            }
        });
    }

    /// Notices about the user data, for the toolbar's row.
    pub(super) fn data_notices(&self, lines: &mut Vec<(Color32, String)>) {
        if !self.store.problems.is_empty() && !self.data_problems_seen {
            lines.push((
                style::WARN,
                format!(
                    "Some of your saved markers and worlds couldn't be read ({} problems; the \
                     file is backed up first). {}",
                    self.store.problems.len(),
                    self.store.problems.first().cloned().unwrap_or_default()
                ),
            ));
        }
        if let Some(e) = &self.store.error {
            lines.push((style::ERROR, format!("Saving your markers failed: {e}")));
        }
        match &self.data_note {
            Some(Ok(note)) => lines.push((style::MUTED, note.clone())),
            Some(Err(e)) => lines.push((style::ERROR, e.clone())),
            None => {}
        }
    }
}

/// A structure in the structure set `set`, for its family's name.
fn set_member(world: &World, set: &str) -> String {
    world
        .structures()
        .find(|id| world.structure_set(id) == Some(set))
        .unwrap_or(set)
        .to_string()
}
