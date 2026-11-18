//! The World tab: what spyglass read from the save shown, and how the map's answers use it
//! (`SaveReport::details`, `docs/specs/world-saves.md`, "The report"). Each warning sits under
//! what it's about.

use spyglass::{PackUse, SaveDetails, SaveHoldings, SaveSubject, SaveWarning, Setting};

use super::*;
use crate::style::{self, Section, Summary};

impl App {
    /// The save the World tab is about, and its details: the last one opened that couldn't be
    /// shown, until its error is dismissed, or else the one shown.
    pub(super) fn world_report(&self) -> Option<(&SaveReport, &SaveDetails)> {
        match (&self.unshown_save, &self.save, &self.save_details) {
            (Some((report, details)), _, _) => Some((report, details)),
            (None, Some(report), Some(details)) => Some((report, details)),
            _ => None,
        }
    }

    pub(super) fn world_tab(&mut self, ui: &mut egui::Ui) {
        // Moving the map makes sense only for the save it shows.
        let shown = self.unshown_save.is_none() && self.save.is_some();
        // Out of the settings while the report is borrowed, and back at the end.
        let mut folded = std::mem::take(&mut self.settings.folded);
        let Some((report, details)) = self.world_report() else {
            ui.label(egui::RichText::new("No save is shown.").color(style::MUTED));
            self.settings.folded = folded;
            return;
        };
        let section = |part: WorldSection| {
            Section::new(part.id(), part.title()).summary(world_summary(details, part))
        };
        let mut go: Option<(Dimension, [i32; 2])> = None;
        ui.label(egui::RichText::new(&report.save.name).strong());
        style::hint(ui, &report.save.dir.display().to_string());
        ui.add_space(4.0);

        if style::section(ui, &mut folded, section(WorldSection::Settings), |_| {}) {
            style::section_body(ui, |ui| {
                for setting in &details.settings {
                    let label = match setting.setting.dimension() {
                        Some(d) => format!("{}, {}", setting.setting.label(), d.name()),
                        None => setting.setting.label().to_string(),
                    };
                    let place = match setting.setting {
                        Setting::Spawn => report.save.spawn.as_ref(),
                        Setting::Player => report.save.player.as_ref(),
                        _ => None,
                    };
                    ui.horizontal_wrapped(|ui| {
                        ui.label(egui::RichText::new(label).color(style::MUTED));
                        ui.label(style::mono(&setting.value));
                        if shown
                            && let Some(place) = place
                            && let Some(dimension) = place.dimension_kind()
                            && ui
                                .small_button("Go")
                                .on_hover_text("Move the map there")
                                .clicked()
                        {
                            let [x, _, z] = place.block();
                            go = Some((dimension, [x, z]));
                        }
                    });
                    let source = &setting.source;
                    let from = if source.present { "from" } else { "not in" };
                    style::hint(
                        ui,
                        &format!(
                            "{from} {} {}; decides {}",
                            source.file,
                            source.field,
                            setting.decides()
                        ),
                    );
                    warnings_about(ui, details, &SaveSubject::Setting(setting.setting));
                }
            });
        }

        if style::section(ui, &mut folded, section(WorldSection::Packs), |_| {}) {
            style::section_body(ui, |ui| {
                for pack in &details.packs {
                    let used = match pack.used {
                        PackUse::Bundle => "the bundle's own data".to_string(),
                        PackUse::NotApplied => "not applied".to_string(),
                        PackUse::Applied | PackUse::Feature => {
                            let names: Vec<&str> = pack.answers.iter().map(|a| a.name()).collect();
                            if names.is_empty() {
                                "applied; changes nothing spyglass reads".to_string()
                            } else {
                                format!("applied; changes {}", names.join(", "))
                            }
                        }
                        other => other.kind().replace('_', " "),
                    };
                    ui.horizontal_wrapped(|ui| {
                        ui.label(style::mono(&pack.name));
                        ui.label(egui::RichText::new(used).color(style::MUTED));
                    });
                    for (registry, changes) in pack.changes.iter().flat_map(|c| &c.registries) {
                        let mut parts = Vec::new();
                        for (list, what) in [
                            (&changes.replaced, "replaced"),
                            (&changes.added, "added"),
                            (&changes.merged, "merged"),
                            (&changes.hidden, "hidden below"),
                            (&changes.shadowed, "shadowed above"),
                            (&changes.unreadable, "unreadable"),
                        ] {
                            if !list.is_empty() {
                                parts.push(format!("{} {what}", list.len()));
                            }
                        }
                        let read = if spyglass::answers_fed_by(registry).is_empty() {
                            " (not read by spyglass)"
                        } else {
                            ""
                        };
                        style::hint(ui, &format!("{registry}: {}{read}", parts.join(", ")));
                    }
                    warnings_about(ui, details, &SaveSubject::Pack(pack.name.clone()));
                }
            });
        }

        if style::section(ui, &mut folded, section(WorldSection::Dimensions), |_| {}) {
            style::section_body(ui, |ui| {
                for dimension in &details.dimensions {
                    let d = dimension.dimension;
                    ui.horizontal_wrapped(|ui| {
                        ui.label(egui::RichText::new(d.name()).strong());
                        let (text, colour) = if dimension.answerable {
                            ("answered", style::GOOD)
                        } else {
                            ("not answered", style::ERROR)
                        };
                        ui.label(egui::RichText::new(text).color(colour));
                        if shown
                            && let Some(holdings) = &dimension.holdings
                            && let Some([min, max]) = holdings.bounds
                            && dimension.answerable
                            && ui
                                .small_button("Go")
                                .on_hover_text("Move the map to the middle of its generated chunks")
                                .clicked()
                        {
                            let middle = |a: i32, b: i32| (a / 2 + b / 2) * 16 + 8;
                            go = Some((d, [middle(min[0], max[0]), middle(min[1], max[1])]));
                        }
                    });
                    warnings_about(ui, details, &SaveSubject::Dimension(d));
                    if let Some(holdings) = &dimension.holdings {
                        holdings_lines(ui, holdings);
                    }
                    warnings_about(ui, details, &SaveSubject::Holdings(d));
                    ui.add_space(2.0);
                }
            });
        }

        let usage = self
            .shown
            .as_ref()
            .and_then(|s| s.session.save())
            .map(|chunks| (chunks.dimension(), chunks.usage()));
        if let Some((dimension, usage)) = usage {
            let map = Section::new("world.map", "This map so far")
                .summary(Summary::new(format!("{} chunks read", usage.chunks_read)));
            if style::section(ui, &mut folded, map, |_| {}) {
                style::section_body(ui, |ui| {
                    style::hint(
                        ui,
                        &format!(
                            "What the map has taken from the save in {}:",
                            dimension.name()
                        ),
                    );
                    for (count, what) in [
                        (usage.chunks_read, "chunks read"),
                        (
                            usage.starts_from_save,
                            "structure starts answered from the save",
                        ),
                        (
                            usage.chunks_without_starts,
                            "chunks the save says have no start",
                        ),
                        (usage.unopened, "containers unopened"),
                        (usage.opened, "containers opened"),
                        (usage.gone, "containers gone"),
                    ] {
                        ui.horizontal(|ui| {
                            ui.label(style::mono(count.to_string()));
                            ui.label(egui::RichText::new(what).color(style::MUTED));
                        });
                    }
                    // The counts grow as the map draws, so keep them current.
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_secs(1));
                });
            }
        }
        self.settings.folded = folded;

