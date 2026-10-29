//! The Search tab: the query, its options, the run and its best seeds.

use spyglass::Outcome;

use super::*;
use crate::search::Area;
use crate::style;

/// A search hit on the map: its seed, what its constraints matched, and each constraint's line,
/// outcome and area.
pub(super) struct Opened {
    pub seed: i64,
    pub matches: Vec<Match>,
    pub constraints: Vec<OpenedConstraint>,
}

pub(super) struct OpenedConstraint {
    pub line: String,
    pub outcome: Outcome,
    pub dimension: Dimension,
    pub area: Area,
}

impl App {
    /// The search query, its options, and the best seeds so far: click one to open it.
    pub(super) fn search_section(&mut self, ui: &mut egui::Ui) {
        let Some(game) = self.game else {
            ui.label("Open a bundle to search.");
            return;
        };
        let s = &mut self.settings.search;
        style::heading(ui, "Query");
        let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap: f32| {
            let mut job = highlight(text.as_str(), 13.0);
            job.wrap.max_width = wrap;
            ui.fonts_mut(|f| f.layout_job(job))
        };
        ui.add(
            egui::TextEdit::multiline(&mut s.query)
                .code_editor()
                .desired_rows(4)
                .desired_width(f32::INFINITY)
                .hint_text("require 2 minecraft:swamp_hut within 600 of spawn")
                .layouter(&mut layouter),
        );
        // Resolving a query can load the Nether or the End, so it's done off this thread; a
        // result for text that has since changed is dropped.
        if let Some((text, pending)) = &self.search_parsing
            && let Some(result) = pending.poll()
        {
            let parsed = result.unwrap_or_else(|()| Err("the query check failed".to_string()));
            self.search_parsed = Some((text.clone(), parsed));
            self.search_parsing = None;
        }
        let is_current = |text: Option<&String>| text.is_some_and(|t| *t == s.query);
        if !is_current(self.search_parsed.as_ref().map(|(t, _)| t))
            && !is_current(self.search_parsing.as_ref().map(|(t, _)| t))
        {
            let text = s.query.clone();
            let pending = Pending::spawn(ui.ctx(), {
                let text = text.clone();
                move || game.search_query(&text).map_err(|e| e.to_string())
            });
            self.search_parsing = Some((text, pending));
        }
        let parsed = self
            .search_parsed
            .as_ref()
            .filter(|(text, _)| *text == s.query)
            .map(|(_, p)| p);
        match parsed {
            Some(Err(e)) => {
                ui.colored_label(style::ERROR, e);
            }
            None => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.weak("Checking the query…");
                });
            }
            Some(Ok(query)) => {
                let (required, preferred) = search::counts(query.query());
                ui.colored_label(
                    style::GOOD,
                    format!("Valid: {required} required, {preferred} preferred"),
                );
            }
        }

        ui.add_space(6.0);
        style::heading(ui, "Options");
        egui::Grid::new("search options")
            .num_columns(2)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                ui.label("Keep the best");
                ui.add(egui::DragValue::new(&mut s.top).range(1..=1000));
                ui.end_row();
                ui.label("Time limit");
                ui.add(egui::DragValue::new(&mut s.seconds).suffix(" s"))
                    .on_hover_text("0 for no time limit.");
                ui.end_row();
                ui.label("Seed limit");
                ui.add(
                    egui::TextEdit::singleline(&mut s.seeds)
                        .hint_text("none")
                        .desired_width(90.0),
                );
                ui.end_row();
            });
        ui.checkbox(&mut s.confirm, "Confirm with terrain").on_hover_text(
            "Decide structure starts with each seed's terrain, as the game does. Slower, but only \
             ties and platform-dependent results stay uncertain.",
        );
        ui.checkbox(&mut s.certain, "Certain only").on_hover_text(
            "Only seeds whose required constraints hold whatever is left undecided, ranked by the \
             worst case. Without confirming, most structures are undecided.",
        );
        egui::CollapsingHeader::new("Seed range and start")
            .default_open(s.range || s.from > 0)
            .show(ui, |ui| {
                ui.checkbox(&mut s.range, "Only seeds in a range")
                    .on_hover_text(
                        "Otherwise every 64-bit seed, with structure placement checked first.",
                    );
                if s.range {
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut s.first).desired_width(100.0));
                        ui.label("to");
                        ui.add(egui::TextEdit::singleline(&mut s.last).desired_width(100.0));
                    });
                }
                ui.horizontal(|ui| {
                    ui.label("Start at index");
                    ui.add(egui::DragValue::new(&mut s.from));
                });
            });

        let options = s.options(search::threads());
        let limit = s.seconds;
        let running = self
            .search_run
            .as_ref()
            .is_some_and(|r| r.running.is_some());
        let mut start = None;
        ui.add_space(6.0);
        let wide = egui::vec2(ui.available_width(), 30.0);
        if running {
            let current = self.search_run.as_ref().and_then(|r| r.running.as_ref());
            // Once stopped, the best seeds are still evaluated; a second click skips the rest.
            let label = if current.is_some_and(|r| r.stopping()) {
                "Skip evaluating"
            } else {
                "■ Stop"
            };
            if ui.add_sized(wide, egui::Button::new(label)).clicked()
                && let Some(running) = current
            {
                running.stop();
            }
        } else {
            let ready = matches!(parsed, Some(Ok(_))) && options.is_ok();
            let button =
                egui::Button::new(egui::RichText::new("Search").strong().color(style::GROUND))
                    .fill(style::ACCENT);
            if ui
                .add_enabled_ui(ready, |ui| ui.add_sized(wide, button))
                .inner
                .clicked()
                && let (Some(Ok(query)), Ok(options)) = (parsed, &options)
            {
                start = Some((query.clone(), options.clone()));
            }
            if let Err(e) = &options {
                style::hint(ui, e);
            }
        }

        let mut open: Option<(i64, Option<Evaluation>)> = None;
        let mut continue_from = None;
        if let Some(run) = &self.search_run {
            let live = run.running.as_ref().and_then(Running::progress);
            let finished = run.outcome.as_ref().and_then(|o| o.as_ref().ok());
            if let Some(Err(e)) = &run.outcome {
                ui.colored_label(style::ERROR, e);
            }
            let progress = finished.map(|o| &o.progress).or(live.as_ref());
            if let Some(p) = progress {
                let elapsed = p.elapsed.as_secs_f64();
                if running && limit > 0 {
                    ui.add(
                        egui::ProgressBar::new((elapsed / limit as f64).min(1.0) as f32)
                            .desired_height(6.0)
                            .fill(style::ACCENT),
                    );
                }
                let rate = p.evaluated as f64 / elapsed.max(0.001);
                ui.horizontal_wrapped(|ui| {
                    if running {
                        ui.spinner();
                    }
                    ui.label(
                        egui::RichText::new(format!(
                            "{} seeds · {rate:.0} a second · {} reported · {elapsed:.0} s",
                            p.evaluated, p.reported
                        ))
                        .size(12.0)
                        .color(style::MUTED),
                    );
                });
            } else if running {
                ui.spinner();
            }
            let opened = self.opened.as_ref().map(|o| o.seed);
            let hits: Vec<(i64, u64, Option<&Evaluation>)> = match finished {
                Some(outcome) => outcome
                    .hits
                    .iter()
                    .map(|h| (h.seed, h.score, Some(&h.evaluation)))
                    .collect(),
                None => live
                    .iter()
                    .flat_map(|p| &p.top)
                    .map(|h| (h.seed, h.score, None))
                    .collect(),
            };
            if !hits.is_empty() {
                ui.add_space(6.0);
                style::heading(
                    ui,
                    if finished.is_some() {
                        "Best seeds"
                    } else {
                        "Best so far"
                    },
                );
                egui::Grid::new("search hits")
                    .num_columns(3)
                    .spacing([10.0, 2.0])
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("#").size(11.0).color(style::MUTED));
                        ui.label(egui::RichText::new("Seed").size(11.0).color(style::MUTED));
                        ui.label(egui::RichText::new("Score").size(11.0).color(style::MUTED));
                        ui.end_row();
                        for (rank, (seed, score, evaluation)) in hits.into_iter().enumerate() {
                            ui.label(style::mono((rank + 1).to_string()).color(style::MUTED));
                            if ui
                                .selectable_label(
                                    opened == Some(seed),
                                    style::mono(seed.to_string()),
                                )
                                .on_hover_text("Show this seed")
                                .clicked()
                            {
                                open = Some((seed, evaluation.cloned()));
                            }
                            ui.label(style::mono(score.to_string()).color(style::ACCENT));
                            ui.end_row();
                        }
                    });
            }
            if let Some(outcome) = finished
                && let Some(next) = outcome.progress.next
                && ui
                    .small_button(format!("Continue from index {next}"))
                    .clicked()
            {
                continue_from = Some((run.query.clone(), next));
            }
        }
        if let Some(e) = &self.search_error {
            ui.colored_label(style::ERROR, e);
        }

        if let Some((query, next)) = continue_from {
            self.settings.search.from = next;
            if let Ok(options) = self.settings.search.options(search::threads()) {
                start = Some((query, options));
            }
        }
        if let Some((query, options)) = start {
            let ctx = ui.ctx().clone();
            self.search_error = None;
            self.search_run = Some(SearchRun {
                confirm: options.confirm,
                running: Some(Running::start(game, query.clone(), options, move || {
                    ctx.request_repaint();
                })),
                query,
                outcome: None,
            });
        }
        if let Some((seed, evaluation)) = open {
            self.search_error = None;
            match evaluation {
                Some(evaluation) => self.open_hit(seed, &evaluation),
                None => {
                    let run = self
                        .search_run
                        .as_ref()
                        .map(|r| (r.query.clone(), r.confirm));
                    if let Some((query, confirm)) = run {
                        self.opening = Some(Pending::spawn(ui.ctx(), move || {
                            game.session(seed)
                                .evaluate(&query, confirm)
                                .map(|e| (seed, e))
                                .map_err(|e| e.to_string())
                        }));
                    }
                }
            }
        }
    }

    /// Shows a search hit: its seed, in its first constraint's dimension, centred on that
    /// constraint's place, with the structures it matched marked and everything its constraints
    /// matched there ringed.
    pub(super) fn open_hit(&mut self, seed: i64, evaluation: &Evaluation) {
        let (Some(run), Some(game)) = (&self.search_run, self.game) else {
            return;
        };
        let matches = search::matches(&run.query, evaluation);
        let query = run.query.clone();
        let dimension = query.first_dimension();
        // The query was resolved against this game, so its dimensions are loaded.
        let Ok(world) = game.world(dimension) else {
            return;
        };
        self.save = None;
        self.set_dimension(dimension);
        self.world = Some(world);
        self.settings.seed = seed.to_string();
        self.apply_seed();
        if self.shown.is_none() {
            return;
        }
        // The Overworld spawn, converted to each dimension; the End has no spawn anchors.
        let overworld_spawn = game
            .session(seed)
            .dimension(Dimension::Overworld)
            .map(|s| s.spawn_origin().map_block())
            .ok();
        let spawn_in = |d: Dimension| {
            overworld_spawn
                .and_then(|at| game.convert(at, Dimension::Overworld, d).ok())
                .unwrap_or([0, 0])
        };
        let spawn = spawn_in(dimension);
        let [x, z] = search::focus(query.query(), spawn);
        self.settings.view.centre = [f64::from(x) + 0.5, f64::from(z) + 0.5];
        for id in search::structures(evaluation) {
            if !self.settings.structures.contains(&id) {
                self.settings.structures.push(id);
            }
        }
        self.select();
        let text = query.to_string();
        let constraints = text
            .lines()
            .zip(&evaluation.constraints)
            .zip(query.query().constraints())
            .map(|((line, result), constraint)| {
                let spawn = spawn_in(result.dimension);
                let area = search::area_of(constraint, spawn);
                OpenedConstraint {
                    line: line.to_string(),
                    outcome: result.outcome,
                    dimension: result.dimension,
                    area,
                }
            })
            .collect();
        self.opened = Some(Opened {
            seed,
            matches,
            constraints,
        });
    }
}

