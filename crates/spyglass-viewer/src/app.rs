//! The viewer window: its state, and the loading and background work behind it. What it draws
//! is in the child modules: the toolbar and settings (`toolbar`), the side panel's tabs
//! (`side_panel`, `search_panel`, `markers_panel`), the map with its controls and status bar
//! (`map`), and the screen shown until there's a bundle (`saves`, `extract_panel`).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

use eframe::egui::{self, Color32, Key, Pos2, Rect, Sense, TextureOptions, Vec2};
use serde::{Deserialize, Serialize};
use spyglass::{
    BiomeQuery, BundleInfo, Dimension, Evaluation, FeatureQuery, FindOutcome, FoundContainer,
    FoundFeature, Game, Heightmap, Image, ItemQuery, LauncherSave, Layers, MAX_UNDERGROUND_SCALE,
    Pack, SaveDetails, SaveReport, Scan, SearchOutcome, SearchQuery, Session, SpawnOrigin,
    StructureLayout, StructureQuery, StructureStart, World,
};

use crate::export::{Export, Source, visible_area};
use crate::extract::{self, Offer};
use crate::icons;
use crate::locate::{self, Found, Target};
use crate::markers::{self, Picked};
use crate::search::{self, FindRun, Match, Running, SearchSettings, SeedRun};
use crate::style;
use crate::tiles::{
    Cache, Entry, FeatureKey, Job, JobKey, LayoutKey, LootMark, MarkerKey, Output, Pool, Style,
    TileData, TileKey, Underground, chunk_features,
};
use crate::userdata::{Marker, Store, WorldKey};
use crate::view::{FEATURE_REGION, Region, TILE, Tile, View};
use crate::zoom;

mod container;
mod extract_panel;
mod map;
mod markers_panel;
mod saves;
mod search_panel;
mod side_panel;
mod toolbar;
mod world_panel;

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
/// Regions of structure starts, features or loot containers kept, each kind: more than a 4K
/// view's regions at its scale, ring included (about 600 at 1:2 for features, or at 1:4 for
/// starts), so drawing the view never evicts what it's drawing.
const CACHE_MARKERS: usize = 1024;
/// The most structure markers drawn at once; more is a zoom level too far out to read.
const MAX_MARKERS: usize = 2_000;
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
    /// Shade the chunks an open save has generated, up close.
    generated: bool,
    /// Shade them by the time players spent near them.
    inhabited: bool,
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
    /// The flower bone meal grows at each block, at the view's height.
    flowers: bool,
    /// The one flower shown, by block id; `None` shows each block's most likely flower.
    flower: Option<String>,
    /// Structure ids to mark.
    structures: Vec<String>,
    /// Placed feature ids to mark, up close.
    features: Vec<String>,
    /// Mark every loot container, up close.
    containers: bool,
    /// The item, or item `#tag`, whose containers are ringed, up close; empty for none.
    loot: String,
    /// Biome ids to keep bright.
    highlight: Vec<String>,
    /// The Search tab's query and options.
    search: SearchSettings,
    /// Draw what's marked done, dimmed; otherwise hide it.
    show_done: bool,
    /// Put the user's markers in exported images.
    export_markers: bool,
    /// The side panel's sections not folded as they are by default, by id
    /// (`layers.underground`, `world.packs`): see `style::is_folded`.
    folded: BTreeSet<String>,
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
            generated: false,
            inhabited: false,
            spawn: true,
            confirm: true,
            relief: false,
            surface: false,
            veins: false,
            noise_caves: false,
            carvers: false,
            flowers: false,
            flower: None,
            structures: Vec::new(),
            features: Vec::new(),
            containers: false,
            loot: String::new(),
            highlight: Vec::new(),
            search: SearchSettings::default(),
            show_done: true,
            export_markers: true,
            folded: BTreeSet::new(),
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

/// A game's key: its bundle directory, and the hashes of the data packs applied over it, lowest
/// first (none for a seed, a save's for a save).
type GameKey = (PathBuf, Vec<String>);

/// Games by bundle and packs, kept for the whole run with every dimension they've loaded, so
/// sessions and tile jobs can borrow their worlds from any thread. A run opens a handful at
/// most.
type Games = Arc<Mutex<HashMap<GameKey, &'static Game>>>;

