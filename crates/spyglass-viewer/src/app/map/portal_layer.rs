//! Nether portals on the map (`docs/specs/portals.md`): portal markers and a save's portals in
//! their own dimension, where those in the other dimension lead (the exit blocks they aim at,
//! the search square around them and a line to the portal they land at), and the link area
//! overlay: where in this dimension a new portal would link to a chosen one on the other side.

use std::path::PathBuf;
use std::sync::Arc;

use egui::{Align2, ColorImage, FontId, Stroke, StrokeKind, TextureHandle};
use spyglass::{
    Border, Destination, LinkMap, LinkOutcome, MapArea, PixelLink, Portal, PortalFlag,
    SavePortalLink, SavePortals, portal_destination, portal_search_radius,
};

use super::*;

/// A save portal's colour, and a portal marker's footprint outline when it has none of its own.
const PORTAL: Color32 = Color32::from_rgb(0xb0, 0x60, 0xf0);
/// A save portal with a flag.
const FLAGGED: Color32 = Color32::from_rgb(0xf0, 0x80, 0x40);
/// The exit blocks a portal aims at.
const EXITS: Color32 = Color32::from_rgba_premultiplied(0x58, 0x30, 0x78, 0x78);
/// The search square around them.
const SQUARE: Color32 = Color32::from_rgba_premultiplied(0x90, 0x60, 0xc0, 0xa0);
/// The link area's shades: where a new portal links to the chosen one, builds one, depends
/// where the player stands or ties, and links elsewhere.
const AREA_LINKS: [u8; 4] = [0x50, 0xc0, 0x60, 0x70];
const AREA_NEW: [u8; 4] = [0xe0, 0x50, 0x50, 0x60];
const AREA_MIXED: [u8; 4] = [0xf0, 0xc0, 0x40, 0x70];
const AREA_ELSEWHERE: [u8; 4] = [0x10, 0x10, 0x10, 0x90];
/// The most pixels the link area is worked out at along a side: coarser than the view when the
/// map is large, so out of the Nether it takes a fraction of a second.
const AREA_SIDE: u32 = 320;

/// A portal on the map, by what holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::app) enum PortalRef {
    /// A portal marker, by id.
    Marker(String),
    /// A save's portal, by index in its portals.
    Save(usize),
}

/// A save's portals and their links among themselves.
pub(in crate::app) struct SaveLinks {
    portals: SavePortals,
    links: Vec<SavePortalLink>,
}

/// What a link area is worked out from: the chosen portal, the dimension shown, the area and
/// the portals on the other side.
#[derive(Clone, PartialEq)]
struct AreaKey {
    target: PortalRef,
    source: Dimension,
    area: MapArea,
    pieces: Vec<Portal>,
}

/// The portals' state between frames.
#[derive(Default)]
pub(in crate::app) struct PortalState {
    /// The save's portals, by its folder: being read, or read.
    save: Option<SaveRead>,
    /// Where each portal leading into a dimension leads, for the portals worked out from.
    links: Option<(Vec<Portal>, Dimension, Vec<SourceLinks>)>,
    /// The portal whose link area is shown.
    pub(in crate::app) area_for: Option<PortalRef>,
    /// The link area drawn, with its texture.
    area: Option<DrawnArea>,
    /// A link area being worked out.
    area_job: Option<(AreaKey, Pending<Result<LinkMap, String>>)>,
}

/// A save's portals: its folder, being read, or read.
struct SaveRead {
    dir: PathBuf,
    reading: Option<Pending<Result<SaveLinks, String>>>,
    read: Option<Result<Arc<SaveLinks>, String>>,
}

/// A link area worked out: what from, and its map and texture, or why it failed.
struct DrawnArea {
    key: AreaKey,
    drawn: Result<(LinkMap, TextureHandle), String>,
}

