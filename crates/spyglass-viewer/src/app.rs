//! The viewer window: its state, and the loading and background work behind it. What it draws
//! is in the child modules: the toolbar and settings (`toolbar`), the side panel's tabs
//! (`side_panel`, `search_panel`, `markers_panel`), the map with its controls and status bar
//! (`map`), and the screen shown until there's a bundle (`saves`, `extract_panel`).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

use eframe::egui::{self, Color32, Key, Pos2, Rect, Sense, TextureOptions, Vec2};
use serde::{Deserialize, Serialize};
use spyglass::{
    BiomeQuery, BundleInfo, Dimension, Evaluation, FeatureQuery, FindOutcome, FoundFeature, Game,
    Heightmap, Image, LauncherSave, Layers, MAX_TERRAIN_SCALE, MAX_UNDERGROUND_SCALE, SaveReport,
    Scan, SearchOutcome, SearchQuery, Session, SpawnOrigin, StructureLayout, StructureQuery,
    StructureStart, World,
};

use crate::export::{Export, Source, visible_area};
use crate::extract::{self, Offer};
use crate::icons;
use crate::locate::{self, Found, Target};
use crate::markers::{self, Picked};
use crate::search::{self, FindRun, Match, Running, SearchSettings, SeedRun};
use crate::style;
use crate::tiles::{
    Cache, Entry, FeatureKey, Job, JobKey, LayoutKey, MarkerKey, Output, Pool, Style, TileKey,
    Underground, slime_features,
};
use crate::userdata::{Marker, Store, WorldKey};
use crate::view::{TILE, Tile, View};

mod extract_panel;
mod map;
mod markers_panel;
mod saves;
mod search_panel;
mod side_panel;
mod toolbar;

use markers_panel::MenuTarget;
use search_panel::Opened;
use side_panel::Tab;

/// The key settings are saved under.
const SETTINGS: &str = "spyglass";
/// Tiles kept: about 64 MB of textures and 16 MB of biome maps at 128 × 128.
const CACHE_TILES: usize = 1024;
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
/// The largest scale jigsaw structures' and mineshafts' bounds are drawn at: a village is then
/// 30 pixels or more across, and few enough are in view to lay them all out.
const MAX_BOUNDS_SCALE: u32 = 4;
/// Structure layouts kept.
const CACHE_LAYOUTS: usize = 512;

/// What's remembered between runs.
#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    /// The folder bundles are looked for in.
    bundles: PathBuf,
    /// The game version last shown.
    version: Option<String>,
    /// The seed box's text, as typed.
    seed: String,
    /// The dimension shown, by id.
    dimension: String,
    /// Where the map looks in the dimension shown.
    view: View,
    /// The other dimensions' views, by id, to go back to.
    views: BTreeMap<String, View>,
    /// Show tied biome cells as a checker of their candidates.
    ties: bool,
    /// Draw slime chunks, up close.
    slime_chunks: bool,
    /// Mark the world spawn.
    spawn: bool,
    /// Decide structure starts and locates with the seed's terrain.
    confirm: bool,
    /// Shade by terrain height, up close.
    relief: bool,
    /// Biomes at the surface, up close.
    surface: bool,
    /// Ore veins at the view's height, up close.
    veins: bool,
    /// Noise caves at the view's height, up close.
    noise_caves: bool,
    /// Carvers' caves and ravines at the view's height, up close.
    carvers: bool,
    /// Structure ids to mark.
    structures: Vec<String>,
    /// Placed feature ids to mark, up close.
    features: Vec<String>,
    /// Biome ids to keep bright.
    highlight: Vec<String>,
    /// The Search tab's query and options.
    search: SearchSettings,
    /// Draw what's marked done, dimmed; otherwise hide it.
    show_done: bool,
    /// Put the user's markers in exported images.
    export_markers: bool,
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
            veins: false,
            noise_caves: false,
            carvers: false,
            structures: Vec::new(),
            features: Vec::new(),
            highlight: Vec::new(),
            search: SearchSettings::default(),
            show_done: true,
            export_markers: true,
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
    /// Runs `work` on a thread of its own, and repaints when it's done so the result is picked
    /// up.
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
    /// The world and seed.
    session: Arc<Session<'static>>,
    /// Where the game's spawn search starts, worked out once.
    spawn: SpawnOrigin,
}

