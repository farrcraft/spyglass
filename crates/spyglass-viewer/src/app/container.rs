//! The open container's window: what a container holds, as predicted and as the save has it.

use spyglass::InSave;

use super::map::contents_lines;
use super::*;

impl App {
    /// The open container's contents, slot by slot, in a window until it's closed.
    pub(super) fn container_window(&mut self, ctx: &egui::Context) {
        let Some(c) = &self.open_container else {
            return;
        };
        let [x, y, z] = c.position;
        let mut open = true;
        egui::Window::new(format!(
            "{} at {x} {y} {z}",
            spyglass::display_name(&c.holder)
        ))
        .open(&mut open)
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.label(egui::RichText::new(&c.table).color(style::MUTED));
            if let Some(seed) = c.seed {
                ui.label(style::mono(format!("Loot seed {seed}")).color(style::MUTED));
            }
            if c.contents_certain() {
                ui.colored_label(style::GOOD, "Certain, contents included");
            } else if let Some(why) = c.reason.as_ref().or(c.contents_doubt.as_ref()) {
                ui.label(format!("Not certain: {why}."));
            }
            ui.separator();
            match &c.contents {
                None => {
                    ui.label("Its contents aren't known.");
                }
                Some(slots) if slots.is_empty() => {
                    ui.label("Empty");
                }
                Some(slots) => {
                    egui::Grid::new("container slots")
                        .striped(true)
                        .show(ui, |ui| {
                            for s in slots {
                                ui.label(style::mono(format!("{:>2}", s.slot)).color(style::MUTED));
                                for line in contents_lines(std::slice::from_ref(s)) {
                                    ui.label(line);
                                }
                                ui.end_row();
                            }
                        });
                }
            }
            ui.separator();
            match &c.saved {
                Some(InSave::Unopened { as_predicted: true }) => {
                    ui.colored_label(style::GOOD, "In your world: not opened yet");
                }
                Some(InSave::Unopened { .. }) => {
                    ui.label("In your world: not opened yet, but not with this table and seed.");
                }
                Some(InSave::Filled { items }) if items.is_empty() => {
                    ui.label("In your world: opened, and now empty.");
                }
                Some(InSave::Filled { items }) => {
                    ui.label("In your world: opened. It holds now:");
                    egui::Grid::new("saved slots").striped(true).show(ui, |ui| {
                        for s in items {
                            ui.label(style::mono(format!("{:>2}", s.slot)).color(style::MUTED));
                            for line in contents_lines(std::slice::from_ref(s)) {
                                ui.label(line);
                            }
                            ui.end_row();
                        }
                    });
                }
                Some(_) => {
                    ui.label("In your world: gone, broken or built over.");
                }
                None => {
                    ui.label(
                        egui::RichText::new(
                            "As the seed generates it: a chest already opened in a world may \
                             hold something else.",
                        )
                        .color(style::MUTED),
                    );
                }
            }
        });
        if !open {
            self.open_container = None;
        }
    }
}