/// One portal everything links among: a marker or a save portal, with its rectangles.
struct Group {
    what: PortalRef,
    name: String,
    dimension: Dimension,
    pieces: Vec<Portal>,
    colour: Color32,
    /// For a save portal, its flags among the save's portals.
    flags: Vec<PortalFlag>,
    /// For a save portal, where it leads among the save's portals.
    save_outcomes: Vec<LinkOutcome>,
}

impl Group {
    /// Its blocks' least and greatest x and z.
    fn bounds(&self) -> [[i64; 2]; 2] {
        let mut b = [[i64::MAX, i64::MIN]; 2];
        for piece in &self.pieces {
            let [x, _, z] = piece.extent();
            for (axis, [lo, hi]) in [x, z].into_iter().enumerate() {
                b[axis] = [b[axis][0].min(lo), b[axis][1].max(hi)];
            }
        }
        b
    }

    fn centre_block(&self) -> [i32; 2] {
        let [x, z] = self.bounds();
        [((x[0] + x[1]) / 2) as i32, ((z[0] + z[1]) / 2) as i32]
    }
}

/// Where one portal in another dimension leads into the one shown.
struct SourceLinks {
    /// The group, by index.
    group: usize,
    /// The exit blocks' x and z, inclusive, and the search's radius; `None` if it's too low to
    /// stand in.
    exits: Option<([[i32; 2]; 2], i32)>,
    /// Where they lead, by group index.
    outcomes: Vec<LinkOutcome>,
}

/// What the pointer is on among the portals.
pub(super) enum PortalHover {
    /// A portal's footprint, by group.
    Portal(usize),
    /// The exit blocks of a portal in the other dimension, by index in the links.
    Exits(usize),
}

/// The portals this frame: every group, the rectangles all of them have, and each rectangle's
/// group.
pub(super) struct Known {
    groups: Vec<Group>,
    pieces: Vec<Portal>,
    owners: Vec<usize>,
    borders: [Border; 3],
}

impl Known {
    /// The rectangles in `dimension`, with each one's group: what a search there takes.
    fn pieces_in(&self, dimension: Dimension) -> (Vec<Portal>, Vec<usize>) {
        self.pieces
            .iter()
            .zip(&self.owners)
            .filter(|(p, _)| p.dimension == dimension)
            .map(|(p, o)| (*p, *o))
            .unzip()
    }

    fn find(&self, what: &PortalRef) -> Option<usize> {
        self.groups.iter().position(|g| &g.what == what)
    }

    /// Whether `what` is one of them: a marker that's a portal, or a save portal.
    pub(super) fn has(&self, what: &PortalRef) -> bool {
        self.find(what).is_some()
    }

    /// What group `i` is.
    pub(super) fn what(&self, i: usize) -> PortalRef {
        self.groups[i].what.clone()
    }
}

impl App {
    /// Starts reading the save's portals when a save is shown, and picks them up when they're
    /// read.
    pub(super) fn poll_save_portals(&mut self, ctx: &egui::Context) {
        let (Some(report), Some(game)) = (&self.save, self.game) else {
            self.portals.save = None;
            return;
        };
        let dir = report.save.dir.clone();
        if self
            .portals
            .save
            .as_ref()
            .is_none_or(|read| read.dir != dir)
        {
            let report = report.clone();
            let pending = Pending::spawn(ctx, move || {
                let portals = report.portals(game).map_err(|e| e.to_string())?;
                let links = game
                    .save_portal_links(&portals)
                    .map_err(|e| e.to_string())?;
                Ok(SaveLinks { portals, links })
            });
            self.portals.save = Some(SaveRead {
                dir,
                reading: Some(pending),
                read: None,
            });
            self.portals.links = None;
        }
        if let Some(save) = &mut self.portals.save
            && let Some(result) = save.reading.as_ref().and_then(Pending::poll)
        {
            save.reading = None;
            save.read = Some(
                result
                    .unwrap_or_else(|()| Err("reading the save's portals failed".into()))
                    .map(Arc::new),
            );
            self.portals.links = None;
        }
    }

