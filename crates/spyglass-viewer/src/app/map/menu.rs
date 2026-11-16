//! The map's context menu.

use super::*;

impl App {
    /// The context menu after a right-click at the block it was on, and what it asks for, done.
    /// `y` is the view's height, which a locate from the block starts at.
    pub(super) fn context_menu(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        world: &World,
        y: i32,
    ) {
        let Some([bx, bz]) = self.menu_block else {
            return;
        };
        let menu_target = self.menu_target.clone();
        let loot = self
            .selection
            .loot
            .as_ref()
            .map(|query| query.argument().to_string());
        let menu_portal = self.menu_portal.clone();
        let mut action = None;
        response.context_menu(|ui| {
            action = menu(
                ui,
                [bx, bz],
                menu_target.as_ref(),
                menu_portal.as_ref(),
                world,
                loot.as_deref(),
            );
        });
        match action {
            Some(Action::Locate(target)) => {
                self.locate(ui.ctx(), target, [bx, y, bz]);
                self.tab = Tab::Locate;
            }
            Some(Action::Centre) => {
                self.settings.view.centre = [f64::from(bx) + 0.5, f64::from(bz) + 0.5];
            }
            Some(Action::AddMarker) => self.add_marker([bx, bz]),
            Some(Action::AddPortal) => self.add_portal([bx, y, bz]),
            Some(Action::LinkArea(what)) => self.show_link_area(ui.ctx(), what),
            Some(Action::Edit(id)) => self.editing = Some(id),
            Some(Action::MarkerDone(id, done)) => self.change_marker(&id, |m| m.done = done),
            Some(Action::Delete(id)) => {
                self.change_marker(&id, Marker::delete);
                if self.editing.as_ref() == Some(&id) {
                    self.editing = None;
                }
            }
            Some(Action::Done(place, done)) => self.set_done(&place, done),
            Some(Action::CopyView) => self.copy_view(ui.ctx()),
            None => {}
        }
    }
}

/// The context menu's contents at block `[bx, bz]`, for `target` if the right-click was on one
/// (and whether it's done), and `portal` if it was on a portal, and what was asked for. `loot`
/// is the Loot item, if one is chosen.
pub(super) fn menu(
    ui: &mut egui::Ui,
    [bx, bz]: [i32; 2],
    target: Option<&(MenuTarget, bool)>,
    portal: Option<&portal_layer::PortalRef>,
    world: &World,
    loot: Option<&str>,
) -> Option<Action> {
    let mut action = None;
    ui.label(style::mono(format!("{bx}, {bz}")).color(style::MUTED));
    match target {
        Some((MenuTarget::Marker(id), done)) => {
            if ui.button("Edit marker").clicked() {
                action = Some(Action::Edit(id.clone()));
                ui.close();
            }
            let text = if *done { "Mark not done" } else { "Mark done" };
            if ui.button(text).clicked() {
                action = Some(Action::MarkerDone(id.clone(), !done));
                ui.close();
            }
            if ui.button("Delete marker").clicked() {
                action = Some(Action::Delete(id.clone()));
                ui.close();
            }
        }
        Some((MenuTarget::Place(place), done)) => {
            let text = if *done { "Mark not done" } else { "Mark done" };
            if ui.button(text).clicked() {
                action = Some(Action::Done(place.clone(), !done));
                ui.close();
            }
        }
        None => {}
    }
    if let Some(portal) = portal
        && world.dimension_kind() != Dimension::End
        && ui
            .button("Show its link area")
            .on_hover_text(
                "Where in the other dimension a new portal would link to this one, at the \
                 view's height",
            )
            .clicked()
    {
        action = Some(Action::LinkArea(portal.clone()));
        ui.close();
    }
    if ui
        .button("Add a marker here")
        .on_hover_text("Or press M")
        .clicked()
    {
        action = Some(Action::AddMarker);
        ui.close();
    }
    if ui
        .button("Add a portal here")
        .on_hover_text("A Nether portal marker, its lowest block here at the view's height")
        .clicked()
    {
        action = Some(Action::AddPortal);
        ui.close();
    }
    ui.separator();
    if let Some(locate) = nearest_menus(ui, world, loot) {
        action = Some(Action::Locate(locate));
    }
    if ui.button("Centre here").clicked() {
        action = Some(Action::Centre);
        ui.close();
    }
    if ui.button("Copy coordinates").clicked() {
        ui.ctx().copy_text(format!("{bx} {bz}"));
        ui.close();
    }
    if ui
        .button("Copy the view text")
        .on_hover_text("A line that opens this view in another spyglass")
        .clicked()
    {
        action = Some(Action::CopyView);
        ui.close();
    }
    action
}

/// The context menu's "Nearest ..." entries: a structure, a feature, the Loot item `loot` if
/// one is chosen, or a biome. What to locate, if one was picked.
pub(super) fn nearest_menus(
    ui: &mut egui::Ui,
    world: &World,
    loot: Option<&str>,
) -> Option<Target> {
    let mut picked = None;
    ui.menu_button("Nearest structure", |ui| {
        egui::ScrollArea::vertical()
            .max_height(320.0)
            .show(ui, |ui| {
                let mut ids: Vec<&str> = world
                    .structures()
                    .filter(|id| world.structure_generates(id))
                    .collect();
                ids.sort_unstable_by_key(|id| spyglass::display_name(id));
                for id in ids {
                    if ui.button(spyglass::display_name(id)).clicked() {
                        picked = Some(Target::Structure(id.to_string()));
                        ui.close();
                    }
                }
            });
    });
    ui.menu_button("Nearest feature", |ui| {
        for (name, placed) in icons::FEATURE_KINDS {
            if ui.button(*name).clicked() {
                picked = Some(Target::Feature {
                    name: name.to_string(),
                    placed: placed.iter().map(|p| p.to_string()).collect(),
                });
                ui.close();
            }
        }
    });
    if let Some(item) = loot {
        let text = format!("Nearest {}", spyglass::display_name(item));
        if ui
            .button(text)
            .on_hover_text("The nearest container holding the Loot item")
            .clicked()
        {
            picked = Some(Target::Loot(item.to_string()));
            ui.close();
        }
    }
    ui.menu_button("Nearest biome", |ui| {
        egui::ScrollArea::vertical()
            .max_height(320.0)
            .show(ui, |ui| {
                let mut ids: Vec<&String> = world.biomes().iter().collect();
                ids.sort_unstable_by_key(|id| spyglass::display_name(id));
                for id in ids {
                    if ui.button(spyglass::display_name(id)).clicked() {
                        picked = Some(Target::Biome(id.clone()));
                        ui.close();
                    }
                }
            });
    });
    picked
}

/// What the map's context menu asked for.
pub(in crate::app) enum Action {
    /// Find the nearest of something from the block clicked.
    Locate(Target),
    /// Centre the map on the block clicked.
    Centre,
    /// Add a marker at the block clicked.
    AddMarker,
    /// Add a portal marker at the block clicked, at the view's height.
    AddPortal,
    /// Show the link area of this portal.
    LinkArea(portal_layer::PortalRef),
    /// Edit the marker with this id.
    Edit(String),
    /// Mark the marker with this id done or not.
    MarkerDone(String, bool),
    /// Delete the marker with this id.
    Delete(String),
    /// Mark a place done or not.
    Done(Place, bool),
    /// Copy the view text.
    CopyView,
}
