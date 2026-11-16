//! The map: tiles drawn as textures (coarse ones first, so the view fills quickly), markers,
//! picking, the controls over the map, and the status bar.

use std::collections::HashSet;

use spyglass::Outcome;

use super::markers_panel::MenuTarget;
use super::search_panel::Opened;
use super::*;
use crate::markers::{DONE_OPACITY, Size};
use crate::search::Area;
use crate::style;
use crate::timing;
use crate::userdata::Place;
use crate::view;

mod controls;
mod jobs;
mod marker_layer;
mod menu;

use controls::scale_bar;
pub(super) use marker_layer::contents_lines;

/// What the map shows behind tiles not drawn yet: faint diagonal lines, so a gap reads as
/// unfinished. Not a checker, which means tied biomes.
const HATCH: Color32 = Color32::from_rgb(0x2a, 0x2a, 0x2a);
/// Points between the hatch's lines.
const HATCH_GAP: f32 = 14.0;

impl App {
    /// The map, each frame: input first (drag, zoom, keys), then the finished jobs into their
    /// caches and a new list of the jobs wanted, then the tiles, with a coarser one scaled up
    /// wherever a tile isn't ready, then areas, bounds and markers over every tile, and last the
    /// controls and the context menu.
    pub(super) fn map(&mut self, ui: &mut egui::Ui) {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        // The view's centre block's corner is pixel size / 2 from the top left, as in
        // `MapArea::centred_on`, and on a whole physical pixel, so texels and marker edges land
        // on screen pixels and an export covers the same blocks.
        let pixels_per_point = ui.ctx().pixels_per_point();
        let snap = |v: f32| (v * pixels_per_point).round() / pixels_per_point;
        let centre = Pos2::new(
            snap(rect.left()) + (rect.width() / 2.0).floor(),
            snap(rect.top()) + (rect.height() / 2.0).floor(),
        );
        self.map_size = [rect.width() as u32, rect.height() as u32];
        let offset = |p: Pos2| [p.x - centre.x, p.y - centre.y];
        let keys = self.map_input(ui, &response, centre);

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, EMPTY);
        draw_hatch(&painter, rect);
        if let Some(text) = &keys.pasted {
            self.open_view_text(ui.ctx(), text);
        }
        if keys.add_marker
            && let Some(block) = self.cursor
        {
            self.add_marker(block);
        }
        let (Some(shown), Some(world)) = (&self.shown, self.world) else {
            return;
        };
        let (source, session, spawn) = (shown.id, shown.session.clone(), shown.spawn);
        let frame = self.frame(source, rect);
        let view = frame.view;
        self.take_finished(source, frame.selection);
        let layout_keys = self.request_jobs(&frame, world, &session);
        let drawn = self.draw_tiles(ui, &painter, &frame, world, centre);
        self.count_flowers(&frame);
        if timing::enabled() {
            self.note_timing(&frame, &layout_keys, drawn);
        }
        self.draw_areas_and_bounds(
            &painter,
            &frame,
            centre,
            rect,
            world,
            &session,
            &layout_keys,
        );

        let canvas = Canvas {
            painter: &painter,
            response: &response,
            rect,
            centre,
        };
        let pointed = self.draw_marker_layer(ui, &canvas, &frame, world, &session, &spawn);
        if response.clicked()
            && let Some((MenuTarget::Marker(id), _)) = &pointed.target
        {
            self.editing = Some(id.clone());
        }
        // A click on a container opens its contents.
        if response.clicked()
            && let Some(container) = pointed.container
        {
            self.open_container = Some(container);
        }
        self.container_window(ui.ctx());
        self.cursor = response
            .hover_pos()
            .map(|hover| view.block_at(offset(hover)).map(|b| b.floor() as i32));

        let spawn_block = spawn.map_block();
        if keys.step != 0 {
            self.step_match(ui.ctx(), keys.step);
        }
        self.map_controls(ui, rect, spawn_block, keys.focus_goto);
        self.map_legend(ui, rect, world);
        scale_bar(&painter, rect, view.scale);

