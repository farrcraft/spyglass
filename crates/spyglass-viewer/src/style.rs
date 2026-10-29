//! The viewer's look: one dark palette with an amber accent, the spacing and corners every
//! widget shares, and the few small pieces (section headings, hints, overlay frames) the
//! screens are built from. Screens use these names, never colours of their own.

use eframe::egui::{
    self, Color32, CornerRadius, FontId, Margin, RichText, Shadow, Stroke, Theme, Vec2,
};

/// Behind everything: the map's surround and text fields.
pub const GROUND: Color32 = Color32::from_rgb(0x14, 0x15, 0x18);
/// Panels, the toolbar and the status bar.
pub const PANEL: Color32 = Color32::from_rgb(0x1c, 0x1d, 0x21);
/// Buttons and other raised controls.
pub const SURFACE: Color32 = Color32::from_rgb(0x25, 0x26, 0x2b);
/// A control under the pointer, or a selected row.
pub const RAISED: Color32 = Color32::from_rgb(0x33, 0x35, 0x3b);
/// Lines between panels and around controls.
pub const BORDER: Color32 = Color32::from_rgb(0x3a, 0x3c, 0x42);
pub const TEXT: Color32 = Color32::from_rgb(0xe6, 0xe6, 0xe6);
/// Labels and hints: still 7:1 on `PANEL`.
pub const MUTED: Color32 = Color32::from_rgb(0xa3, 0xa6, 0xad);
/// The one accent: the selected tab, the main button, the spawn.
pub const ACCENT: Color32 = Color32::from_rgb(0xe5, 0xa8, 0x4b);
/// Text that says something holds.
pub const GOOD: Color32 = Color32::from_rgb(0x8f, 0xdc, 0x9f);
pub const WARN: Color32 = Color32::from_rgb(0xf2, 0xc2, 0x7a);
pub const ERROR: Color32 = Color32::from_rgb(0xf0, 0x80, 0x78);
/// Floating frames over the map: `PANEL`, nearly opaque.
pub const OVERLAY: Color32 = Color32::from_rgba_premultiplied(0x1a, 0x1b, 0x1f, 0xf0);

/// Sets the viewer's look on `ctx`: dark whatever the system prefers.
pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(Theme::Dark);
    ctx.style_mut_of(Theme::Dark, |style| {
        let spacing = &mut style.spacing;
        spacing.item_spacing = Vec2::new(8.0, 6.0);
        spacing.button_padding = Vec2::new(8.0, 4.0);
        spacing.interact_size.y = 24.0;
        spacing.menu_margin = Margin::same(6);
        spacing.window_margin = Margin::same(12);

        let v = &mut style.visuals;
        v.dark_mode = true;
        v.panel_fill = PANEL;
        v.window_fill = PANEL;
        v.window_stroke = Stroke::new(1.0, BORDER);
        v.window_corner_radius = CornerRadius::same(10);
        v.menu_corner_radius = CornerRadius::same(8);
        v.window_shadow = Shadow {
            offset: [0, 6],
            blur: 20,
            spread: 0,
            color: Color32::from_black_alpha(110),
        };
        v.popup_shadow = v.window_shadow;
        v.extreme_bg_color = GROUND;
        v.text_edit_bg_color = Some(GROUND);
        v.faint_bg_color = SURFACE;
        v.code_bg_color = GROUND;
        v.hyperlink_color = ACCENT;
        v.warn_fg_color = WARN;
        v.error_fg_color = ERROR;
        v.selection.bg_fill = Color32::from_rgb(0x5a, 0x43, 0x1e);
        v.selection.stroke = Stroke::new(1.0, ACCENT);
        v.slider_trailing_fill = true;

        let radius = CornerRadius::same(6);
        let w = &mut v.widgets;
        w.noninteractive.bg_fill = PANEL;
        w.noninteractive.weak_bg_fill = PANEL;
        w.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x2c, 0x2e, 0x33));
        w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
        w.noninteractive.corner_radius = radius;
        for (state, fill, stroke) in [
            (&mut w.inactive, SURFACE, BORDER),
            (&mut w.hovered, RAISED, Color32::from_rgb(0x4a, 0x4c, 0x52)),
            (&mut w.active, Color32::from_rgb(0x3e, 0x40, 0x47), ACCENT),
            (&mut w.open, RAISED, BORDER),
        ] {
            state.bg_fill = fill;
            state.weak_bg_fill = fill;
            state.bg_stroke = Stroke::new(1.0, stroke);
            state.fg_stroke = Stroke::new(1.0, TEXT);
            state.corner_radius = radius;
            state.expansion = 0.0;
        }
    });
}

/// A section's heading in a panel: small, spaced capitals in the muted colour.
pub fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(4.0);
    ui.label(
        RichText::new(text.to_uppercase())
            .size(11.0)
            .strong()
            .extra_letter_spacing(0.8)
            .color(MUTED),
    );
}

/// A line under a control saying what it does, or why it's off.
pub fn hint(ui: &mut egui::Ui, text: &str) {
    ui.horizontal(|ui| {
        ui.add_space(ui.spacing().icon_width + ui.spacing().icon_spacing);
        ui.add(egui::Label::new(RichText::new(text).size(12.0).color(MUTED)).wrap());
    });
}

/// Numbers: coordinates, seeds, scores.
pub fn mono(text: impl Into<String>) -> RichText {
    RichText::new(text).font(FontId::monospace(12.5))
}

/// The frame of something floating over the map.
pub fn overlay() -> egui::Frame {
    egui::Frame::new()
        .fill(OVERLAY)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(10, 8))
        .shadow(Shadow {
            offset: [0, 4],
            blur: 14,
            spread: 0,
            color: Color32::from_black_alpha(90),
        })
}
