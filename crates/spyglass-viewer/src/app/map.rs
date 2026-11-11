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

/// What the map shows behind tiles not drawn yet: faint diagonal lines, so a gap reads as
/// unfinished. Not a checker, which means tied biomes.
const HATCH: Color32 = Color32::from_rgb(0x2a, 0x2a, 0x2a);
/// Points between the hatch's lines.
const HATCH_GAP: f32 = 14.0;
/// The scale bar's longest length, in points.
const SCALE_BAR: f32 = 120.0;

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
        let mut focus_goto = false;
        // N and P step through the last find's matches.
        let mut step = 0;
        // M adds a marker under the pointer; a view text pasted onto the map opens it.
        let mut add_marker = false;
        let mut pasted = None;
        if ui.memory(|m| m.focused().is_none()) {
            ui.input(|i| {
                add_marker = response.hovered() && i.key_pressed(Key::M);
                for event in &i.events {
                    if let egui::Event::Paste(text) = event
                        && text.contains("spyglass:")
                    {
                        pasted = Some(text.clone());
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
                focus_goto = i.key_pressed(Key::G);
                if i.key_pressed(Key::N) {
                    step = 1;
                } else if i.key_pressed(Key::P) {
                    step = -1;
                }
            });
        }

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, EMPTY);
        // The hatch: lines at 45°, each from the bottom edge to the top, starting a height to
        // the left so the bottom-left corner is covered too.
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
        if let Some(text) = pasted {
            self.open_view_text(ui.ctx(), &text);
        }
        if add_marker && let Some(block) = self.cursor {
            self.add_marker(block);
        }
        let (Some(shown), Some(world)) = (&self.shown, self.world) else {
            return;
        };
        let (source, session, spawn) = (shown.id, shown.session.clone(), shown.spawn);
        // The user's markers here, and what they've marked done.
        let dimension = world.dimension_kind().id();
        let (user, done): (Vec<Marker>, HashSet<Place>) =
            self.here().map_or_else(Default::default, |here| {
                (
                    here.live_markers()
                        .filter(|m| m.dimension == dimension)
                        .cloned()
                        .collect(),
                    here.done
                        .iter()
                        .filter(|d| d.done && d.dimension == dimension)
                        .map(|d| d.place.clone())
                        .collect(),
                )
            });
        let show_done = self.settings.show_done;
        let view = self.settings.view;
        let tiles = view.tiles([rect.width(), rect.height()]);
        // Terrain layers on the view's own tiles, and on the coarser ones shown while they load
        // only where heights are cheap, so the stand-ins fill in quickly.
        let terrain_here = self.terrain_allowed();
        let (relief, surface) = (
            self.settings.relief && terrain_here,
            self.settings.surface && terrain_here,
        );
        // The underground layers only on the view's own tiles: the coarser ones shown while
        // they load would cost as much again.
        let underground = self.underground();
        // The key each tile's biomes are kept under, with the layers that tile can have.
        let key = |tile: Tile| {
            let terrain = tile.has_terrain(view.scale);
            let deep = tile.scale == view.scale && tile.scale <= MAX_UNDERGROUND_SCALE;
            TileKey {
                source,
                y: view.y,
                heights: terrain && (relief || surface),
                surface: terrain && surface,
                underground: if deep {
                    underground
                } else {
                    Underground::default()
                },
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
        // Features only up close: each chunk they could be in is placed.
        let featuring =
            !self.selection.features.is_empty() && view.scale <= spyglass::MAX_FEATURE_SCALE;
        let feature_key = |tile: Tile| FeatureKey {
            source,
            selection,
            tile,
        };
        // Loot containers too, every one or those of the Loot item.
        let looting = (self.settings.containers || self.selection.loot.is_some())
            && view.scale <= spyglass::MAX_FEATURE_SCALE;
        // Finished jobs into their caches; those for another world, seed or selection are
        // dropped.
        for (finished, result) in self.pool.finished() {
            match (finished, result) {
                (JobKey::Biomes(tile_key), result) if tile_key.source == source => {
                    let data = result.and_then(|out| match out {
                        Output::Biomes(data) => Ok(data),
                        _ => Err("not a biome tile".to_string()),
                    });
                    self.cache.insert(tile_key, Entry::new(data));
                }
                (JobKey::Markers(markers_key), result)
                    if markers_key.source == source && markers_key.selection == selection =>
                {
                    let starts = result.and_then(|out| match out {
                        Output::Markers(starts) => Ok(starts),
                        _ => Err("not markers".to_string()),
                    });
                    self.markers.insert(markers_key, starts);
                }
                (JobKey::Features(features_key), result)
                    if features_key.source == source && features_key.selection == selection =>
                {
                    let found = result.and_then(|out| match out {
                        Output::Features(found) => Ok(found),
                        _ => Err("not features".to_string()),
                    });
                    self.feature_markers.insert(features_key, found);
                }
                (JobKey::Loot(loot_key), result)
                    if loot_key.source == source && loot_key.selection == selection =>
                {
                    let marks = result.and_then(|out| match out {
                        Output::Loot(marks) => Ok(marks),
                        _ => Err("not loot".to_string()),
                    });
                    self.loot_markers.insert(loot_key, marks);
                }
                (JobKey::Layout(layout_key), result) if layout_key.source == source => {
                    let layout = result.and_then(|out| match out {
                        Output::Layout(layout) => Ok(layout),
                        _ => Err("not a layout".to_string()),
                    });
                    self.layouts.insert(layout_key, layout);
                }
                _ => {}
            }
        }
        self.cache.next_frame();
        self.markers.next_frame();
        self.feature_markers.next_frame();
        self.loot_markers.next_frame();
        self.layouts.next_frame();
        let job = |key| Job {
            key,
            session: session.clone(),
            structures: self.selection.queries.clone(),
            features: self.selection.features.clone(),
            loot: self.selection.loot.clone(),
            all_containers: self.settings.containers,
        };
        // The coarse tiles over the view's missing ones first: a sixteenth of the pixels each,
        // and no heights past `view::MAX_COARSE_TERRAIN_SCALE`, so the view fills in seconds.
        // Then the view's own biomes and markers, each nearest the centre first.
        let missing: Vec<Tile> = tiles
            .iter()
            .copied()
            .filter(|&t| !self.cache.contains(&key(t)))
            .collect();
        let mut jobs: Vec<Job> = view::coarse(&missing)
            .into_iter()
            .chain(missing.iter().copied())
            .map(key)
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
        // Features last, nearest the centre first: each tile takes seconds.
        if featuring {
            jobs.extend(
                tiles
                    .iter()
                    .map(|&t| feature_key(t))
                    .filter(|k| !self.feature_markers.contains(k))
                    .map(|k| job(JobKey::Features(k))),
            );
        }
        if looting {
            jobs.extend(
                tiles
                    .iter()
                    .map(|&t| feature_key(t))
                    .filter(|k| !self.loot_markers.contains(k))
                    .map(|k| job(JobKey::Loot(k))),
            );
        }
        self.pool.want(jobs);

        let style = self.style();
        let mut made = 0;
        // Tiles drawn at all, and drawn at their own scale.
        let (mut covered, mut detailed) = (0, 0);
        for &tile in &tiles {
            let at = view.tile_offset(tile);
            let tile_rect =
                Rect::from_min_size(centre + Vec2::new(at[0], at[1]), Vec2::splat(TILE as f32));
            if let Some(texture) = self.texture(ui.ctx(), world, key(tile), &style, &mut made) {
                painter.image(texture, tile_rect, full_uv(), Color32::WHITE);
                covered += 1;
                detailed += 1;
                continue;
            }
            // A coarser tile, scaled up, until this one is ready. Each step up, the part of the
            // parent this tile covers halves, and moves to the quarter the child is in: `uv` is
            // that part, in the parent's texture coordinates.
            let mut child = tile;
            let mut uv = full_uv();
            while let Some((parent, [quarter_x, quarter_z])) = child.parent() {
                let half = uv.size() / 2.0;
                uv = Rect::from_min_size(
                    Pos2::new(
                        quarter_x as f32 * 0.5 + uv.min.x * 0.5,
                        quarter_z as f32 * 0.5 + uv.min.y * 0.5,
                    ),
                    half,
                );
                // The coarse tile may not have a texture yet: make one, in the frame's budget.
                if let Some(texture) = self.texture(ui.ctx(), world, key(parent), &style, &mut made)
                {
                    painter.image(texture, tile_rect, uv, Color32::WHITE);
                    covered += 1;
                    break;
                }
                // Three steps up at most, eight times the scale; the stand-ins asked for are two
                // up (`view::COARSE`).
                if parent.scale >= view.scale * 8 {
                    break;
                }
                child = parent;
            }
        }
        if made >= TEXTURES_PER_FRAME {
            ui.ctx().request_repaint();
        }
        // For viewer-timing: when the view is first covered, and first fully drawn, since what's
        // shown last changed.
        if timing::enabled() {
            let marked = !marking || tiles.iter().all(|&t| self.markers.contains(&marker_key(t)));
            let laid_out = layout_keys.iter().all(|(k, _)| self.layouts.contains(k));
            let shown = (
                (
                    source,
                    selection,
                    confirm,
                    marking,
                    relief,
                    surface,
                    underground,
                ),
                (
                    view.centre.map(f64::to_bits),
                    view.scale,
                    view.y,
                    self.map_size,
                ),
            );
            self.fill.frame(
                shown,
                covered == tiles.len(),
                detailed == tiles.len() && marked && laid_out,
            );
        }

        // What the open search hit's constraints were judged in, under everything else.
        if let Some(opened) = &self.opened
            && opened.seed == session.seed()
        {
            for constraint in opened
                .constraints
                .iter()
                .filter(|c| c.dimension == world.dimension_kind())
            {
                draw_area(&painter, &view, centre, constraint.area);
            }
        }

        // Bounds under the markers, over every tile.
        for (key, set) in &layout_keys {
            if let Some(Ok(Some(layout))) = self.layouts.get(key) {
                let bounds = layout.bounds;
                let at = markers::block_rect(
                    &view,
                    centre,
                    [bounds.min[0], bounds.min[2]],
                    [bounds.max[0], bounds.max[2]],
                );
                if rect.intersects(at) {
                    markers::bounds(&painter, at, set);
                }
            }
        }

        // Markers over every tile, so none is hidden under a neighbouring tile. The one under
        // the pointer is found first, so it's drawn large, last, with its name.
        let mut starts: Vec<StructureStart> = Vec::new();
        if marking {
            for &tile in &tiles {
                if let Some(Ok(found)) = self.markers.get(&marker_key(tile)) {
                    starts.extend(found.iter().cloned());
                }
            }
        }
        let mut found_features: Vec<FoundFeature> = Vec::new();
        if featuring {
            for &tile in &tiles {
                if let Some(Ok(found)) = self.feature_markers.get(&feature_key(tile)) {
                    found_features.extend(found.iter().cloned());
                }
            }
        }
        let mut loot_marks: Vec<LootMark> = Vec::new();
        if looting {
            for &tile in &tiles {
                if let Some(Ok(marks)) = self.loot_markers.get(&feature_key(tile)) {
                    loot_marks.extend(marks.iter().cloned());
                }
            }
        }
        self.too_many_markers = starts.len() > MAX_MARKERS;
        if self.too_many_markers {
            starts.clear();
        }
        let size = Size::at(view.scale);
        // Everything that can be picked, with its centre on screen and how far from it the
        // pointer still picks it, in drawing order.
        let mut placed: Vec<(Pos2, f32, Picked<'_>)> = Vec::new();
        let mut in_view: HashMap<String, usize> = HashMap::new();
        for start in &starts {
            let at = markers::pixel_centre(&view, centre, [start.position[0], start.position[2]]);
            if rect.expand(12.0).contains(at) {
                placed.push((at, size.reach(), Picked::Structure(start)));
                for id in &start.structures {
                    *in_view.entry(id.clone()).or_default() += 1;
                }
            }
        }
        self.in_view = in_view;
        for feature in &found_features {
            let at =
                markers::pixel_centre(&view, centre, [feature.position[0], feature.position[2]]);
            if rect.expand(12.0).contains(at) {
                placed.push((at, 11.0, Picked::Feature(feature)));
            }
        }
        for mark in &loot_marks {
            let [x, _, z] = mark.container.position;
            let at = markers::pixel_centre(&view, centre, [x, z]);
            if rect.expand(12.0).contains(at) {
                placed.push((at, 8.0, Picked::Container(mark)));
            }
        }
        let spawn_at = (self.settings.spawn && self.spawn_allowed())
            .then(|| markers::pixel_centre(&view, centre, spawn.map_block()));
        if let Some(at) = spawn_at {
            placed.push((at, 10.0, Picked::Spawn(&spawn)));
        }
        if let Some(Ok(found)) = &self.found {
            for hit in found.hits.iter().chain(&found.nearest) {
                let at = markers::pixel_centre(&view, centre, [hit.x, hit.z]);
                placed.push((at, 10.0, Picked::Hit(hit)));
            }
        }
        for marker in &user {
            let at = markers::pixel_centre(&view, centre, [marker.x, marker.z]);
            if rect.expand(30.0).contains(at) {
                placed.push((at, 11.0, Picked::Marker(marker)));
            }
        }
        if let Some(opened) = &self.opened
            && opened.seed == session.seed()
        {
            for found_match in opened
                .matches
                .iter()
                .filter(|m| m.dimension == world.dimension_kind())
            {
                let at = markers::pixel_centre(&view, centre, found_match.at);
                placed.push((at, 15.0, Picked::Match(found_match)));
            }
        }
        // What's done: dimmed, or left out while done things are hidden.
        let is_done = |what: &Picked<'_>| match what {
            Picked::Marker(m) => m.done,
            other => other.place().is_some_and(|p| done.contains(&p)),
        };
        if !show_done {
            placed.retain(|(_, _, what)| !is_done(what));
        }
        let faded: Vec<bool> = placed.iter().map(|(_, _, what)| is_done(what)).collect();
        let mut dimmed = painter.clone();
        dimmed.multiply_opacity(DONE_OPACITY);
        let painter_for = |i: usize| if faded[i] { &dimmed } else { &painter };
        let hovered = response
            .hover_pos()
            .filter(|_| !response.dragged())
            .and_then(|pointer| {
                markers::pick(
                    pointer,
                    placed
                        .iter()
                        .map(|(at, reach, what)| (*at, *reach, what.clone())),
                )
            })
            .map(|(i, _)| i);
        // Rings first, so the icons they surround stay readable; then icons, the spawn, pins,
        // and the user's markers over everything.
        for (i, (at, _, what)) in placed.iter().enumerate() {
            if let Picked::Match(m) = what {
                markers::ring(painter_for(i), *at, m.certain);
            }
        }
        for (i, (at, _, what)) in placed.iter().enumerate() {
            match what {
                Picked::Structure(start) if Some(i) != hovered => {
                    markers::structure(ui, painter_for(i), *at, start, size, false, faded[i]);
                }
                Picked::Feature(f) if Some(i) != hovered => {
                    markers::feature(painter_for(i), *at, f, false);
                }
                Picked::Container(mark) if Some(i) != hovered => {
                    markers::container(painter_for(i), *at, mark, false);
                }
                Picked::Spawn(_) => markers::spawn(&painter, *at),
                Picked::Hit(hit) => markers::pin(painter_for(i), *at, hit.certain),
                _ => {}
            }
        }
        for (i, (at, _, what)) in placed.iter().enumerate() {
            if let Picked::Marker(m) = what {
                markers::user(
                    painter_for(i),
                    *at,
                    m,
                    size == Size::Full || Some(i) == hovered,
                );
            }
        }
        if let Some(i) = hovered {
            let (at, _, what) = &placed[i];
            match what {
                Picked::Structure(start) => {
                    markers::structure(ui, painter_for(i), *at, start, size, true, faded[i]);
                }
                Picked::Feature(f) => markers::feature(painter_for(i), *at, f, true),
                Picked::Container(mark) => markers::container(painter_for(i), *at, mark, true),
                _ => {}
            }
            response
                .clone()
                .on_hover_ui_at_pointer(|ui| describe(ui, what, faded[i]));
        }
        // What a click or right-click is on, if anything.
        let target = hovered.and_then(|i| match &placed[i].2 {
            Picked::Marker(m) => Some(MenuTarget::Marker(m.id.clone())),
            other => other.place().map(MenuTarget::Place),
        });
        let target_done = hovered.is_some_and(|i| faded[i]);
        if response.clicked()
            && let Some(MenuTarget::Marker(id)) = &target
        {
            self.editing = Some(id.clone());
        }
        // A click on a container opens its contents.
        if response.clicked()
            && let Some(Picked::Container(mark)) = hovered.map(|i| &placed[i].2)
        {
            self.open_container = Some(mark.container.clone());
        }
        self.container_window(ui.ctx());
        self.cursor = response
            .hover_pos()
            .map(|hover| view.block_at(offset(hover)).map(|b| b.floor() as i32));

        let spawn_block = spawn.map_block();
        if step != 0 {
            self.step_match(ui.ctx(), step);
        }
        self.map_controls(ui, rect, spawn_block, focus_goto);
        self.map_legend(ui, rect, world);
        scale_bar(&painter, rect, view.scale);

        if response.secondary_clicked()
            && let Some(p) = response.interact_pointer_pos()
        {
            self.menu_block = Some(view.block_at(offset(p)).map(|b| b.floor() as i32));
            self.menu_target = target.map(|t| (t, target_done));
        }
        let Some([bx, bz]) = self.menu_block else {
            return;
        };
        let menu_target = self.menu_target.clone();
        let mut action = None;
        response.context_menu(|ui| {
            ui.label(style::mono(format!("{bx}, {bz}")).color(style::MUTED));
            match &menu_target {
                Some((MenuTarget::Marker(id), done)) => {
                    if ui.button("Edit marker").clicked() {
                        action = Some(Action::Edit(id.clone()));
                        ui.close();
                    }
                    let text = if *done { "Mark not done" } else { "Mark done" };
                    if ui.button(text).clicked() {
                        action = Some(Action::MarkerDone(id.clone(), !done));
                        ui.close();
                    }
                    if ui.button("Delete marker").clicked() {
                        action = Some(Action::Delete(id.clone()));
                        ui.close();
                    }
                }
                Some((MenuTarget::Place(place), done)) => {
                    let text = if *done { "Mark not done" } else { "Mark done" };
                    if ui.button(text).clicked() {
                        action = Some(Action::Done(place.clone(), !done));
                        ui.close();
                    }
                }
                None => {}
            }
            if ui
                .button("Add a marker here")
                .on_hover_text("Or press M")
                .clicked()
            {
                action = Some(Action::AddMarker);
                ui.close();
            }
            ui.separator();
            ui.menu_button("Nearest structure", |ui| {
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .show(ui, |ui| {
                        let mut ids: Vec<&str> = world
                            .structures()
                            .filter(|id| world.structure_generates(id))
                            .collect();
                        ids.sort_unstable_by_key(|id| spyglass::display_name(id));
                        for id in ids {
                            if ui.button(spyglass::display_name(id)).clicked() {
                                action = Some(Action::Locate(Target::Structure(id.to_string())));
                                ui.close();
                            }
                        }
                    });
            });
            ui.menu_button("Nearest feature", |ui| {
                for (name, placed) in icons::FEATURE_KINDS {
                    if ui.button(*name).clicked() {
                        action = Some(Action::Locate(Target::Feature {
                            name: name.to_string(),
                            placed: placed.iter().map(|p| p.to_string()).collect(),
                        }));
                        ui.close();
                    }
                }
            });
            if let Some(query) = &self.selection.loot {
                let text = format!("Nearest {}", spyglass::display_name(query.argument()));
                if ui
                    .button(text)
                    .on_hover_text("The nearest container holding the Loot item")
                    .clicked()
                {
                    action = Some(Action::Locate(Target::Loot(query.argument().to_string())));
                    ui.close();
                }
            }
            ui.menu_button("Nearest biome", |ui| {
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .show(ui, |ui| {
                        let mut ids: Vec<&String> = world.biomes().iter().collect();
                        ids.sort_unstable_by_key(|id| spyglass::display_name(id));
                        for id in ids {
                            if ui.button(spyglass::display_name(id)).clicked() {
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
            if ui
                .button("Copy the view text")
                .on_hover_text("A line that opens this view in another spyglass")
                .clicked()
            {
                action = Some(Action::CopyView);
                ui.close();
            }
        });
        match action {
            Some(Action::Locate(target)) => {
                self.locate(ui.ctx(), target, [bx, view.y, bz]);
                self.tab = Tab::Locate;
            }
            Some(Action::Centre) => {
                self.settings.view.centre = [f64::from(bx) + 0.5, f64::from(bz) + 0.5];
            }
            Some(Action::AddMarker) => self.add_marker([bx, bz]),
            Some(Action::Edit(id)) => self.editing = Some(id),
            Some(Action::MarkerDone(id, done)) => self.change_marker(&id, |m| m.done = done),
            Some(Action::Delete(id)) => {
                self.change_marker(&id, Marker::delete);
                if self.editing.as_ref() == Some(&id) {
                    self.editing = None;
                }
            }
            Some(Action::Done(place, done)) => self.set_done(&place, done),
            Some(Action::CopyView) => self.copy_view(ui.ctx()),
            None => {}
        }
    }

    /// The controls over the map: go to (top left), the open search hit's card under it, and
    /// centre on spawn and zoom in a row (top right).
    fn map_controls(&mut self, ui: &mut egui::Ui, rect: Rect, spawn: [i32; 2], focus_goto: bool) {
        let inner = rect.shrink(12.0);
        let mut step = 0;
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(egui::Layout::top_down(egui::Align::Min)),
            |ui| {
                // The field is the whole box, its text centred in it; G focuses it.
                style::overlay()
                    .inner_margin(egui::Margin::same(0))
                    .show(ui, |ui| {
                        let edit = ui
                            .add(
                                egui::TextEdit::singleline(&mut self.goto)
                                    .font(egui::TextStyle::Monospace)
                                    .hint_text("Go to x z  or  x y z")
                                    // The field's padding is its frame's, so an empty frame
                                    // with a margin.
                                    .frame(
                                        egui::Frame::NONE
                                            .inner_margin(egui::Margin::symmetric(14, 0)),
                                    )
                                    .align(egui::Align2::LEFT_CENTER)
                                    .min_size(egui::vec2(240.0, 34.0))
                                    .desired_width(240.0),
                            )
                            .on_hover_text("Press G to type a position, then Enter");
                        if focus_goto {
                            edit.request_focus();
                        }
                        if edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                            match parse_goto(&self.goto) {
                                Some((x, y, z)) => {
                                    self.settings.view.centre =
                                        [f64::from(x) + 0.5, f64::from(z) + 0.5];
                                    if let Some(y) = y {
                                        self.settings.view.y = y;
                                    }
                                    self.goto_error = false;
                                }
                                None => self.goto_error = true,
                            }
                        }
                        if self.goto_error {
                            ui.add(egui::Label::new(
                                egui::RichText::new("Type x z or x y z, in blocks.")
                                    .size(12.0)
                                    .color(style::ERROR),
                            ));
                            ui.add_space(6.0);
                        }
                    });
                if let Some(opened) = &self.opened
                    && self
                        .shown
                        .as_ref()
                        .is_some_and(|s| s.session.seed() == opened.seed)
                {
                    ui.add_space(8.0);
                    step = hit_card(ui, opened);
                }
            },
        );
        if step != 0 {
            self.step_match(ui.ctx(), step);
        }
        // Top right, in one row: centre on spawn, then zoom as − 1:N +. Right to left, so the
        // zoom is in the corner.
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(egui::Layout::right_to_left(egui::Align::Min)),
            |ui| {
                // Each part as wide as its text plus a little padding, so the control is as
                // wide as the scale it shows.
                const HEIGHT: f32 = 28.0;
                const PAD: f32 = 7.0;
                let text_width = |text: &str, font: egui::FontId| {
                    ui.painter()
                        .layout_no_wrap(text.to_string(), font, style::TEXT)
                        .size()
                        .x
                };
                let symbol = egui::FontId::proportional(17.0);
                let number = egui::FontId::monospace(11.5);
                let scale_text = format!("1:{}", self.settings.view.scale);
                let minus = egui::vec2(text_width("−", symbol.clone()) + 2.0 * PAD, HEIGHT);
                let plus = egui::vec2(text_width("+", symbol.clone()) + 2.0 * PAD, HEIGHT);
                let scale = egui::vec2(text_width(&scale_text, number.clone()) + PAD, HEIGHT);
                let button = egui::vec2(HEIGHT, HEIGHT);
                style::overlay()
                    .inner_margin(egui::Margin::same(4))
                    .show(ui, |ui| {
                        ui.allocate_ui_with_layout(
                            egui::vec2(minus.x + scale.x + plus.x, HEIGHT),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                let view = &mut self.settings.view;
                                let zoom = |ui: &mut egui::Ui, text: &str, size, hint: &str| {
                                    let text = egui::RichText::new(text).font(symbol.clone());
                                    ui.add(egui::Button::new(text).frame(false).min_size(size))
                                        .on_hover_text(hint)
                                        .clicked()
                                };
                                if zoom(ui, "−", minus, "Zoom out (−)") {
                                    view.zoom(false, [0.0, 0.0]);
                                }
                                ui.add_sized(
                                    scale,
                                    egui::Label::new(
                                        egui::RichText::new(scale_text).font(number.clone()),
                                    ),
                                )
                                .on_hover_text(format!(
                                    "1 pixel = {} block{}",
                                    view.scale,
                                    if view.scale == 1 { "" } else { "s" }
                                ));
                                if zoom(ui, "+", plus, "Zoom in (+)") {
                                    view.zoom(true, [0.0, 0.0]);
                                }
                            },
                        );
                    });
                style::overlay()
                    .inner_margin(egui::Margin::same(4))
                    .show(ui, |ui| {
                        let (button_rect, response) =
                            ui.allocate_exact_size(button, Sense::click());
                        if response.hovered() {
                            ui.painter().rect_filled(button_rect, 5.0, style::RAISED);
                        }
                        markers::spawn_sized(ui.painter(), button_rect.center(), 7.0);
                        if response.on_hover_text("Centre on the spawn").clicked() {
                            self.settings.view.centre =
                                [f64::from(spawn[0]) + 0.5, f64::from(spawn[1]) + 0.5];
                        }
                    });
            },
        );
    }

    /// What's marked, bottom right: the structure families chosen, highlighted biomes and the
    /// spawn. Painted, not laid out, so it takes only the room its rows need.
    fn map_legend(&self, ui: &egui::Ui, rect: Rect, world: &World) {
        /// A legend row's swatch.
        enum Key<'a> {
            /// A structure's family, by its first structure id.
            Structure(&'a str),
            /// A highlighted biome, by id.
            Biome(&'a str),
            /// The world spawn.
            Spawn,
            /// Families past the first few, unlisted.
            More,
        }
        const MAX_ROWS: usize = 8;
        // One row per family chosen, with the first of its structures for the icon.
        let mut families: Vec<(String, &str)> = Vec::new();
        for id in self
            .settings
            .structures
            .iter()
            .filter(|id| world.structure_generates(id))
        {
            let name = icons::family_name(id);
            if !families.iter().any(|(n, _)| *n == name) {
                families.push((name, id));
            }
        }
        families.sort();
        let mut rows: Vec<(Key<'_>, String)> = Vec::new();
        let more = families.len().saturating_sub(MAX_ROWS);
        for (name, id) in families.into_iter().take(MAX_ROWS) {
            rows.push((Key::Structure(id), name));
        }
        if more > 0 {
            rows.push((Key::More, format!("and {more} more")));
        }
        for id in self.settings.highlight.iter().take(MAX_ROWS) {
            rows.push((Key::Biome(id), spyglass::display_name(id)));
        }
        if self.settings.spawn && self.spawn_allowed() {
            rows.push((Key::Spawn, "World spawn".to_string()));
        }
        if rows.is_empty() {
            return;
        }
        let painter = ui.painter_at(rect);
        let font = egui::FontId::proportional(12.5);
        let galleys: Vec<_> = rows
            .iter()
            .map(|(_, text)| painter.layout_no_wrap(text.clone(), font.clone(), style::TEXT))
            .collect();
        let (swatch, gap, row, pad) = (14.0, 8.0, 20.0, 10.0);
        let width = galleys.iter().map(|g| g.size().x).fold(0.0, f32::max) + swatch + gap;
        let size = Vec2::new(width + 2.0 * pad, row * rows.len() as f32 + 2.0 * pad - 6.0);
        let frame = Rect::from_min_size(rect.right_bottom() - Vec2::splat(12.0) - size, size);
        painter.rect(
            frame,
            8.0,
            style::OVERLAY,
            egui::Stroke::new(1.0, style::BORDER),
            egui::StrokeKind::Inside,
        );
        for (i, ((key, _), galley)) in rows.iter().zip(galleys).enumerate() {
            let top = frame.top() + pad + row * i as f32;
            let swatch_rect =
                Rect::from_min_size(Pos2::new(frame.left() + pad, top), Vec2::splat(swatch));
            match key {
                Key::Structure(id) => {
                    if let Some(badge) = world.structure_set(id).map(icons::badge) {
                        painter.rect(
                            swatch_rect,
                            egui::CornerRadius::same(4),
                            badge.fill,
                            egui::Stroke::new(1.0, style::GROUND),
                            egui::StrokeKind::Inside,
                        );
                        icons::glyph(id)
                            .tint(badge.ink)
                            .paint_at(ui, swatch_rect.shrink(2.5));
                    }
                }
                Key::Biome(id) => {
                    let [red, green, blue] = spyglass::biome_colour(id);
                    painter.rect_filled(
                        swatch_rect.shrink(1.5),
                        2.0,
                        Color32::from_rgb(red, green, blue),
                    );
                }
                Key::Spawn => markers::spawn_sized(&painter, swatch_rect.center(), 6.5),
                Key::More => {}
            }
            let colour = if matches!(key, Key::More) {
                style::MUTED
            } else {
                style::TEXT
            };
            let at = Pos2::new(
                swatch_rect.right() + gap,
                swatch_rect.center().y - galley.size().y / 2.0,
            );
            painter.galley(at, galley, colour);
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

/// The open search hit or find match: its seed, and each constraint with its outcome. Returns
/// 1 or −1 when the next or previous match is asked for.
fn hit_card(ui: &mut egui::Ui, opened: &Opened) -> isize {
    let mut step = 0;
    style::overlay().show(ui, |ui| {
        ui.set_max_width(460.0);
        ui.horizontal(|ui| {
            ui.strong(&opened.title);
            ui.label(style::mono(opened.seed.to_string()).color(style::MUTED));
            if ui.small_button("Copy seed").clicked() {
                ui.ctx().copy_text(opened.seed.to_string());
            }
            if let Some((at, count)) = opened.step {
                if ui
                    .add_enabled(at > 0, egui::Button::new("Previous").small())
                    .on_hover_text("Previous (P)")
                    .clicked()
                {
                    step = -1;
                }
                if ui
                    .add_enabled(at + 1 < count, egui::Button::new("Next").small())
                    .on_hover_text("Next (N)")
                    .clicked()
                {
                    step = 1;
                }
            }
        });
        for constraint in &opened.constraints {
            ui.horizontal(|ui| {
                let (mark, colour, what) = match constraint.outcome {
                    Outcome::Met => ("Met", style::GOOD, "It holds whatever is undecided."),
                    Outcome::PossiblyMet => (
                        "Maybe",
                        style::WARN,
                        "It holds only in the best case: it depends on terrain, a tied biome or \
                         the platform.",
                    ),
                    Outcome::Failed => ("No", style::MUTED, "It doesn't hold."),
                };
                ui.add_sized(
                    egui::vec2(40.0, 16.0),
                    egui::Label::new(egui::RichText::new(mark).size(11.5).strong().color(colour)),
                )
                .on_hover_text(what);
                ui.add(
                    egui::Label::new(search_panel::highlight(&constraint.line, 12.0)).truncate(),
                );
            });
        }
    });
    step
}

/// A constraint's area: a dashed disc or box in the accent colour.
fn draw_area(painter: &egui::Painter, view: &View, centre: Pos2, area: Area) {
    let stroke = egui::Stroke::new(2.0, style::ACCENT);
    let fill = style::ACCENT.gamma_multiply(0.07);
    let points: Vec<Pos2> = match area {
        Area::Disc { centre: at, radius } => {
            let disc_centre = markers::pixel_centre(view, centre, at);
            let radius_points = radius as f32 / view.scale as f32;
            painter.circle_filled(disc_centre, radius_points, fill);
            // A point every two points of the edge or so, so a large disc stays round.
            let steps = ((radius_points * 0.5) as usize).clamp(24, 256);
            (0..=steps)
                .map(|i| {
                    disc_centre
                        + Vec2::angled(std::f32::consts::TAU * i as f32 / steps as f32)
                            * radius_points
                })
                .collect()
        }
        Area::Box { min, max } => {
            let box_rect = markers::block_rect(view, centre, min, max);
            painter.rect_filled(box_rect, 0.0, fill);
            vec![
                box_rect.left_top(),
                box_rect.right_top(),
                box_rect.right_bottom(),
                box_rect.left_bottom(),
                box_rect.left_top(),
            ]
        }
    };
    painter.extend(egui::Shape::dashed_line(&points, stroke, 8.0, 6.0));
}

/// A scale bar, bottom left: a round number of blocks.
fn scale_bar(painter: &egui::Painter, rect: Rect, scale: u32) {
    let (blocks, length) = bar_length(scale, SCALE_BAR);
    let text = format!("{} blocks", thousands(blocks));
    let galley = painter.layout_no_wrap(text, egui::FontId::monospace(11.0), style::TEXT);
    let width = length.max(galley.size().x);
    let frame = Rect::from_min_size(
        Pos2::new(rect.left() + 12.0, rect.bottom() - 12.0 - 30.0),
        Vec2::new(width + 20.0, 30.0),
    );
    painter.rect_filled(frame, 6.0, style::OVERLAY);
    let y = frame.top() + 9.0;
    let (x0, x1) = (frame.left() + 10.0, frame.left() + 10.0 + length);
    let line = egui::Stroke::new(2.0, style::TEXT);
    painter.line_segment([Pos2::new(x0, y), Pos2::new(x1, y)], line);
    painter.line_segment([Pos2::new(x0, y - 4.0), Pos2::new(x0, y)], line);
    painter.line_segment([Pos2::new(x1, y - 4.0), Pos2::new(x1, y)], line);
    painter.galley(Pos2::new(x0, y + 4.0), galley, style::TEXT);
}

/// A scale bar's length at `scale` blocks per point: the largest 1, 2 or 5 times a power of ten
/// blocks at most `max` points long, and its length in points.
fn bar_length(scale: u32, max: f32) -> (u32, f32) {
    let mut best = 1;
    let mut step: u32 = 1;
    'search: loop {
        for multiple in [1, 2, 5] {
            let Some(blocks) = step.checked_mul(multiple) else {
                break 'search;
            };
            if blocks as f32 / scale as f32 > max {
                break 'search;
            }
            best = blocks;
        }
        let Some(next) = step.checked_mul(10) else {
            break;
        };
        step = next;
    }
    (best, best as f32 / scale as f32)
}

/// `n` with commas between thousands.
fn thousands(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

impl App {
    /// The open container's contents, slot by slot, in a window until it's closed.
    fn container_window(&mut self, ctx: &egui::Context) {
        let Some(c) = &self.open_container else {
            return;
        };
        let [x, y, z] = c.position;
        let mut open = true;
        egui::Window::new(format!(
            "{} at {x} {y} {z}",
            spyglass::display_name(&c.holder)
        ))
        .open(&mut open)
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.label(egui::RichText::new(&c.table).color(style::MUTED));
            if let Some(seed) = c.seed {
                ui.label(style::mono(format!("Loot seed {seed}")).color(style::MUTED));
            }
            if c.contents_certain() {
                ui.colored_label(style::GOOD, "Certain, contents included");
            } else if let Some(why) = c.reason.as_ref().or(c.contents_doubt.as_ref()) {
                ui.label(format!("Not certain: {why}."));
            }
            ui.separator();
            match &c.contents {
                None => {
                    ui.label("Its contents aren't known.");
                }
                Some(slots) if slots.is_empty() => {
                    ui.label("Empty");
                }
                Some(slots) => {
                    egui::Grid::new("container slots")
                        .striped(true)
                        .show(ui, |ui| {
                            for s in slots {
                                ui.label(style::mono(format!("{:>2}", s.slot)).color(style::MUTED));
                                for line in contents_lines(std::slice::from_ref(s)) {
                                    ui.label(line);
                                }
                                ui.end_row();
                            }
                        });
                }
            }
            ui.label(
                egui::RichText::new(
                    "As the seed generates it: a chest already opened in a world may hold \
                         something else.",
                )
                .color(style::MUTED),
            );
        });
        if !open {
            self.open_container = None;
        }
    }
}

/// What the map's context menu asked for.
pub(super) enum Action {
    /// Find the nearest of something from the block clicked.
    Locate(Target),
    /// Centre the map on the block clicked.
    Centre,
    /// Add a marker at the block clicked.
    AddMarker,
    /// Edit the marker with this id.
    Edit(String),
    /// Mark the marker with this id done or not.
    MarkerDone(String, bool),
    /// Delete the marker with this id.
    Delete(String),
    /// Mark a place done or not.
    Done(Place, bool),
    /// Copy the view text.
    CopyView,
}

/// A marker's tooltip; `done` if it's marked done.
pub(super) fn describe(ui: &mut egui::Ui, picked: &Picked<'_>, done: bool) {
    if done {
        ui.colored_label(style::GOOD, "Done");
    }
    match picked {
        Picked::Structure(start) => {
            let [x, _, z] = start.position;
            let names: Vec<String> = start
                .structures
                .iter()
                .map(|s| spyglass::display_name(s))
                .collect();
            ui.strong(names.join(" or "));
            ui.label(style::mono(format!(
                "{x}, {z} · chunk {}, {}",
                start.chunk[0], start.chunk[1]
            )));
            ui.label(egui::RichText::new(&start.set).color(style::MUTED));
            if start.certain {
                ui.colored_label(style::GOOD, "Certain");
            } else {
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
        Picked::Feature(f) => {
            let [x, y, z] = f.position;
            ui.strong(icons::feature_name(&f.feature));
            ui.label(style::mono(format!("{x} {y} {z}")));
            ui.label(egui::RichText::new(&f.feature).color(style::MUTED));
            if f.certain {
                ui.colored_label(style::GOOD, "Certain");
            } else {
                ui.label("Likely: it places on spyglass's blocks, but something spyglass can't know could change that.");
            }
            if let Some(reason) = &f.reason {
                ui.label(egui::RichText::new(reason).color(style::MUTED));
            }
            ui.label(
                egui::RichText::new("For the chunk decorated before its neighbours.")
                    .color(style::MUTED),
            );
        }
        Picked::Container(mark) => {
            let c = &mark.container;
            let [x, y, z] = c.position;
            ui.strong(spyglass::display_name(&c.holder));
            ui.label(style::mono(format!("{x} {y} {z}")));
            let by = match &c.source {
                spyglass::ContainerSource::Structure(id)
                | spyglass::ContainerSource::Feature(id) => spyglass::display_name(id),
                _ => String::new(),
            };
            ui.label(format!("In {by}"));
            ui.label(egui::RichText::new(&c.table).color(style::MUTED));
            if c.contents_certain() {
                ui.colored_label(style::GOOD, "Certain, contents included");
            } else if c.certain {
                ui.label("Certain, but its contents may not be the game's.");
            } else {
                ui.label("Candidate: something spyglass can't know could change it.");
            }
            if let Some(reason) = c.reason.as_ref().or(c.contents_doubt.as_ref()) {
                ui.label(egui::RichText::new(reason).color(style::MUTED));
            }
            if let Some(slots) = &c.contents {
                ui.separator();
                for line in contents_lines(slots).iter().take(12) {
                    ui.label(line);
                }
                ui.label(egui::RichText::new("Click to keep it open").color(style::MUTED));
            }
        }
        Picked::Hit(hit) => {
            ui.strong(&hit.what);
            let y = hit.y.map_or("~".to_string(), |y| y.to_string());
            ui.label(style::mono(format!(
                "{} {y} {} · {} blocks away",
                hit.x, hit.z, hit.blocks_away
            )));
            if let Some(answer) = hit.answer {
                ui.label(answer);
            }
            if !hit.certain {
                ui.label("Candidate: the game's answer is the first of these it confirms.");
            }
        }
        Picked::Match(m) => {
            ui.strong(&m.what);
            ui.label(style::mono(format!("{}, {}", m.at[0], m.at[1])));
            ui.label(egui::RichText::new(&m.constraint).color(style::MUTED));
            if !m.certain {
                ui.label("Possible: it depends on terrain, a tied biome or the platform.");
            }
        }
        Picked::Marker(m) => {
            if m.name.is_empty() {
                ui.strong("Marker");
            } else {
                ui.strong(&m.name);
            }
            ui.label(style::mono(format!("{}, {}", m.x, m.z)));
            ui.label(egui::RichText::new("Click to edit").color(style::MUTED));
        }
    }
}

/// A container's contents, a line per kind of stack with its total: `3 × Bread`, with the
/// components its table set.
pub(super) fn contents_lines(slots: &[spyglass::Slot]) -> Vec<String> {
    let mut totals: Vec<(&spyglass::Item, i32)> = Vec::new();
    for s in slots {
        let same = |i: &spyglass::Item| {
            let (mut a, mut b) = (i.clone(), s.item.clone());
            (a.count, b.count) = (0, 0);
            a == b
        };
        match totals.iter_mut().find(|(i, _)| same(i)) {
            Some((_, total)) => *total += s.item.count,
            None => totals.push((&s.item, s.item.count)),
        }
    }
    if totals.is_empty() {
        return vec!["Empty".to_string()];
    }
    totals
        .iter()
        .map(|(item, total)| {
            let mut extra: Vec<String> = item
                .enchantments
                .iter()
                .chain(&item.stored_enchantments)
                .map(|(e, level)| format!("{} {level}", spyglass::display_name(e)))
                .collect();
            if let Some(potion) = &item.potion {
                extra.push(spyglass::display_name(potion));
            }
            if let Some(map) = &item.map {
                extra.push(format!("to {} {}", map.target[0], map.target[1]));
            }
            let extra = if extra.is_empty() {
                String::new()
            } else {
                format!(" ({})", extra.join(", "))
            };
            format!("{total} × {}{extra}", spyglass::display_name(&item.id))
        })
        .collect()
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

    #[test]
    fn the_scale_bar_is_a_round_length_at_every_zoom() {
        let mut scale = 1;
        while scale <= spyglass::MAX_SCALE {
            let (blocks, points) = bar_length(scale, SCALE_BAR);
            let mut lead = blocks;
            while lead >= 10 && lead % 10 == 0 {
                lead /= 10;
            }
            assert!(matches!(lead, 1 | 2 | 5), "{blocks} at 1:{scale}");
            // At most the longest, and never less than 40% of it: the next round length up
            // would be at most 2.5 times longer.
            assert!(
                (SCALE_BAR * 0.4..=SCALE_BAR).contains(&points),
                "{points} at 1:{scale}"
            );
            assert_eq!(points, blocks as f32 / scale as f32);
            scale *= 2;
        }
        assert_eq!(bar_length(4, SCALE_BAR), (200, 50.0));
        assert_eq!(thousands(200_000), "200,000");
        assert_eq!(thousands(5), "5");
        assert_eq!(thousands(1000), "1,000");
    }
}