        if response.secondary_clicked()
            && let Some(p) = response.interact_pointer_pos()
        {
            self.menu_block = Some(view.block_at(offset(p)).map(|b| b.floor() as i32));
            self.menu_target = pointed.target;
        }
        self.context_menu(ui, &response, world, view.y);
    }

    /// The map's input this frame: dragging and scrolling move and zoom the view, as do the
    /// arrow keys and + and −, unless a text field has focus; the other keys are returned.
    fn map_input(&mut self, ui: &egui::Ui, response: &egui::Response, centre: Pos2) -> MapKeys {
        let offset = |p: Pos2| [p.x - centre.x, p.y - centre.y];
        let view = &mut self.settings.view;
        if response.dragged() {
            let drag = response.drag_delta();
            view.pan([drag.x, drag.y]);
        }
        if let Some(hover) = response.hover_pos() {
            // Scrolling builds up until it's a whole step, so a trackpad's small deltas zoom as
            // a wheel's notches do; a pinch is a step at once.
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
        let mut keys = MapKeys::default();
        if ui.memory(|m| m.focused().is_none()) {
            ui.input(|i| {
                keys.add_marker = response.hovered() && i.key_pressed(Key::M);
                for event in &i.events {
                    if let egui::Event::Paste(text) = event
                        && text.contains("spyglass:")
                    {
                        keys.pasted = Some(text.clone());
                    }
                }
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
                keys.focus_goto = i.key_pressed(Key::G);
                if i.key_pressed(Key::N) {
                    keys.step = 1;
                } else if i.key_pressed(Key::P) {
                    keys.step = -1;
                }
            });
        }
        keys
    }

    /// What this frame draws of world `source` in `rect`, and the keys its tiles are cached
    /// under.
    fn frame(&self, source: u64, rect: Rect) -> Frame {
        let view = self.settings.view;
        // Terrain layers on the view's own tiles, and on the coarser ones shown while they load
        // only where heights are cheap, so the stand-ins fill in quickly.
        let terrain_here = self.terrain_allowed();
        Frame {
            source,
            view,
            tiles: view.tiles([rect.width(), rect.height()]),
            selection: self.selection.id,
            confirm: self.settings.confirm,
            relief: self.settings.relief && terrain_here,
            surface: self.settings.surface && terrain_here,
            // The underground layers only on the view's own tiles: the coarser ones shown
            // while they load would cost as much again.
            underground: self.underground(),
            // Flowers cost about twice what biomes do, cheap enough for the stand-ins too.
            flowers: self.settings.flowers,
            marking: !self.selection.queries.is_empty() && self.structures_generate(),
            // Features only up close: each chunk they could be in is placed.
            featuring: !self.selection.features.is_empty()
                && view.scale <= spyglass::MAX_FEATURE_SCALE,
            // Loot containers too, every one or those of the Loot item.
            looting: (self.settings.containers || self.selection.loot.is_some())
                && view.scale <= spyglass::MAX_FEATURE_SCALE,
        }
    }

    /// A tile's texture in `style`, making it if there's budget left this frame.
    pub(super) fn texture(
        &mut self,
        ctx: &egui::Context,
        world: &'static World,
        key: TileKey,
        style: &Style,
        made: &mut usize,
    ) -> Option<egui::TextureId> {
        let entry = self.cache.get(&key)?;
        let stale = entry
            .texture
            .as_ref()
            .is_none_or(|(made_in, _)| made_in != style);
        if stale
            && *made < TEXTURES_PER_FRAME
            && let Ok(data) = &entry.data
        {
            // On the UI thread: what every new tile costs the frame it's drawn in.
            timing::job(
                || "texture".to_string(),
                || {
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
                        // As `Session::render` does wherever it has heights.
                        if let Some(heights) = &data.heights {
                            image.mark_void(heights);
                        }
                        // The map has only the layers the key asked for.
                        if let Some(underground) = &data.underground {
                            image.draw_underground(underground, &key.underground.layers());
                        }
                        if let Some(Ok(flowers)) = &data.flowers {
                            image.draw_flowers(&flowers.map, style.flower.as_deref());
                        }
                        if style.slime_chunks || style.generated {
                            image.draw_features(data.biomes.area(), &chunk_features(data, style));
                        }
                        let texture = ctx.load_texture(
                            format!("{key:?}"),
                            color_image(&image),
                            TextureOptions::NEAREST,
                        );
                        entry.texture = Some((style.clone(), texture));
                        *made += 1;
                    }
                },
            );
        }
        entry.texture.as_ref().map(|(_, t)| t.id())
    }

    /// The bar under the map: the block under the pointer, its chunk and biome (or a hint),
    /// and on the right the dimension, version and how many tiles are left.
    pub(super) fn status_bar(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 16.0;
            if let (Some([x, z]), Some(shown)) = (self.cursor, &self.shown) {
                let session = &shown.session;
                let height = session.height_at([x, z], Heightmap::WorldSurfaceWg);
                let surface = self.settings.surface && self.terrain_allowed();
                let at = if surface {
                    session.surface_biome_at([x, z])
                } else {
                    session.biome_at([x, self.settings.view.y, z])
                };
                ui.label(style::mono(format!("{x}, {height}, {z}")))
                    .on_hover_text("x, terrain height, z");
                ui.label(style::mono(format!("chunk {}, {}", x >> 4, z >> 4)).color(style::MUTED));
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let [r, g, b] = spyglass::biome_colour(&at.biome);
                    let (swatch, _) =
                        ui.allocate_exact_size(egui::vec2(10.0, 10.0), Sense::hover());
                    ui.painter()
                        .rect_filled(swatch, 2.0, Color32::from_rgb(r, g, b));
                    let name = spyglass::display_name(&at.biome);
                    if at.tied.is_empty() {
                        ui.label(name).on_hover_text(&at.biome);
                    } else {
                        let tied: Vec<String> =
                            at.tied.iter().map(|b| spyglass::display_name(b)).collect();
                        ui.label(format!("{name} (tied: {})", tied.join(", ")));
                    }
                });
                if self.settings.flowers {
                    let y = self.settings.view.y;
                    if let Ok(answer) = session.flower_at([x, y, z]) {
                        ui.label(flower_summary(&answer))
                            .on_hover_text(format!("What bone meal grows at y {y}"));
                    }
                }
            } else {
                ui.label(
                    egui::RichText::new(
                        "Drag to move · scroll to zoom · right-click to locate or mark · M adds \
                         a marker",
                    )
                    .color(style::MUTED),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(world) = self.world {
                    ui.label(
                        egui::RichText::new(format!(
                            "{} · {}",
                            dimension_label(world.dimension_kind()),
                            world.version()
                        ))
                        .color(style::MUTED),
                    );
                }
                let pending = self.pool.pending();
                if pending > 0 {
                    ui.label(
                        egui::RichText::new(format!("Drawing, {pending} left")).color(style::MUTED),
                    );
                    ui.spinner();
                }
                if self.too_many_markers {
                    ui.colored_label(style::WARN, "Too many structures to show here; zoom in.");
                }
            });
        });
    }
}

