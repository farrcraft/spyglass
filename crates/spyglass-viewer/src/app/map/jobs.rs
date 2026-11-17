//! The map's jobs: finished ones into their caches, the ones a frame wants asked for, and the
//! tiles drawn from what's cached.

use super::*;

impl App {
    /// Finished jobs into their caches; those for another world, seed or selection are
    /// dropped. Then each cache starts a new frame.
    pub(super) fn take_finished(&mut self, source: u64, selection: u64) {
        for (finished, result) in self.pool.finished() {
            match (finished, result) {
                (JobKey::Biomes(tile_key), result) if tile_key.source == source => {
                    let data = result.and_then(|out| match out {
                        Output::Biomes(data) => Ok(*data),
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
    }

    /// Asks the pool for the jobs `tiles` still need, in order: the coarse tiles over the
    /// view's missing ones first (a sixteenth of the pixels each, and no heights past
    /// `view::MAX_COARSE_TERRAIN_SCALE`, so the view fills in seconds); then the view's own
    /// biomes and markers, each nearest the centre first; then the layouts of the structures
    /// certainly starting in view, up close; and features last, since each tile takes seconds.
    /// Returns the layouts' keys, each with its structure's set.
    pub(super) fn request_jobs(
        &mut self,
        frame: &Frame,
        world: &World,
        session: &Arc<Session<'static>>,
    ) -> Vec<(LayoutKey, String)> {
        let job = |key| Job {
            key,
            session: session.clone(),
            structures: self.selection.queries.clone(),
            features: self.selection.features.clone(),
            loot: self.selection.loot.clone(),
            all_containers: self.settings.containers,
        };
        let tiles = &frame.tiles;
        let missing: Vec<Tile> = tiles
            .iter()
            .copied()
            .filter(|&t| !self.cache.contains(&frame.key(t)))
            .collect();
        let mut jobs: Vec<Job> = view::coarse(&missing)
            .into_iter()
            .chain(missing.iter().copied())
            .map(|t| frame.key(t))
            .filter(|k| !self.cache.contains(k))
            .map(|k| job(JobKey::Biomes(k)))
            .collect();
        if frame.marking {
            jobs.extend(
                tiles
                    .iter()
                    .map(|&t| frame.marker_key(t))
                    .filter(|k| !self.markers.contains(k))
                    .map(|k| job(JobKey::Markers(k))),
            );
        }
        let mut layout_keys: Vec<(LayoutKey, String)> = Vec::new();
        if frame.marking && frame.confirm && zoom::BOUNDS.shows(frame.view.scale) {
            for &tile in tiles {
                let Some(Ok(found)) = self.markers.get(&frame.marker_key(tile)) else {
                    continue;
                };
                for start in found.iter() {
                    let index = start
                        .starting()
                        .and_then(|s| world.structures().position(|w| w == s));
                    if let Some(structure) = index {
                        let key = LayoutKey {
                            source: frame.source,
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
        if frame.featuring {
            jobs.extend(
                frame
                    .regions
                    .iter()
                    .map(|&r| frame.feature_key(r))
                    .filter(|k| !self.feature_markers.contains(k))
                    .map(|k| job(JobKey::Features(k))),
            );
        }
        if frame.looting {
            jobs.extend(
                frame
                    .regions
                    .iter()
                    .map(|&r| frame.feature_key(r))
                    .filter(|k| !self.loot_markers.contains(k))
                    .map(|k| job(JobKey::Loot(k))),
            );
        }
        self.pool.want(jobs);
        layout_keys
    }

    /// Draws `tiles`, each with a coarser one scaled up in its place until it's ready, making
    /// textures within the frame's budget. Returns how many were drawn at all, and how many at
    /// their own scale.
    pub(super) fn draw_tiles(
        &mut self,
        ui: &egui::Ui,
        painter: &egui::Painter,
        frame: &Frame,
        world: &'static World,
        centre: Pos2,
    ) -> [usize; 2] {
        let (view, tiles) = (frame.view, &frame.tiles);
        let style = self.style();
        let mut made = 0;
        let (mut covered, mut detailed) = (0, 0);
        for &tile in tiles {
            let at = view.tile_offset(tile);
            let tile_rect =
                Rect::from_min_size(centre + Vec2::new(at[0], at[1]), Vec2::splat(TILE as f32));
            if let Some(texture) = self.texture(ui.ctx(), world, frame.key(tile), &style, &mut made)
            {
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
                if let Some(texture) =
                    self.texture(ui.ctx(), world, frame.key(parent), &style, &mut made)
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
        [covered, detailed]
    }

    /// The flowers the view's own tiles show, for the Layers tab's key: each tile's key summed,
    /// whole tiles, so the edges count a little past the view. Tiles still loading count as
    /// nothing.
    pub(super) fn count_flowers(&mut self, frame: &Frame) {
        self.flowers_in_view.clear();
        self.flowers_in_view_pixels = 0;
        self.flower_error = None;
        if !frame.flowers {
            return;
        }
        let tile_pixels = u64::from(TILE) * u64::from(TILE);
        for &tile in &frame.tiles {
            let Some(entry) = self.cache.get(&frame.key(tile)) else {
                continue;
            };
            match &entry.data {
                Ok(TileData {
                    flowers: Some(Ok(flowers)),
                    ..
                }) => {
                    self.flowers_in_view_pixels += tile_pixels;
                    for count in &flowers.key {
                        match self
                            .flowers_in_view
                            .iter_mut()
                            .find(|f| f.flower == count.flower)
                        {
                            Some(f) => {
                                f.possible += count.possible;
                                f.certain += count.certain;
                            }
                            None => self.flowers_in_view.push(InView {
                                flower: count.flower.clone(),
                                possible: count.possible,
                                certain: count.certain,
                            }),
                        }
                    }
                }
                Ok(TileData {
                    flowers: Some(Err(e)),
                    ..
                }) => self.flower_error = Some(e.clone()),
                _ => {}
            }
        }
        self.flowers_in_view.sort_by(|a, b| {
            b.possible
                .cmp(&a.possible)
                .then_with(|| a.flower.cmp(&b.flower))
        });
    }

    /// For viewer-timing: when the view is first covered, and first fully drawn, since what's
    /// shown last changed. `drawn` is [`draw_tiles`](Self::draw_tiles)'s count.
    pub(super) fn note_timing(
        &mut self,
        frame: &Frame,
        layout_keys: &[(LayoutKey, String)],
        [covered, detailed]: [usize; 2],
    ) {
        let tiles = &frame.tiles;
        let marked = !frame.marking
            || tiles
                .iter()
                .all(|&t| self.markers.contains(&frame.marker_key(t)));
        let laid_out = layout_keys.iter().all(|(k, _)| self.layouts.contains(k));
        let view = frame.view;
        let shown = (
            (
                frame.source,
                frame.selection,
                frame.confirm,
                frame.marking,
                frame.relief,
                frame.surface,
                frame.underground,
                frame.flowers,
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
}
