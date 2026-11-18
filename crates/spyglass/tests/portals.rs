//! `Game::portal_target` and `portal_link` against the game's own portal code, played on its
//! server (`docs/specs/portals.md`, `docs/specs/oracle.md#portal-files`).

use std::collections::BTreeSet;
use std::fs;

use serde_json::Value;
use spyglass::{Axis, Border, Dimension, Game, Link, Portal, portal_link};

/// Every kind of case the portal file must have: random layouts and each constructed edge.
const KINDS: &[&str] = &[
    "random",
    "radius-in",
    "radius-out",
    "height",
    "y-tie",
    "chunk-tie-z",
    "chunk-tie-x",
    "section-tie",
    "border-outside",
    "border-inside",
    "border-clamp",
    "y-unclamped",
    "stale",
    "end",
    "built",
];

#[test]
#[ignore = "needs oracle-out/<version>/portals and a bundle: cargo xtask oracle portals"]
fn portal_links_match_the_game() {
    let path = spyglass_oracle::out_dir()
        .join("portals")
        .join("portals.json");
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}; run cargo xtask oracle portals", path.display()));
    let file: Value = serde_json::from_str(&text).unwrap();
    let game =
        Game::open_with_packs(spyglass_oracle::bundle_dir(), &spyglass_oracle::packs()).unwrap();
    let mut kinds = BTreeSet::new();
    let mut failures = Vec::new();
    let (mut answers, mut tied) = (0, 0);
    for layout in file["layouts"].as_array().unwrap() {
        let name = layout["name"].as_str().unwrap();
        let kind = layout["kind"].as_str().unwrap();
        kinds.insert(kind);
        let mut borders = [Border::default(); 3];
        for border in layout["borders"].as_array().unwrap() {
            let center = floats(&border["center"]);
            borders[dimension(&border["dimension"]).index()] = Border {
                center: [center[0], center[1]],
                size: border["size"].as_f64().unwrap(),
                ..Border::default()
            };
        }
        let portals: Vec<Portal> = layout["portals"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| {
                let axis = if p["axis"] == "x" { Axis::X } else { Axis::Z };
                Portal::new(
                    dimension(&p["dimension"]),
                    axis,
                    block(&p["corner"]),
                    int(&p["width"]),
                    int(&p["height"]),
                )
                .unwrap()
            })
            .collect();
        // The stale records aren't portals: their blocks were replaced.
        let mut check = |what: String, link: Link, found: &Value, failures: &mut Vec<String>| {
            answers += 1;
            tied += usize::from(matches!(link, Link::Tied(_)));
            if let Some(why) = misjudged(kind, &link, found) {
                failures.push(format!(
                    "{name}: {what}: {why}: spyglass {link:?}, the game {found}"
                ));
            }
        };
        for search in layout["searches"].as_array().unwrap() {
            let to = dimension(&search["dimension"]);
            let exit = block(&search["exit"]);
            let link = portal_link(to, exit, &portals, &borders[to.index()]);
            check(
                format!("search {to} {exit:?}"),
                link,
                &search["found"],
                &mut failures,
            );
        }
        for trip in layout["trips"].as_array().unwrap() {
            let from = dimension(&trip["dimension"]);
            let pos = floats(&trip["pos"]);
            let pos = [pos[0], pos[1], pos[2]];
            let to = dimension(&trip["to"]);
            let target = game.portal_target(from, pos, &borders[to.index()]).unwrap();
            let exit = block(&trip["exit"]);
            if target.dimension != to || target.exit != exit {
                failures.push(format!(
                    "{name}: trip from {from} {pos:?}: spyglass aims at {} {:?}, the game {to} {exit:?}",
                    target.dimension, target.exit
                ));
                continue;
            }
            let link = portal_link(to, exit, &portals, &borders[to.index()]);
            // The game builds a portal exactly when its search finds none.
            if trip["built"].as_bool().unwrap() != trip["found"].is_null() {
                failures.push(format!(
                    "{name}: trip from {from} {pos:?}: built and found disagree"
                ));
            }
            check(
                format!("trip from {from} {pos:?}"),
                link,
                &trip["found"],
                &mut failures,
            );
        }
    }
    let missing: Vec<&&str> = KINDS.iter().filter(|k| !kinds.contains(**k)).collect();
    assert!(
        missing.is_empty(),
        "{}: no cases of {missing:?}",
        path.display()
    );
    assert!(
        failures.is_empty(),
        "{} of {answers} answers differ ({tied} tied), first: {:#?}",
        failures.len(),
        &failures[..failures.len().min(20)]
    );
    assert!(tied > 0, "no tied answers at all");
}

