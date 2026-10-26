//! The viewer window: the side panel (source and view), the map, and the status bar.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

use eframe::egui::{self, Color32, Key, Pos2, Rect, Sense, TextureOptions, Vec2};
use serde::{Deserialize, Serialize};
use spyglass::{
    BiomeQuery, BundleInfo, Image, LauncherSave, SaveReport, Scan, Session, SpawnOrigin,
    StructureQuery, StructureStart, World,
};

use crate::locate::{self, Found, Target};
use crate::markers::{self, Picked};
use crate::tiles::{
    Cache, Entry, Job, JobKey, MarkerKey, Output, Pool, Style, TileKey, slime_features,
};
use crate::view::{TILE, Tile, View};

/// The key settings are saved under.
const SETTINGS: &str = "spyglass";
/// Tiles kept: about 64 MB of textures and 16 MB of biome maps at 256 × 256.
const CACHE_TILES: usize = 256;
/// Textures made per frame, so restyling a full screen doesn't stall one frame.
const TEXTURES_PER_FRAME: usize = 12;
/// Points of scrolling per zoom step.
const SCROLL_PER_STEP: f32 = 50.0;
/// Points the arrow keys pan by.
const KEY_PAN: f32 = 64.0;
/// What the map shows where there's no tile yet.
const EMPTY: Color32 = Color32::from_rgb(0x20, 0x20, 0x20);
/// Tiles' structure markers kept.
const CACHE_MARKERS: usize = 512;
/// The most structure markers drawn at once; more is a zoom level too far out to read.
const MAX_MARKERS: usize = 2_000;

/// What's remembered between runs.
#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    bundles: PathBuf,
    /// The game version last shown.
    version: Option<String>,
    seed: String,
    view: View,
    ties: bool,
    slime_chunks: bool,
    spawn: bool,
    /// Structure ids to mark.
    structures: Vec<String>,
    /// Biome ids to keep bright.
    highlight: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            bundles: PathBuf::from("bundles"),
            version: None,
            seed: "0".to_string(),
            view: View::default(),
            ties: false,
            slime_chunks: false,
            spawn: true,
            structures: Vec::new(),
            highlight: Vec::new(),
        }
    }
}

/// A result coming from another thread.
struct Pending<T>(Receiver<T>);

impl<T: Send + 'static> Pending<T> {
    fn spawn(ctx: &egui::Context, work: impl FnOnce() -> T + Send + 'static) -> Self {
        let (send, receive) = channel();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let _ = send.send(work());
            ctx.request_repaint();
        });
        Pending(receive)
    }

    /// The result if it's ready; `Err(())` if the thread died without one.
    fn poll(&self) -> Option<Result<T, ()>> {
        match self.0.try_recv() {
            Ok(value) => Some(Ok(value)),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err(())),
        }
    }
}

/// Worlds by bundle directory, loaded once and kept for the whole run, so sessions and tile
/// jobs can borrow them from any thread. A run opens a handful at most.
type Worlds = Arc<Mutex<HashMap<PathBuf, &'static World>>>;

fn open_world(worlds: &Worlds, dir: &Path) -> Result<&'static World, String> {
    if let Some(world) = worlds
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(dir)
    {
        return Ok(world);
    }
    let world: &'static World = Box::leak(Box::new(World::open(dir).map_err(|e| e.to_string())?));
    worlds
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(dir.to_path_buf(), world);
    Ok(world)
}

/// The world and seed on the map.
struct Shown {
    /// Numbers tile keys; new for every session.
    id: u64,
    session: Arc<Session<'static>>,
    spawn: SpawnOrigin,
}

/// The structures chosen, resolved against the world shown.
#[derive(Default)]
struct Selection {
    /// Numbers marker keys; new for every change.
    id: u64,
    queries: Arc<[StructureQuery]>,
}