        if let Some((dimension, [x, z])) = go {
            if dimension != self.settings.dimension() {
                self.switch_dimension(ui.ctx(), dimension);
            }
            if dimension == self.settings.dimension() {
                self.settings.view.centre = [f64::from(x) + 0.5, f64::from(z) + 0.5];
            }
        }
    }
}

/// The World tab's sections that warnings are under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WorldSection {
    /// The save's settings.
    Settings,
    /// Its data packs.
    Packs,
    /// Its dimensions, and what their region files hold.
    Dimensions,
}

impl WorldSection {
    pub(super) const ALL: [WorldSection; 3] = [
        WorldSection::Settings,
        WorldSection::Packs,
        WorldSection::Dimensions,
    ];

    /// The id its fold is kept under: one for every save, since it names the section.
    pub(super) fn id(self) -> &'static str {
        match self {
            WorldSection::Settings => "world.settings",
            WorldSection::Packs => "world.packs",
            WorldSection::Dimensions => "world.dimensions",
        }
    }

    fn title(self) -> &'static str {
        match self {
            WorldSection::Settings => "Settings",
            WorldSection::Packs => "Data packs",
            WorldSection::Dimensions => "Dimensions",
        }
    }

    /// Whether a warning about `subject` is shown in this section.
    fn holds(self, subject: &SaveSubject) -> bool {
        matches!(
            (self, subject),
            (WorldSection::Settings, SaveSubject::Setting(_))
                | (WorldSection::Packs, SaveSubject::Pack(_))
                | (
                    WorldSection::Dimensions,
                    SaveSubject::Dimension(_) | SaveSubject::Holdings(_)
                )
        )
    }
}

