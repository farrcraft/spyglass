//! `spyglass portal link`: where a Nether portal leads, from a portal given by position or from
//! every portal in a save (`docs/specs/portals.md`). Needs a version, not a seed.

use anyhow::{Context, Result, bail};
use spyglass::{
    Axis, Border, Dimension, Game, LinkOutcome, Portal, PortalFlag, PortalNote, SavePortal,
    SavePortals, SaveReport, Scan, portal_destination, portal_search_radius,
};

use crate::args::{PortalCmd, PortalLink};
use crate::json;

pub fn run(what: &PortalCmd) -> Result<()> {
    match what {
        PortalCmd::Link(args) => link(args),
    }
}

/// Where a portal searched for links came from.
#[derive(Clone, Copy)]
enum Owner {
    /// The `--portal` with this index.
    Given(usize),
    /// The save's portal with this index.
    Save(usize),
}

/// The game, and with `--world` the save and its portals.
struct Loaded {
    game: Game,
    save: Option<(SaveReport, SavePortals)>,
}

impl Loaded {
    fn open(args: &PortalLink) -> Result<Loaded> {
        match &args.world {
            Some(world) => {
                let dir = spyglass::find_save(world)?;
                let report = spyglass::inspect_save(&dir, &args.bundles, Scan::Quick)?;
                let game = report.open_game()?;
                let portals = report
                    .portals(&game)
                    .with_context(|| format!("reading the portals of {}", dir.display()))?;
                Ok(Loaded {
                    game,
                    save: Some((report, portals)),
                })
            }
            None => Ok(Loaded {
                game: crate::open_game(
                    args.bundle.as_deref(),
                    &args.bundles,
                    args.game_version.as_deref(),
                    &args.packs,
                )?,
                save: None,
            }),
        }
    }

    fn portals(&self) -> Option<&SavePortals> {
        self.save.as_ref().map(|(_, portals)| portals)
    }

    fn report(&self) -> Option<&SaveReport> {
        self.save.as_ref().map(|(report, _)| report)
    }

    /// `to`'s border: the save's, or the default.
    fn border(&self, to: Dimension) -> Border {
        self.portals()
            .map_or_else(Border::default, |p| p.borders[to.index()])
    }

    /// The header every answer starts with.
    fn header<'a>(&'a self, command: &'a str) -> json::PortalHeader<'a> {
        json::PortalHeader {
            format: json::FORMAT,
            command,
            version: self.game.version(),
            world: self.report().map(|r| json::WorldRef {
                name: r.save.name.clone(),
                dir: json::path(&r.save.dir),
            }),
            warnings: warnings(self.portals(), self.game.unchecked_version()),
        }
    }

    /// Prints the notes after a text answer, and says when the bundle's version is unchecked.
    fn notes(&self) {
        if let Some(portals) = self.portals() {
            for note in &portals.notes {
                out!("note: {note}");
            }
        }
    }
}

/// The save's notes and an unchecked version, as JSON warnings.
fn warnings(
    portals: Option<&SavePortals>,
    unchecked: Option<spyglass::UncheckedVersion>,
) -> Option<Vec<json::Warning>> {
    let mut all = portals.map(|p| {
        p.notes
            .iter()
            .map(|note: &PortalNote| json::Warning {
                kind: note.kind(),
                message: note.to_string(),
                concern: None,
                dimension: Some(note.dimension().id()),
                about: None,
            })
            .collect::<Vec<_>>()
    });
    if let Some(unchecked) = unchecked {
        all.get_or_insert_default().push(json::Warning {
            kind: unchecked.kind(),
            message: unchecked.to_string(),
            concern: None,
            dimension: None,
            about: None,
        });
    }
    all
}

fn link(args: &PortalLink) -> Result<()> {
    let loaded = Loaded::open(args)?;
    crate::warn_unchecked(loaded.game.unchecked_version(), args.json);
    match (args.dimension, args.position) {
        (Some(dimension), Some(position)) => from_position(args, &loaded, dimension, position),
        (None, None) if loaded.save.is_some() => {
            if !args.portals.is_empty() {
                bail!("--portal needs a portal to link from: give DIMENSION and POSITION");
            }
            from_save(args, &loaded)
        }
        _ => bail!("give a portal, DIMENSION x,y,z, or a save with --world"),
    }
}

