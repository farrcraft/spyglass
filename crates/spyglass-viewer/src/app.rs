//! The viewer window: the side panel (source and view), the map, and the status bar.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

use eframe::egui::{self, Color32, Key, Pos2, Rect, Sense, TextureOptions, Vec2};
use serde::{Deserialize, Serialize};
use spyglass::{BiomeQuery, BundleInfo, Image, Session, World};

use crate::tiles::{Cache, Job, Pool, Style, TileKey, slime_features};
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
    cache: Cache,
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
                    session: Arc::new(session),
                });
                self.cache.clear();
            }
            Err(e) => self.seed_error = Some(e.to_string()),
        }
    }

    fn style(&self) -> Style {
        Style {
            ties: self.settings.ties,
            highlight: Vec::new(),
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
                self.apply_seed();
            }
        });
        if let Some(e) = &self.seed_error {
            ui.colored_label(ui.visuals().error_fg_color, e);
        } else if let Some(shown) = &self.shown {
            ui.weak(format!("seed {}", shown.session.seed()));
        }
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
    }

    /// The screen shown until there's a bundle to read.
    /// The bundles folder, made absolute so it's clear where it is.
    fn bundles_dir(&self) -> PathBuf {
        std::path::absolute(&self.settings.bundles)
            .unwrap_or_else(|_| self.settings.bundles.clone())
    }

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
        let centre = rect.center();
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
        let (source, session) = (shown.id, shown.session.clone());
        let view = self.settings.view;
        let tiles = view.tiles([rect.width(), rect.height()]);
        let key = |tile: Tile| TileKey {
            source,
            y: view.y,
            tile,
        };
        for (done, result) in self.pool.finished() {
            if done.source == source {
                self.cache.insert(done, result);
            }
        }
        self.cache.next_frame();
        let jobs = tiles
            .iter()
            .map(|&t| key(t))
            .filter(|k| !self.cache.contains(k))
            .map(|key| Job {
                key,
                session: session.clone(),
            })
            .collect();
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
        self.cursor = response
            .hover_pos()
            .map(|hover| view.block_at(offset(hover)).map(|b| b.floor() as i32));
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
            });
        });
    }
}

fn full_uv() -> Rect {
    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0))
}

fn color_image(image: &Image) -> egui::ColorImage {
    let [w, h] = image.size();
    egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], image.rgba())
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
