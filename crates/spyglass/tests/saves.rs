//! The real saves on this machine: each world spyglass can answer for, and that no older version
//! generated part of, must have its saved spawn in the 11 × 11 chunks around spyglass's spawn
//! origin (`docs/specs/spawn.md`). A spawn moved by `/setworldspawn` would fail this, so it's a
//! sanity check of the user's own worlds, ignored by default. Run with
//! `cargo test --release -p spyglass --test saves -- --ignored --nocapture`.

use spyglass::{SaveWarning, Scan};

#[test]
#[ignore = "reads the saves on this machine"]
fn saved_spawns_are_near_the_spawn_origin() {
    let bundles = spyglass_oracle::default_dir().join("../bundles");
    let (mut checked, mut failures) = (0, Vec::new());
    for found in spyglass::launcher_saves() {
        let Ok(quick) = spyglass::inspect_save(&found.dir, &bundles, Scan::Quick) else {
            continue;
        };
        if !quick.answerable() {
            continue;
        }
        let report = spyglass::inspect_save(&found.dir, &bundles, Scan::Chunks).unwrap();
        let upgraded = report
            .warnings
            .iter()
            .any(|w| matches!(w, SaveWarning::OlderChunks { .. }));
        let Some(spawn) = report.save.spawn.as_ref().filter(|_| !upgraded) else {
            continue;
        };
        let world = report.open_world().unwrap();
        let origin = world.session(report.save.seed).unwrap().spawn_origin();
        let [x, _, z] = spawn.block();
        let chunk = [x >> 4, z >> 4];
        let offset = [chunk[0] - origin.chunk[0], chunk[1] - origin.chunk[1]];
        println!(
            "{}: spawn chunk {chunk:?}, origin {:?}",
            found.dir.display(),
            origin.chunk
        );
        checked += 1;
        if offset.iter().any(|d| d.abs() > 5) {
            failures.push(format!(
                "{}: spawn chunk {chunk:?} is outside the 11 x 11 around {:?} (moved by a command?)",
                found.dir.display(),
                origin.chunk
            ));
        }
    }
    println!("{checked} saves checked");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