    /// The save's portals, once read.
    fn save_links(&self) -> Option<Arc<SaveLinks>> {
        match self.portals.save.as_ref().and_then(|s| s.read.as_ref()) {
            Some(Ok(links)) => Some(links.clone()),
            _ => None,
        }
    }

    /// Every portal: the portal markers, in every dimension, then the save's.
    pub(super) fn known_portals(&self) -> Known {
        let mut groups = Vec::new();
        if let Some(here) = self.here() {
            for marker in here.live_markers() {
                let Some(portal) = marker.portal() else {
                    continue;
                };
                let [r, g, b] = marker.colour;
                let name = if marker.name.is_empty() {
                    format!("the portal marker at {} {}", marker.x, marker.z)
                } else {
                    marker.name.clone()
                };
                groups.push(Group {
                    what: PortalRef::Marker(marker.id.clone()),
                    name,
                    dimension: portal.dimension,
                    pieces: vec![portal],
                    colour: Color32::from_rgb(r, g, b),
                    flags: Vec::new(),
                    save_outcomes: Vec::new(),
                });
            }
        }
        let mut borders = [Border::default(); 3];
        if let Some(save) = self.save_links() {
            borders = save.portals.borders;
            let mut flags: Vec<(Vec<PortalFlag>, Vec<LinkOutcome>)> =
                vec![(Vec::new(), Vec::new()); save.portals.portals.len()];
            for link in &save.links {
                flags[link.portal] = (link.flags.clone(), link.outcomes.clone());
            }
            // Save portals come after the markers among the groups.
            let first = groups.len();
            for (i, (portal, (flags, mut outcomes))) in
                save.portals.portals.iter().zip(flags).enumerate()
            {
                for outcome in &mut outcomes {
                    for p in &mut outcome.portals {
                        *p += first;
                    }
                }
                let [x, y, z] = portal.corner();
                groups.push(Group {
                    what: PortalRef::Save(i),
                    name: format!("save portal #{} at {x} {y} {z}", i + 1),
                    dimension: portal.dimension,
                    pieces: portal.pieces.clone(),
                    colour: if flags.is_empty() { PORTAL } else { FLAGGED },
                    flags,
                    save_outcomes: outcomes,
                });
            }
        }
        let mut pieces = Vec::new();
        let mut owners = Vec::new();
        for (i, group) in groups.iter().enumerate() {
            pieces.extend(&group.pieces);
            owners.extend(std::iter::repeat_n(i, group.pieces.len()));
        }
        Known {
            groups,
            pieces,
            owners,
            borders,
        }
    }

    /// Where each portal of another dimension that leads into `dimension` leads, worked out
    /// again only when the portals change.
    fn links_into(&mut self, known: &Known, dimension: Dimension) -> &[SourceLinks] {
        let fresh = self
            .portals
            .links
            .as_ref()
            .is_some_and(|(pieces, d, _)| *d == dimension && *pieces == known.pieces);
        if !fresh {
            let links = match self.game {
                Some(game) => source_links(game, known, dimension),
                None => Vec::new(),
            };
            self.portals.links = Some((known.pieces.clone(), dimension, links));
        }
        &self.portals.links.as_ref().expect("just set").2
    }

