//! The real saves on this machine: every `poi/` chunk and world border reads, and the portal
//! records are counted. A check of the readers on files the game wrote, ignored by default. Run
//! with `cargo test --release -p spyglass-world --test local_portals -- --ignored --nocapture`.

use spyglass_world::{BorderRead, Save, launchers, poi};

#[test]
#[ignore = "reads the saves on this machine"]
fn every_save_s_portal_records_and_borders_read() {
    let (mut folders, mut chunks, mut portals, mut invalid) = (0, 0, 0, 0);
    let mut failures = Vec::new();
    for saves in launchers::saves_dirs() {
        for entry in std::fs::read_dir(&saves.dir)
            .into_iter()
            .flatten()
            .flatten()
        {
            let Ok(save) = Save::read(entry.path()) else {
                continue;
            };
            for (dimension, sections) in [
                ("minecraft:overworld", -4..=19),
                ("minecraft:the_nether", 0..=15),
                ("minecraft:the_end", 0..=15),
            ] {
                let dir = save.region_dir(dimension).unwrap().with_file_name("poi");
                if dir.is_dir() {
                    folders += 1;
                    let index = poi::read(&dir, sections);
                    chunks += index.chunks.len();
                    for (_, valid) in index.portals() {
                        portals += 1;
                        invalid += usize::from(!valid);
                    }
                    if index.unreadable_chunks > 0 || index.unreadable_files > 0 {
                        failures.push(format!(
                            "{}: {} unreadable chunks, {} unreadable files",
                            dir.display(),
                            index.unreadable_chunks,
                            index.unreadable_files
                        ));
                    }
                }
                match save.world_border(dimension) {
                    Ok(BorderRead::Unreadable(why)) => failures.push(format!(
                        "{} {dimension}: border unreadable: {why}",
                        save.dir.display()
                    )),
                    Err(e) => failures.push(format!("{}: {e}", save.dir.display())),
                    // Older versions' default size was 6E7, which clamps to the same bounds.
                    Ok(BorderRead::Saved(border))
                        if border.center != [0.0, 0.0] || border.final_size() < 5.999_996_8E7 =>
                    {
                        println!("{} {dimension}: {border:?}", save.dir.display());
                    }
                    Ok(_) => {}
                }
            }
        }
    }
    println!(
        "{folders} poi folders, {chunks} chunks, {portals} portal records ({invalid} in sections not valid)"
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
