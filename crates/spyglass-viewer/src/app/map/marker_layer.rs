//! The map's markers: what's placed over the tiles, the one under the pointer, and their
//! tooltips.

use super::*;

impl App {
    /// The markers over every tile, so none is hidden under a neighbouring tile: structures,
    /// features, containers, the spawn, pins, the user's markers and the open hit's matches.
    /// The one under the pointer is found first, so it's drawn large, last, with its name.
    /// Returns what the pointer is on.
    pub(super) fn draw_marker_layer(
        &mut self,
        ui: &egui::Ui,
        canvas: &Canvas<'_>,
        frame: &Frame,
        world: &World,
        session: &Session<'static>,
        spawn: &SpawnOrigin,
    ) -> Pointed {
        let (view, centre, rect) = (frame.view, canvas.centre, canvas.rect);
        let (starts, found_features, loot_marks) = self.markers_in_view(frame);
        let (user, done) = self.user_markers(world);
        let size = Size::at(view.scale);
        let spawn_at = (self.settings.spawn && self.spawn_allowed())
            .then(|| markers::pixel_centre(&view, centre, spawn.map_block()));
        let opened = self
            .opened
            .as_ref()
            .filter(|opened| opened.seed == session.seed());
        let mut placed = place_markers(&Placing {
            view: &view,
            centre,
            rect,
            size,
            starts: &starts,
            features: &found_features,
            loot: &loot_marks,
            spawn: spawn_at.map(|at| (at, spawn)),
            found: &self.found,
            user: &user,
            opened,
            dimension: world.dimension_kind(),
        });
        let mut in_view: HashMap<String, usize> = HashMap::new();
        for (_, _, what) in &placed {
            if let Picked::Structure(start) = what {
                for id in &start.structures {
                    *in_view.entry(id.clone()).or_default() += 1;
                }
            }
        }
        // What's done: dimmed, or left out while done things are hidden.
        let is_done = |what: &Picked<'_>| match what {
            Picked::Marker(m) => m.done,
            other => other.place().is_some_and(|p| done.contains(&p)),
        };
        if !self.settings.show_done {
            placed.retain(|(_, _, what)| !is_done(what));
        }
        let faded: Vec<bool> = placed.iter().map(|(_, _, what)| is_done(what)).collect();
        let hovered = canvas
            .response
            .hover_pos()
            .filter(|_| !canvas.response.dragged())
            .and_then(|pointer| {
                markers::pick(
                    pointer,
                    placed
                        .iter()
                        .map(|(at, reach, what)| (*at, *reach, what.clone())),
                )
            })
            .map(|(i, _)| i);
        draw_markers(ui, canvas.painter, &placed, &faded, hovered, size);
        if let Some(i) = hovered {
            let what = &placed[i].2;
            canvas
                .response
                .clone()
                .on_hover_ui_at_pointer(|ui| describe(ui, what, faded[i]));
        }
        // What a click or right-click is on, if anything.
        let target = hovered.and_then(|i| match &placed[i].2 {
            Picked::Marker(m) => Some(MenuTarget::Marker(m.id.clone())),
            other => other.place().map(MenuTarget::Place),
        });
        let target_done = hovered.is_some_and(|i| faded[i]);
        let clicked_container = hovered.and_then(|i| match &placed[i].2 {
            Picked::Container(mark) => Some(mark.container.clone()),
            _ => None,
        });
        drop(placed);
        self.in_view = in_view;
        Pointed {
            target: target.map(|t| (t, target_done)),
            container: clicked_container,
        }
    }

    /// The user's markers in the dimension shown, and the places they've marked done there.
    pub(super) fn user_markers(&self, world: &World) -> (Vec<Marker>, HashSet<Place>) {
        let dimension = world.dimension_kind().id();
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
        })
    }

    /// Under the markers, over every tile: the areas the open search hit's constraints were
    /// judged in, under everything else, then the bounds of the structures laid out.
    #[expect(
        clippy::too_many_arguments,
        reason = "the frame's own values, each drawn from"
    )]
    pub(super) fn draw_areas_and_bounds(
        &mut self,
        painter: &egui::Painter,
        frame: &Frame,
        centre: Pos2,
        rect: Rect,
        world: &World,
        session: &Session<'static>,
        layout_keys: &[(LayoutKey, String)],
    ) {
        let view = frame.view;
        if let Some(opened) = &self.opened
            && opened.seed == session.seed()
        {
            for constraint in opened
                .constraints
                .iter()
                .filter(|c| c.dimension == world.dimension_kind())
            {
                draw_area(painter, &view, centre, constraint.area);
            }
        }
        for (key, set) in layout_keys {
            if let Some(Ok(Some(layout))) = self.layouts.get(key) {
                let bounds = layout.bounds;
                let at = markers::block_rect(
                    &view,
                    centre,
                    [bounds.min[0], bounds.min[2]],
                    [bounds.max[0], bounds.max[2]],
                );
                if rect.intersects(at) {
                    markers::bounds(painter, at, set);
                }
            }
        }
    }

    /// The structure starts, features and loot containers the caches hold for the view's
    /// regions. Past [`MAX_MARKERS`] starts on the view's tiles, none are drawn, and the map
    /// says so.
    pub(super) fn markers_in_view(
        &mut self,
        frame: &Frame,
    ) -> (Vec<StructureStart>, Vec<FoundFeature>, Vec<LootMark>) {
        let mut starts: Vec<StructureStart> = Vec::new();
        if frame.marking {
            for &region in &frame.start_regions {
                if let Some(Ok(found)) = self.markers.get(&frame.marker_key(region)) {
                    starts.extend(found.iter().cloned());
                }
            }
        }
        let mut found_features: Vec<FoundFeature> = Vec::new();
        if frame.featuring {
            for &region in &frame.regions {
                if let Some(Ok(found)) = self.feature_markers.get(&frame.feature_key(region)) {
                    found_features.extend(found.iter().cloned());
                }
            }
        }
        let mut loot_marks: Vec<LootMark> = Vec::new();
        if frame.looting {
            for &region in &frame.regions {
                if let Some(Ok(marks)) = self.loot_markers.get(&frame.feature_key(region)) {
                    loot_marks.extend(marks.iter().cloned());
                }
            }
        }
        // Counted on the tiles alone, so the ring of regions around them doesn't count.
        let on_tiles = view::blocks_under(&frame.tiles).map_or(0, |(lo, hi)| {
            let inside = |s: &&StructureStart| {
                [s.position[0], s.position[2]]
                    .into_iter()
                    .enumerate()
                    .all(|(axis, b)| (lo[axis]..=hi[axis]).contains(&i64::from(b)))
            };
            starts.iter().filter(inside).count()
        });
        self.too_many_markers = on_tiles > MAX_MARKERS;
        if self.too_many_markers {
            starts.clear();
        }
        (starts, found_features, loot_marks)
    }
}

