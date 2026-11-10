//! What's drawn over the map: structure and feature icons, the spawn, locate pins and search
//! rings, the user's markers, and jigsaw and mineshaft bounds. Markers sit where the library's
//! `Image::draw_features` puts its own (`docs/specs/render.md#layers`): one tile pixel is one
//! point, and a marker's centre is the pixel `floor(block / scale)` on the world-aligned grid.
//! Their shapes are the viewer's own (ADR 0044), in the library's set colours.
//!
//! These are painted straight onto the map's painter each frame, not laid out as widgets: the
//! map draws hundreds of them, and picks the one under the pointer itself (`pick`).

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, Shape, Stroke, StrokeKind,
    Vec2,
};
use spyglass::{FoundFeature, SpawnOrigin, StructureStart, feature_colour, structure_colour};

use crate::tiles::LootMark;

use crate::icons;
use crate::locate::Hit;
use crate::style;
use crate::userdata::{Kind, Marker, Place};
use crate::view::View;

/// How opaque done things are drawn.
pub const DONE_OPACITY: f32 = 0.4;

/// The screen position of the centre of the pixel containing `block`.
pub fn pixel_centre(view: &View, map_centre: Pos2, block: [i32; 2]) -> Pos2 {
    let scale = i64::from(view.scale);
    let origin = view.origin();
    let at =
        |axis: usize| (i64::from(block[axis]).div_euclid(scale) as f64 + origin[axis]) as f32 + 0.5;
    map_centre + Vec2::new(at(0), at(1))
}

/// How big structure icons are drawn: dots when they're too many and small to read, badges
/// with their glyph up close.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    /// A dot in the set's colour, far out.
    Dot,
    /// A small badge.
    Small,
    /// A badge large enough to read its glyph, up close.
    Full,
}

impl Size {
    /// The size at `scale` blocks per pixel.
    pub fn at(scale: u32) -> Size {
        match scale {
            64.. => Size::Dot,
            16.. => Size::Small,
            _ => Size::Full,
        }
    }

    /// The badge's side, or the dot's diameter, in points.
    fn side(self) -> f32 {
        match self {
            Size::Dot => 7.0,
            Size::Small => 14.0,
            Size::Full => 22.0,
        }
    }

    /// How close, in points, the pointer must be to a marker's centre to pick it.
    pub fn reach(self) -> f32 {
        self.side() / 2.0 + 3.0
    }
}

/// A feature's icon (a dungeon, geode, well, fossil or lava lake): a diamond in its colour,
/// filled if it's certain and outlined if it's a likely candidate, larger and ringed when
/// hovered. Features are drawn only up close, so it has one size.
pub fn feature(painter: &Painter, centre: Pos2, found: &FoundFeature, hovered: bool) {
    let [r, g, b] = feature_colour(&found.feature);
    let colour = Color32::from_rgb(r, g, b);
    let half = if hovered { 13.0 } else { 9.0 };
    // A diamond's corners, `half_width` from the centre.
    let diamond = |half_width: f32| {
        vec![
            centre + Vec2::new(0.0, -half_width),
            centre + Vec2::new(half_width, 0.0),
            centre + Vec2::new(0.0, half_width),
            centre + Vec2::new(-half_width, 0.0),
        ]
    };
    if hovered {
        painter.add(Shape::closed_line(
            diamond(half + 3.0),
            Stroke::new(2.0, Color32::WHITE),
        ));
    }
    let (fill, stroke) = if found.certain {
        (colour, Stroke::new(1.5, style::GROUND))
    } else {
        (style::GROUND, Stroke::new(2.5, colour))
    };
    painter.add(Shape::convex_polygon(diamond(half), fill, stroke));
}

/// A loot container's icon: a small chest-brown square, ringed in gold when it holds the Loot
/// item (a full ring when that's certain, a thin one when it might), larger when hovered.
pub fn container(painter: &Painter, centre: Pos2, mark: &LootMark, hovered: bool) {
    let half = if hovered { 6.0 } else { 4.0 };
    let square = Rect::from_center_size(centre, Vec2::splat(half * 2.0));
    painter.rect(
        square,
        CornerRadius::same(1),
        Color32::from_rgb(0x9c, 0x6a, 0x2c),
        Stroke::new(1.0, style::GROUND),
        StrokeKind::Outside,
    );
    if mark.ringed {
        let gold = Color32::from_rgb(0xff, 0xc8, 0x30);
        let width = if mark.container.contents_certain() {
            3.0
        } else {
            1.5
        };
        painter.circle_stroke(centre, half + 5.0, Stroke::new(width + 2.0, style::GROUND));
        painter.circle_stroke(centre, half + 5.0, Stroke::new(width, gold));
    }
    if hovered {
        painter.rect_stroke(
            square.expand(3.0),
            CornerRadius::same(2),
            Stroke::new(2.0, Color32::WHITE),
            StrokeKind::Outside,
        );
    }
}