/// The structures and features chosen, resolved against the world shown.
struct Selection {
    /// Numbers marker keys; new for every change.
    id: u64,
    /// The structures to mark.
    queries: Arc<[StructureQuery]>,
    /// The features to mark, up close.
    features: Arc<[FeatureQuery]>,
}

impl Default for Selection {
    fn default() -> Self {
        Selection {
            id: 0,
            queries: Arc::from(Vec::new()),
            features: Arc::from(Vec::new()),
        }
    }
}

/// The viewer's state between frames.
pub struct App {
    /// What's remembered between runs.
    settings: Settings,
    /// The bundles found in the bundles folder, or why it couldn't be read.
    bundles: Result<Vec<BundleInfo>, String>,
    /// Every bundle opened this run.
    games: Games,
    /// The bundle shown, every dimension of it.
    game: Option<&'static Game>,
    /// The dimension shown.
    world: Option<&'static World>,
    /// A world being loaded in the background.
    loading: Option<Loading>,
    /// Why the last load failed.
    load_error: Option<String>,
    /// The world and seed on the map; `None` while loading or with a bad seed.
    shown: Option<Shown>,
    /// Why the seed box's text isn't a seed.
    seed_error: Option<String>,
    /// The last number given to a session or a selection, so keys from older ones never match.
    next_id: u64,
    /// The worker threads computing tiles.
    pool: Pool,
    /// Biome tiles and their textures.
    cache: Cache<TileKey, Entry>,
    /// Each tile's structure starts.
    markers: Cache<MarkerKey, Result<Vec<StructureStart>, String>>,
    /// Each tile's features, up close.
    feature_markers: Cache<FeatureKey, Result<Vec<FoundFeature>, String>>,
    /// Structure layouts, for their bounds; `None` for a structure that has none.
    layouts: Cache<LayoutKey, Result<Option<StructureLayout>, String>>,
    /// The structures and features marked.
    selection: Selection,
    /// The Layers tab's structure filter, as typed.
    structure_filter: String,
    /// The Layers tab's biome filter, as typed.
    biome_filter: String,
    /// Whether the last frame had more markers than it could show.
    too_many_markers: bool,
    /// The block the map's context menu was opened on.
    menu_block: Option<[i32; 2]>,
    /// A locate running.
    locating: Option<Pending<Result<Found, String>>>,
    /// The last locate's hits, or why it failed.
    found: Option<Result<Found, String>>,
    /// Saves in the launchers' folders, listed when first wanted.
    saves: Option<Vec<LauncherSave>>,
    /// A save being read.
    inspecting: Option<Pending<Result<SaveReport, String>>>,
    /// The save on the map, if it came from one.
    save: Option<SaveReport>,
    /// Why the last save couldn't be opened or shown.
    save_error: Option<String>,
    /// The saves list's filter, as typed.
    save_filter: String,
    /// The map's size in pixels, for exports.
    map_size: [u32; 2],
    /// An export being drawn and saved.
    exporting: Option<Pending<Result<PathBuf, String>>>,
    /// The last export's outcome, or a copied command.
    export_note: Option<Result<String, String>>,
    /// The go-to box's text.
    goto: String,
    /// Whether the go-to box's text didn't parse.
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
    /// The `find` running or last run in the seed shown.
    find_run: Option<FindState>,
    /// A live hit being evaluated before it opens.
    opening: Option<Pending<Result<(i64, Evaluation), String>>>,
    /// The hit last opened: its seed, and what its constraints matched and where.
    opened: Option<Opened>,
    /// Why the last search or find couldn't run or open.
    search_error: Option<String>,
    /// Game versions that can be extracted, listed when first wanted.
    offers: Option<Result<Vec<Offer>, String>>,
    /// The versions being listed.
    offering: Option<Pending<Result<Vec<Offer>, String>>>,
    /// The version picked to extract.
    extract_version: Option<String>,
    /// An extraction running.
    extracting: Option<extract::Running>,
    /// The last extraction's outcome.
    extract_note: Option<Result<String, String>>,
    /// Whether the bundles folder couldn't be created, so the per-user one is offered.
    offer_user_folder: bool,
    /// The side panel's tab.
    tab: Tab,
    /// Whether the settings window is open.
    settings_open: bool,
    /// How many starts of each structure the map showed last frame, for the Layers tab.
    in_view: HashMap<String, usize>,
    /// The structure families opened to their variants in the Layers tab.
    expanded: std::collections::HashSet<String>,
    /// How long the map takes to fill the window, with `SPYGLASS_VIEWER_TIMING`.
    fill: crate::timing::Fill,
    /// The user's markers, done places and worlds (`docs/specs/user-data.md`).
    store: Store,
    /// The world shown, as the user data keys it.
    world_key: Option<WorldKey>,
    /// The marker being edited, by id.
    editing: Option<String>,
    /// What the map's context menu was opened on, and whether it's done.
    menu_target: Option<(MenuTarget, bool)>,
    /// A view text being typed into the Share menu.
    share_text: String,
    /// The last import's, export's or view text's outcome.
    data_note: Option<Result<String, String>>,
    /// Whether the user dismissed the problems reading the data file.
    data_problems_seen: bool,
    /// The name being typed for a world, and which.
    world_name: (Option<WorldKey>, String),
}

