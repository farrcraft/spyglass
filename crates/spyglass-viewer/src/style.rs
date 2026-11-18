//! The viewer's look: one dark palette with an amber accent, the spacing and corners every
//! widget shares, and the few small pieces (section headings, hints, overlay frames) the
//! screens are built from. Screens use these names, never colours of their own.

use std::collections::BTreeSet;

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
/// Text.
pub const TEXT: Color32 = Color32::from_rgb(0xe6, 0xe6, 0xe6);
/// Labels and hints: still 7:1 on `PANEL`.
pub const MUTED: Color32 = Color32::from_rgb(0xa3, 0xa6, 0xad);
/// The one accent: the selected tab, the main button, the spawn.
pub const ACCENT: Color32 = Color32::from_rgb(0xe5, 0xa8, 0x4b);
/// Text that says something holds.
pub const GOOD: Color32 = Color32::from_rgb(0x8f, 0xdc, 0x9f);
/// Text that warns: something only maybe holds, or needs the user's attention.
pub const WARN: Color32 = Color32::from_rgb(0xf2, 0xc2, 0x7a);
/// Text that says something failed.
pub const ERROR: Color32 = Color32::from_rgb(0xf0, 0x80, 0x78);
/// Floating frames over the map: `PANEL`, nearly opaque.
pub const OVERLAY: Color32 = Color32::from_rgba_premultiplied(0x1a, 0x1b, 0x1f, 0xf0);

/// Sets the viewer's look on `ctx`: dark whatever the system prefers.
/// The space around a button's or a text box's text.
const BUTTON_PADDING: Vec2 = Vec2::new(8.0, 4.0);

pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(Theme::Dark);
    ctx.style_mut_of(Theme::Dark, |style| {
        let spacing = &mut style.spacing;
        spacing.item_spacing = Vec2::new(8.0, 6.0);
        spacing.button_padding = BUTTON_PADDING;
        spacing.interact_size.y = 24.0;
        spacing.menu_margin = Margin::same(6);
        spacing.window_margin = Margin::same(12);

        let visuals = &mut style.visuals;
        visuals.dark_mode = true;
        visuals.panel_fill = PANEL;
        visuals.window_fill = PANEL;
        visuals.window_stroke = Stroke::new(1.0, BORDER);
        visuals.window_corner_radius = CornerRadius::same(10);
        visuals.menu_corner_radius = CornerRadius::same(8);
        visuals.window_shadow = Shadow {
            offset: [0, 6],
            blur: 20,
            spread: 0,
            color: Color32::from_black_alpha(110),
        };
        visuals.popup_shadow = visuals.window_shadow;
        visuals.extreme_bg_color = GROUND;
        visuals.text_edit_bg_color = Some(GROUND);
        visuals.faint_bg_color = SURFACE;
        visuals.code_bg_color = GROUND;
        visuals.hyperlink_color = ACCENT;
        visuals.warn_fg_color = WARN;
        visuals.error_fg_color = ERROR;
        visuals.selection.bg_fill = Color32::from_rgb(0x5a, 0x43, 0x1e);
        visuals.selection.stroke = Stroke::new(1.0, ACCENT);
        visuals.slider_trailing_fill = true;

        let radius = CornerRadius::same(6);
        let widgets = &mut visuals.widgets;
        widgets.noninteractive.bg_fill = PANEL;
        widgets.noninteractive.weak_bg_fill = PANEL;
        widgets.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x2c, 0x2e, 0x33));
        widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
        widgets.noninteractive.corner_radius = radius;
        for (state, fill, stroke) in [
            (&mut widgets.inactive, SURFACE, BORDER),
            (
                &mut widgets.hovered,
                RAISED,
                Color32::from_rgb(0x4a, 0x4c, 0x52),
            ),
            (
                &mut widgets.active,
                Color32::from_rgb(0x3e, 0x40, 0x47),
                ACCENT,
            ),
            (&mut widgets.open, RAISED, BORDER),
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

/// Space between a panel's sections, split around the rule above each.
pub const SECTION_GAP: f32 = 12.0;
/// How far a section's body is indented: past the fold chevron, under its title.
pub const SECTION_INDENT: f32 = 16.0;

/// What a section's header says after its title while it's folded: a count, what's on, or
/// its warnings, which are drawn in the warning colour so a fold never hides one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Summary {
    /// The text; nothing is drawn if it's empty.
    pub text: String,
    /// Whether it's a warning.
    pub warns: bool,
}

impl Summary {
    /// A plain summary.
    pub fn new(text: impl Into<String>) -> Self {
        Summary {
            text: text.into(),
            warns: false,
        }
    }

