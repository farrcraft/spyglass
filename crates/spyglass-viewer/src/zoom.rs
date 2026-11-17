//! The zoom rule: the layers that are drawn only up to a scale, and how their controls behave
//! past it. Past its limit a layer's control is greyed, its hover text says how far to zoom in,
//! and a layer that's on stays on, paused: nothing in the settings changes, so it draws again
//! on zooming back in. The map, the jobs and the side panel all ask here, so they can't
//! disagree. The limits are the library's, except structure bounds', which are the viewer's.

use eframe::egui;

use crate::style;

/// The largest scale, in blocks per pixel, a layer is drawn at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limit(pub u32);

/// Ore veins, noise caves and carver caves: the library draws them up to 1:4.
pub const UNDERGROUND: Limit = Limit(spyglass::MAX_UNDERGROUND_SCALE);
/// Features: each chunk they could be in is placed, so only up close.
pub const FEATURES: Limit = Limit(spyglass::MAX_FEATURE_SCALE);
/// Loot containers, which place chunks as features do.
pub const LOOT: Limit = Limit(spyglass::MAX_FEATURE_SCALE);
/// Slime chunks: a square per chunk.
pub const SLIME_CHUNKS: Limit = Limit(spyglass::MAX_SLIME_SCALE);
/// A save's generated chunks, and their time played, drawn as slime chunks are.
pub const GENERATED: Limit = Limit(spyglass::MAX_SLIME_SCALE);
/// Jigsaw and mineshaft bounds, when confirmed: the viewer's own limit, since the library lays
/// a structure out at any scale, but each one shown is a job and their outlines crowd the map.
pub const BOUNDS: Limit = Limit(4);

/// Whether features and loot containers found earlier are drawn past their limit, from the
/// regions already cached, up to [`FOUND_REACH`] times it. Each region drawn is outlined where
/// it meets one not looked at, and those are hatched, so an empty region reads as "nothing
/// here" and a hatched one as "not looked at". Nothing new is computed past the limit
/// (`docs/specs/render.md#reuse-across-scales`).
pub const DRAW_FOUND: bool = true;

/// How many times its limit's scale a layer's points found earlier are drawn out to: 1:4 for
/// features and loot, one step out. At 1:8 dungeons, as dense as features get, pile into one
/// blob, and the hatch covers most of the map.
pub const FOUND_REACH: u32 = 2;

/// What a layer with a limit is doing at a scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerState {
    /// Off.
    Off,
    /// On, and drawn.
    Shown,
    /// On, but past its limit: not drawn until the view zooms back in.
    Paused,
}

impl Limit {
    /// Whether the layer is drawn at `scale`.
    pub fn shows(self, scale: u32) -> bool {
        scale <= self.0
    }

    /// What a layer that's `on` is doing at `scale`.
    pub fn state(self, on: bool, scale: u32) -> LayerState {
        match (on, self.shows(scale)) {
            (false, _) => LayerState::Off,
            (true, true) => LayerState::Shown,
            (true, false) => LayerState::Paused,
        }
    }

    /// Whether points found earlier are drawn at `scale`, past the limit: for the layers whose
    /// points are kept by region, features and loot.
    pub fn draws_found(self, scale: u32) -> bool {
        DRAW_FOUND && !self.shows(scale) && scale <= self.0.saturating_mul(FOUND_REACH)
    }

    /// How far to zoom in: "Zoom to 1:4 or closer".
    pub fn hint(self) -> String {
        format!("Zoom to 1:{} or closer", self.0)
    }
}