    /// The portals' layer, under the markers: the link area, then where portals of the other
    /// dimension lead, then the portals here. Returns what the pointer is on.
    pub(super) fn draw_portal_layer(
        &mut self,
        ui: &egui::Ui,
        canvas: &Canvas<'_>,
        frame: &Frame,
        world: &World,
    ) -> (Known, Option<PortalHover>) {
        let known = self.known_portals();
        let here = world.dimension_kind();
        let (view, centre, painter) = (frame.view, canvas.centre, canvas.painter);
        self.draw_link_area(ui.ctx(), painter, &known, here, &view, centre);
        let pointer = canvas
            .response
            .hover_pos()
            .filter(|_| !canvas.response.dragged());
        let mut hover = None;
        let block_rect =
            |min: [i32; 2], max: [i32; 2]| markers::block_rect(&view, centre, min, max);
        let links = self.links_into(&known, here);
        for (i, link) in links.iter().enumerate() {
            let Some(([x, z], radius)) = link.exits else {
                continue;
            };
            let exits = block_rect([x[0], z[0]], [x[1], z[1]]);
            let square = block_rect(
                [x[0].saturating_sub(radius), z[0].saturating_sub(radius)],
                [x[1].saturating_add(radius), z[1].saturating_add(radius)],
            );
            if !canvas.rect.intersects(square) {
                continue;
            }
            painter.rect_stroke(square, 0.0, Stroke::new(1.0, SQUARE), StrokeKind::Inside);
            let shown = exits.expand(((4.0 - exits.width()) / 2.0).max(0.0));
            painter.rect_filled(shown, 0.0, EXITS);
            painter.rect_stroke(
                shown,
                0.0,
                Stroke::new(1.5, known.groups[link.group].colour),
                StrokeKind::Outside,
            );
            for outcome in &link.outcomes {
                let colour = if outcome.tied {
                    style::WARN
                } else {
                    style::TEXT
                };
                for &to in &outcome.portals {
                    let at = markers::pixel_centre(&view, centre, known.groups[to].centre_block());
                    painter.line_segment([shown.center(), at], Stroke::new(1.5, colour));
                }
                if outcome.portals.is_empty() {
                    painter.text(
                        shown.center_bottom() + Vec2::new(0.0, 4.0),
                        Align2::CENTER_TOP,
                        "new portal",
                        FontId::proportional(11.0),
                        style::ERROR,
                    );
                }
            }
            if pointer.is_some_and(|p| shown.expand(3.0).contains(p)) {
                hover = Some(PortalHover::Exits(i));
            }
        }
        for (i, group) in known.groups.iter().enumerate() {
            if group.dimension != here {
                continue;
            }
            let [x, z] = group.bounds();
            let rect = block_rect([x[0] as i32, z[0] as i32], [x[1] as i32, z[1] as i32]);
            let rect = rect.expand(((6.0 - rect.width().min(rect.height())) / 2.0).max(0.0));
            if !canvas.rect.intersects(rect) {
                continue;
            }
            let picked = self.portals.area_for.as_ref() == Some(&group.what);
            painter.rect_filled(rect, 1.0, group.colour.gamma_multiply(0.35));
            painter.rect_stroke(
                rect,
                1.0,
                Stroke::new(if picked { 3.0 } else { 1.5 }, group.colour),
                StrokeKind::Outside,
            );
            if pointer.is_some_and(|p| rect.expand(3.0).contains(p)) {
                hover = Some(PortalHover::Portal(i));
            }
        }
        (known, hover)
    }

    /// The tooltip for what the pointer is on among the portals.
    pub(super) fn portal_tooltip(
        &self,
        response: &egui::Response,
        known: &Known,
        hover: &PortalHover,
    ) {
        let links = self.portals.links.as_ref().map_or(&[][..], |(_, _, l)| l);
        response.clone().on_hover_ui_at_pointer(|ui| match hover {
            PortalHover::Portal(i) => {
                let group = &known.groups[*i];
                ui.strong(capitalise(&group.name));
                let [x, z] = group.bounds();
                ui.label(
                    style::mono(format!("x {} to {}, z {} to {}", x[0], x[1], z[0], z[1]))
                        .color(style::MUTED),
                );
                if let PortalRef::Save(_) = group.what {
                    for line in outcome_lines(known, &group.save_outcomes) {
                        ui.label(line);
                    }
                    for flag in &group.flags {
                        ui.colored_label(FLAGGED, flag_text(known, flag));
                    }
                }
                ui.label(
                    egui::RichText::new(format!(
                        "Leads to {}: switch dimension to see where. Right-click for its link \
                         area.",
                        portal_destination(group.dimension).name()
                    ))
                    .color(style::MUTED),
                );
            }
            PortalHover::Exits(i) => {
                let link = &links[*i];
                let group = &known.groups[link.group];
                ui.strong(format!("Where {} aims", group.name));
                if let Some(([x, z], radius)) = link.exits {
                    ui.label(
                        style::mono(format!(
                            "exits x {} to {}, z {} to {}; searched {radius} around",
                            x[0], x[1], z[0], z[1]
                        ))
                        .color(style::MUTED),
                    );
                }
                for line in outcome_lines(known, &link.outcomes) {
                    ui.label(line);
                }
                ui.label(
                    egui::RichText::new(
                        "For a standing player; one moving through can aim a little further.",
                    )
                    .color(style::MUTED),
                );
            }
        });
    }

