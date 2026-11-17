//! `spyglass world`: the saves in the launchers, and what spyglass makes of one.

use std::path::Path;

use anyhow::Result;
use spyglass::{
    PackUse, SaveDetails, SaveHoldings, SaveInspector, SaveReport, SaveSubject, SaveWarning, Scan,
};

use crate::args::WorldCmd;
use crate::json;

pub fn run(what: WorldCmd) -> Result<()> {
    match what {
        WorldCmd::List { bundles, json } => list(&bundles, json),
        WorldCmd::Info {
            save,
            bundles,
            quick,
            json,
        } => {
            let dir = spyglass::find_save(&save)?;
            let scan = if quick { Scan::Quick } else { Scan::Chunks };
            let report = spyglass::inspect_save(&dir, &bundles, scan)?;
            info(&report, json)
        }
    }
}

fn list(bundles: &Path, as_json: bool) -> Result<()> {
    let inspector = SaveInspector::new(bundles)?;
    let mut rows = Vec::new();
    for found in spyglass::launcher_saves() {
        let report = inspector.inspect(&found.dir, Scan::Quick);
        rows.push((found, report));
    }
    if as_json {
        let saves = rows
            .iter()
            .map(|(found, report)| match report {
                Ok(r) => json::ListedSave::Read {
                    launcher: &found.launcher,
                    save: Box::new(json::Save::new(r)),
                },
                Err(e) => json::ListedSave::Unreadable {
                    launcher: &found.launcher,
                    dir: json::path(&found.dir),
                    error: e.to_string(),
                },
            })
            .collect();
        return json::print(&json::WorldList {
            format: json::FORMAT,
            command: "world list",
            saves,
        });
    }
    if rows.is_empty() {
        out!("no saves found in the official launcher's or the Modrinth App's folders");
        return Ok(());
    }
    let mut launcher = "";
    for (found, report) in &rows {
        if found.launcher != launcher {
            launcher = &found.launcher;
            out!("{launcher}");
        }
        let folder = found.dir.file_name().unwrap_or_default().to_string_lossy();
        match report {
            Ok(r) => out!(
                "  {:<32} {:<9} {}",
                folder,
                r.save.version_name.as_deref().unwrap_or("?"),
                if r.answerable() {
                    "supported".to_string()
                } else {
                    r.warnings
                        .iter()
                        .find(|w| w.blocks_answers())
                        .map_or_else(String::new, |w| format!("unsupported: {w}"))
                }
            ),
            Err(e) => out!("  {folder:<32} unreadable: {e}"),
        }
    }
    Ok(())
}

fn info(report: &SaveReport, as_json: bool) -> Result<()> {
    let details = report.details();
    if as_json {
        return json::print(&json::WorldInfo::new(report, &details));
    }
    let save = &report.save;
    out!("{} ({})", save.name, save.dir.display());
    match &report.bundle {
        Some(b) => out!("  bundle  {}", b.dir.display()),
        None => out!("  bundle  none"),
    }

    out!();
    out!("Settings");
    for setting in &details.settings {
        let label = match setting.setting.dimension() {
            Some(d) => format!("{}, {}", setting.setting.label(), d.name()),
            None => setting.setting.label().to_string(),
        };
        out!("  {label:<28} {}", setting.value);
        let source = &setting.source;
        let from = if source.present { "from" } else { "not in" };
        out!(
            "  {:<28} {from} {} {}; decides {}",
            "",
            source.file,
            source.field,
            setting.decides()
        );
        warnings_about(&details, &SaveSubject::Setting(setting.setting));
    }

    out!();
    out!("Data packs");
    for pack in &details.packs {
        let used = match pack.used {
            PackUse::Bundle => "the bundle's own data".to_string(),
            PackUse::Applied | PackUse::Feature => {
                let from = if pack.used == PackUse::Applied {
                    "applied from datapacks/"
                } else {
                    "applied from the bundle"
                };
                match answer_list(&pack.answers) {
                    Some(answers) => format!("{from}: changes {answers}"),
                    None => format!("{from}: changes nothing spyglass reads"),
                }
            }
            PackUse::NotApplied => "not applied".to_string(),
            other => other.kind().replace('_', " "),
        };
        out!("  {:<28} {used}", pack.name);
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
            out!("  {:<28}   {registry}: {}{read}", "", parts.join(", "));
        }
        warnings_about(&details, &SaveSubject::Pack(pack.name.clone()));
    }

    out!();
    out!("Dimensions");
    for dimension in &details.dimensions {
        let answers = if dimension.answerable {
            "answered"
        } else {
            "not answered"
        };
        let region = dimension
            .region
            .strip_prefix(&save.dir)
            .unwrap_or(&dimension.region);
        out!(
            "  {:<28} {answers}; regions in {}",
            dimension.dimension.name(),
            region.display()
        );
        warnings_about(&details, &SaveSubject::Dimension(dimension.dimension));
        match &dimension.holdings {
            Some(holdings) => print_holdings(holdings),
            None => out!("  {:<28}   not scanned (--quick)", ""),
        }
        warnings_about(&details, &SaveSubject::Holdings(dimension.dimension));
    }

    out!();
    if details.warnings.iter().all(SaveWarning::is_note) {
        out!("spyglass's answers for this world are exact where they say so.");
    } else {
        let blocking = details
            .warnings
            .iter()
            .filter(|w| w.blocks_answers())
            .count();
        let other = details.warnings.iter().filter(|w| !w.is_note()).count() - blocking;
        out!("{blocking} errors and {other} warnings, each under what it's about.");
    }
    Ok(())
}

