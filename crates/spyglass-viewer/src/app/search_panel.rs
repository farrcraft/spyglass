//! The Search tab: seeds that fit a query, or places in the seed shown that fit a `find` query
//! (ADR 0047), with the query, its options, the run and its results.

use spyglass::Outcome;

use super::*;
use crate::search::{Area, Mode, PRESETS};
use crate::style;

/// A search hit or `find` match on the map: its seed, what its constraints matched, and each
/// constraint's line, outcome and area.
pub(super) struct Opened {
    pub seed: i64,
    /// What the card on the map calls it.
    pub title: String,
    /// For a `find` match: its index and how many there are, for stepping through them.
    pub step: Option<(usize, usize)>,
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
    /// The query's text parsed against `game`, once it's ready: resolving can load a dimension,
    /// so it's done off this thread, and a result for text that has since changed is dropped.
    fn parsed_query(
        &mut self,
        ctx: &egui::Context,
        game: &'static Game,
        text: &str,
    ) -> Option<Result<SearchQuery, String>> {
        if let Some((done, pending)) = &self.search_parsing
            && let Some(result) = pending.poll()
        {
            let parsed = result.unwrap_or_else(|()| Err("the query check failed".to_string()));
            self.search_parsed = Some((done.clone(), parsed));
            self.search_parsing = None;
        }
        let is_current = |t: Option<&String>| t.is_some_and(|t| t == text);
        if !is_current(self.search_parsed.as_ref().map(|(t, _)| t))
            && !is_current(self.search_parsing.as_ref().map(|(t, _)| t))
        {
            let owned = text.to_string();
            let pending = Pending::spawn(ctx, {
                let owned = owned.clone();
                move || game.search_query(&owned).map_err(|e| e.to_string())
            });
            self.search_parsing = Some((owned, pending));
        }
        self.search_parsed
            .as_ref()
            .filter(|(t, _)| t == text)
            .map(|(_, p)| p.clone())
    }

    /// The Search tab: what it searches, then that search's section.
    pub(super) fn search_section(&mut self, ui: &mut egui::Ui) {
        let Some(game) = self.game else {
            ui.label("Open a bundle to search.");
            return;
        };
        let mode = &mut self.settings.search.mode;
        ui.horizontal(|ui| {
            ui.selectable_value(mode, Mode::Seeds, "Seeds")
                .on_hover_text("Seeds that fit a query.");
            ui.selectable_value(mode, Mode::ThisSeed, "This seed")
                .on_hover_text("Places in the seed shown that fit a find query.");
        });
        ui.add_space(4.0);
        match self.settings.search.mode {
            Mode::Seeds => self.seeds_section(ui, game),
            Mode::ThisSeed => self.find_section(ui, game),
        }
    }