/// A layer's checkbox, by the zoom rule: greyed past `limit` at `scale`, with hover text saying
/// how far to zoom in, and "paused" beside it while it's on there. `unavailable` is another
/// reason it can't be drawn here (only in the Overworld, say), which comes first. Hover text the
/// caller adds shows while it's usable.
pub fn checkbox(
    ui: &mut egui::Ui,
    on: &mut bool,
    label: &str,
    limit: Limit,
    scale: u32,
    unavailable: Option<&str>,
) -> egui::Response {
    let usable = unavailable.is_none() && limit.shows(scale);
    let paused = unavailable.is_none() && limit.state(*on, scale) == LayerState::Paused;
    let response = ui
        .horizontal(|ui| {
            let response = ui.add_enabled(usable, egui::Checkbox::new(on, label));
            if paused {
                ui.label(egui::RichText::new("paused").size(12.0).color(style::MUTED));
            }
            response
        })
        .inner;
    let why = match unavailable {
        Some(reason) => reason.to_string(),
        None => format!("{} to see it.", limit.hint()),
    };
    response.on_disabled_hover_text(why)
}

/// The line under a section with layers paused by `limit`: how far to zoom in.
pub fn paused_hint(ui: &mut egui::Ui, limit: Limit) {
    style::hint(ui, &format!("Paused: {}.", limit.hint().to_lowercase()));
}

/// What's drawn of a layer paused by `limit` at `scale`, when points found earlier are: for
/// the hover text and the line under its section.
pub fn found_note(limit: Limit, scale: u32) -> Option<&'static str> {
    limit.draws_found(scale).then_some(
        "What's drawn was found earlier, closer in: outlined where it was looked for, \
         hatched where it wasn't.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_limit_is_shown_to_its_scale_and_paused_past_it() {
        for limit in [UNDERGROUND, FEATURES, LOOT, SLIME_CHUNKS, GENERATED, BOUNDS] {
            let at = limit.0;
            assert_eq!(limit.state(true, at), LayerState::Shown);
            assert_eq!(limit.state(true, at / 2), LayerState::Shown);
            assert_eq!(limit.state(true, at * 2), LayerState::Paused);
            // Back in, it's drawn again: the setting never changed.
            assert_eq!(limit.state(true, at), LayerState::Shown);
            assert_eq!(limit.state(false, at * 2), LayerState::Off);
            assert_eq!(limit.state(false, at), LayerState::Off);
        }
    }

    #[test]
    fn points_found_earlier_are_drawn_to_twice_the_limit() {
        assert!(!FEATURES.draws_found(1));
        assert!(!FEATURES.draws_found(2), "drawn as usual");
        assert_eq!(FEATURES.draws_found(4), DRAW_FOUND);
        assert_eq!(LOOT.draws_found(4), DRAW_FOUND);
        assert!(!LOOT.draws_found(8));
        assert_eq!(found_note(FEATURES, 4).is_some(), DRAW_FOUND);
        assert!(found_note(FEATURES, 2).is_none());
    }

    #[test]
    fn the_hints_are_the_library_s_limits() {
        assert_eq!(UNDERGROUND.hint(), "Zoom to 1:4 or closer");
        assert_eq!(FEATURES.hint(), "Zoom to 1:2 or closer");
        assert_eq!(SLIME_CHUNKS.hint(), "Zoom to 1:16 or closer");
        assert_eq!(
            [
                UNDERGROUND.0,
                FEATURES.0,
                LOOT.0,
                SLIME_CHUNKS.0,
                GENERATED.0
            ],
            [
                spyglass::MAX_UNDERGROUND_SCALE,
                spyglass::MAX_FEATURE_SCALE,
                spyglass::MAX_FEATURE_SCALE,
                spyglass::MAX_SLIME_SCALE,
                spyglass::MAX_SLIME_SCALE,
            ]
        );
    }

    /// A paused layer keeps its setting: the checkbox, greyed, doesn't change it.
    #[test]
    fn a_paused_checkbox_keeps_its_setting() {
        let ctx = egui::Context::default();
        let mut on = true;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let response = checkbox(ui, &mut on, "Dungeons", FEATURES, 16, None);
            assert!(!response.enabled());
        });
        output.textures_delta.clear();
        assert!(on);
    }
}