/// Prints the warnings about `subject`, under it.
fn warnings_about(details: &SaveDetails, subject: &SaveSubject) {
    for warning in details.warnings_about(subject) {
        let level = if warning.blocks_answers() {
            "error"
        } else if warning.is_note() {
            "note"
        } else {
            "warning"
        };
        out!("    {level}: {warning}");
    }
}

/// `biomes, terrain and structures`, or `None` for no answers.
fn answer_list(answers: &[spyglass::Answer]) -> Option<String> {
    let names: Vec<&str> = answers.iter().map(|a| a.name()).collect();
    match names.as_slice() {
        [] => None,
        [one] => Some((*one).to_string()),
        [rest @ .., last] => Some(format!("{} and {last}", rest.join(", "))),
    }
}

/// Counts as `name 3, other 1`, at most `shown` of them, then how many more.
fn counts(counts: &[(String, usize)], shown: usize) -> String {
    let mut parts: Vec<String> = counts
        .iter()
        .take(shown)
        .map(|(name, n)| format!("{name} {n}"))
        .collect();
    if counts.len() > shown {
        parts.push(format!("and {} more", counts.len() - shown));
    }
    parts.join(", ")
}

fn print_holdings(holdings: &SaveHoldings) {
    let line = |label: &str, text: String| out!("  {:<28}   {label:<9} {text}", "");
    line(
        "chunks",
        format!(
            "{}: {} full, {} proto, {} unreadable",
            holdings.chunks, holdings.full, holdings.proto, holdings.unreadable
        ),
    );
    if holdings.unreadable_files > 0 {
        line(
            "",
            format!("and {} unreadable region files", holdings.unreadable_files),
        );
    }
    if !holdings.versions.is_empty() {
        let versions: Vec<String> = holdings
            .versions
            .iter()
            .map(|v| match (v.data_version, &v.version) {
                (Some(d), Some(name)) => format!("{name} ({d}) {}", v.chunks),
                (Some(d), None) => format!("data version {d} {}", v.chunks),
                (None, _) => format!("before 1.9 {}", v.chunks),
            })
            .collect();
        line("versions", versions.join(", "));
    }
    if let Some([min, max]) = holdings.bounds {
        line(
            "bounds",
            format!(
                "chunks {}, {} to {}, {}; InhabitedTime summed {} ticks",
                min[0], min[1], max[0], max[1], holdings.inhabited_ticks
            ),
        );
    }
    let total = |c: &[(String, usize)]| c.iter().map(|(_, n)| n).sum::<usize>();
    if !holdings.starts.is_empty() {
        line(
            "starts",
            format!(
                "{} answered from the save: {}",
                total(&holdings.starts),
                counts(&holdings.starts, 6)
            ),
        );
    }
    if !holdings.dropped_starts.is_empty() {
        line(
            "",
            format!(
                "{} dropped, naming no structure the bundle has: {}",
                total(&holdings.dropped_starts),
                counts(&holdings.dropped_starts, 6)
            ),
        );
    }
    if !holdings.older_starts.is_empty() {
        line(
            "",
            format!(
                "{} in chunks before 1.18.2, predicted: {}",
                total(&holdings.older_starts),
                counts(&holdings.older_starts, 6)
            ),
        );
    }
    if !holdings.sealed_containers.is_empty() {
        line(
            "sealed",
            format!(
                "{} loot containers not yet opened: {}",
                total(&holdings.sealed_containers),
                counts(&holdings.sealed_containers, 6)
            ),
        );
    }
}