/// How many of `warnings`, notes left out, `part` of the World tab holds.
pub(super) fn warnings_in(warnings: &[SaveWarning], part: WorldSection) -> usize {
    warnings
        .iter()
        .filter(|w| !w.is_note() && part.holds(&w.subject()))
        .count()
}

/// What `part` of the World tab says folded.
fn world_summary(details: &SaveDetails, part: WorldSection) -> Summary {
    let listed = match part {
        WorldSection::Settings => details.settings.len(),
        WorldSection::Packs => details.packs.len(),
        WorldSection::Dimensions => details.dimensions.len(),
    };
    section_summary(part, warnings_in(&details.warnings, part), listed)
}

/// What `part` says folded with `warnings` in it and `listed` things listed: its warnings, in
/// the warning colour, so a fold never hides one, or else how many things it lists.
fn section_summary(part: WorldSection, warnings: usize, listed: usize) -> Summary {
    match warnings {
        0 => Summary::new(match part {
            WorldSection::Settings => format!("{listed} settings"),
            WorldSection::Packs => format!("{listed} packs"),
            WorldSection::Dimensions => format!("{listed} dimensions"),
        }),
        1 => Summary::warning("1 warning"),
        n => Summary::warning(format!("{n} warnings")),
    }
}

/// The warnings about `subject`, under it: warnings in the warning colour, notes muted.
fn warnings_about(ui: &mut egui::Ui, details: &SaveDetails, subject: &SaveSubject) {
    for warning in details.warnings_about(subject) {
        let colour = if warning.blocks_answers() {
            style::ERROR
        } else if warning.is_note() {
            style::MUTED
        } else {
            style::WARN
        };
        ui.add(egui::Label::new(egui::RichText::new(warning.to_string()).color(colour)).wrap());
    }
}

/// What a dimension's region files hold, a line each.
fn holdings_lines(ui: &mut egui::Ui, holdings: &SaveHoldings) {
    style::hint(
        ui,
        &format!(
            "{} chunks: {} full, {} partway, {} unreadable",
            holdings.chunks, holdings.full, holdings.proto, holdings.unreadable
        ),
    );
    if !holdings.versions.is_empty() {
        let versions: Vec<String> = holdings
            .versions
            .iter()
            .map(|v| match (v.data_version, &v.version) {
                (Some(_), Some(name)) => format!("{name}: {}", v.chunks),
                (Some(d), None) => format!("data version {d}: {}", v.chunks),
                (None, _) => format!("before 1.9: {}", v.chunks),
            })
            .collect();
        style::hint(ui, &format!("by version: {}", versions.join(", ")));
    }
    let total = |c: &[(String, usize)]| c.iter().map(|(_, n)| n).sum::<usize>();
    let some = |c: &[(String, usize)]| {
        let mut names: Vec<String> = c
            .iter()
            .take(4)
            .map(|(id, n)| format!("{id} {n}"))
            .collect();
        if c.len() > 4 {
            names.push(format!("and {} more", c.len() - 4));
        }
        names.join(", ")
    };
    if !holdings.starts.is_empty() {
        style::hint(
            ui,
            &format!(
                "{} structure starts from the save: {}",
                total(&holdings.starts),
                some(&holdings.starts)
            ),
        );
    }
    if !holdings.dropped_starts.is_empty() {
        style::hint(
            ui,
            &format!(
                "{} starts dropped, naming no structure the bundle has: {}",
                total(&holdings.dropped_starts),
                some(&holdings.dropped_starts)
            ),
        );
    }
    if !holdings.older_starts.is_empty() {
        style::hint(
            ui,
            &format!(
                "{} starts in chunks before 1.18.2, predicted instead: {}",
                total(&holdings.older_starts),
                some(&holdings.older_starts)
            ),
        );
    }
    if !holdings.sealed_containers.is_empty() {
        style::hint(
            ui,
            &format!(
                "{} loot containers not yet opened: {}",
                total(&holdings.sealed_containers),
                some(&holdings.sealed_containers)
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folded_section_with_a_warning_says_so() {
        let warnings = [SaveWarning::StructuresDisabled, SaveWarning::Modded];
        assert_eq!(warnings_in(&warnings, WorldSection::Settings), 2);
        assert_eq!(warnings_in(&warnings, WorldSection::Packs), 0);
        let settings = section_summary(WorldSection::Settings, 2, 9);
        assert_eq!(settings, Summary::warning("2 warnings"));
        assert!(settings.warns);
        assert_eq!(
            section_summary(WorldSection::Packs, 0, 3),
            Summary::new("3 packs")
        );
        assert_eq!(
            section_summary(WorldSection::Dimensions, 1, 3),
            Summary::warning("1 warning")
        );
    }
}