/// The keys pressed on the map this frame that the map acts on after drawing.
#[derive(Default)]
struct MapKeys {
    /// G: focus the go-to field.
    focus_goto: bool,
    /// N and P step through the last find's matches: 1, −1, or 0.
    step: isize,
    /// M adds a marker under the pointer.
    add_marker: bool,
    /// A view text pasted onto the map, which opens it.
    pasted: Option<String>,
}

/// What one frame of the map draws, and the keys its tiles' biomes, markers and features are
/// cached under.
struct Frame {
    /// The world shown.
    source: u64,
    view: View,
    /// The tiles the view covers, nearest the centre first.
    tiles: Vec<Tile>,
    /// The marker selection's number.
    selection: u64,
    /// Whether starts are confirmed by terrain.
    confirm: bool,
    /// Whether relief and the surface are drawn, where terrain is allowed.
    relief: bool,
    surface: bool,
    /// The underground layers, on the view's own tiles only.
    underground: Underground,
    /// Whether flowers are drawn.
    flowers: bool,
    /// Whether structures are marked, features placed, and loot containers marked.
    marking: bool,
    featuring: bool,
    looting: bool,
}

impl Frame {
    /// The key a tile's biomes are kept under, with the layers that tile can have.
    fn key(&self, tile: Tile) -> TileKey {
        let terrain = tile.has_terrain(self.view.scale);
        let deep = tile.scale == self.view.scale && tile.scale <= MAX_UNDERGROUND_SCALE;
        TileKey {
            source: self.source,
            y: self.view.y,
            heights: terrain && (self.relief || self.surface),
            surface: terrain && self.surface,
            underground: if deep {
                self.underground
            } else {
                Underground::default()
            },
            flowers: self.flowers,
            tile,
        }
    }

