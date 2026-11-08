//! Native seed-map viewer. A front end over the `spyglass` library only (ADR 0014): tiles,
//! colours and markers all come from its `render` module (`docs/specs/render.md`).

// No console window beside the viewer's on Windows. Debug builds keep theirs, for panics and logs.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};

use eframe::egui;

mod app;
mod export;
mod extract;
mod icons;
mod locate;
mod markers;
mod search;
mod share;
mod style;
mod tiles;
mod timing;
mod userdata;
mod view;

// Tile workers and threaded queries allocate from many threads at once (ADR 0064).
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// The app's name, which names its settings folder too.
const APP: &str = "spyglass";

fn main() -> eframe::Result {
    // The one argument the viewer takes: for bug reports, and for checking a release archive on a
    // machine with no display. A Windows release build has no console, so this prints only where
    // stdout is redirected or piped, as the release smoke test runs it.
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("spyglass-viewer {}", spyglass::build::version());
        return Ok(());
    }
    // User data lives beside the settings, in their file's folder when it's given (ADR 0048).
    let settings: Option<PathBuf> = std::env::var_os("SPYGLASS_VIEWER_SETTINGS").map(Into::into);
    let data_dir = match &settings {
        Some(file) => file.parent().map(Path::to_path_buf),
        None => eframe::storage_dir(APP),
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("spyglass")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([640.0, 400.0]),
        // The map is exact colours; dithering would nudge them by one here and there.
        dithering: false,
        // Another settings file than the per-user one, for measuring the viewer from known
        // settings without touching its user's.
        persistence_path: settings.clone(),
        ..Default::default()
    };
    eframe::run_native(
        APP,
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc, data_dir)))),
    )
}
