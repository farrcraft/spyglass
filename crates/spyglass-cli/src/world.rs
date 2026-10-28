//! `spyglass world`: the saves in the launchers, and what spyglass makes of one.

use std::path::Path;

use anyhow::Result;
use spyglass::{SaveInspector, SaveReport, Scan};

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
                    save: json::Save::new(r),
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
    if as_json {
        return json::print(&json::WorldInfo {
            format: json::FORMAT,
            command: "world info",
            save: json::Save::new(report),
        });
    }
    let save = &report.save;
    let place = |l: &spyglass::SaveLocation| {
        let [x, y, z] = l.block();
        format!("{x} {y} {z} in {}", l.dimension)
    };
    out!("{} ({})", save.name, save.dir.display());
    out!(
        "  version  {} (data version {})",
        save.version_name.as_deref().unwrap_or("unknown"),
        save.data_version
            .map_or("none, before 1.9".to_string(), |v| v.to_string())
    );
    out!("  seed     {}", save.seed);
    if let Some(spawn) = &save.spawn {
        out!("  spawn    {}", place(spawn));
    }
    if let Some(player) = &save.player {
        out!("  player   {}", place(player));
    }
    out!("  packs    {}", save.enabled_packs.join(", "));
    match &report.bundle {
        Some(b) => out!("  bundle   {}", b.dir.display()),
        None => out!("  bundle   none"),
    }
    if report.bundle.is_some() {
        let answers: Vec<&str> = spyglass::Dimension::ALL
            .into_iter()
            .filter(|&d| report.answerable_in(d))
            .map(spyglass::Dimension::name)
            .collect();
        let answers = if answers.is_empty() {
            "nothing".to_string()
        } else {
            answers.join(", ")
        };
        out!("  answers  {answers}");
    }
    if report.warnings.is_empty() {
        out!("spyglass's answers for this world are exact where they say so.");
    }
    for warning in &report.warnings {
        let level = if warning.blocks_answers() {
            "error"
        } else {
            "warning"
        };
        out!("{level}: {warning}");
    }
    Ok(())
}