    /// Shows the link area for `what`: switches to the dimension whose portals lead to it, and
    /// centres on where it is there.
    pub(in crate::app) fn show_link_area(&mut self, ctx: &egui::Context, what: PortalRef) {
        let known = self.known_portals();
        let (Some(i), Some(game)) = (known.find(&what), self.game) else {
            return;
        };
        let group = &known.groups[i];
        if group.dimension == Dimension::End {
            return;
        }
        let source = portal_destination(group.dimension);
        let [x, z] = group.centre_block();
        let scale = game
            .coordinate_scale(group.dimension)
            .and_then(|from| Ok(from / game.coordinate_scale(source)?))
            .unwrap_or(1.0);
        self.portals.area_for = Some(what);
        self.switch_dimension(ctx, source);
        self.settings.view.centre = [f64::from(x) * scale, f64::from(z) * scale];
    }

    /// The link area over the tiles, when one is chosen and this dimension's portals lead to
    /// it: worked out in the background for the view, and drawn as it was last worked out.
    fn draw_link_area(
        &mut self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        known: &Known,
        here: Dimension,
        view: &View,
        centre: Pos2,
    ) {
        let Some(target) = self.portals.area_for.clone() else {
            return;
        };
        let Some(t) = known.find(&target) else {
            self.portals.area_for = None;
            return;
        };
        let to = known.groups[t].dimension;
        if portal_destination(here) != to {
            return;
        }
        let (pieces, owners) = known.pieces_in(to);
        // Coarser than the view, so the area has at most AREA_SIDE pixels a side.
        let [w, h] = self.map_size;
        let mut scale = view.scale;
        while (w.max(h) * view.scale) / scale > AREA_SIDE && scale < spyglass::MAX_SCALE {
            scale *= 2;
        }
        let blocks = view.block_at([0.0, 0.0]).map(|b| b as i32);
        let size = [w, h].map(|side| (side * view.scale).div_ceil(scale) + 2);
        let Ok(area) = MapArea::centred_on(blocks, size, scale, view.y) else {
            return;
        };
        let key = AreaKey {
            target: target.clone(),
            source: here,
            area,
            pieces: pieces.clone(),
        };
        // A finished job: its map made a texture.
        if let Some((job_key, pending)) = &self.portals.area_job
            && let Some(result) = pending.poll()
        {
            let job_key = job_key.clone();
            self.portals.area_job = None;
            let result = result
                .unwrap_or_else(|()| Err("working out the link area failed".into()))
                .map(|map| {
                    let image = area_image(&map, &owners, t);
                    let texture = ctx.load_texture("link area", image, TextureOptions::NEAREST);
                    (map, texture)
                });
            self.portals.area = Some(DrawnArea {
                key: job_key,
                drawn: result,
            });
        }
        let current = self.portals.area.as_ref().is_some_and(|a| a.key == key);
        if !current
            && self.portals.area_job.is_none()
            && let Some(game) = self.game
        {
            let border = known.borders[to.index()];
            let job_key = key.clone();
            self.portals.area_job = Some((
                key,
                Pending::spawn(ctx, move || {
                    game.portal_link_map(here, &job_key.area, &job_key.pieces, &border)
                        .map_err(|e| e.to_string())
                }),
            ));
        }
        if let Some(DrawnArea {
            key: drawn,
            drawn: Ok((map, texture)),
        }) = &self.portals.area
            && drawn.target == target
            && drawn.source == here
        {
            let area = map.area();
            let s = area.scale as i32;
            let max = [0, 1].map(|d| area.min[d] + area.size[d] as i32 * s - 1);
            let rect = markers::block_rect(view, centre, area.min, max);
            let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
            painter.image(texture.id(), rect, uv, Color32::WHITE);
        }
    }