    /// A summary that warns.
    pub fn warning(text: impl Into<String>) -> Self {
        Summary {
            text: text.into(),
            warns: true,
        }
    }
}

/// A section of a panel, by its stable id (`layers.underground`), which its fold is kept under.
pub struct Section<'a> {
    /// The id the fold is kept under in the settings.
    pub id: &'a str,
    /// The title.
    pub title: &'a str,
    /// What the header says while folded.
    pub summary: Summary,
    /// Folded until the user opens it.
    pub folded_by_default: bool,
}

impl<'a> Section<'a> {
    /// An open section with nothing to say when folded.
    pub fn new(id: &'a str, title: &'a str) -> Self {
        Section {
            id,
            title,
            summary: Summary::default(),
            folded_by_default: false,
        }
    }

    /// With `summary` shown while it's folded.
    pub fn summary(self, summary: Summary) -> Self {
        Section { summary, ..self }
    }

    /// Folded until the user opens it.
    pub fn folded_by_default(self) -> Self {
        Section {
            folded_by_default: true,
            ..self
        }
    }
}

/// Whether section `id` is folded, by `folded`, the ids whose fold isn't their default.
pub fn is_folded(folded: &BTreeSet<String>, id: &str, by_default: bool) -> bool {
    folded.contains(id) != by_default
}

/// Opens or folds section `id`, keeping in `folded` only the ids not in their default state.
pub fn set_folded(folded: &mut BTreeSet<String>, id: &str, by_default: bool, fold: bool) {
    if fold == by_default {
        folded.remove(id);
    } else {
        folded.insert(id.to_string());
    }
}

/// A section's header: a rule above it, a chevron and the title, which fold and open it,
/// the summary while folded, and `right`'s controls at the right. Returns whether it's open;
/// draw its body with [`section_body`] if it is.
pub fn section(
    ui: &mut egui::Ui,
    folded: &mut BTreeSet<String>,
    section: Section<'_>,
    right: impl FnOnce(&mut egui::Ui),
) -> bool {
    let was_folded = is_folded(folded, section.id, section.folded_by_default);
    // The first section in a panel has the panel's own line above it.
    let first = ui.cursor().top() - ui.min_rect().top() < SECTION_GAP;
    if first {
        ui.add_space(SECTION_GAP / 2.0 - ui.spacing().item_spacing.y);
    } else {
        ui.add_space(SECTION_GAP / 2.0);
        let (rule, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
        ui.painter().rect_filled(rule, 0.0, BORDER);
        ui.add_space(SECTION_GAP / 2.0 - ui.spacing().item_spacing.y);
    }
    let toggled = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let (icon, chevron) = ui
                .allocate_exact_size(egui::vec2(SECTION_INDENT - 6.0, 14.0), egui::Sense::click());
            let colour = if chevron.hovered() { TEXT } else { MUTED };
            let c = icon.center();
            let points = if was_folded {
                vec![
                    c + egui::vec2(-2.5, -4.5),
                    c + egui::vec2(3.5, 0.0),
                    c + egui::vec2(-2.5, 4.5),
                ]
            } else {
                vec![
                    c + egui::vec2(-4.5, -2.5),
                    c + egui::vec2(4.5, -2.5),
                    c + egui::vec2(0.0, 3.5),
                ]
            };
            ui.painter()
                .add(egui::Shape::convex_polygon(points, colour, Stroke::NONE));
            let title = ui.add(
                egui::Label::new(RichText::new(section.title).size(14.5).strong().color(TEXT))
                    .sense(egui::Sense::click())
                    .selectable(false),
            );
            if was_folded && !section.summary.text.is_empty() {
                let colour = if section.summary.warns { WARN } else { MUTED };
                ui.add(
                    egui::Label::new(
                        RichText::new(&section.summary.text)
                            .size(12.0)
                            .color(colour),
                    )
                    .truncate(),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), right);
            let hover = if was_folded { "Show" } else { "Fold" };
            let chevron = chevron.on_hover_text(hover);
            let title = title.on_hover_text(hover);
            chevron.clicked() || title.clicked()
        })
        .inner;
    if toggled {
        set_folded(folded, section.id, section.folded_by_default, !was_folded);
    }
    !is_folded(folded, section.id, section.folded_by_default)
}

/// A section's body, indented under its title.
pub fn section_body<R>(ui: &mut egui::Ui, body: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .inner_margin(Margin {
            left: SECTION_INDENT as i8,
            ..Margin::ZERO
        })
        .show(ui, body)
        .inner
}

/// A heading within a section, such as a list's: small, spaced capitals in the muted colour.
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