pub struct App {
    settings: Settings,
    bundles: Result<Vec<BundleInfo>, String>,
    worlds: Worlds,
    world: Option<&'static World>,
    loading: Option<(PathBuf, Pending<Result<&'static World, String>>)>,
    load_error: Option<String>,
    shown: Option<Shown>,
    seed_error: Option<String>,
    next_id: u64,
    pool: Pool,
    cache: Cache<TileKey, Entry>,
    markers: Cache<MarkerKey, Result<Vec<StructureStart>, String>>,
    selection: Selection,
    structure_filter: String,
    biome_filter: String,
    /// Whether the last frame had more markers than it could show.
    too_many_markers: bool,
    /// The block the map's context menu was opened on.
    menu_block: Option<[i32; 2]>,
    locating: Option<Pending<Result<Found, String>>>,
    found: Option<Result<Found, String>>,
    /// Saves in the launchers' folders, listed when first wanted.
    saves: Option<Vec<LauncherSave>>,
    inspecting: Option<Pending<Result<SaveReport, String>>>,
    /// The save on the map, if it came from one.
    save: Option<SaveReport>,
    save_error: Option<String>,
    save_filter: String,
    goto: String,
    goto_error: bool,
    /// Scrolling not yet turned into a zoom step.
    scrolled: f32,
    /// The block under the pointer, for the status bar.
    cursor: Option<[i32; 2]>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let settings: Settings = cc
            .storage
            .and_then(|s| eframe::get_value(s, SETTINGS))
            .unwrap_or_default();
        let ctx = cc.egui_ctx.clone();
        let mut app = App {
            settings: Settings {
                view: settings.view.sanitized(),
                ..settings
            },
            bundles: Ok(Vec::new()),
            worlds: Worlds::default(),
            world: None,
            loading: None,
            load_error: None,
            shown: None,
            seed_error: None,
            next_id: 0,
            pool: Pool::new(move || ctx.request_repaint()),
            cache: Cache::new(CACHE_TILES),
            markers: Cache::new(CACHE_MARKERS),
            selection: Selection::default(),
            structure_filter: String::new(),
            biome_filter: String::new(),
            too_many_markers: false,
            menu_block: None,
            locating: None,
            found: None,
            saves: None,
            inspecting: None,
            save: None,
            save_error: None,
            save_filter: String::new(),
            goto: String::new(),
            goto_error: false,
            scrolled: 0.0,
            cursor: None,
        };
        app.scan(&cc.egui_ctx);
        app
    }

    /// Looks for bundles, and opens the remembered version or the newest.
    fn scan(&mut self, ctx: &egui::Context) {
        self.bundles = spyglass::bundles_in(&self.settings.bundles).map_err(|e| e.to_string());
        let pick = self.bundles.as_ref().ok().and_then(|found| {
            found
                .iter()
                .find(|b| Some(&b.version) == self.settings.version.as_ref())
                .or(found.first())
                .cloned()
        });
        match pick {
            Some(bundle) => self.open(ctx, &bundle),
            None => {
                self.world = None;
                self.shown = None;
            }
        }
    }

    /// Starts loading a bundle's world in the background.
    fn open(&mut self, ctx: &egui::Context, bundle: &BundleInfo) {
        self.settings.version = Some(bundle.version.clone());
        if self
            .loading
            .as_ref()
            .is_some_and(|(dir, _)| *dir == bundle.dir)
        {
            return;
        }
        let (worlds, dir) = (self.worlds.clone(), bundle.dir.clone());
        self.load_error = None;
        self.loading = Some((
            bundle.dir.clone(),
            Pending::spawn(ctx, move || open_world(&worlds, &dir)),
        ));
    }

    /// Picks up a finished world load.
    fn poll_loading(&mut self) {
        let Some((dir, pending)) = &self.loading else {
            return;
        };
        let Some(result) = pending.poll() else {
            return;
        };
        let dir = dir.clone();
        self.loading = None;
        match result {
            Ok(Ok(world)) => {
                self.world = Some(world);
                self.apply_seed();
            }
            Ok(Err(e)) => self.load_error = Some(e),
            Err(()) => self.load_error = Some(format!("loading {} failed", dir.display())),
        }
    }