/// The key of the bundle in `dir` with `packs` applied.
fn game_key(dir: &Path, packs: &[Pack]) -> GameKey {
    (
        dir.to_path_buf(),
        packs.iter().map(|p| p.hash().to_string()).collect(),
    )
}

/// `dimension` of the bundle in `dir` with `packs` applied, loading it if no one has yet.
fn open_world(
    games: &Games,
    dir: &Path,
    packs: &[Pack],
    dimension: Dimension,
) -> Result<(&'static Game, &'static World), String> {
    let key = game_key(dir, packs);
    let known = games
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&key)
        .copied();
    let game = match known {
        Some(game) => game,
        None => {
            let game: &'static Game = Box::leak(Box::new(
                Game::open_with_packs(dir, packs).map_err(|e| e.to_string())?,
            ));
            *games
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .entry(key)
                .or_insert(game)
        }
    };
    let world = game.world(dimension).map_err(|e| e.to_string())?;
    Ok((game, world))
}

/// A world being loaded: its bundle and packs, and dimension.
type Loading = (
    (GameKey, Dimension),
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
    /// The item whose containers are ringed, up close.
    loot: Option<ItemQuery>,
    /// Why the Loot item isn't one, if it isn't.
    loot_error: Option<String>,
}

impl Default for Selection {
    fn default() -> Self {
        Selection {
            id: 0,
            queries: Arc::from(Vec::new()),
            features: Arc::from(Vec::new()),
            loot: None,
            loot_error: None,
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
    /// Each tile's loot containers, up close.
    loot_markers: Cache<FeatureKey, Result<Vec<LootMark>, String>>,
    /// The container whose contents are open in a window.
    open_container: Option<FoundContainer>,
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
    inspecting: Option<Pending<Result<(SaveReport, SaveDetails), String>>>,
    /// The save on the map, if it came from one.
    save: Option<SaveReport>,
    /// The report of what spyglass read from the last save opened, for the World tab while
    /// [`save`](Self::save) or [`unshown_save`](Self::unshown_save) is that save.
    save_details: Option<SaveDetails>,
    /// The last save opened, if spyglass couldn't show it: the World tab says why.
    unshown_save: Option<SaveReport>,
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
    /// The zoom still to make while timing (`SPYGLASS_VIEWER_TIMING_ZOOM`).
    timing_zoom: Option<crate::timing::Zoom>,
    /// The user's markers, done places and worlds (`docs/specs/user-data.md`).
    store: Store,
    /// The world shown, as the user data keys it.
    world_key: Option<WorldKey>,
    /// The marker being edited, by id.
    editing: Option<String>,
    /// What the map's context menu was opened on, and whether it's done.
    menu_target: Option<(MenuTarget, bool)>,
    /// The portal the map's context menu was opened on, if any.
    menu_portal: Option<map::portal_layer::PortalRef>,
    /// Portal markers' and the save's portals' links, and the link area.
    portals: map::portal_layer::PortalState,
    /// A view text being typed into the Share menu.
    share_text: String,
    /// The last import's, export's or view text's outcome.
    data_note: Option<Result<String, String>>,
    /// Whether the user dismissed the problems reading the data file.
    data_problems_seen: bool,
    /// The name being typed for a world, and which.
    world_name: (Option<WorldKey>, String),
    /// Best height being worked out for a flower, and the answer last worked out.
    flower_heights: Option<Pending<Result<side_panel::BestHeights, String>>>,
    best_heights: Option<Result<side_panel::BestHeights, String>>,
    /// The flowers the view's own tiles showed last frame, most pixels first, and how many
    /// pixels those tiles have: the Layers tab's key.
    flowers_in_view: Vec<InView>,
    flowers_in_view_pixels: u64,
    /// Why a tile's flowers couldn't be worked out, if one's couldn't.
    flower_error: Option<String>,
}

/// A flower in the view's key.
struct InView {
    /// Its block id.
    flower: String,
    /// The pixels it can grow at.
    possible: u64,
    /// The pixels it certainly grows at.
    certain: u64,
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
            loot_markers: Cache::new(CACHE_MARKERS),
            open_container: None,
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
            save_details: None,
            unshown_save: None,
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
            timing_zoom: crate::timing::zoom(),
            store: data_dir.map_or_else(Store::detached, |dir| Store::open(&dir)),
            world_key: None,
            editing: None,
            menu_target: None,
            menu_portal: None,
            portals: Default::default(),
            share_text: String::new(),
            data_note: None,
            data_problems_seen: false,
            world_name: (None, String::new()),
            flower_heights: None,
            best_heights: None,
            flowers_in_view: Vec::new(),
            flowers_in_view_pixels: 0,
            flower_error: None,
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

    /// Starts loading the dimension shown of a bundle in the background, with the data packs
    /// of the save shown if it's that bundle's.
    fn open(&mut self, ctx: &egui::Context, bundle: &BundleInfo) {
        self.settings.version = Some(bundle.version.clone());
        let packs: Vec<Pack> = self
            .save
            .as_ref()
            .filter(|r| r.bundle.as_ref().is_some_and(|b| b.dir == bundle.dir))
            .map(|r| r.packs.clone())
            .unwrap_or_default();
        let key = (game_key(&bundle.dir, &packs), self.settings.dimension());
        if self.loading.as_ref().is_some_and(|(k, _)| *k == key) {
            return;
        }
        let games = self.games.clone();
        let dir = bundle.dir.clone();
        let dimension = key.1;
        self.load_error = None;
        self.loading = Some((
            key,
            Pending::spawn(ctx, move || open_world(&games, &dir, &packs, dimension)),
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
        if let (Some(report), Some(details)) = (&self.save, &self.save_details)
            && !details.answerable_in(dimension)
        {
            let reasons: Vec<String> = details
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
        let Some((((dir, _), _), pending)) = &self.loading else {
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
        if let Some(result) = self.flower_heights.as_ref().and_then(Pending::poll) {
            self.flower_heights = None;
            self.best_heights =
                Some(result.unwrap_or_else(|()| Err("working out heights failed".to_string())));
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
                Ok((report, details)) => self.show_save(ctx, report, details),
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
        // A save of the seed shown answers where the game reads it (ADR 0071).
        let session = spyglass::parse_seed(&self.settings.seed).and_then(|seed| {
            let session = world.session(seed)?;
            match &self.save {
                Some(report) if report.save.seed == seed => {
                    session.with_save(Arc::new(report.chunks(world.dimension_kind())))
                }
                _ => Ok(session),
            }
        });
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
                self.loot_markers.clear();
                self.open_container = None;
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
        let item = self.settings.loot.trim();
        let (loot, loot_error) = if item.is_empty() {
            (None, None)
        } else {
            match world.item_query(item, None) {
                Ok(query) => (Some(query), None),
                Err(e) => (None, Some(e.to_string())),
            }
        };
        self.next_id += 1;
        self.selection = Selection {
            id: self.next_id,
            queries: queries.into(),
            features: features.into(),
            loot,
            loot_error,
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
                && zoom::SLIME_CHUNKS.shows(self.settings.view.scale),
            relief: self.settings.relief && self.terrain_allowed(),
            generated: self.settings.generated
                && self.generated_allowed()
                && zoom::GENERATED.shows(self.settings.view.scale),
            inhabited: self.settings.inhabited,
            flower: self.settings.flower.clone(),
        }
    }

    /// Whether the session shown has a save, whose generated chunks can be drawn.
    fn generated_allowed(&self) -> bool {
        self.shown
            .as_ref()
            .is_some_and(|shown| shown.session.save().is_some())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_are_kept_with_the_settings_and_old_settings_load() {
        let mut settings = Settings::default();
        style::set_folded(&mut settings.folded, "layers.underground", false, true);
        style::set_folded(&mut settings.folded, "search.range", true, false);
        let text = serde_json::to_string(&settings).unwrap();
        let again: Settings = serde_json::from_str(&text).unwrap();
        assert_eq!(again.folded, settings.folded);
        assert!(style::is_folded(&again.folded, "layers.underground", false));
        assert!(!style::is_folded(&again.folded, "search.range", true));
        // Settings saved before folds load with every section as it is by default.
        let mut old: serde_json::Value = serde_json::from_str(&text).unwrap();
        old.as_object_mut().unwrap().remove("folded");
        let old: Settings = serde_json::from_value(old).unwrap();
        assert!(old.folded.is_empty());
        assert!(!style::is_folded(&old.folded, "layers.biomes", false));
        assert!(style::is_folded(&old.folded, "search.range", true));
    }
}