/// A search and its query.
struct SearchRun {
    /// The query searched for.
    query: SearchQuery,
    /// Whether the search decided starts with terrain, so opening a hit does too.
    confirm: bool,
    /// The search, while it runs.
    running: Option<SeedRun>,
    /// The hits once it's done, or why it failed.
    outcome: Option<Result<SearchOutcome, String>>,
}

/// A `find` in one seed: its query, where it ran, and its matches.
struct FindState {
    /// The query whose `find` it is.
    query: SearchQuery,
    /// The seed it ran in.
    seed: i64,
    /// The dimension its targets are in.
    dimension: Dimension,
    /// The find, while it runs.
    running: Option<FindRun>,
    /// The matches once it's done, or why it failed.
    outcome: Option<Result<FindOutcome, String>>,
    /// The match open on the map, by index.
    current: Option<usize>,
}

impl App {
    /// The viewer, with its user data in `data_dir` (`None` if there's no folder for it).
    pub fn new(cc: &eframe::CreationContext<'_>, data_dir: Option<PathBuf>) -> Self {
        let stored: Option<Settings> = cc.storage.and_then(|s| eframe::get_value(s, SETTINGS));
        let first_run = stored.is_none();
        let settings = stored.unwrap_or_default();
        style::apply(&cc.egui_ctx);
        egui_extras::install_image_loaders(&cc.egui_ctx);
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
            feature_markers: Cache::new(CACHE_MARKERS),
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
            find_run: None,
            opening: None,
            opened: None,
            search_error: None,
            offers: None,
            offering: None,
            extract_version: None,
            extracting: None,
            extract_note: None,
            offer_user_folder: false,
            tab: Tab::default(),
            settings_open: false,
            in_view: HashMap::new(),
            expanded: std::collections::HashSet::new(),
            fill: crate::timing::Fill::default(),
            store: data_dir.map_or_else(Store::detached, |dir| Store::open(&dir)),
            world_key: None,
            editing: None,
            menu_target: None,
            share_text: String::new(),
            data_note: None,
            data_problems_seen: false,
            world_name: (None, String::new()),
        };
        app.scan(&cc.egui_ctx);
        // With nothing found on a first run, bundles go beside spyglass unless the user picks
        // another place (ADR 0043).
        if first_run
            && app.bundles.as_ref().is_ok_and(Vec::is_empty)
            && let Some(dir) = spyglass::app_bundles_dir()
        {
            app.settings.bundles = dir;
            app.scan(&cc.egui_ctx);
        }
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
        if let Some(running) = self.search_run.as_ref().and_then(|r| r.running.as_ref()) {
            running.yield_to_map(self.pool.pending() > 0);
        }
        if let Some(run) = &mut self.find_run
            && let Some(outcome) = run.running.as_ref().and_then(Running::poll)
        {
            run.running = None;
            let first = outcome.as_ref().is_ok_and(|o| !o.matches.is_empty());
            run.outcome = Some(outcome);
            if first && run.current.is_none() {
                self.open_match(ctx, 0);
            }
        }
        if let Some(running) = self.find_run.as_ref().and_then(|r| r.running.as_ref()) {
            running.yield_to_map(self.pool.pending() > 0);
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
                self.feature_markers.clear();
                self.layouts.clear();
                self.select();
                self.remember_world();
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
        // A tag (from settings written by hand or by `cargo xtask viewer-timing`) becomes its
        // members, so the structure list shows what's marked and can turn each off. One with
        // no members here is kept for the dimensions that have them.
        let mut chosen: Vec<String> = Vec::new();
        for id in &self.settings.structures {
            let members = if id.starts_with('#') {
                world
                    .structure_query(id)
                    .and_then(|q| {
                        world
                            .query_structures(&q)
                            .map(|ids| ids.into_iter().map(str::to_string).collect())
                    })
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            if members.is_empty() {
                chosen.push(id.clone());
            } else {
                chosen.extend(members);
            }
        }
        chosen.sort();
        chosen.dedup();
        self.settings.structures = chosen;
        let queries: Vec<StructureQuery> = self
            .settings
            .structures
            .iter()
            .filter_map(|id| world.structure_query(id).ok())
            .collect();
        // Features the world doesn't place are kept in the settings but not marked.
        let features: Vec<FeatureQuery> = self
            .settings
            .features
            .iter()
            .filter_map(|id| world.feature_query(id).ok())
            .collect();
        self.next_id += 1;
        self.selection = Selection {
            id: self.next_id,
            queries: queries.into(),
            features: features.into(),
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

    /// Whether ore veins are drawn in the dimension shown.
    fn veins_allowed(&self) -> bool {
        self.allows(Layers {
            veins: true,
            ..Layers::default()
        })
    }

    /// The underground layers on, where the dimension shown has them, at any scale.
    fn underground(&self) -> Underground {
        Underground {
            veins: self.settings.veins && self.veins_allowed(),
            noise_caves: self.settings.noise_caves,
            carvers: self.settings.carvers,
        }
    }

    /// How tiles are coloured now: each layer only where it can be drawn.
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
        self.store.flush(false);
        self.poll_extract(ui.ctx());
        let ready = self.bundles.as_ref().is_ok_and(|found| !found.is_empty());
        if ready {
            egui::Panel::top("toolbar")
                .frame(
                    egui::Frame::new()
                        .fill(style::PANEL)
                        .inner_margin(egui::Margin::symmetric(12, 6))
                        .stroke(egui::Stroke::new(1.0, style::BORDER)),
                )
                .show(ui, |ui| self.toolbar(ui));
            egui::Panel::left("side")
                .resizable(true)
                .default_size(300.0)
                .size_range(260.0..=460.0)
                .frame(
                    egui::Frame::new()
                        .fill(style::PANEL)
                        .inner_margin(egui::Margin::symmetric(14, 8)),
                )
                .show(ui, |ui| self.side_panel(ui));
            egui::Panel::bottom("status")
                .frame(
                    egui::Frame::new()
                        .fill(style::PANEL)
                        .inner_margin(egui::Margin::symmetric(12, 4)),
                )
                .show(ui, |ui| self.status_bar(ui));
        }
        self.settings_window(ui.ctx());
        self.marker_editor(ui.ctx());
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(style::GROUND))
            .show(ui, |ui| {
                if ready {
                    self.map(ui);
                } else {
                    self.no_bundle(ui);
                }
            });
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, SETTINGS, &self.settings);
        // Also on shutdown, so the last changes and views are kept.
        self.keep_views();
        self.store.flush(true);
    }
}