    /// Picks up finished locates and save inspections.
    fn poll_tasks(&mut self, ctx: &egui::Context) {
        if let Some(result) = self.locating.as_ref().and_then(Pending::poll) {
            self.locating = None;
            self.found = Some(result.unwrap_or_else(|()| Err("the search failed".to_string())));
        }
        if let Some(result) = self.inspecting.as_ref().and_then(Pending::poll) {
            self.inspecting = None;
            match result.unwrap_or_else(|()| Err("reading the save failed".to_string())) {
                Ok(report) => self.show_save(ctx, report),
                Err(e) => self.save_error = Some(e),
            }
        }
    }

    /// Reads a save in the background, then shows it.
    fn open_save(&mut self, ctx: &egui::Context, dir: PathBuf) {
        let bundles = self.settings.bundles.clone();
        self.save_error = None;
        self.inspecting = Some(Pending::spawn(ctx, move || {
            spyglass::inspect_save(&dir, &bundles, Scan::Chunks).map_err(|e| e.to_string())
        }));
    }

    /// Shows an inspected save: its world and seed, centred on its player or else its spawn.
    fn show_save(&mut self, ctx: &egui::Context, report: SaveReport) {
        let bundle = match (&report.bundle, report.answerable()) {
            (Some(bundle), true) => bundle.clone(),
            _ => {
                let reasons: Vec<String> = report
                    .warnings
                    .iter()
                    .filter(|w| w.blocks_answers())
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
        let save = &report.save;
        let overworld = |l: &&spyglass::SaveLocation| l.dimension == "minecraft:overworld";
        if let Some(place) = save
            .player
            .as_ref()
            .filter(overworld)
            .or(save.spawn.as_ref().filter(overworld))
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

    /// Starts a locate from `from`.
    fn locate(&mut self, ctx: &egui::Context, target: Target, from: [i32; 3]) {
        let Some(shown) = &self.shown else {
            return;
        };
        let session = shown.session.clone();
        self.found = None;
        self.locating = Some(Pending::spawn(ctx, move || {
            locate::run(&session, target, from)
        }));
    }

    /// Whether structures generate in what's shown.
    fn structures_generate(&self) -> bool {
        self.save
            .as_ref()
            .is_none_or(|r| r.save.generate_structures)
    }

    /// Shows the seed in the text box, if it parses.
    fn apply_seed(&mut self) {
        let Some(world) = self.world else {
            return;
        };
        let session =
            spyglass::parse_seed(&self.settings.seed).and_then(|seed| world.session(seed));
        match session {
            Ok(session) => {
                self.seed_error = None;
                self.next_id += 1;
                self.shown = Some(Shown {
                    id: self.next_id,
                    spawn: session.spawn_origin(),
                    session: Arc::new(session),
                });
                self.cache.clear();
                self.markers.clear();
                self.select();
            }
            Err(e) => self.seed_error = Some(e.to_string()),
        }
    }

    /// Resolves the structures chosen against the world shown. Ids the world doesn't have
    /// (from another version) are kept in the settings but not marked.
    fn select(&mut self) {
        let Some(world) = self.world else {
            return;
        };
        let queries: Vec<StructureQuery> = self
            .settings
            .structures
            .iter()
            .filter_map(|s| world.structure_query(s).ok())
            .collect();
        self.next_id += 1;
        self.selection = Selection {
            id: self.next_id,
            queries: queries.into(),
        };
    }

    fn style(&self) -> Style {
        Style {
            ties: self.settings.ties,
            highlight: self.settings.highlight.clone(),
            slime_chunks: self.settings.slime_chunks
                && self.settings.view.scale <= spyglass::MAX_SLIME_SCALE,
        }
    }

    fn side_panel(&mut self, ui: &mut egui::Ui) {
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
        if self.locating.is_some() || self.found.is_some() {
            egui::CollapsingHeader::new("Locate")
                .default_open(true)
                .show(ui, |ui| self.locate_section(ui));
        }
    }

    /// The last locate's answer: click a hit to go there.
    fn locate_section(&mut self, ui: &mut egui::Ui) {
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

    fn world_section(&mut self, ui: &mut egui::Ui) {
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
    fn save_section(&mut self, ui: &mut egui::Ui) {
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

    fn choose_bundles(&mut self, ctx: &egui::Context) {
        if let Some(dir) = rfd::FileDialog::new()
            .set_directory(&self.settings.bundles)
            .pick_folder()
        {
            self.settings.bundles = dir;
            self.scan(ctx);
        }
    }

    fn view_section(&mut self, ui: &mut egui::Ui) {
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
                .on_hover_text("Biomes are drawn at this y: a slice, not the surface.");
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

    fn layers_section(&mut self, ui: &mut egui::Ui) {
        ui.checkbox(&mut self.settings.ties, "Tied biome cells")
            .on_hover_text("Where the game's biome depends on lookup history, show a checker of the biomes it may give.");
        let slime_visible = self.settings.view.scale <= spyglass::MAX_SLIME_SCALE;
        ui.add_enabled(
            slime_visible,
            egui::Checkbox::new(&mut self.settings.slime_chunks, "Slime chunks"),
        )
        .on_disabled_hover_text("Zoom in to 16 blocks per pixel or closer to see slime chunks.");
        ui.checkbox(&mut self.settings.spawn, "World spawn")
            .on_hover_text("Where the game's spawn search starts.");
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

    /// The bundles folder, made absolute so it's clear where it is.
    fn bundles_dir(&self) -> PathBuf {
        std::path::absolute(&self.settings.bundles)
            .unwrap_or_else(|_| self.settings.bundles.clone())
    }

    /// The screen shown until there's a bundle to read.
    fn no_bundle(&mut self, ui: &mut egui::Ui) {
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

    fn no_bundle_text(&mut self, ui: &mut egui::Ui) {
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

    fn map(&mut self, ui: &mut egui::Ui) {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        // On a whole physical pixel, so tile texels and marker edges land on screen pixels.
        let ppp = ui.ctx().pixels_per_point();
        let centre = Pos2::new(
            (rect.center().x * ppp).round() / ppp,
            (rect.center().y * ppp).round() / ppp,
        );
        let offset = |p: Pos2| [p.x - centre.x, p.y - centre.y];
        let view = &mut self.settings.view;
        if response.dragged() {
            let d = response.drag_delta();
            view.pan([d.x, d.y]);
        }
        if let Some(hover) = response.hover_pos() {
            let (scroll, zoom) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            self.scrolled += scroll;
            if zoom > 1.0 {
                self.scrolled = SCROLL_PER_STEP;
            } else if zoom < 1.0 {
                self.scrolled = -SCROLL_PER_STEP;
            }
            while self.scrolled.abs() >= SCROLL_PER_STEP {
                view.zoom(self.scrolled > 0.0, offset(hover));
                self.scrolled -= SCROLL_PER_STEP.copysign(self.scrolled);
            }
        }
        if ui.memory(|m| m.focused().is_none()) {
            ui.input(|i| {
                for (key, delta) in [
                    (Key::ArrowLeft, [-KEY_PAN, 0.0]),
                    (Key::ArrowRight, [KEY_PAN, 0.0]),
                    (Key::ArrowUp, [0.0, -KEY_PAN]),
                    (Key::ArrowDown, [0.0, KEY_PAN]),
                ] {
                    if i.key_pressed(key) {
                        view.pan(delta.map(|d| -d));
                    }
                }
                if i.key_pressed(Key::Plus) || i.key_pressed(Key::Equals) {
                    view.zoom(true, [0.0, 0.0]);
                }
                if i.key_pressed(Key::Minus) {
                    view.zoom(false, [0.0, 0.0]);
                }
            });
        }

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, EMPTY);
        let (Some(shown), Some(world)) = (&self.shown, self.world) else {
            return;
        };
        let (source, session, spawn) = (shown.id, shown.session.clone(), shown.spawn);
        let view = self.settings.view;
        let tiles = view.tiles([rect.width(), rect.height()]);
        let key = |tile: Tile| TileKey {
            source,
            y: view.y,
            tile,
        };
        let selection = self.selection.id;
        let marker_key = |tile: Tile| MarkerKey {
            source,
            selection,
            tile,
        };
        for (done, result) in self.pool.finished() {
            match (done, result) {
                (JobKey::Biomes(k), result) if k.source == source => {
                    let data = result.and_then(|out| match out {
                        Output::Biomes(data) => Ok(data),
                        Output::Markers(_) => Err("not a biome tile".to_string()),
                    });
                    self.cache.insert(k, Entry::new(data));
                }
                (JobKey::Markers(k), result) if k.source == source && k.selection == selection => {
                    let starts = result.and_then(|out| match out {
                        Output::Markers(starts) => Ok(starts),
                        Output::Biomes(_) => Err("not markers".to_string()),
                    });
                    self.markers.insert(k, starts);
                }
                _ => {}
            }
        }
        self.cache.next_frame();
        self.markers.next_frame();
        let job = |key| Job {
            key,
            session: session.clone(),
            structures: self.selection.queries.clone(),
        };
        // Biomes first, then markers, each nearest the centre first.
        let mut jobs: Vec<Job> = tiles
            .iter()
            .map(|&t| key(t))
            .filter(|k| !self.cache.contains(k))
            .map(|k| job(JobKey::Biomes(k)))
            .collect();
        let marking = !self.selection.queries.is_empty() && self.structures_generate();
        if marking {
            jobs.extend(
                tiles
                    .iter()
                    .map(|&t| marker_key(t))
                    .filter(|k| !self.markers.contains(k))
                    .map(|k| job(JobKey::Markers(k))),
            );
        }
        self.pool.want(jobs);

        let style = self.style();
        let mut made = 0;
        for &tile in &tiles {
            let at = view.tile_offset(tile);
            let tile_rect =
                Rect::from_min_size(centre + Vec2::new(at[0], at[1]), Vec2::splat(TILE as f32));
            if let Some(texture) = self.texture(ui.ctx(), world, key(tile), &style, &mut made) {
                painter.image(texture, tile_rect, full_uv(), Color32::WHITE);
                continue;
            }
            // A coarser tile, scaled up, until this one is ready.
            let mut child = tile;
            let mut uv = full_uv();
            while let Some((parent, [qx, qz])) = child.parent() {
                let half = uv.size() / 2.0;
                uv = Rect::from_min_size(
                    Pos2::new(
                        qx as f32 * 0.5 + uv.min.x * 0.5,
                        qz as f32 * 0.5 + uv.min.y * 0.5,
                    ),
                    half,
                );
                if let Some(entry) = self.cache.get(&key(parent))
                    && let Some((_, texture)) = &entry.texture
                {
                    painter.image(texture.id(), tile_rect, uv, Color32::WHITE);
                    break;
                }
                if parent.scale >= view.scale * 8 {
                    break;
                }
                child = parent;
            }
        }
        if made >= TEXTURES_PER_FRAME {
            ui.ctx().request_repaint();
        }

        // Markers over every tile, so none is hidden under a neighbouring tile.
        let mut shown_markers: Vec<(Pos2, Picked<'_>)> = Vec::new();
        let mut starts: Vec<StructureStart> = Vec::new();
        if marking {
            for &tile in &tiles {
                if let Some(Ok(found)) = self.markers.get(&marker_key(tile)) {
                    starts.extend(found.iter().cloned());
                }
            }
        }
        self.too_many_markers = starts.len() > MAX_MARKERS;
        if !self.too_many_markers {
            for start in &starts {
                let at =
                    markers::pixel_centre(&view, centre, [start.position[0], start.position[2]]);
                if rect.expand(8.0).contains(at) {
                    markers::structure(&painter, at, start);
                    shown_markers.push((at, Picked::Structure(start)));
                }
            }
        }
        if self.settings.spawn {
            let at = markers::pixel_centre(&view, centre, spawn.map_block());
            markers::spawn(&painter, at);
            shown_markers.push((at, Picked::Spawn(&spawn)));
        }
        if let Some(Ok(found)) = &self.found {
            for hit in &found.hits {
                let at = markers::pixel_centre(&view, centre, [hit.x, hit.z]);
                markers::hit(&painter, at, hit.certain);
                shown_markers.push((at, Picked::Hit(hit)));
            }
        }
        if let Some(pointer) = response.hover_pos()
            && !response.dragged()
            && let Some(picked) = markers::pick(pointer, shown_markers)
        {
            response
                .clone()
                .on_hover_ui_at_pointer(|ui| describe(ui, &picked));
        }
        self.cursor = response
            .hover_pos()
            .map(|hover| view.block_at(offset(hover)).map(|b| b.floor() as i32));

        if response.secondary_clicked()
            && let Some(p) = response.interact_pointer_pos()
        {
            self.menu_block = Some(view.block_at(offset(p)).map(|b| b.floor() as i32));
        }
        let Some([bx, bz]) = self.menu_block else {
            return;
        };
        let mut action = None;
        response.context_menu(|ui| {
            ui.weak(format!("{bx}, {bz}"));
            ui.menu_button("Nearest structure", |ui| {
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .show(ui, |ui| {
                        let mut ids: Vec<&str> = world.structures().collect();
                        ids.sort_unstable();
                        for id in ids {
                            if ui
                                .button(id.strip_prefix("minecraft:").unwrap_or(id))
                                .clicked()
                            {
                                action = Some(Action::Locate(Target::Structure(id.to_string())));
                                ui.close();
                            }
                        }
                    });
            });
            ui.menu_button("Nearest biome", |ui| {
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .show(ui, |ui| {
                        let mut ids: Vec<&String> = world.biomes().iter().collect();
                        ids.sort_unstable();
                        for id in ids {
                            if ui
                                .button(id.strip_prefix("minecraft:").unwrap_or(id))
                                .clicked()
                            {
                                action = Some(Action::Locate(Target::Biome(id.clone())));
                                ui.close();
                            }
                        }
                    });
            });
            if ui.button("Centre here").clicked() {
                action = Some(Action::Centre);
                ui.close();
            }
            if ui.button("Copy coordinates").clicked() {
                ui.ctx().copy_text(format!("{bx} {bz}"));
                ui.close();
            }
        });
        match action {
            Some(Action::Locate(target)) => self.locate(ui.ctx(), target, [bx, view.y, bz]),
            Some(Action::Centre) => {
                self.settings.view.centre = [f64::from(bx) + 0.5, f64::from(bz) + 0.5];
            }
            None => {}
        }
    }
    /// A tile's texture in `style`, making it if there's budget left this frame.
    fn texture(
        &mut self,
        ctx: &egui::Context,
        world: &'static World,
        key: TileKey,
        style: &Style,
        made: &mut usize,
    ) -> Option<egui::TextureId> {
        let entry = self.cache.get(&key)?;
        let stale = entry.texture.as_ref().is_none_or(|(s, _)| s != style);
        if stale
            && *made < TEXTURES_PER_FRAME
            && let Ok(data) = &entry.data
        {
            let highlight: Vec<BiomeQuery> = style
                .highlight
                .iter()
                .filter_map(|b| world.biome_query(b).ok())
                .collect();
            if let Ok(mut image) = data.biomes.colorize(world, style.ties, &highlight) {
                if style.slime_chunks {
                    image.draw_features(data.biomes.area(), &slime_features(data));
                }
                let texture = ctx.load_texture(
                    format!("{key:?}"),
                    color_image(&image),
                    TextureOptions::NEAREST,
                );
                entry.texture = Some((style.clone(), texture));
                *made += 1;
            }
        }
        entry.texture.as_ref().map(|(_, t)| t.id())
    }

    fn status_bar(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if let (Some([x, z]), Some(shown)) = (self.cursor, &self.shown) {
                let y = self.settings.view.y;
                let at = shown.session.biome_at([x, y, z]);
                ui.monospace(format!(
                    "x {x:>9}  z {z:>9}  chunk {:>7} {:>7}",
                    x >> 4,
                    z >> 4
                ));
                ui.separator();
                if at.tied.is_empty() {
                    ui.label(&at.biome);
                } else {
                    ui.label(format!("{} (tied: {})", at.biome, at.tied.join(", ")));
                }
            } else {
                ui.weak("Drag to move, scroll to zoom.");
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(world) = self.world {
                    ui.weak(format!("Minecraft {}", world.version()));
                }
                if self.pool.busy() {
                    ui.spinner();
                }
                if self.too_many_markers {
                    ui.label("Too many structures to show here; zoom in.");
                }
            });
        });
    }
}

/// The launchers' saves, most recently played first.
fn listed_saves() -> Vec<LauncherSave> {
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

/// What the map's context menu asked for.
enum Action {
    Locate(Target),
    Centre,
}

/// A marker's tooltip.
fn describe(ui: &mut egui::Ui, picked: &Picked<'_>) {
    match picked {
        Picked::Structure(start) => {
            let [x, _, z] = start.position;
            ui.strong(start.structures.join(" or "));
            ui.label(format!(
                "{x}, {z} (chunk {}, {})",
                start.chunk[0], start.chunk[1]
            ));
            ui.weak(&start.set);
            if !start.certain {
                ui.label(
                    "Candidate: whether it generates depends on terrain or a tied biome, which \
                     spyglass doesn't compute yet.",
                );
            }
        }
        Picked::Spawn(origin) => {
            ui.strong("World spawn");
            let [cx, cz] = origin.chunk;
            ui.label(format!(
                "The game's spawn search starts in chunk {cx}, {cz}; the spawn is in the 11 × 11 \
                 chunks around it, decided by terrain."
            ));
            if let Some([x, _, z]) = origin.position {
                ui.label(format!("Best climate at {x}, {z}."));
            }
            if origin.platform_dependent {
                ui.label("This answer can differ between platforms the game runs on.");
            }
        }
        Picked::Hit(hit) => {
            ui.strong(&hit.what);
            let y = hit.y.map_or("~".to_string(), |y| y.to_string());
            ui.label(format!(
                "{} {y} {}, {} blocks away",
                hit.x, hit.z, hit.blocks_away
            ));
            if !hit.certain {
                ui.label("Candidate: the game's answer is the first of these it confirms.");
            }
        }
    }
}

fn full_uv() -> Rect {
    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0))
}

fn color_image(image: &Image) -> egui::ColorImage {
    let [w, h] = image.size();
    egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], image.rgba())
}

/// A filtered checklist of ids, with the chosen ones in `chosen`. Returns whether it changed.
fn pick_list(
    ui: &mut egui::Ui,
    salt: &str,
    ids: &[&str],
    chosen: &mut Vec<String>,
    filter: &mut String,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(filter)
                .hint_text("filter")
                .desired_width(140.0),
        );
        if ui
            .add_enabled(!chosen.is_empty(), egui::Button::new("Clear"))
            .clicked()
        {
            chosen.clear();
            changed = true;
        }
    });
    let needle = filter.to_lowercase();
    egui::ScrollArea::vertical()
        .id_salt(salt)
        .max_height(220.0)
        .show(ui, |ui| {
            for id in ids.iter().filter(|id| id.to_lowercase().contains(&needle)) {
                let mut on = chosen.iter().any(|c| c == id);
                let name = id.strip_prefix("minecraft:").unwrap_or(id);
                if ui.checkbox(&mut on, name).changed() {
                    if on {
                        chosen.push((*id).to_string());
                        chosen.sort();
                    } else {
                        chosen.retain(|c| c != id);
                    }
                    changed = true;
                }
            }
        });
    changed
}

/// `x z` or `x y z`, in blocks, separated by spaces or commas.
fn parse_goto(text: &str) -> Option<(i32, Option<i32>, i32)> {
    let parts: Vec<i32> = text
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|p| !p.is_empty())
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    match parts[..] {
        [x, z] => Some((x, None, z)),
        [x, y, z] => Some((x, Some(y), z)),
        _ => None,
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_loading();
        self.poll_tasks(ui.ctx());
        egui::Panel::left("side")
            .resizable(true)
            .default_size(260.0)
            .size_range(200.0..=420.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.side_panel(ui));
            });
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| match &self.bundles {
                Ok(found) if !found.is_empty() => self.map(ui),
                _ => self.no_bundle(ui),
            });
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, SETTINGS, &self.settings);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn go_to_takes_two_or_three_numbers() {
        assert_eq!(parse_goto("100 -200"), Some((100, None, -200)));
        assert_eq!(parse_goto(" 1, 70, -3 "), Some((1, Some(70), -3)));
        assert_eq!(parse_goto("1"), None);
        assert_eq!(parse_goto("a b"), None);
        assert_eq!(parse_goto("1 2 3 4"), None);
    }
}