    /// The link area's notice over the map: what its shades mean, and a button to hide it.
    pub(super) fn link_area_notice(
        &mut self,
        ui: &egui::Ui,
        rect: Rect,
        known: &Known,
        here: Dimension,
    ) {
        let Some(target) = self.portals.area_for.clone() else {
            return;
        };
        let Some(t) = known.find(&target) else {
            return;
        };
        let group = &known.groups[t];
        let leads_here = portal_destination(here) == group.dimension;
        let mut clear = false;
        egui::Window::new("Link area")
            .title_bar(false)
            .resizable(false)
            .frame(style::overlay())
            .fixed_pos(rect.center_top() + Vec2::new(-180.0, 12.0))
            .show(ui.ctx(), |ui| {
                ui.set_width(360.0);
                ui.horizontal(|ui| {
                    ui.strong(format!("Link area for {}", group.name));
                    if ui.small_button("Hide").clicked() {
                        clear = true;
                    }
                });
                if !leads_here {
                    ui.label(format!(
                        "Shown in {}, whose portals lead to it.",
                        portal_destination(group.dimension).name()
                    ));
                    return;
                }
                let working = self.portals.area_job.is_some();
                if let Some(DrawnArea { drawn: Err(e), .. }) = &self.portals.area {
                    ui.colored_label(style::ERROR, e);
                }
                for (rgba, text) in [
                    (AREA_LINKS, "a portal built here links to it"),
                    (AREA_ELSEWHERE, "links to another portal"),
                    (AREA_NEW, "finds none: the game builds one"),
                    (AREA_MIXED, "depends where you stand, or ties"),
                ] {
                    ui.horizontal(|ui| {
                        let (swatch, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                        let [r, g, b, _] = rgba;
                        ui.painter()
                            .rect_filled(swatch, 2.0, Color32::from_rgb(r, g, b));
                        ui.label(text);
                    });
                }
                ui.label(
                    egui::RichText::new(format!(
                        "At y {}, the view's height{}.",
                        self.settings.view.y,
                        if working { " · working it out" } else { "" }
                    ))
                    .color(style::MUTED),
                );
            });
        if clear {
            self.portals.area_for = None;
            self.portals.area = None;
        }
    }
}

/// Where each group in another dimension that leads into `dimension` leads.
fn source_links(game: &Game, known: &Known, dimension: Dimension) -> Vec<SourceLinks> {
    let border = known.borders[dimension.index()];
    let mut out = Vec::new();
    for (index, group) in known.groups.iter().enumerate() {
        if group.dimension == dimension || portal_destination(group.dimension) != dimension {
            continue;
        }
        let mut exits: Option<[[i32; 2]; 2]> = None;
        let mut outcomes: Vec<LinkOutcome> = Vec::new();
        for piece in &group.pieces {
            let Ok(Some(links)) = game.portal_links(piece, &known.pieces, &border) else {
                continue;
            };
            let [x, _, z] = links.exits;
            exits = Some(match exits {
                None => [x, z],
                Some([ex, ez]) => [
                    [ex[0].min(x[0]), ex[1].max(x[1])],
                    [ez[0].min(z[0]), ez[1].max(z[1])],
                ],
            });
            for outcome in links.outcomes_by(|i| known.owners[i]) {
                match outcomes
                    .iter_mut()
                    .find(|o| o.portals == outcome.portals && o.tied == outcome.tied)
                {
                    Some(o) => o.exits += outcome.exits,
                    None => outcomes.push(outcome),
                }
            }
        }
        outcomes.sort_by_key(|o| std::cmp::Reverse(o.exits));
        out.push(SourceLinks {
            group: index,
            exits: exits.map(|e| (e, portal_search_radius(dimension))),
            outcomes,
        });
    }
    out
}

/// The link area as an image: each pixel shaded by whether a portal there links to group `t`,
/// with `owners` giving the group of each rectangle searched.
fn area_image(map: &LinkMap, owners: &[usize], t: usize) -> ColorImage {
    let [w, h] = map.area().size;
    let groups = |d: &Destination| -> Vec<usize> {
        let mut g: Vec<usize> = match d {
            Destination::Portal(p) => vec![owners[*p]],
            Destination::Tied(ps) => ps.iter().map(|p| owners[*p]).collect(),
            Destination::NewPortal => Vec::new(),
        };
        g.sort_unstable();
        g.dedup();
        g
    };
    let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
    for j in 0..h {
        for i in 0..w {
            let shade = match map.at([i, j]) {
                PixelLink::One(d) => match &groups(d)[..] {
                    [] => AREA_NEW,
                    [only] if *only == t => AREA_LINKS,
                    many if many.contains(&t) => AREA_MIXED,
                    _ => AREA_ELSEWHERE,
                },
                PixelLink::Mixed(ds) => {
                    let all: Vec<Vec<usize>> = ds.iter().map(groups).collect();
                    if all.iter().all(|g| g == &[t]) {
                        AREA_LINKS
                    } else if all.iter().any(|g| g.contains(&t)) {
                        AREA_MIXED
                    } else if all.iter().all(Vec::is_empty) {
                        AREA_NEW
                    } else {
                        AREA_ELSEWHERE
                    }
                }
            };
            rgba.extend(shade);
        }
    }
    ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba)
}

