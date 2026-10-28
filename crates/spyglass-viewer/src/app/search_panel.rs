//! The search panel: the query, its options, the run and its best seeds.

use super::*;

impl App {
    /// The search query, its options, and the best seeds so far: click one to open it.
    pub(super) fn search_section(&mut self, ui: &mut egui::Ui) {
        let Some(game) = self.game else {
            ui.weak("Open a bundle to search.");
            return;
        };
        let s = &mut self.settings.search;
        ui.add(
            egui::TextEdit::multiline(&mut s.query)
                .code_editor()
                .desired_rows(4)
                .desired_width(f32::INFINITY)
                .hint_text("require 2 minecraft:swamp_hut within 600 of spawn"),
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
                ui.colored_label(ui.visuals().error_fg_color, e);
            }
            None => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.weak("Checking the query…");
                });
            }
            Some(Ok(_)) => {}
        }
        ui.horizontal(|ui| {
            ui.label("Keep the best");
            ui.add(egui::DragValue::new(&mut s.top).range(1..=1000));
        });
        ui.horizontal(|ui| {
            ui.label("Stop after");
            ui.add(
                egui::TextEdit::singleline(&mut s.seeds)
                    .hint_text("any number of")
                    .desired_width(80.0),
            );
            ui.label("seeds or");
            ui.add(egui::DragValue::new(&mut s.seconds).suffix(" s"))
                .on_hover_text("0 for no time limit.");
        });
        ui.checkbox(&mut s.range, "Only seeds in a range")
            .on_hover_text("Otherwise every 64-bit seed, with structure placement checked first.");
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
        ui.checkbox(&mut s.confirm, "Confirm with terrain").on_hover_text(
            "Decide structure starts with each seed's terrain, as the game does. Slower, but only \
             ties and platform-dependent results stay uncertain.",
        );
        ui.checkbox(&mut s.certain, "Certain only").on_hover_text(
            "Only seeds whose required constraints hold whatever is left undecided, ranked by the \
             worst case. Without confirming, most structures are undecided.",
        );

        let options = s.options(search::threads());
        let running = self
            .search_run
            .as_ref()
            .is_some_and(|r| r.running.is_some());
        let mut start = None;
        if running {
            let current = self.search_run.as_ref().and_then(|r| r.running.as_ref());
            // Once stopped, the best seeds are still evaluated; a second click skips the rest.
            let label = if current.is_some_and(|r| r.stopping()) {
                "Skip evaluating"
            } else {
                "Stop"
            };
            if ui.button(label).clicked()
                && let Some(running) = current
            {
                running.stop();
            }
        } else {
            let ready = matches!(parsed, Some(Ok(_))) && options.is_ok();
            if ui.add_enabled(ready, egui::Button::new("Search")).clicked()
                && let (Some(Ok(query)), Ok(options)) = (parsed, &options)
            {
                start = Some((query.clone(), options.clone()));
            }
            if let Err(e) = &options {
                ui.weak(e);
            }
        }

        let mut open: Option<(i64, Option<Evaluation>)> = None;
        let mut continue_from = None;
        if let Some(run) = &self.search_run {
            let live = run.running.as_ref().and_then(Running::progress);
            let finished = run.outcome.as_ref().and_then(|o| o.as_ref().ok());
            if let Some(Err(e)) = &run.outcome {
                ui.colored_label(ui.visuals().error_fg_color, e);
            }
            let progress = finished.map(|o| &o.progress).or(live.as_ref());
            if let Some(p) = progress {
                ui.horizontal(|ui| {
                    if running {
                        ui.spinner();
                    }
                    let rate = p.evaluated as f64 / p.elapsed.as_secs_f64().max(0.001);
                    ui.label(format!(
                        "{} seeds, {rate:.0} a second, {} reported",
                        p.evaluated, p.reported
                    ));
                });
            } else if running {
                ui.spinner();
            }
            let opened = self.opened.as_ref().map(|(seed, _)| *seed);
            match finished {
                Some(outcome) => {
                    for hit in &outcome.hits {
                        let label = format!("{}  score {}", hit.seed, hit.score);
                        if ui
                            .selectable_label(opened == Some(hit.seed), label)
                            .clicked()
                        {
                            open = Some((hit.seed, Some(hit.evaluation.clone())));
                        }
                    }
                    if let Some(next) = outcome.progress.next
                        && ui
                            .small_button(format!("Continue from index {next}"))
                            .clicked()
                    {
                        continue_from = Some((run.query.clone(), next));
                    }
                }
                None => {
                    for hit in live.iter().flat_map(|p| &p.top) {
                        let label = format!("{}  score {}", hit.seed, hit.score);
                        if ui
                            .selectable_label(opened == Some(hit.seed), label)
                            .clicked()
                        {
                            open = Some((hit.seed, None));
                        }
                    }
                }
            }
        }
        if let Some(e) = &self.search_error {
            ui.colored_label(ui.visuals().error_fg_color, e);
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
        // The Overworld spawn, converted; the End has no spawn anchors.
        let spawn = game
            .session(seed)
            .dimension(Dimension::Overworld)
            .map(|s| s.spawn_origin().map_block())
            .and_then(|at| game.convert(at, Dimension::Overworld, dimension))
            .unwrap_or([0, 0]);
        let [x, z] = search::focus(query.query(), spawn);
        self.settings.view.centre = [f64::from(x) + 0.5, f64::from(z) + 0.5];
        for id in search::structures(evaluation) {
            if !self.settings.structures.contains(&id) {
                self.settings.structures.push(id);
            }
        }
        self.select();
        self.opened = Some((seed, matches));
    }
}