/// A real save's portals, read and linked by spyglass, against trips the game's server made from
/// every portal block a copy of the save records (`cargo xtask oracle portals --world SAVE`): the
/// records are the server's portal blocks, every trip aims where spyglass says, and lands at the
/// portal spyglass links it to, or builds one where spyglass finds none.
#[test]
#[ignore = "needs oracle-out/<version>/portals/world-*.json: cargo xtask oracle portals --world SAVE"]
fn a_save_s_portal_links_match_the_game() {
    let dir = spyglass_oracle::out_dir().join("portals");
    let files: Vec<_> = fs::read_dir(&dir)
        .unwrap_or_else(|e| {
            panic!(
                "{}: {e}; run cargo xtask oracle portals --world SAVE",
                dir.display()
            )
        })
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("world-"))
        })
        .collect();
    assert!(!files.is_empty(), "no world-*.json in {}", dir.display());
    let bundles = spyglass_oracle::bundle_dir()
        .parent()
        .unwrap()
        .to_path_buf();
    let mut failures = Vec::new();
    let mut trips = 0;
    for path in files {
        let file: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let save = file["save"].as_str().unwrap();
        let report = spyglass::inspect_save(save, &bundles, spyglass::Scan::Quick).unwrap();
        let game = report.open_game().unwrap();
        let portals = report.portals(&game).unwrap();
        let (pieces, _) = portals.pieces();
        let layout = &file["layouts"][0];
        for existing in layout["existing"].as_array().unwrap() {
            if existing["portal"] != true {
                failures.push(format!(
                    "{save}: the record at {} {} isn't a portal on the server",
                    existing["dimension"], existing["block"]
                ));
            }
        }
        for trip in layout["trips"].as_array().unwrap() {
            if trip["skipped"] == true {
                continue;
            }
            trips += 1;
            let from = dimension(&trip["dimension"]);
            let pos = floats(&trip["pos"]);
            let pos = [pos[0], pos[1], pos[2]];
            let to = dimension(&trip["to"]);
            let border = &portals.borders[to.index()];
            let target = game.portal_target(from, pos, border).unwrap();
            let exit = block(&trip["exit"]);
            if target.dimension != to || target.exit != exit {
                failures.push(format!(
                    "{save}: trip from {from} {pos:?}: spyglass aims at {} {:?}, the game {to} {exit:?}",
                    target.dimension, target.exit
                ));
                continue;
            }
            let link = portal_link(to, exit, &pieces, border);
            if let Some(why) = misjudged("random", &link, &trip["found"]) {
                failures.push(format!(
                    "{save}: trip from {from} {pos:?}: {why}: spyglass {link:?}, the game {}",
                    trip["found"]
                ));
            }
        }
    }
    println!("{trips} trips");
    assert!(trips > 0, "no trips");
    assert!(
        failures.is_empty(),
        "{} of {trips} differ, first: {:#?}",
        failures.len(),
        &failures[..failures.len().min(20)]
    );
}

/// Why spyglass's `link` isn't the game's answer, `found` (a block or null), for a case of
/// `kind`, or `None` if it is. A certain link must be the game's; a tie must hold it, and only
/// random layouts and section ties may be tied, while section ties must be.
fn misjudged(kind: &str, link: &Link, found: &Value) -> Option<&'static str> {
    let found = (!found.is_null()).then(|| block(found));
    let may_tie = matches!(kind, "random" | "section-tie");
    match (link, found) {
        (Link::NewPortal, None) => None,
        (Link::Lands(landing), Some(b)) if landing.block == b => None,
        (Link::Tied(landings), Some(b)) if landings.iter().any(|l| l.block == b) => None,
        (Link::Tied(_), Some(_)) => Some("the tie doesn't hold the game's"),
        _ => Some("a different portal"),
    }
    .or_else(|| match link {
        Link::Tied(_) if !may_tie => Some("tied where the game's order is known"),
        Link::Lands(_) | Link::NewPortal if kind == "section-tie" => Some("not tied"),
        _ => None,
    })
}

fn dimension(value: &Value) -> Dimension {
    let id = value.as_str().unwrap();
    Dimension::from_id(id).unwrap_or_else(|| panic!("unknown dimension {id}"))
}

fn int(value: &Value) -> i32 {
    i32::try_from(value.as_i64().unwrap()).unwrap()
}

fn block(value: &Value) -> [i32; 3] {
    let a = value.as_array().unwrap();
    [int(&a[0]), int(&a[1]), int(&a[2])]
}

fn floats(value: &Value) -> Vec<f64> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect()
}