/// What the pointer is on: a click's or right-click's target (and whether it's done), and a
/// container a click opens.
pub(super) struct Pointed {
    pub(super) target: Option<(MenuTarget, bool)>,
    pub(super) container: Option<FoundContainer>,
}

/// Everything [`place_markers`] places.
pub(super) struct Placing<'a> {
    view: &'a View,
    centre: Pos2,
    rect: Rect,
    size: Size,
    starts: &'a [StructureStart],
    features: &'a [FoundFeature],
    loot: &'a [LootMark],
    /// The spawn, where it's shown, and where on screen.
    spawn: Option<(Pos2, &'a SpawnOrigin)>,
    found: &'a Option<Result<Found, String>>,
    user: &'a [Marker],
    /// The open search hit, if it's for the seed shown.
    opened: Option<&'a Opened>,
    dimension: Dimension,
}

/// Everything that can be picked, with its centre on screen and how far from it the pointer
/// still picks it, in drawing order: structures, features, containers, the spawn, a find's
/// pins, the user's markers and the open hit's matches.
pub(super) fn place_markers<'a>(placing: &Placing<'a>) -> Vec<(Pos2, f32, Picked<'a>)> {
    let Placing {
        view,
        centre,
        rect,
        size,
        ..
    } = *placing;
    let mut placed: Vec<(Pos2, f32, Picked<'a>)> = Vec::new();
    for start in placing.starts {
        let at = markers::pixel_centre(view, centre, [start.position[0], start.position[2]]);
        if rect.expand(12.0).contains(at) {
            placed.push((at, size.reach(), Picked::Structure(start)));
        }
    }
    for feature in placing.features {
        let at = markers::pixel_centre(view, centre, [feature.position[0], feature.position[2]]);
        if rect.expand(12.0).contains(at) {
            placed.push((at, 11.0, Picked::Feature(feature)));
        }
    }
    for mark in placing.loot {
        let [x, _, z] = mark.container.position;
        let at = markers::pixel_centre(view, centre, [x, z]);
        if rect.expand(12.0).contains(at) {
            placed.push((at, 8.0, Picked::Container(mark)));
        }
    }
    if let Some((at, spawn)) = placing.spawn {
        placed.push((at, 10.0, Picked::Spawn(spawn)));
    }
    if let Some(Ok(found)) = placing.found {
        for hit in found.hits.iter().chain(&found.nearest) {
            let at = markers::pixel_centre(view, centre, [hit.x, hit.z]);
            placed.push((at, 10.0, Picked::Hit(hit)));
        }
    }
    for marker in placing.user {
        let at = markers::pixel_centre(view, centre, [marker.x, marker.z]);
        if rect.expand(30.0).contains(at) {
            placed.push((at, 11.0, Picked::Marker(marker)));
        }
    }
    if let Some(opened) = placing.opened {
        for found_match in opened
            .matches
            .iter()
            .filter(|m| m.dimension == placing.dimension)
        {
            let at = markers::pixel_centre(view, centre, found_match.at);
            placed.push((at, 15.0, Picked::Match(found_match)));
        }
    }
    placed
}

/// Draws `placed`, those `faded` dimmed: rings first, so the icons they surround stay
/// readable; then icons, the spawn and pins; the user's markers over everything; and last the
/// `hovered` one, large.
pub(super) fn draw_markers(
    ui: &egui::Ui,
    painter: &egui::Painter,
    placed: &[(Pos2, f32, Picked<'_>)],
    faded: &[bool],
    hovered: Option<usize>,
    size: Size,
) {
    let mut dimmed = painter.clone();
    dimmed.multiply_opacity(DONE_OPACITY);
    let painter_for = |i: usize| if faded[i] { &dimmed } else { painter };
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
            Picked::Spawn(_) => markers::spawn(painter, *at),
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
    }
}

/// A constraint's area: a dashed disc or box in the accent colour.
pub(super) fn draw_area(painter: &egui::Painter, view: &View, centre: Pos2, area: Area) {
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

/// A marker's tooltip; `done` if it's marked done.
pub(in crate::app) fn describe(ui: &mut egui::Ui, picked: &Picked<'_>, done: bool) {
    if done {
        ui.colored_label(style::GOOD, "Done");
    }
    match picked {
        Picked::Structure(start) => describe_structure(ui, start),
        Picked::Spawn(origin) => describe_spawn(ui, origin),
        Picked::Feature(f) => describe_feature(ui, f),
        Picked::Container(mark) => describe_container(ui, mark),
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

/// A structure start's tooltip: what may start there, where, its set, and how sure it is.
pub(super) fn describe_structure(ui: &mut egui::Ui, start: &StructureStart) {
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
    if start.from_save {
        ui.colored_label(style::GOOD, "Saved: your world has generated it");
    } else if start.certain {
        ui.colored_label(style::GOOD, "Certain");
    } else {
        ui.label(
            "Candidate: whether it generates depends on terrain (unless confirmed), a \
             tied biome or a platform-dependent ring position.",
        );
    }
}

/// The spawn's tooltip: where the game's spawn search starts, and what decides it.
pub(super) fn describe_spawn(ui: &mut egui::Ui, origin: &SpawnOrigin) {
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

/// A feature's tooltip: which, where, and how sure.
pub(super) fn describe_feature(ui: &mut egui::Ui, f: &FoundFeature) {
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
        egui::RichText::new("For the chunk decorated before its neighbours.").color(style::MUTED),
    );
}

/// A container's tooltip: its holder, what placed it, its table, how sure, and its contents.
pub(super) fn describe_container(ui: &mut egui::Ui, mark: &LootMark) {
    let c = &mark.container;
    let [x, y, z] = c.position;
    ui.strong(spyglass::display_name(&c.holder));
    ui.label(style::mono(format!("{x} {y} {z}")));
    let by = match &c.source {
        spyglass::ContainerSource::Structure(id) | spyglass::ContainerSource::Feature(id) => {
            spyglass::display_name(id)
        }
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

/// A container's contents, a line per kind of stack with its total: `3 × Bread`, with the
/// components its table set.
pub(in crate::app) fn contents_lines(slots: &[spyglass::Slot]) -> Vec<String> {
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
