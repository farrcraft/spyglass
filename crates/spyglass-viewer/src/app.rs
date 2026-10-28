//! The viewer window: the side panel (source and view), the map, and the status bar.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

use eframe::egui::{self, Color32, Key, Pos2, Rect, Sense, TextureOptions, Vec2};
use serde::{Deserialize, Serialize};
use spyglass::{
    BiomeQuery, BundleInfo, Dimension, Evaluation, Game, Heightmap, Image, LauncherSave, Layers,
    MAX_TERRAIN_SCALE, SaveReport, Scan, SearchOutcome, SearchQuery, Session, SpawnOrigin,
    StructureLayout, StructureQuery, StructureStart, World,
};

use crate::export::{Export, Source, visible_area};
use crate::locate::{self, Found, Target};
use crate::markers::{self, Picked};
use crate::search::{self, Match, Running, SearchSettings};
use crate::tiles::{
    Cache, Entry, Job, JobKey, LayoutKey, MarkerKey, Output, Pool, Style, TileKey, slime_features,
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
/// The largest scale jigsaw structures' bounds are drawn at: a village is then 30 pixels or
/// more across, and few enough are in view to lay them all out.
const MAX_BOUNDS_SCALE: u32 = 4;
/// Structure layouts kept.
const CACHE_LAYOUTS: usize = 512;

/// What's remembered between runs.
#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    bundles: PathBuf,
    /// The game version last shown.
    version: Option<String>,
    seed: String,
    /// The dimension shown, by id.
    dimension: String,
    view: View,
    /// The other dimensions' views, by id, to go back to.
    views: BTreeMap<String, View>,
    ties: bool,
    slime_chunks: bool,
    spawn: bool,
    /// Decide structure starts and locates with the seed's terrain.
    confirm: bool,
    /// Shade by terrain height, up close.
    relief: bool,
    /// Biomes at the surface, up close.
    surface: bool,
    /// Structure ids to mark.
    structures: Vec<String>,
    /// Biome ids to keep bright.
    highlight: Vec<String>,
    search: SearchSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            bundles: PathBuf::from("bundles"),
            version: None,
            seed: "0".to_string(),
            dimension: Dimension::Overworld.id().to_string(),
            view: View::default(),
            views: BTreeMap::new(),
            ties: false,
            slime_chunks: false,
            spawn: true,
            confirm: true,
            relief: false,
            surface: false,
            structures: Vec::new(),
            highlight: Vec::new(),
            search: SearchSettings::default(),
        }
    }
}

impl Settings {
    /// The dimension shown.
    fn dimension(&self) -> Dimension {
        Dimension::from_id(&self.dimension).unwrap_or(Dimension::Overworld)
    }
}

/// A dimension's name in the window.
fn dimension_label(dimension: Dimension) -> &'static str {
    match dimension {
        Dimension::Overworld => "Overworld",
        Dimension::Nether => "Nether",
        Dimension::End => "End",
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

/// Games by bundle directory, kept for the whole run with every dimension they've loaded, so
/// sessions and tile jobs can borrow their worlds from any thread. A run opens a handful at
/// most.
type Games = Arc<Mutex<HashMap<PathBuf, &'static Game>>>;

/// `dimension` of the bundle in `dir`, loading it if no one has yet.
fn open_world(
    games: &Games,
    dir: &Path,
    dimension: Dimension,
) -> Result<(&'static Game, &'static World), String> {
    let known = games
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(dir)
        .copied();
    let game = match known {
        Some(game) => game,
        None => {
            let game: &'static Game =
                Box::leak(Box::new(Game::open(dir).map_err(|e| e.to_string())?));
            *games
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .entry(dir.to_path_buf())
                .or_insert(game)
        }
    };
    let world = game.world(dimension).map_err(|e| e.to_string())?;
    Ok((game, world))
}

/// A world being loaded: its bundle and dimension.
type Loading = (
    (PathBuf, Dimension),
    Pending<Result<(&'static Game, &'static World), String>>,
);

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
    games: Games,
    /// The bundle shown, every dimension of it.
    game: Option<&'static Game>,
    /// The dimension shown.
    world: Option<&'static World>,
    loading: Option<Loading>,
    load_error: Option<String>,
    shown: Option<Shown>,
    seed_error: Option<String>,
    next_id: u64,
    pool: Pool,
    cache: Cache<TileKey, Entry>,
    markers: Cache<MarkerKey, Result<Vec<StructureStart>, String>>,
    layouts: Cache<LayoutKey, Result<Option<StructureLayout>, String>>,
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
    /// The map's size in pixels, for exports.
    map_size: [u32; 2],
    exporting: Option<Pending<Result<PathBuf, String>>>,
    /// The last export's outcome, or a copied command.
    export_note: Option<Result<String, String>>,
    goto: String,
    goto_error: bool,
    /// Scrolling not yet turned into a zoom step.
    scrolled: f32,
    /// The block under the pointer, for the status bar.
    cursor: Option<[i32; 2]>,
    /// The search query's text and what it parsed to, against the world shown.
    search_parsed: Option<(String, Result<SearchQuery, String>)>,
    /// The search running or last run.
    search_run: Option<SearchRun>,
    /// A live hit being evaluated before it opens.
    opening: Option<Pending<Result<(i64, Evaluation), String>>>,
    /// The seed of the hit last opened, and what its constraints matched.
    opened: Option<(i64, Vec<Match>)>,
    search_error: Option<String>,
}