/// A structure start's icon: its family's glyph on a badge in its set's colour, filled if it's
/// certain and hollow if it's a candidate. The hovered one is drawn larger, with a halo and its
/// name. A done one is dimmed: `painter` should be too.
pub fn structure(
    ui: &egui::Ui,
    painter: &Painter,
    centre: Pos2,
    start: &StructureStart,
    size: Size,
    hovered: bool,
    done: bool,
) {
    let glyph_opacity = if done { DONE_OPACITY } else { 1.0 };
    let badge = icons::badge(&start.set);
    if size == Size::Dot && !hovered {
        let (fill, stroke) = if start.certain {
            (badge.fill, Stroke::new(1.5, style::GROUND))
        } else {
            (style::GROUND, Stroke::new(1.5, badge.tint))
        };
        painter.circle(centre, size.side() / 2.0, fill, stroke);
        return;
    }
    let side = if hovered { 28.0 } else { size.side() };
    let rect = Rect::from_center_size(centre, Vec2::splat(side));
    let radius = CornerRadius::same((side / 3.5) as u8);
    if hovered {
        painter.rect_stroke(
            rect.expand(2.0),
            radius,
            Stroke::new(2.0, Color32::WHITE),
            StrokeKind::Outside,
        );
    }
    let ink = if start.certain {
        painter.rect(
            rect,
            radius,
            badge.fill,
            Stroke::new(1.5, style::GROUND),
            StrokeKind::Inside,
        );
        badge.ink
    } else {
        painter.rect(
            rect,
            radius,
            style::GROUND.gamma_multiply(0.88),
            Stroke::new(2.0, badge.tint),
            StrokeKind::Inside,
        );
        badge.tint
    };
    let id = start.structures.first().map_or("", String::as_str);
    icons::glyph(id)
        .tint(ink.gamma_multiply(glyph_opacity))
        .paint_at(ui, Rect::from_center_size(centre, Vec2::splat(side * 0.68)));
    if hovered {
        let name = start
            .structures
            .iter()
            .map(|s| spyglass::display_name(s))
            .collect::<Vec<_>>()
            .join(" or ");
        label(painter, centre + Vec2::new(side / 2.0 + 8.0, 0.0), &name);
    }
}

/// `text` on a dark chip, starting at `left` and centred on its height.
pub fn label(painter: &Painter, left: Pos2, text: &str) {
    let galley = painter.layout_no_wrap(text.to_string(), FontId::proportional(12.5), style::TEXT);
    let rect = Align2::LEFT_CENTER.anchor_size(left, galley.size() + Vec2::new(12.0, 6.0));
    painter.rect_filled(
        rect,
        CornerRadius::same(4),
        style::GROUND.gamma_multiply(0.92),
    );
    painter.galley(rect.min + Vec2::new(6.0, 3.0), galley, style::TEXT);
}

/// The screen rectangle covering blocks `min..=max` (x and z).
pub fn block_rect(view: &View, map_centre: Pos2, min: [i32; 2], max: [i32; 2]) -> Rect {
    let scale = f64::from(view.scale);
    let origin = view.origin();
    // A block edge's screen offset on one axis.
    let edge = |block: i64, axis: usize| (block as f64 / scale + origin[axis]) as f32;
    Rect::from_min_max(
        map_centre + Vec2::new(edge(i64::from(min[0]), 0), edge(i64::from(min[1]), 1)),
        map_centre
            + Vec2::new(
                edge(i64::from(max[0]) + 1, 0),
                edge(i64::from(max[1]) + 1, 1),
            ),
    )
}

/// A structure's bounds: an outline in its set's colour over a black one.
pub fn bounds(painter: &Painter, rect: Rect, set: &str) {
    let [r, g, b] = structure_colour(set);
    painter.rect_stroke(
        rect,
        0.0,
        Stroke::new(3.0, Color32::BLACK),
        StrokeKind::Inside,
    );
    painter.rect_stroke(
        rect.shrink(1.0),
        0.0,
        Stroke::new(1.0, Color32::from_rgb(r, g, b)),
        StrokeKind::Inside,
    );
}

