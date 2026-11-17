//! The controls over the map, the legend, the open search hit's card and the scale bar.

use super::*;

/// The scale bar's longest length, in points.
const SCALE_BAR: f32 = 120.0;

impl App {
    /// The controls over the map: go to (top left), the open search hit's card under it, and
    /// centre on spawn and zoom in a row (top right).
    pub(super) fn map_controls(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        spawn: [i32; 2],
        focus_goto: bool,
    ) {
        let inner = rect.shrink(12.0);
        let step = self.go_to_controls(ui, inner, focus_goto);
        if step != 0 {
            self.step_match(ui.ctx(), step);
        }
        self.zoom_controls(ui, inner, spawn);
    }

    /// Top left: the go-to field, which G focuses, and the open search hit's card under it.
    /// Returns the card's step through the hit's matches: 1, −1 or 0.
    pub(super) fn go_to_controls(
        &mut self,
        ui: &mut egui::Ui,
        inner: Rect,
        focus_goto: bool,
    ) -> isize {
        let mut step = 0;
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(egui::Layout::top_down(egui::Align::Min)),
            |ui| {
                // The field is the whole box, its text centred in it; G focuses it.
                style::overlay()
                    .inner_margin(egui::Margin::same(0))
                    .show(ui, |ui| {
                        let edit = ui
                            .add(
                                style::text_box(&mut self.goto)
                                    .font(egui::TextStyle::Monospace)
                                    .hint_text("Go to x z  or  x y z")
                                    // The field's padding is its frame's, so an empty frame
                                    // with a margin.
                                    .frame(
                                        egui::Frame::NONE
                                            .inner_margin(egui::Margin::symmetric(14, 0)),
                                    )
                                    .align(egui::Align2::LEFT_CENTER)
                                    .min_size(egui::vec2(240.0, 34.0))
                                    .desired_width(240.0),
                            )
                            .on_hover_text("Press G to type a position, then Enter");
                        if focus_goto {
                            edit.request_focus();
                        }
                        if edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                            match parse_goto(&self.goto) {
                                Some((x, y, z)) => {
                                    self.settings.view.centre =
                                        [f64::from(x) + 0.5, f64::from(z) + 0.5];
                                    if let Some(y) = y {
                                        self.settings.view.y = y;
                                    }
                                    self.goto_error = false;
                                }
                                None => self.goto_error = true,
                            }
                        }
                        if self.goto_error {
                            ui.add(egui::Label::new(
                                egui::RichText::new("Type x z or x y z, in blocks.")
                                    .size(12.0)
                                    .color(style::ERROR),
                            ));
                            ui.add_space(6.0);
                        }
                    });
                if let Some(opened) = &self.opened
                    && self
                        .shown
                        .as_ref()
                        .is_some_and(|s| s.session.seed() == opened.seed)
                {
                    ui.add_space(8.0);
                    step = hit_card(ui, opened);
                }
            },
        );
        step
    }

    /// Top right, in one row: centre on spawn, then zoom as − 1:N +. Right to left, so the
    /// zoom is in the corner.
    pub(super) fn zoom_controls(&mut self, ui: &mut egui::Ui, inner: Rect, spawn: [i32; 2]) {
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(egui::Layout::right_to_left(egui::Align::Min)),
            |ui| {
                // Each part as wide as its text plus a little padding, so the control is as
                // wide as the scale it shows.
                const HEIGHT: f32 = 28.0;
                const PAD: f32 = 7.0;
                let text_width = |text: &str, font: egui::FontId| {
                    ui.painter()
                        .layout_no_wrap(text.to_string(), font, style::TEXT)
                        .size()
                        .x
                };
                let symbol = egui::FontId::proportional(17.0);
                let number = egui::FontId::monospace(11.5);
                let scale_text = format!("1:{}", self.settings.view.scale);
                let minus = egui::vec2(text_width("−", symbol.clone()) + 2.0 * PAD, HEIGHT);
                let plus = egui::vec2(text_width("+", symbol.clone()) + 2.0 * PAD, HEIGHT);
                let scale = egui::vec2(text_width(&scale_text, number.clone()) + PAD, HEIGHT);
                let button = egui::vec2(HEIGHT, HEIGHT);
                style::overlay()
                    .inner_margin(egui::Margin::same(4))
                    .show(ui, |ui| {
                        ui.allocate_ui_with_layout(
                            egui::vec2(minus.x + scale.x + plus.x, HEIGHT),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                let view = &mut self.settings.view;
                                let zoom = |ui: &mut egui::Ui, text: &str, size, hint: &str| {
                                    let text = egui::RichText::new(text).font(symbol.clone());
                                    ui.add(egui::Button::new(text).frame(false).min_size(size))
                                        .on_hover_text(hint)
                                        .clicked()
                                };
                                if zoom(ui, "−", minus, "Zoom out (−)") {
                                    view.zoom(false, [0.0, 0.0]);
                                }
                                ui.add_sized(
                                    scale,
                                    egui::Label::new(
                                        egui::RichText::new(scale_text).font(number.clone()),
                                    ),
                                )
                                .on_hover_text(format!(
                                    "1 pixel = {} block{}",
                                    view.scale,
                                    if view.scale == 1 { "" } else { "s" }
                                ));
                                if zoom(ui, "+", plus, "Zoom in (+)") {
                                    view.zoom(true, [0.0, 0.0]);
                                }
                            },
                        );
                    });
                style::overlay()
                    .inner_margin(egui::Margin::same(4))
                    .show(ui, |ui| {
                        let (button_rect, response) =
                            ui.allocate_exact_size(button, Sense::click());
                        if response.hovered() {
                            ui.painter().rect_filled(button_rect, 5.0, style::RAISED);
                        }
                        markers::spawn_sized(ui.painter(), button_rect.center(), 7.0);
                        if response.on_hover_text("Centre on the spawn").clicked() {
                            self.settings.view.centre =
                                [f64::from(spawn[0]) + 0.5, f64::from(spawn[1]) + 0.5];
                        }
                    });
            },
        );
    }

    /// What's marked, bottom right: the structure families chosen, highlighted biomes and the
    /// spawn. Painted, not laid out, so it takes only the room its rows need.
    pub(super) fn map_legend(&self, ui: &egui::Ui, rect: Rect, world: &World) {
        let rows = self.legend_rows(world);
        if rows.is_empty() {
            return;
        }
        let painter = ui.painter_at(rect);
        let font = egui::FontId::proportional(12.5);
        let galleys: Vec<_> = rows
            .iter()
            .map(|(_, text)| painter.layout_no_wrap(text.clone(), font.clone(), style::TEXT))
            .collect();
        let (swatch, gap, row, pad) = (14.0, 8.0, 20.0, 10.0);
        let width = galleys.iter().map(|g| g.size().x).fold(0.0, f32::max) + swatch + gap;
        let size = Vec2::new(width + 2.0 * pad, row * rows.len() as f32 + 2.0 * pad - 6.0);
        let frame = Rect::from_min_size(rect.right_bottom() - Vec2::splat(12.0) - size, size);
        painter.rect(
            frame,
            8.0,
            style::OVERLAY,
            egui::Stroke::new(1.0, style::BORDER),
            egui::StrokeKind::Inside,
        );
        for (i, ((key, _), galley)) in rows.iter().zip(galleys).enumerate() {
            let top = frame.top() + pad + row * i as f32;
            let swatch_rect =
                Rect::from_min_size(Pos2::new(frame.left() + pad, top), Vec2::splat(swatch));
            paint_swatch(ui, &painter, world, key, swatch_rect);
            let colour = if matches!(key, LegendKey::More) {
                style::MUTED
            } else {
                style::TEXT
            };
            let at = Pos2::new(
                swatch_rect.right() + gap,
                swatch_rect.center().y - galley.size().y / 2.0,
            );
            painter.galley(at, galley, colour);
        }
    }

    /// The legend's rows, each with its swatch and its text.
    pub(super) fn legend_rows<'w>(&'w self, world: &World) -> Vec<(LegendKey<'w>, String)> {
        const MAX_ROWS: usize = 8;
        // One row per family chosen, with the first of its structures for the icon.
        let mut families: Vec<(String, &str)> = Vec::new();
        for id in self
            .settings
            .structures
            .iter()
            .filter(|id| world.structure_generates(id))
        {
            let name = icons::family_name(id);
            if !families.iter().any(|(n, _)| *n == name) {
                families.push((name, id));
            }
        }
        families.sort();
        let mut rows: Vec<(LegendKey<'w>, String)> = Vec::new();
        let more = families.len().saturating_sub(MAX_ROWS);
        for (name, id) in families.into_iter().take(MAX_ROWS) {
            rows.push((LegendKey::Structure(id), name));
        }
        if more > 0 {
            rows.push((LegendKey::More, format!("and {more} more")));
        }
        for id in self.settings.highlight.iter().take(MAX_ROWS) {
            rows.push((LegendKey::Biome(id), spyglass::display_name(id)));
        }
        if self.settings.spawn && self.spawn_allowed() {
            rows.push((LegendKey::Spawn, "World spawn".to_string()));
        }
        rows
    }
}