/// A search and its query.
struct SearchRun {
    query: SearchQuery,
    /// Whether the search decided starts with terrain, so opening a hit does too.
    confirm: bool,
    running: Option<Running>,
    outcome: Option<Result<SearchOutcome, String>>,
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
            games: Games::default(),
            game: None,
            world: None,
            loading: None,
            load_error: None,
            shown: None,
            seed_error: None,
            next_id: 0,
            pool: Pool::new(move || ctx.request_repaint()),
            cache: Cache::new(CACHE_TILES),
            markers: Cache::new(CACHE_MARKERS),
            layouts: Cache::new(CACHE_LAYOUTS),
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
            map_size: [0, 0],
            exporting: None,
            export_note: None,
            goto: String::new(),
            goto_error: false,
            scrolled: 0.0,
            cursor: None,
            search_parsed: None,
            search_run: None,
            opening: None,
            opened: None,
            search_error: None,
        };
        app.scan(&cc.egui_ctx);
        app
    }

    /// Looks for bundles, and opens the remembered version, else the newest release (ADR 0028),
    /// else the newest snapshot: the version picker shows which.
    fn scan(&mut self, ctx: &egui::Context) {
        self.bundles = spyglass::bundles_in(&self.settings.bundles).map_err(|e| e.to_string());
        let pick = self.bundles.as_ref().ok().and_then(|found| {
            found
                .iter()
                .find(|b| Some(&b.version) == self.settings.version.as_ref())
                .or_else(|| found.iter().find(|b| b.kind == spyglass::Kind::Release))
                .or(found.first())
                .cloned()
        });
        match pick {
            Some(bundle) => self.open(ctx, &bundle),
            None => {
                self.game = None;
                self.world = None;
                self.shown = None;
            }
        }
    }

    /// Starts loading the dimension shown of a bundle in the background.
    fn open(&mut self, ctx: &egui::Context, bundle: &BundleInfo) {
        self.settings.version = Some(bundle.version.clone());
        let key = (bundle.dir.clone(), self.settings.dimension());
        if self.loading.as_ref().is_some_and(|(k, _)| *k == key) {
            return;
        }
        let games = self.games.clone();
        let (dir, dimension) = key.clone();
        self.load_error = None;
        self.loading = Some((
            key,
            Pending::spawn(ctx, move || open_world(&games, &dir, dimension)),
        ));
    }

    /// The bundle of the version shown.
    fn current_bundle(&self) -> Option<BundleInfo> {
        let version = self.settings.version.as_ref()?;
        self.bundles
            .as_ref()
            .ok()?
            .iter()
            .find(|b| b.version == *version)
            .cloned()
    }

    /// Shows `dimension` from now on, with its own view: where it was last left, or the default.
    fn set_dimension(&mut self, dimension: Dimension) {
        let from = self.settings.dimension();
        if dimension == from {
            return;
        }
        let view = self
            .settings
            .views
            .remove(dimension.id())
            .unwrap_or_default();
        let left = std::mem::replace(&mut self.settings.view, view);
        self.settings.views.insert(from.id().to_string(), left);
        self.settings.dimension = dimension.id().to_string();
        self.found = None;
        self.menu_block = None;
    }

    /// Switches the map to `dimension`, loading it in the background.
    fn switch_dimension(&mut self, ctx: &egui::Context, dimension: Dimension) {
        if let Some(report) = &self.save
            && !report.answerable_in(dimension)
        {
            let reasons: Vec<String> = report
                .warnings
                .iter()
                .filter(|w| w.blocks(dimension))
                .map(ToString::to_string)
                .collect();
            self.save_error = Some(format!(
                "spyglass can't show {} of {}: {}",
                dimension.name(),
                report.save.name,
                reasons.join("; ")
            ));
            return;
        }
        self.set_dimension(dimension);
        self.world = None;
        self.shown = None;
        if let Some(bundle) = self.current_bundle() {
            self.open(ctx, &bundle);
        }
    }

    /// Picks up a finished world load.
    fn poll_loading(&mut self) {
        let Some(((dir, _), pending)) = &self.loading else {
            return;
        };
        let Some(result) = pending.poll() else {
            return;
        };
        let dir = dir.clone();
        self.loading = None;
        match result {
            Ok(Ok((game, world))) => {
                if self.game.is_none_or(|g| !std::ptr::eq(g, game)) {
                    self.search_parsed = None;
                }
                self.game = Some(game);
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
        if let Some(result) = self.exporting.as_ref().and_then(Pending::poll) {
            self.exporting = None;
            self.export_note = Some(
                result
                    .unwrap_or_else(|()| Err("exporting failed".to_string()))
                    .map(|path| format!("Saved {}", path.display())),
            );
        }
        if let Some(run) = &mut self.search_run
            && let Some(outcome) = run.running.as_ref().and_then(Running::poll)
        {
            run.running = None;
            run.outcome = Some(outcome);
        }
        if let Some(result) = self.opening.as_ref().and_then(Pending::poll) {
            self.opening = None;
            match result.unwrap_or_else(|()| Err("evaluating the seed failed".to_string())) {
                Ok((seed, evaluation)) => self.open_hit(seed, &evaluation),
                Err(e) => self.search_error = Some(e),
            }
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

    /// Shows an inspected save: its world and seed, in its player's dimension if spyglass
    /// answers there (otherwise the Overworld), centred on its player or else its spawn.
    fn show_save(&mut self, ctx: &egui::Context, report: SaveReport) {
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

    /// Starts a locate from `from`.
    fn locate(&mut self, ctx: &egui::Context, target: Target, from: [i32; 3]) {
        let Some(shown) = &self.shown else {
            return;
        };
        let session = shown.session.clone();
        let confirm = self.settings.confirm;
        self.found = None;
        self.locating = Some(Pending::spawn(ctx, move || {
            locate::run(&session, target, from, confirm)
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
                self.layouts.clear();
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

    /// Whether the dimension shown has `layers` (`Layers::check`).
    fn allows(&self, layers: Layers) -> bool {
        self.world.is_none_or(|w| layers.check(w).is_ok())
    }

    /// Whether slime chunks are drawn in the dimension shown.
    fn slime_allowed(&self) -> bool {
        self.allows(Layers {
            slime_chunks: true,
            ..Layers::default()
        })
    }

    /// Whether the spawn is drawn in the dimension shown.
    fn spawn_allowed(&self) -> bool {
        self.allows(Layers {
            spawn: true,
            ..Layers::default()
        })
    }

    /// Whether terrain layers are drawn in the dimension shown: not under a ceiling.
    fn terrain_allowed(&self) -> bool {
        self.allows(Layers {
            relief: true,
            ..Layers::default()
        })
    }

    fn style(&self) -> Style {
        Style {
            ties: self.settings.ties,
            highlight: self.settings.highlight.clone(),
            slime_chunks: self.settings.slime_chunks
                && self.slime_allowed()
                && self.settings.view.scale <= spyglass::MAX_SLIME_SCALE,
            relief: self.settings.relief
                && self.terrain_allowed()
                && self.settings.view.scale <= MAX_TERRAIN_SCALE,
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
    fn export(&self) -> Option<Export> {
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
    fn export_section(&mut self, ui: &mut egui::Ui) {
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
    fn start_export(&mut self, ctx: &egui::Context, export: Export, path: PathBuf) {
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

    /// The search query, its options, and the best seeds so far: click one to open it.
    fn search_section(&mut self, ui: &mut egui::Ui) {
        let Some(game) = self.game else {
            ui.weak("Open a bundle to search.");
            return;
        };
        let s = &mut self.settings.search;
        ui.add(
            egui::TextEdit::multiline(&mut s.query)
                .code_editor()
                .desired_rows(4)
                .desired_width(f32::INFINITY)
                .hint_text("require 2 minecraft:swamp_hut within 600 of spawn"),
        );
        if self
            .search_parsed
            .as_ref()
            .is_none_or(|(text, _)| *text != s.query)
        {
            let parsed = game.search_query(&s.query).map_err(|e| e.to_string());
            self.search_parsed = Some((s.query.clone(), parsed));
        }
        let parsed = self.search_parsed.as_ref().map(|(_, p)| p);
        if let Some(Err(e)) = parsed {
            ui.colored_label(ui.visuals().error_fg_color, e);
        }
        ui.horizontal(|ui| {
            ui.label("Keep the best");
            ui.add(egui::DragValue::new(&mut s.top).range(1..=1000));
        });
        ui.horizontal(|ui| {
            ui.label("Stop after");
            ui.add(
                egui::TextEdit::singleline(&mut s.seeds)
                    .hint_text("any number of")
                    .desired_width(80.0),
            );
            ui.label("seeds or");
            ui.add(egui::DragValue::new(&mut s.seconds).suffix(" s"))
                .on_hover_text("0 for no time limit.");
        });
        ui.checkbox(&mut s.range, "Only seeds in a range")
            .on_hover_text("Otherwise every 64-bit seed, with structure placement checked first.");
        if s.range {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut s.first).desired_width(100.0));
                ui.label("to");
                ui.add(egui::TextEdit::singleline(&mut s.last).desired_width(100.0));
            });
        }
        ui.horizontal(|ui| {
            ui.label("Start at index");
            ui.add(egui::DragValue::new(&mut s.from));
        });
        ui.checkbox(&mut s.confirm, "Confirm with terrain").on_hover_text(
            "Decide structure starts with each seed's terrain, as the game does. Slower, but only \
             ties and platform-dependent results stay uncertain.",
        );
        ui.checkbox(&mut s.certain, "Certain only").on_hover_text(
            "Only seeds whose required constraints hold whatever is left undecided, ranked by the \
             worst case. Without confirming, most structures are undecided.",
        );

        let options = s.options(search::threads());
        let running = self
            .search_run
            .as_ref()
            .is_some_and(|r| r.running.is_some());
        let mut start = None;
        if running {
            let current = self.search_run.as_ref().and_then(|r| r.running.as_ref());
            // Once stopped, the best seeds are still evaluated; a second click skips the rest.
            let label = if current.is_some_and(|r| r.stopping()) {
                "Skip evaluating"
            } else {
                "Stop"
            };
            if ui.button(label).clicked()
                && let Some(running) = current
            {
                running.stop();
            }
        } else {
            let ready = matches!(parsed, Some(Ok(_))) && options.is_ok();
            if ui.add_enabled(ready, egui::Button::new("Search")).clicked()
                && let (Some(Ok(query)), Ok(options)) = (parsed, &options)
            {
                start = Some((query.clone(), options.clone()));
            }
            if let Err(e) = &options {
                ui.weak(e);
            }
        }

        let mut open: Option<(i64, Option<Evaluation>)> = None;
        let mut continue_from = None;
        if let Some(run) = &self.search_run {
            let live = run.running.as_ref().and_then(Running::progress);
            let finished = run.outcome.as_ref().and_then(|o| o.as_ref().ok());
            if let Some(Err(e)) = &run.outcome {
                ui.colored_label(ui.visuals().error_fg_color, e);
            }
            let progress = finished.map(|o| &o.progress).or(live.as_ref());
            if let Some(p) = progress {
                ui.horizontal(|ui| {
                    if running {
                        ui.spinner();
                    }
                    let rate = p.evaluated as f64 / p.elapsed.as_secs_f64().max(0.001);
                    ui.label(format!(
                        "{} seeds, {rate:.0} a second, {} reported",
                        p.evaluated, p.reported
                    ));
                });
            } else if running {
                ui.spinner();
            }
            let opened = self.opened.as_ref().map(|(seed, _)| *seed);
            match finished {
                Some(outcome) => {
                    for hit in &outcome.hits {
                        let label = format!("{}  score {}", hit.seed, hit.score);
                        if ui
                            .selectable_label(opened == Some(hit.seed), label)
                            .clicked()
                        {
                            open = Some((hit.seed, Some(hit.evaluation.clone())));
                        }
                    }
                    if let Some(next) = outcome.progress.next
                        && ui
                            .small_button(format!("Continue from index {next}"))
                            .clicked()
                    {
                        continue_from = Some((run.query.clone(), next));
                    }
                }
                None => {
                    for hit in live.iter().flat_map(|p| &p.top) {
                        let label = format!("{}  score {}", hit.seed, hit.score);
                        if ui
                            .selectable_label(opened == Some(hit.seed), label)
                            .clicked()
                        {
                            open = Some((hit.seed, None));
                        }
                    }
                }
            }
        }
        if let Some(e) = &self.search_error {
            ui.colored_label(ui.visuals().error_fg_color, e);
        }

        if let Some((query, next)) = continue_from {
            self.settings.search.from = next;
            if let Ok(options) = self.settings.search.options(search::threads()) {
                start = Some((query, options));
            }
        }
        if let Some((query, options)) = start {
            let ctx = ui.ctx().clone();
            self.search_error = None;
            self.search_run = Some(SearchRun {
                confirm: options.confirm,
                running: Some(Running::start(game, query.clone(), options, move || {
                    ctx.request_repaint();
                })),
                query,
                outcome: None,
            });
        }
        if let Some((seed, evaluation)) = open {
            self.search_error = None;
            match evaluation {
                Some(evaluation) => self.open_hit(seed, &evaluation),
                None => {
                    let run = self
                        .search_run
                        .as_ref()
                        .map(|r| (r.query.clone(), r.confirm));
                    if let Some((query, confirm)) = run {
                        self.opening = Some(Pending::spawn(ui.ctx(), move || {
                            game.session(seed)
                                .evaluate(&query, confirm)
                                .map(|e| (seed, e))
                                .map_err(|e| e.to_string())
                        }));
                    }
                }
            }
        }
    }

    /// Shows a search hit: its seed, in its first constraint's dimension, centred on that
    /// constraint's place, with the structures it matched marked and everything its constraints
    /// matched there ringed.
    fn open_hit(&mut self, seed: i64, evaluation: &Evaluation) {
        let (Some(run), Some(game)) = (&self.search_run, self.game) else {
            return;
        };
        let matches = search::matches(&run.query, evaluation);
        let query = run.query.clone();
        let dimension = query.dimension(0);
        // The query was resolved against this game, so its dimensions are loaded.
        let Ok(world) = game.world(dimension) else {
            return;
        };
        self.save = None;
        self.set_dimension(dimension);
        self.world = Some(world);
        self.settings.seed = seed.to_string();
        self.apply_seed();
        if self.shown.is_none() {
            return;
        }
        // The Overworld spawn, converted; the End has no spawn anchors.
        let spawn = game
            .session(seed)
            .dimension(Dimension::Overworld)
            .map(|s| s.spawn_origin().map_block())
            .and_then(|at| game.convert(at, Dimension::Overworld, dimension))
            .unwrap_or([0, 0]);
        let [x, z] = search::focus(query.query(), spawn);
        self.settings.view.centre = [f64::from(x) + 0.5, f64::from(z) + 0.5];
        for id in search::structures(evaluation) {
            if !self.settings.structures.contains(&id) {
                self.settings.structures.push(id);
            }
        }
        self.select();
        self.opened = Some((seed, matches));
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

    fn layers_section(&mut self, ui: &mut egui::Ui) {
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
        // The view's centre block's corner is pixel size / 2 from the top left, as in
        // `MapArea::centred_on`, and on a whole physical pixel, so texels and marker edges land
        // on screen pixels and an export covers the same blocks.
        let ppp = ui.ctx().pixels_per_point();
        let snap = |v: f32| (v * ppp).round() / ppp;
        let centre = Pos2::new(
            snap(rect.left()) + (rect.width() / 2.0).floor(),
            snap(rect.top()) + (rect.height() / 2.0).floor(),
        );
        self.map_size = [rect.width() as u32, rect.height() as u32];
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
        // Terrain layers by each tile's own scale, so the coarser tiles shown while one loads
        // are found under the keys they were made with.
        let terrain_here = self.terrain_allowed();
        let (relief, surface) = (
            self.settings.relief && terrain_here,
            self.settings.surface && terrain_here,
        );
        let key = |tile: Tile| {
            let terrain = tile.scale <= MAX_TERRAIN_SCALE;
            TileKey {
                source,
                y: view.y,
                heights: terrain && (relief || surface),
                surface: terrain && surface,
                tile,
            }
        };
        let selection = self.selection.id;
        let confirm = self.settings.confirm;
        let marker_key = |tile: Tile| MarkerKey {
            source,
            selection,
            confirm,
            tile,
        };
        for (done, result) in self.pool.finished() {
            match (done, result) {
                (JobKey::Biomes(k), result) if k.source == source => {
                    let data = result.and_then(|out| match out {
                        Output::Biomes(data) => Ok(data),
                        _ => Err("not a biome tile".to_string()),
                    });
                    self.cache.insert(k, Entry::new(data));
                }
                (JobKey::Markers(k), result) if k.source == source && k.selection == selection => {
                    let starts = result.and_then(|out| match out {
                        Output::Markers(starts) => Ok(starts),
                        _ => Err("not markers".to_string()),
                    });
                    self.markers.insert(k, starts);
                }
                (JobKey::Layout(k), result) if k.source == source => {
                    let layout = result.and_then(|out| match out {
                        Output::Layout(layout) => Ok(layout),
                        _ => Err("not a layout".to_string()),
                    });
                    self.layouts.insert(k, layout);
                }
                _ => {}
            }
        }
        self.cache.next_frame();
        self.markers.next_frame();
        self.layouts.next_frame();
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
        // Then the layouts of the structures certainly starting in view, up close.
        let mut layout_keys: Vec<(LayoutKey, String)> = Vec::new();
        if marking && confirm && view.scale <= MAX_BOUNDS_SCALE {
            for &tile in &tiles {
                let Some(Ok(found)) = self.markers.get(&marker_key(tile)) else {
                    continue;
                };
                for start in found.iter() {
                    let index = start
                        .starting()
                        .and_then(|s| world.structures().position(|w| w == s));
                    if let Some(structure) = index {
                        let key = LayoutKey {
                            source,
                            structure,
                            chunk: start.chunk,
                        };
                        layout_keys.push((key, start.set.clone()));
                    }
                }
            }
            jobs.extend(
                layout_keys
                    .iter()
                    .filter(|(k, _)| !self.layouts.contains(k))
                    .map(|(k, _)| job(JobKey::Layout(*k))),
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

        // Bounds under the markers, over every tile.
        for (key, set) in &layout_keys {
            if let Some(Ok(Some(layout))) = self.layouts.get(key) {
                let b = layout.bounds;
                let at =
                    markers::block_rect(&view, centre, [b.min[0], b.min[2]], [b.max[0], b.max[2]]);
                if rect.intersects(at) {
                    markers::bounds(&painter, at, set);
                }
            }
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
        if self.settings.spawn && self.spawn_allowed() {
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
        if let Some((seed, matches)) = &self.opened
            && *seed == session.seed()
        {
            for m in matches
                .iter()
                .filter(|m| m.dimension == world.dimension_kind())
            {
                let at = markers::pixel_centre(&view, centre, m.at);
                markers::hit(&painter, at, m.certain);
                shown_markers.push((at, Picked::Match(m)));
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
                if style.relief
                    && let Some(heights) = &data.heights
                {
                    image.shade(heights);
                }
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
                let session = &shown.session;
                let height = session.height_at([x, z], Heightmap::WorldSurfaceWg);
                let surface = self.settings.surface
                    && self.terrain_allowed()
                    && self.settings.view.scale <= MAX_TERRAIN_SCALE;
                let at = if surface {
                    session.surface_biome_at([x, z])
                } else {
                    session.biome_at([x, self.settings.view.y, z])
                };
                ui.monospace(format!(
                    "x {x:>9}  z {z:>9}  chunk {:>7} {:>7}  height {height:>4}",
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
                    ui.weak(format!(
                        "{}, Minecraft {}",
                        dimension_label(world.dimension_kind()),
                        world.version()
                    ));
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
                    "Candidate: whether it generates depends on terrain (unless confirmed), a \
                     tied biome or a platform-dependent ring position.",
                );
            }
        }
        Picked::Spawn(origin) => {
            ui.strong("World spawn");
            let [cx, cz] = origin.chunk;
            ui.label(format!(
                "The game's spawn search starts in chunk {cx}, {cz}; the spawn is in the 11 × 11 \
                 chunks around it, decided by generated blocks."
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
        Picked::Match(m) => {
            ui.strong(&m.what);
            ui.label(format!("{}, {}", m.at[0], m.at[1]));
            ui.weak(&m.constraint);
            if !m.certain {
                ui.label("Possible: it depends on terrain, a tied biome or the platform.");
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