fn from_position(
    args: &PortalLink,
    loaded: &Loaded,
    dimension: Dimension,
    position: [i32; 3],
) -> Result<()> {
    let [width, height] = args.size;
    let source = Portal::new(dimension, args.axis, position, width, height)?;
    let mut candidates = Vec::new();
    let mut owners = Vec::new();
    for (i, spec) in args.portals.iter().enumerate() {
        let [w, h] = spec.size;
        candidates.push(
            Portal::new(spec.dimension, spec.axis, spec.corner, w, h)
                .with_context(|| format!("--portal {}", i + 1))?,
        );
        owners.push(Owner::Given(i));
    }
    // Ids for grouping: given portals first, then the save's.
    let given = args.portals.len();
    if let Some(save) = loaded.portals() {
        let (pieces, of) = save.pieces();
        candidates.extend(pieces);
        owners.extend(of.into_iter().map(Owner::Save));
    }
    let group = |i: usize| match owners[i] {
        Owner::Given(g) => g,
        Owner::Save(s) => given + s,
    };
    let owner_of = |id: usize| {
        if id < given {
            Owner::Given(id)
        } else {
            Owner::Save(id - given)
        }
    };
    let to = portal_destination(dimension);
    let border = loaded.border(to);
    let Some(links) = loaded.game.portal_links(&source, &candidates, &border)? else {
        bail!("a portal {height} high is too low for a player to stand in");
    };
    let outcomes = links.outcomes_by(group);
    let radius = portal_search_radius(to);

    if args.json {
        let describe = |id: usize| match owner_of(id) {
            // The given portals come first among the candidates.
            Owner::Given(g) => json::PortalRef {
                source: "given",
                index: g,
                portal: json::PortalOrSave::Given(json::portal(&candidates[g])),
            },
            Owner::Save(s) => json::PortalRef {
                source: "save",
                index: s,
                portal: json::PortalOrSave::Save(json::save_portal(
                    &loaded.portals().expect("a save").portals[s],
                )),
            },
        };
        return json::print(&json::PortalLinkAnswer {
            header: loaded.header("portal link"),
            portal: json::portal(&source),
            to: to.id(),
            exits: json::Exits {
                x: links.exits[0],
                y: links.exits[1],
                z: links.exits[2],
            },
            radius,
            outcomes: outcomes
                .iter()
                .map(|o| json::PortalOutcome {
                    exits: o.exits,
                    tied: o.tied,
                    portals: o.portals.iter().map(|&id| describe(id)).collect(),
                })
                .collect(),
        });
    }

    let name = |id: usize| match owner_of(id) {
        Owner::Given(g) => {
            let [x, y, z] = candidates[g].corner;
            format!(
                "--portal {} ({} {x} {y} {z})",
                g + 1,
                candidates[g].dimension.name()
            )
        }
        Owner::Save(s) => {
            let save = &loaded.portals().expect("a save").portals[s];
            format!("save portal #{} {}", s + 1, place(save))
        }
    };
    out!(
        "A {width} × {height} portal along {} at {} {} {} in {} leads to {} (Minecraft {}):",
        axis_name(args.axis),
        position[0],
        position[1],
        position[2],
        dimension.name(),
        to.name(),
        loaded.game.version()
    );
    let [x, y, z] = links.exits;
    out!(
        "  {} exit blocks, x {} to {}, y {} to {}, z {} to {}, each searched {radius} blocks around in x and z",
        links.links.len(),
        x[0],
        x[1],
        y[0],
        y[1],
        z[0],
        z[1]
    );
    print_outcomes(&outcomes, links.links.len(), &name);
    out!("For a standing player: one moving through the portal can aim a tick's movement further.");
    loaded.notes();
    Ok(())
}

/// Each outcome as a line: how many exits, and where they land.
fn print_outcomes(outcomes: &[LinkOutcome], total: usize, name: &dyn Fn(usize) -> String) {
    for outcome in outcomes {
        let share = if outcome.exits == total {
            "all".to_string()
        } else {
            format!("{} of {total}", outcome.exits)
        };
        let lands = match &outcome.portals[..] {
            [] => "find no portal in range: the game builds one".to_string(),
            [one] => format!("land at {}", name(*one)),
            many => format!(
                "are tied between {} (the game's pick can't be known)",
                many.iter()
                    .map(|&p| name(p))
                    .collect::<Vec<_>>()
                    .join(" and ")
            ),
        };
        out!("  {share} exits {lands}");
    }
}

fn from_save(args: &PortalLink, loaded: &Loaded) -> Result<()> {
    let (report, portals) = loaded.save.as_ref().expect("a save");
    let links = loaded.game.save_portal_links(portals)?;
    if args.json {
        return json::print(&json::SavePortalsAnswer {
            header: loaded.header("portal link"),
            portals: links
                .iter()
                .map(|link| json::SavePortalLinkJson {
                    index: link.portal,
                    portal: json::save_portal(&portals.portals[link.portal]),
                    to: link.dimension.id(),
                    outcomes: link
                        .outcomes
                        .iter()
                        .map(|o| json::SaveOutcome {
                            exits: o.exits,
                            tied: o.tied,
                            portals: o.portals.clone(),
                        })
                        .collect(),
                    flags: link.flags.iter().map(json::flag).collect(),
                })
                .collect(),
        });
    }
    out!(
        "{} portals in {} (Minecraft {}):",
        portals.portals.len(),
        report.save.name,
        loaded.game.version()
    );
    let name = |i: usize| format!("#{} {}", i + 1, place(&portals.portals[i]));
    for link in &links {
        out!("{}", name(link.portal));
        let total = link.outcomes.iter().map(|o| o.exits).sum();
        print_outcomes(&link.outcomes, total, &name);
        for flag in &link.flags {
            let why = match flag {
                PortalFlag::Mislinked { to, back } => {
                    let back = if back.is_empty() {
                        "a new portal".to_string()
                    } else {
                        back.iter()
                            .map(|&b| format!("#{}", b + 1))
                            .collect::<Vec<_>>()
                            .join(" or ")
                    };
                    format!("mislinked: #{} leads to {back}, not back here", to + 1)
                }
                PortalFlag::Split => {
                    "split: different places in it lead to different portals".into()
                }
                PortalFlag::Unlinked => "unlinked: no portal is in range on the other side".into(),
                PortalFlag::Tied => "tied: some exits are equally near two portals".into(),
                PortalFlag::TooLow => "too low for a player to stand in".into(),
            };
            out!("  {why}");
        }
    }
    loaded.notes();
    Ok(())
}

/// A save portal's dimension, lowest block and shape, e.g. `the Nether -20 119 284, 2 × 3
/// along x`.
fn place(portal: &SavePortal) -> String {
    let [x, y, z] = portal.corner();
    let shape = match &portal.pieces[..] {
        [one] => format!(
            "{} × {} along {}",
            one.width,
            one.height,
            axis_name(one.axis)
        ),
        _ => format!("{} blocks", portal.blocks),
    };
    format!("{} {x} {y} {z}, {shape}", portal.dimension.name())
}

fn axis_name(axis: Axis) -> &'static str {
    match axis {
        Axis::X => "x",
        Axis::Z => "z",
    }
}