/// A legend row's swatch.
pub(super) enum LegendKey<'a> {
    /// A structure's family, by its first structure id.
    Structure(&'a str),
    /// A highlighted biome, by id.
    Biome(&'a str),
    /// The world spawn.
    Spawn,
    /// Families past the first few, unlisted.
    More,
}

/// Paints a legend row's swatch for `key` in `swatch_rect`: a structure's badge, a biome's colour, or
/// the spawn's marker.
pub(super) fn paint_swatch(
    ui: &egui::Ui,
    painter: &egui::Painter,
    world: &World,
    key: &LegendKey<'_>,
    swatch_rect: Rect,
) {
    match key {
        LegendKey::Structure(id) => {
            if let Some(badge) = world.structure_set(id).map(icons::badge) {
                painter.rect(
                    swatch_rect,
                    egui::CornerRadius::same(4),
                    badge.fill,
                    egui::Stroke::new(1.0, style::GROUND),
                    egui::StrokeKind::Inside,
                );
                icons::glyph(id)
                    .tint(badge.ink)
                    .paint_at(ui, swatch_rect.shrink(2.5));
            }
        }
        LegendKey::Biome(id) => {
            let [red, green, blue] = spyglass::biome_colour(id);
            painter.rect_filled(
                swatch_rect.shrink(1.5),
                2.0,
                Color32::from_rgb(red, green, blue),
            );
        }
        LegendKey::Spawn => markers::spawn_sized(painter, swatch_rect.center(), 6.5),
        LegendKey::More => {}
    }
}

/// The open search hit or find match: its seed, and each constraint with its outcome. Returns
/// 1 or −1 when the next or previous match is asked for.
pub(super) fn hit_card(ui: &mut egui::Ui, opened: &Opened) -> isize {
    let mut step = 0;
    style::overlay().show(ui, |ui| {
        ui.set_max_width(460.0);
        ui.horizontal(|ui| {
            ui.strong(&opened.title);
            ui.label(style::mono(opened.seed.to_string()).color(style::MUTED));
            if ui.small_button("Copy seed").clicked() {
                ui.ctx().copy_text(opened.seed.to_string());
            }
            if let Some((at, count)) = opened.step {
                if ui
                    .add_enabled(at > 0, egui::Button::new("Previous").small())
                    .on_hover_text("Previous (P)")
                    .clicked()
                {
                    step = -1;
                }
                if ui
                    .add_enabled(at + 1 < count, egui::Button::new("Next").small())
                    .on_hover_text("Next (N)")
                    .clicked()
                {
                    step = 1;
                }
            }
        });
        for constraint in &opened.constraints {
            ui.horizontal(|ui| {
                let (mark, colour, what) = match constraint.outcome {
                    Outcome::Met => ("Met", style::GOOD, "It holds whatever is undecided."),
                    Outcome::PossiblyMet => (
                        "Maybe",
                        style::WARN,
                        "It holds only in the best case: it depends on terrain, a tied biome or \
                         the platform.",
                    ),
                    Outcome::Failed => ("No", style::MUTED, "It doesn't hold."),
                };
                ui.add_sized(
                    egui::vec2(40.0, 16.0),
                    egui::Label::new(egui::RichText::new(mark).size(11.5).strong().color(colour)),
                )
                .on_hover_text(what);
                ui.add(
                    egui::Label::new(search_panel::highlight(&constraint.line, 12.0)).truncate(),
                );
            });
        }
    });
    step
}

/// A scale bar, bottom left: a round number of blocks.
pub(super) fn scale_bar(painter: &egui::Painter, rect: Rect, scale: u32) {
    let (blocks, length) = bar_length(scale, SCALE_BAR);
    let text = format!("{} blocks", thousands(blocks));
    let galley = painter.layout_no_wrap(text, egui::FontId::monospace(11.0), style::TEXT);
    let width = length.max(galley.size().x);
    let frame = Rect::from_min_size(
        Pos2::new(rect.left() + 12.0, rect.bottom() - 12.0 - 30.0),
        Vec2::new(width + 20.0, 30.0),
    );
    painter.rect_filled(frame, 6.0, style::OVERLAY);
    let y = frame.top() + 9.0;
    let (x0, x1) = (frame.left() + 10.0, frame.left() + 10.0 + length);
    let line = egui::Stroke::new(2.0, style::TEXT);
    painter.line_segment([Pos2::new(x0, y), Pos2::new(x1, y)], line);
    painter.line_segment([Pos2::new(x0, y - 4.0), Pos2::new(x0, y)], line);
    painter.line_segment([Pos2::new(x1, y - 4.0), Pos2::new(x1, y)], line);
    painter.galley(Pos2::new(x0, y + 4.0), galley, style::TEXT);
}

/// A scale bar's length at `scale` blocks per point: the largest 1, 2 or 5 times a power of ten
/// blocks at most `max` points long, and its length in points.
pub(super) fn bar_length(scale: u32, max: f32) -> (u32, f32) {
    let mut best = 1;
    let mut step: u32 = 1;
    'search: loop {
        for multiple in [1, 2, 5] {
            let Some(blocks) = step.checked_mul(multiple) else {
                break 'search;
            };
            if blocks as f32 / scale as f32 > max {
                break 'search;
            }
            best = blocks;
        }
        let Some(next) = step.checked_mul(10) else {
            break;
        };
        step = next;
    }
    (best, best as f32 / scale as f32)
}

/// `n` with commas between thousands.
pub(super) fn thousands(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scale_bar_is_a_round_length_at_every_zoom() {
        let mut scale = 1;
        while scale <= spyglass::MAX_SCALE {
            let (blocks, points) = bar_length(scale, SCALE_BAR);
            let mut lead = blocks;
            while lead >= 10 && lead % 10 == 0 {
                lead /= 10;
            }
            assert!(matches!(lead, 1 | 2 | 5), "{blocks} at 1:{scale}");
            // At most the longest, and never less than 40% of it: the next round length up
            // would be at most 2.5 times longer.
            assert!(
                (SCALE_BAR * 0.4..=SCALE_BAR).contains(&points),
                "{points} at 1:{scale}"
            );
            assert_eq!(points, blocks as f32 / scale as f32);
            scale *= 2;
        }
        assert_eq!(bar_length(4, SCALE_BAR), (200, 50.0));
        assert_eq!(thousands(200_000), "200,000");
        assert_eq!(thousands(5), "5");
        assert_eq!(thousands(1000), "1,000");
    }
}