/// A single-line text box, padded as it is in a panel wherever it is. egui takes a text box's
/// padding from the buttons', which a menu shrinks to almost nothing (`menu_style`), so in a
/// menu the text would sit in the box's top-left corner.
pub fn text_box(text: &mut dyn egui::TextBuffer) -> egui::TextEdit<'_> {
    egui::TextEdit::singleline(text)
        .margin(Margin::symmetric(
            BUTTON_PADDING.x as i8,
            BUTTON_PADDING.y as i8,
        ))
        .align(egui::Align2::LEFT_CENTER)
}

/// A checkbox in a menu, its box drawn as in a panel. A menu takes away its widgets' resting
/// fill and outline (`menu_style`), so its rows don't read as buttons, which leaves an
/// unchecked box almost invisible.
pub fn menu_checkbox(
    ui: &mut egui::Ui,
    checked: &mut bool,
    text: impl Into<egui::WidgetText>,
) -> egui::Response {
    let panel = ui.ctx().global_style();
    ui.scope(|ui| {
        framed_as(ui.style_mut(), &panel);
        ui.checkbox(checked, text)
    })
    .inner
}

/// `style`'s widgets with `panel`'s fills and outlines, as they are outside a menu.
fn framed_as(style: &mut egui::Style, panel: &egui::Style) {
    let (widgets, panel) = (&mut style.visuals.widgets, &panel.visuals.widgets);
    for (widget, panel) in [
        (&mut widgets.inactive, &panel.inactive),
        (&mut widgets.hovered, &panel.hovered),
        (&mut widgets.active, &panel.active),
        (&mut widgets.open, &panel.open),
    ] {
        widget.bg_fill = panel.bg_fill;
        widget.weak_bg_fill = panel.weak_bg_fill;
        widget.bg_stroke = panel.bg_stroke;
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Where a text box's text sits in it, `[left, above, below]`, in a menu or not.
    fn padding(in_menu: bool, text_box: impl Fn(&mut String) -> egui::TextEdit<'_>) -> [f32; 3] {
        let ctx = egui::Context::default();
        apply(&ctx);
        let mut padding = [0.0; 3];
        let mut text = "Filter saves".to_string();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            if in_menu {
                egui::containers::menu::menu_style(ui.style_mut());
            }
            let output = text_box(&mut text).show(ui);
            let rect = output.response.rect;
            let text_rect = egui::Rect::from_min_size(output.galley_pos, output.galley.size());
            padding = [
                text_rect.left() - rect.left(),
                text_rect.top() - rect.top(),
                rect.bottom() - text_rect.bottom(),
            ];
        });
        // The font atlas made on the first frame, which nothing here uploads.
        output.textures_delta.clear();
        padding
    }

    #[test]
    fn a_text_box_in_a_menu_is_padded_as_in_a_panel() {
        let panel = padding(false, |t| egui::TextEdit::singleline(t));
        for in_menu in [false, true] {
            let ours = padding(in_menu, |t| text_box(t));
            for (ours, panel) in ours.into_iter().zip(panel) {
                assert!((ours - panel).abs() <= 1.0, "{ours:?} against {panel:?}");
            }
        }
    }

    #[test]
    fn a_menu_s_checkbox_is_framed_as_in_a_panel() {
        let ctx = egui::Context::default();
        apply(&ctx);
        let panel = ctx.global_style();
        let mut menu = (*panel).clone();
        egui::containers::menu::menu_style(&mut menu);
        assert_eq!(menu.visuals.widgets.inactive.bg_stroke, Stroke::NONE);
        framed_as(&mut menu, &panel);
        assert_eq!(menu.visuals.widgets, panel.visuals.widgets);
    }

    #[test]
    fn only_folds_away_from_the_default_are_kept() {
        let mut folded = BTreeSet::new();
        assert!(!is_folded(&folded, "layers.biomes", false));
        assert!(is_folded(&folded, "search.range", true));
        set_folded(&mut folded, "layers.biomes", false, true);
        set_folded(&mut folded, "search.range", true, false);
        assert!(is_folded(&folded, "layers.biomes", false));
        assert!(!is_folded(&folded, "search.range", true));
        set_folded(&mut folded, "layers.biomes", false, false);
        set_folded(&mut folded, "search.range", true, true);
        assert!(folded.is_empty());
    }

    #[test]
    fn egui_s_text_box_in_a_menu_is_not() {
        // What `text_box` is for: if this starts failing, egui pads a menu's text box itself.
        let [left, above, _] = padding(true, |t| egui::TextEdit::singleline(t));
        let [panel_left, panel_above, _] = padding(false, |t| egui::TextEdit::singleline(t));
        assert!(left < panel_left - 1.0 && above < panel_above - 1.0);
    }
}
