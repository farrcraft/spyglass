//! Native seed-map viewer. A front end over the `spyglass` library only (ADR 0014): tiles,
//! colours and markers all come from its `render` module (`docs/specs/render.md`).

use eframe::egui;

mod app;
mod export;
mod locate;
mod markers;
mod tiles;
mod view;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("spyglass")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([640.0, 400.0]),
        // The map is exact colours; dithering would nudge them by one here and there.
        dithering: false,
        ..Default::default()
    };
    eframe::run_native(
        "spyglass",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