/// A query's text coloured by kind of word, in monospace of `size`: keywords, ids, numbers,
/// the rest. The hit card on the map colours its constraints' lines the same way.
pub(super) fn highlight(text: &str, size: f32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let font = egui::FontId::monospace(size);
    let keywords = [
        "require",
        "prefer",
        "within",
        "of",
        "in",
        "to",
        "weight",
        "cluster",
        "each",
        "other",
        "no",
        "covers",
        "at",
        "step",
        "spawn",
        "biome",
        "slime",
        "nether",
        "end",
        "overworld",
    ];
    let mut rest = text;
    while !rest.is_empty() {
        let end = rest
            .find(|c: char| c.is_whitespace() || c == ',')
            .map_or(rest.len(), |i| if i == 0 { 1 } else { i });
        let (word, tail) = rest.split_at(end);
        let colour = if word.trim().is_empty() || word == "," {
            style::MUTED
        } else if keywords.contains(&word) {
            if matches!(word, "require" | "prefer") {
                style::ACCENT
            } else {
                style::MUTED
            }
        } else if word.contains(':') {
            egui::Color32::from_rgb(0xc7, 0xb0, 0xff)
        } else if word.trim_end_matches('%').parse::<f64>().is_ok() {
            egui::Color32::from_rgb(0x7f, 0xd4, 0xc8)
        } else {
            style::TEXT
        };
        job.append(
            word,
            0.0,
            egui::TextFormat {
                font_id: font.clone(),
                color: colour,
                ..Default::default()
            },
        );
        rest = tail;
    }
    job
}
