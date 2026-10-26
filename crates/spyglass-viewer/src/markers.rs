//! Structure and spawn markers drawn over the map, the same shapes and places as the library's
//! `Image::draw_features` (`docs/specs/render.md#layers`): one tile pixel is one point, and a
//! marker's centre is the pixel `floor(block / scale)` on the world-aligned grid.

use eframe::egui::Stroke;
use eframe::egui::{Color32, Painter, Pos2, Rect, Vec2};
use spyglass::{SpawnOrigin, StructureStart, structure_colour};

use crate::locate::Hit;
use crate::view::View;

/// How close, in points, the pointer must be to a marker's centre to pick it.
const PICK: f32 = 6.0;

/// The screen position of the centre of the pixel containing `block`.
pub fn pixel_centre(view: &View, map_centre: Pos2, block: [i32; 2]) -> Pos2 {
    let s = i64::from(view.scale);
    let origin = view.origin();
    let at = |d: usize| (i64::from(block[d]).div_euclid(s) as f64 + origin[d]) as f32 + 0.5;
    map_centre + Vec2::new(at(0), at(1))
}

/// Pixels `from..=to` around `centre` (in pixels, either way) filled with `colour`.
fn square(painter: &Painter, centre: Pos2, from: [f32; 2], to: [f32; 2], colour: Color32) {
    let rect = Rect::from_min_max(
        centre + Vec2::new(from[0] - 0.5, from[1] - 0.5),
        centre + Vec2::new(to[0] + 0.5, to[1] + 0.5),
    );
    painter.rect_filled(rect, 0.0, colour);
}

/// The ring of pixels at Chebyshev distance `d` from the centre.
fn ring(painter: &Painter, centre: Pos2, d: f32, colour: Color32) {
    square(painter, centre, [-d, -d], [d, -d], colour);
    square(painter, centre, [-d, d], [d, d], colour);
    square(painter, centre, [-d, 1.0 - d], [-d, d - 1.0], colour);
    square(painter, centre, [d, 1.0 - d], [d, d - 1.0], colour);
}

fn rgb([r, g, b]: [u8; 3]) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// A 9 × 9 square with a black border, in the set's colour; a candidate's middle 3 × 3 is left
/// showing the map.
pub fn structure(painter: &Painter, centre: Pos2, start: &StructureStart) {
    let colour = rgb(structure_colour(&start.set));
    ring(painter, centre, 4.0, Color32::BLACK);
    ring(painter, centre, 3.0, colour);
    ring(painter, centre, 2.0, colour);
    if start.certain {
        square(painter, centre, [-1.0, -1.0], [1.0, 1.0], colour);
    }
}

/// A white cross reaching 4 pixels over a black one 3 wide reaching 5.
pub fn spawn(painter: &Painter, centre: Pos2) {
    square(painter, centre, [-5.0, -1.0], [5.0, 1.0], Color32::BLACK);
    square(painter, centre, [-1.0, -5.0], [1.0, 5.0], Color32::BLACK);
    square(painter, centre, [-4.0, 0.0], [4.0, 0.0], Color32::WHITE);
    square(painter, centre, [0.0, -4.0], [0.0, 4.0], Color32::WHITE);
}

/// A locate hit: a ring, yellow if the game certainly stops there.
pub fn hit(painter: &Painter, centre: Pos2, certain: bool) {
    let colour = if certain {
        Color32::from_rgb(0xff, 0xd0, 0x40)
    } else {
        Color32::WHITE
    };
    painter.circle_stroke(centre, 8.0, Stroke::new(4.0, Color32::BLACK));
    painter.circle_stroke(centre, 8.0, Stroke::new(2.0, colour));
}

/// Something on the map the pointer can point at.
pub enum Picked<'a> {
    Structure(&'a StructureStart),
    Spawn(&'a SpawnOrigin),
    Hit(&'a Hit),
}

/// The marker nearest `pointer` within reach, from markers at screen positions.
pub fn pick<'a>(
    pointer: Pos2,
    markers: impl IntoIterator<Item = (Pos2, Picked<'a>)>,
) -> Option<Picked<'a>> {
    markers
        .into_iter()
        .map(|(at, what)| ((at - pointer).length(), what))
        .filter(|(d, _)| *d <= PICK)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, what)| what)
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
    fn the_nearest_marker_within_reach_is_picked() {
        let spawn = SpawnOrigin {
            chunk: [0, 0],
            position: None,
            platform_dependent: false,
        };
        let near = [(Pos2::new(10.0, 10.0), Picked::Spawn(&spawn))];
        assert!(pick(Pos2::new(13.0, 14.0), near).is_some());
        let far = [(Pos2::new(10.0, 10.0), Picked::Spawn(&spawn))];
        assert!(pick(Pos2::new(20.0, 10.0), far).is_none());
    }
}
