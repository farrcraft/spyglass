//! The viewer window: its state, and the loading and background work behind it. What it draws
//! is in the child modules: the side panel's sections (`side_panel`, `saves`, `search_panel`)
//! and the map with its status bar (`map`).

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

mod map;
mod saves;
mod search_panel;
mod side_panel;

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
            bundles: spyglass::default_bundles_dir(),
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
    /// A query being parsed on its own thread: resolving it can load a dimension.
    search_parsing: Option<(String, Pending<Result<SearchQuery, String>>)>,
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
            search_parsing: None,
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
                    self.search_parsing = None;
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

    /// The bundles folder, made absolute so it's clear where it is.
    fn bundles_dir(&self) -> PathBuf {
        std::path::absolute(&self.settings.bundles)
            .unwrap_or_else(|_| self.settings.bundles.clone())
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
