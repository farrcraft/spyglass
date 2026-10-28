//! Reads every save the launchers have on this machine. Saves are the user's own, so this is
//! ignored by default; run with `cargo test --release -p spyglass-world --test local_saves --
//! --ignored --nocapture`.

use std::time::Instant;

use spyglass_world::{Save, launchers, region};

#[test]
#[ignore = "reads the saves on this machine"]
fn every_local_save_reads() {
    let mut failures = Vec::new();
    let mut read = 0;
    for saves in launchers::saves_dirs() {
        for dir in launchers::saves_in(&saves.dir) {
            match Save::read(&dir) {
                Ok(save) => {
                    read += 1;
                    let t0 = Instant::now();
                    let chunks =
                        region::chunk_versions(&save.region_dir("minecraft:overworld").unwrap());
                    let summary = match &chunks {
                        Ok(c) => format!(
                            "{} chunks, {} older ({} unversioned), {} unreadable",
                            c.total(),
                            c.older_than(save.data_version.unwrap_or(0)),
                            c.unversioned,
                            c.unreadable
                        ),
                        Err(e) => format!("regions: {e}"),
                    };
                    println!(
                        "{} [{}] {:?} {} seed {} {:?}, {} packs, player {}; {summary} in {:.1} s",
                        saves.launcher,
                        save.name,
                        save.layout,
                        save.version_name.as_deref().unwrap_or("?"),
                        save.seed,
                        save.generators
                            .get("minecraft:overworld")
                            .and_then(|g| g.get("type")),
                        save.enabled_packs.len(),
                        save.player.is_some(),
                        t0.elapsed().as_secs_f64()
                    );
                    if let Err(e) = chunks {
                        failures.push(format!("{}: {e}", dir.display()));
                    }
                }
                Err(e) => failures.push(format!("{}: {e}", dir.display())),
            }
        }
    }
    println!("{read} saves read");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