    /// The seed search's query, its options, and the best seeds so far: click one to open it.
    fn seeds_section(&mut self, ui: &mut egui::Ui, game: &'static Game) {
        let text = self.settings.search.query.clone();
        let parsed = self.parsed_query(ui.ctx(), game, &text);
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
        // The text being typed is checked on the next frame.
        let parsed = parsed.filter(|_| s.query == text);
        let parsed = parsed.as_ref();
        match parsed {
            Some(Ok(query)) if query.find_dimension().is_some() => {
                ui.colored_label(
                    style::ERROR,
                    "A find query searches the seed shown: switch to This seed.",
                );
            }
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
            let ready =
                matches!(parsed, Some(Ok(q)) if q.find_dimension().is_none()) && options.is_ok();
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
        let title = "Search hit".to_string();
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
                let area = search::area_of(constraint, spawn, spawn);
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
            title,
            step: None,
            matches,
            constraints,
        });
    }

    /// The `find` query, its options, and the places found in the seed shown: click one, or
    /// step with N and P, to open it on the map.
    fn find_section(&mut self, ui: &mut egui::Ui, game: &'static Game) {
        let text = self.settings.search.find.clone();
        let parsed = self.parsed_query(ui.ctx(), game, &text);
        let s = &mut self.settings.search;
        ui.horizontal(|ui| {
            style::heading(ui, "Query");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button("Presets", |ui| {
                    for (name, preset) in PRESETS {
                        if ui.button(*name).clicked() {
                            s.find = (*preset).to_string();
                            ui.close();
                        }
                    }
                });
            });
        });
        let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap: f32| {
            let mut job = highlight(text.as_str(), 13.0);
            job.wrap.max_width = wrap;
            ui.fonts_mut(|f| f.layout_job(job))
        };
        ui.add(
            egui::TextEdit::multiline(&mut s.find)
                .code_editor()
                .desired_rows(4)
                .desired_width(f32::INFINITY)
                .hint_text("find minecraft:swamp_hut within 4096")
                .layouter(&mut layouter),
        );
        let parsed = parsed.filter(|_| s.find == text);
        let query = match &parsed {
            None => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.weak("Checking the query…");
                });
                None
            }
            Some(Err(e)) => {
                ui.colored_label(style::ERROR, e);
                None
            }
            Some(Ok(query)) if query.find_dimension().is_none() => {
                ui.colored_label(
                    style::ERROR,
                    "Start with a find line, such as `find minecraft:swamp_hut within 4096`, or \
                     search seeds with Seeds.",
                );
                None
            }
            Some(Ok(query)) => {
                let (required, preferred) = search::counts(query.query());
                ui.colored_label(
                    style::GOOD,
                    format!("Valid: {required} required, {preferred} preferred"),
                );
                Some(query.clone())
            }
        };

        ui.add_space(6.0);
        style::heading(ui, "Options");
        ui.horizontal(|ui| {
            ui.label("Find at most");
            ui.add(egui::DragValue::new(&mut s.find_top).range(1..=1000));
        });
        ui.checkbox(&mut s.confirm, "Confirm with terrain").on_hover_text(
            "Decide structure starts with the seed's terrain, as the game does. Slower, but only \
             ties and platform-dependent results stay uncertain.",
        );
        ui.checkbox(&mut s.certain, "Certain only").on_hover_text(
            "Only places whose required constraints hold whatever is left undecided. Without \
             confirming, most structures are undecided.",
        );
        ui.label(
            egui::RichText::new("Searches around the view's centre, nearest first.")
                .size(12.0)
                .color(style::MUTED),
        );

        let running = self.find_run.as_ref().is_some_and(|r| r.running.is_some());
        let wide = egui::vec2(ui.available_width(), 30.0);
        ui.add_space(6.0);
        if running {
            if ui.add_sized(wide, egui::Button::new("■ Stop")).clicked()
                && let Some(running) = self.find_run.as_ref().and_then(|r| r.running.as_ref())
            {
                running.stop();
            }
        } else {
            let shown = self
                .shown
                .as_ref()
                .map(|s| (s.session.seed(), s.session.world().dimension_kind()));
            let button =
                egui::Button::new(egui::RichText::new("Find").strong().color(style::GROUND))
                    .fill(style::ACCENT);
            let ready = query.is_some() && shown.is_some();
            if ui
                .add_enabled_ui(ready, |ui| ui.add_sized(wide, button))
                .inner
                .clicked()
                && let (Some(query), Some((seed, shown_in))) = (query, shown)
            {
                self.start_find(ui.ctx(), game, query, seed, shown_in);
            }
        }

        let mut open = None;
        if let Some(run) = &self.find_run {
            let live = run.running.as_ref().and_then(Running::progress);
            let finished = run.outcome.as_ref().and_then(|o| o.as_ref().ok());
            if let Some(Err(e)) = &run.outcome {
                ui.colored_label(style::ERROR, e);
            }
            if let Some(p) = finished.map(|o| &o.progress).or(live.as_ref()) {
                ui.horizontal_wrapped(|ui| {
                    if running {
                        ui.spinner();
                    }
                    ui.label(
                        egui::RichText::new(format!(
                            "{} of {} places checked · {} found · {:.1} s",
                            p.judged,
                            p.targets,
                            p.matches,
                            p.elapsed.as_secs_f64()
                        ))
                        .size(12.0)
                        .color(style::MUTED),
                    );
                });
            } else if running {
                ui.spinner();
            }
            if let Some(outcome) = finished {
                ui.add_space(6.0);
                if outcome.matches.is_empty() {
                    ui.label("Nothing found here: try a larger area, or another seed.");
                } else {
                    ui.horizontal(|ui| {
                        style::heading(ui, &format!("Found {}", outcome.matches.len()));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let at = run.current.unwrap_or(0);
                            let last = outcome.matches.len() - 1;
                            if ui
                                .add_enabled(at < last, egui::Button::new("Next").small())
                                .on_hover_text("Next (N)")
                                .clicked()
                            {
                                open = Some(at + 1);
                            }
                            if ui
                                .add_enabled(at > 0, egui::Button::new("Previous").small())
                                .on_hover_text("Previous (P)")
                                .clicked()
                            {
                                open = Some(at - 1);
                            }
                        });
                    });
                    egui::Grid::new("find matches")
                        .num_columns(4)
                        .spacing([10.0, 2.0])
                        .striped(true)
                        .show(ui, |ui| {
                            for head in ["#", "Where", "Away", "Score"] {
                                ui.label(egui::RichText::new(head).size(11.0).color(style::MUTED));
                            }
                            ui.end_row();
                            for (i, m) in outcome.matches.iter().enumerate() {
                                ui.label(style::mono((i + 1).to_string()).color(style::MUTED));
                                let [x, z] = m.position;
                                if ui
                                    .selectable_label(
                                        run.current == Some(i),
                                        style::mono(format!("{x} ~ {z}")),
                                    )
                                    .on_hover_text("Show this place")
                                    .clicked()
                                {
                                    open = Some(i);
                                }
                                ui.label(style::mono(m.distance.to_string()));
                                ui.label(style::mono(m.score.to_string()).color(style::ACCENT));
                                ui.end_row();
                            }
                        });
                }
            }
        }
        if let Some(i) = open {
            self.open_match(ui.ctx(), i);
        }
    }

    /// Starts `query` in `seed`, around the view's centre in `shown`'s coordinates converted
    /// into the dimension its targets are in.
    fn start_find(
        &mut self,
        ctx: &egui::Context,
        game: &'static Game,
        query: SearchQuery,
        seed: i64,
        shown: Dimension,
    ) {
        let Some(dimension) = query.find_dimension() else {
            return;
        };
        let [x, z] = self.settings.view.centre.map(|c| c.floor() as i32);
        let origin = match game.convert([x, z], shown, dimension) {
            Ok(origin) => origin,
            Err(e) => {
                self.find_run = Some(FindState {
                    query,
                    seed,
                    dimension,
                    running: None,
                    outcome: Some(Err(e.to_string())),
                    current: None,
                });
                return;
            }
        };
        if self.opened.as_ref().is_some_and(|o| o.step.is_some()) {
            self.opened = None;
        }
        let options = self.settings.search.find_options(origin, search::threads());
        let repaint = {
            let ctx = ctx.clone();
            move || ctx.request_repaint()
        };
        self.find_run = Some(FindState {
            running: Some(FindRun::find(game, seed, query.clone(), options, repaint)),
            query,
            seed,
            dimension,
            outcome: None,
            current: None,
        });
    }

    /// Opens match `index` of the last `find`: centres the map on it, in its dimension, with
    /// its structures marked and what its constraints matched ringed.
    pub(super) fn open_match(&mut self, ctx: &egui::Context, index: usize) {
        let Some(run) = &mut self.find_run else {
            return;
        };
        let Some(Ok(outcome)) = &run.outcome else {
            return;
        };
        let Some(m) = outcome.matches.get(index).cloned() else {
            return;
        };
        run.current = Some(index);
        let count = outcome.matches.len();
        let (query, seed, dimension) = (run.query.clone(), run.seed, run.dimension);
        if self.settings.dimension() != dimension {
            self.switch_dimension(ctx, dimension);
        }
        let [x, z] = m.position;
        self.settings.view.centre = [f64::from(x) + 0.5, f64::from(z) + 0.5];
        let mut ids = search::structures(&m.evaluation);
        if let Some(spyglass::Feature::Structure(s)) = &m.target {
            ids.extend(s.structures.iter().cloned());
        }
        for id in ids {
            if !self.settings.structures.contains(&id) {
                self.settings.structures.push(id);
            }
        }
        self.select();
        let spawn = self.game.and_then(|game| {
            let at = game
                .session(seed)
                .dimension(Dimension::Overworld)
                .ok()?
                .spawn_origin()
                .map_block();
            Some((game, at))
        });
        let spawn_in = |d: Dimension| {
            spawn
                .and_then(|(game, at)| game.convert(at, Dimension::Overworld, d).ok())
                .unwrap_or([0, 0])
        };
        let constraints = search::constraint_lines(&query)
            .into_iter()
            .zip(&m.evaluation.constraints)
            .zip(query.query().constraints())
            .map(|((line, result), constraint)| {
                // `here` in another dimension is the match, converted.
                let here = spawn
                    .and_then(|(game, _)| {
                        game.convert(m.position, dimension, result.dimension).ok()
                    })
                    .unwrap_or(m.position);
                OpenedConstraint {
                    line,
                    outcome: result.outcome,
                    dimension: result.dimension,
                    area: search::area_of(constraint, spawn_in(result.dimension), here),
                }
            })
            .collect();
        self.opened = Some(Opened {
            seed,
            title: format!("Match {} of {count}", index + 1),
            step: Some((index, count)),
            matches: search::find_matches(&query, dimension, &m),
            constraints,
        });
    }

    /// Opens the next (`delta` 1) or previous (−1) match of the last `find`, if there is one.
    pub(super) fn step_match(&mut self, ctx: &egui::Context, delta: isize) {
        let Some(run) = &self.find_run else {
            return;
        };
        let count = match &run.outcome {
            Some(Ok(o)) => o.matches.len(),
            _ => return,
        };
        let at = run.current.map_or(0, |c| c as isize + delta);
        if (0..count as isize).contains(&at) {
            self.open_match(ctx, at as usize);
        }
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
        "find",
        "here",
        "apart",
        "point",
        "height",
        "flat",
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
            if matches!(word, "require" | "prefer" | "find") {
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