    fn marker_key(&self, tile: Tile) -> MarkerKey {
        MarkerKey {
            source: self.source,
            selection: self.selection,
            confirm: self.confirm,
            tile,
        }
    }

    fn feature_key(&self, tile: Tile) -> FeatureKey {
        FeatureKey {
            source: self.source,
            selection: self.selection,
            tile,
        }
    }
}

/// Where the map is drawn this frame.
struct Canvas<'a> {
    painter: &'a egui::Painter,
    response: &'a egui::Response,
    rect: Rect,
    /// The view's centre on screen.
    centre: Pos2,
}

/// The most flowers the status bar lists at a block.
const STATUS_FLOWERS: usize = 3;

/// What bone meal grows at a block, for the status bar: the flower if it's certain, else the
/// likeliest few with their chances in spyglass's biome, and whether a tie could change it.
fn flower_summary(answer: &spyglass::FlowerAt) -> String {
    if let Some(flower) = answer.certain() {
        return format!("{} (certain)", spyglass::display_name(flower));
    }
    let first = answer.first();
    if first.flowers.is_empty() {
        return "No flower".to_string();
    }
    // A flower's states together (pink petals' counts), in the order they first come.
    let mut blocks: Vec<&str> = Vec::new();
    for f in &first.flowers {
        let block = f
            .flower
            .split_once('[')
            .map_or(f.flower.as_str(), |(id, _)| id);
        if !blocks.contains(&block) {
            blocks.push(block);
        }
    }
    let mut parts: Vec<String> = blocks
        .iter()
        .take(STATUS_FLOWERS)
        .map(|block| {
            let chance = first.chance_of(block).as_f64();
            format!("{} {:.0}%", spyglass::display_name(block), 100.0 * chance)
        })
        .collect();
    if blocks.len() > STATUS_FLOWERS {
        parts.push(format!("{} more", blocks.len() - STATUS_FLOWERS));
    }
    let tied = if answer.biomes.len() > 1 {
        " (tied)"
    } else {
        ""
    };
    format!("{}{tied}", parts.join(", "))
}

/// The hatch behind tiles not drawn yet: lines at 45°, each from the bottom edge to the top,
/// starting a height to the left so the bottom-left corner is covered too.
fn draw_hatch(painter: &egui::Painter, rect: Rect) {
    let mut x = -rect.height();
    while x < rect.width() {
        painter.line_segment(
            [
                Pos2::new(rect.left() + x, rect.bottom()),
                Pos2::new(rect.left() + x + rect.height(), rect.top()),
            ],
            egui::Stroke::new(1.0, HATCH),
        );
        x += HATCH_GAP;
    }
}

/// The whole of a texture, in texture coordinates.
pub(super) fn full_uv() -> Rect {
    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0))
}

/// The library's image as egui's.
pub(super) fn color_image(image: &Image) -> egui::ColorImage {
    let [w, h] = image.size();
    egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], image.rgba())
}

/// `x z` or `x y z`, in blocks, separated by spaces or commas.
pub(super) fn parse_goto(text: &str) -> Option<(i32, Option<i32>, i32)> {
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