/// The world spawn: a white disc with a dark cross, the library's cross inverted.
pub fn spawn(painter: &Painter, centre: Pos2) {
    spawn_sized(painter, centre, 9.0);
}

/// The spawn's marker with radius `radius`, for the legend and buttons too.
pub fn spawn_sized(painter: &Painter, centre: Pos2, radius: f32) {
    painter.circle(
        centre,
        radius,
        Color32::WHITE,
        Stroke::new(radius / 4.5, style::GROUND),
    );
    let stroke = Stroke::new(radius / 4.0, style::GROUND);
    let arm = radius * 0.6;
    painter.line_segment([centre - Vec2::X * arm, centre + Vec2::X * arm], stroke);
    painter.line_segment([centre - Vec2::Y * arm, centre + Vec2::Y * arm], stroke);
}

/// A locate hit: a pin with its point on the block, yellow if the game certainly stops there.
pub fn pin(painter: &Painter, tip: Pos2, certain: bool) {
    let (fill, line, dot) = if certain {
        (
            Color32::from_rgb(0xff, 0xd0, 0x40),
            style::GROUND,
            style::GROUND,
        )
    } else {
        (style::GROUND, Color32::WHITE, Color32::WHITE)
    };
    teardrop(painter, tip, 7.5, fill, line, dot);
}

/// A user's marker: a pin in its colour, larger than a locate pin, with its name beside it when
/// `named`.
pub fn user(painter: &Painter, tip: Pos2, marker: &Marker, named: bool) {
    let [r, g, b] = marker.colour;
    let fill = Color32::from_rgb(r, g, b);
    teardrop(painter, tip, 8.5, fill, style::GROUND, style::GROUND);
    if named && !marker.name.is_empty() {
        label(painter, tip + Vec2::new(12.0, -17.0), &marker.name);
    }
}

/// A pin: a circle of radius `radius` whose centre is twice that above `tip`, and the two
/// tangents to it from the tip, a convex teardrop, with a dot in the middle.
fn teardrop(painter: &Painter, tip: Pos2, radius: f32, fill: Color32, line: Color32, dot: Color32) {
    let height = 2.0 * radius;
    let head = tip - Vec2::Y * height;
    // A tangent from the tip touches the circle this far round from straight down (y is down
    // on screen, so down is a quarter turn): the outline is the tip, then the arc from one
    // tangent point round the top to the other.
    let spread = (radius / height).acos();
    let down = std::f32::consts::FRAC_PI_2;
    let steps = 18;
    let mut points = vec![tip];
    for i in 0..=steps {
        let angle =
            down + spread + (std::f32::consts::TAU - 2.0 * spread) * i as f32 / steps as f32;
        points.push(head + Vec2::angled(angle) * radius);
    }
    painter.add(Shape::convex_polygon(points, fill, Stroke::new(2.0, line)));
    painter.circle_filled(head, radius * 0.4, dot);
}

/// Something a search hit's constraint matched: a ring around it, white if certain.
pub fn ring(painter: &Painter, centre: Pos2, certain: bool) {
    let colour = if certain {
        Color32::WHITE
    } else {
        style::MUTED
    };
    painter.circle_stroke(centre, 15.0, Stroke::new(4.0, style::GROUND));
    painter.circle_stroke(centre, 15.0, Stroke::new(2.0, colour));
}

