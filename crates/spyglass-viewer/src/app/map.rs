//! The map: tiles drawn as textures, markers, picking, going to a position, and the status bar.

use super::*;

impl App {
    pub(super) fn map(&mut self, ui: &mut egui::Ui) {
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
    pub(super) fn texture(
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

    pub(super) fn status_bar(&self, ui: &mut egui::Ui) {
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

/// What the map's context menu asked for.
pub(super) enum Action {
    Locate(Target),
    Centre,
}

/// A marker's tooltip.
pub(super) fn describe(ui: &mut egui::Ui, picked: &Picked<'_>) {
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

pub(super) fn full_uv() -> Rect {
    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0))
}

pub(super) fn color_image(image: &Image) -> egui::ColorImage {
    let [w, h] = image.size();
    egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], image.rgba())
}

/// A filtered checklist of ids, with the chosen ones in `chosen`. Returns whether it changed.
pub(super) fn pick_list(
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