/// Each outcome as a line: how many exits, and where they land.
fn outcome_lines(known: &Known, outcomes: &[LinkOutcome]) -> Vec<String> {
    let total: usize = outcomes.iter().map(|o| o.exits).sum();
    outcomes
        .iter()
        .map(|o| {
            let share = if o.exits == total {
                "All exits".to_string()
            } else {
                format!("{} of {total} exits", o.exits)
            };
            let names: Vec<&str> = o
                .portals
                .iter()
                .map(|&p| known.groups[p].name.as_str())
                .collect();
            match &names[..] {
                [] => format!("{share} find no portal: the game builds one"),
                [one] => format!("{share} land at {one}"),
                many => format!("{share} are tied between {}", many.join(" and ")),
            }
        })
        .collect()
}

/// A save portal's flag, for people. Its portals are save portal indices, which come after
/// the markers among the groups.
fn flag_text(known: &Known, flag: &PortalFlag) -> String {
    let name = |save: usize| {
        known.find(&PortalRef::Save(save)).map_or_else(
            || format!("#{}", save + 1),
            |g| known.groups[g].name.clone(),
        )
    };
    match flag {
        PortalFlag::Mislinked { to, back } => {
            let back = if back.is_empty() {
                "a new portal".to_string()
            } else {
                back.iter()
                    .map(|&b| name(b))
                    .collect::<Vec<_>>()
                    .join(" or ")
            };
            format!(
                "Mislinked: it leads to {}, which leads to {back}",
                name(*to)
            )
        }
        PortalFlag::Split => "Split: different places in it lead to different portals".into(),
        PortalFlag::Unlinked => "Unlinked: no portal is in range on the other side".into(),
        PortalFlag::Tied => "Tied: some exits are equally near two portals".into(),
        PortalFlag::TooLow => "Too low for a player to stand in".into(),
    }
}

fn capitalise(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}