/// Something on the map the pointer can point at.
#[derive(Clone)]
pub enum Picked<'a> {
    /// A structure start's icon.
    Structure(&'a StructureStart),
    /// A feature's icon.
    Feature(&'a FoundFeature),
    /// A loot container.
    Container(&'a LootMark),
    /// The world spawn.
    Spawn(&'a SpawnOrigin),
    /// A locate hit's pin.
    Hit(&'a Hit),
    /// Something a search hit's constraint matched.
    Match(&'a crate::search::Match),
    /// One of the user's markers.
    Marker(&'a Marker),
}

impl Picked<'_> {
    /// What marking it done marks, if it can be.
    pub fn place(&self) -> Option<Place> {
        match self {
            Picked::Structure(start) => Some(Place {
                kind: Kind::Structure,
                id: start.set.clone(),
                at: [start.position[0], start.position[2]],
            }),
            Picked::Hit(hit) => Some(hit.place.clone()),
            Picked::Match(found) => found.place.clone(),
            Picked::Feature(feature) => Some(crate::locate::feature_place(feature)),
            Picked::Container(mark) => Some(Place {
                kind: Kind::Place,
                id: String::new(),
                at: [mark.container.position[0], mark.container.position[2]],
            }),
            Picked::Spawn(_) | Picked::Marker(_) => None,
        }
    }
}

/// The marker nearest `pointer` within its reach, from markers at screen positions.
pub fn pick<'a>(
    pointer: Pos2,
    markers: impl IntoIterator<Item = (Pos2, f32, Picked<'a>)>,
) -> Option<(usize, Picked<'a>)> {
    markers
        .into_iter()
        .enumerate()
        .map(|(i, (at, reach, what))| ((at - pointer).length(), reach, i, what))
        .filter(|(distance, reach, _, _)| distance <= reach)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, _, i, what)| (i, what))
}

#[cfg(test)]
mod tests {
    use spyglass::MapArea;

    use super::*;

    #[test]
    fn markers_sit_on_the_same_pixels_as_the_library() {
        // A tile's pixels and the view's agree: the marker centre is the same pixel the
        // library's `MapArea::pixel` gives, offset by the tile's place on screen.
        let view = View {
            centre: [-1000.0, 300.0],
            scale: 8,
            y: 64,
        };
        let map_centre = Pos2::new(400.0, 300.0);
        let tile = crate::view::Tile {
            scale: 8,
            x: -1,
            z: 0,
        };
        let area: MapArea = tile.area(64).unwrap();
        let at = view.tile_offset(tile);
        for block in [[-1000, 300], [-2048, 0], [-1, 2047], [-1500, 77]] {
            let [px, pz] = area.pixel(block);
            let expected = map_centre + Vec2::new(at[0] + px as f32 + 0.5, at[1] + pz as f32 + 0.5);
            assert_eq!(
                pixel_centre(&view, map_centre, block),
                expected,
                "{block:?}"
            );
        }
    }

    #[test]
    fn bounds_cover_their_blocks() {
        let view = View {
            centre: [0.0, 0.0],
            scale: 2,
            y: 64,
        };
        let map_centre = Pos2::new(100.0, 100.0);
        // Blocks 0..=3 are pixels 0 and 1; the rectangle runs from the first pixel's left edge
        // to the last one's right edge.
        let rect = block_rect(&view, map_centre, [0, -4], [3, -1]);
        let first = pixel_centre(&view, map_centre, [0, -4]);
        let last = pixel_centre(&view, map_centre, [3, -1]);
        assert_eq!(rect.min, first - Vec2::splat(0.5));
        assert_eq!(rect.max, last + Vec2::splat(0.5));
    }

    #[test]
    fn icons_shrink_as_the_map_zooms_out() {
        assert_eq!(Size::at(1), Size::Full);
        assert_eq!(Size::at(8), Size::Full);
        assert_eq!(Size::at(16), Size::Small);
        assert_eq!(Size::at(32), Size::Small);
        assert_eq!(Size::at(64), Size::Dot);
        assert_eq!(Size::at(spyglass::MAX_SCALE), Size::Dot);
        assert!(Size::Dot.reach() < Size::Small.reach());
        assert!(Size::Small.reach() < Size::Full.reach());
    }

    #[test]
    fn the_nearest_marker_within_reach_is_picked() {
        // Any spawn will do; a hand-written world has one.
        let fixture = spyglass_oracle::fixtures::structured_overworld(false);
        let world = spyglass::World::open(fixture.path()).unwrap();
        let spawn = world.session(0).unwrap().spawn_origin();
        let near = [(Pos2::new(10.0, 10.0), 6.0, Picked::Spawn(&spawn))];
        assert!(pick(Pos2::new(13.0, 14.0), near).is_some());
        let far = [(Pos2::new(10.0, 10.0), 6.0, Picked::Spawn(&spawn))];
        assert!(pick(Pos2::new(20.0, 10.0), far).is_none());
        // A bigger marker reaches further, and the nearer of two wins.
        let two = [
            (Pos2::new(10.0, 10.0), 14.0, Picked::Spawn(&spawn)),
            (Pos2::new(30.0, 10.0), 14.0, Picked::Spawn(&spawn)),
        ];
        assert_eq!(pick(Pos2::new(22.0, 10.0), two).map(|(i, _)| i), Some(1));
    }
}
