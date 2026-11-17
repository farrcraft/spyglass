//! `world info --json` against the library's report (`SaveReport::details`, which the viewer's
//! World tab shows), field by field, on real saves on this machine: one with chunks from an
//! older version, and one with a data pack. Ignored by default; run with
//! `cargo test --release -p spyglass-cli --test local_report -- --ignored --nocapture`.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use spyglass::{SaveDetails, SaveReport, SaveSubject, Scan};
use spyglass_data::nbt::{self, Tag};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The report from the command and from the library.
fn both(dir: &Path) -> (Value, SaveReport, SaveDetails) {
    let bundles = root().join("bundles");
    let out = Command::new(env!("CARGO_BIN_EXE_spyglass"))
        .args(["world", "info", "--json", "--bundles"])
        .arg(&bundles)
        .arg(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    let report = spyglass::inspect_save(dir, &bundles, Scan::Chunks).unwrap();
    let details = report.details();
    (json, report, details)
}

/// Checks every field of the command's report against the library's.
fn same(json: &Value, details: &SaveDetails) {
    let settings = json["settings"].as_array().unwrap();
    assert_eq!(settings.len(), details.settings.len());
    for (j, s) in settings.iter().zip(&details.settings) {
        assert_eq!(j["setting"], s.setting.kind());
        assert_eq!(
            j["dimension"].as_str(),
            s.setting.dimension().map(|d| d.id())
        );
        assert_eq!(j["value"], s.value.as_str());
        assert_eq!(j["file"], s.source.file.as_str());
        assert_eq!(j["field"], s.source.field.as_str());
        assert_eq!(j["present"], s.source.present);
        assert_eq!(j["decides"], s.decides());
    }
    let packs = json["packs"].as_array().unwrap();
    assert_eq!(packs.len(), details.packs.len());
    for (j, p) in packs.iter().zip(&details.packs) {
        assert_eq!(j["name"], p.name.as_str());
        assert_eq!(j["used"], p.used.kind());
        let answers: Vec<&str> = p.answers.iter().map(|a| a.name()).collect();
        assert_eq!(j["answers"], serde_json::json!(answers));
        match &p.changes {
            None => assert!(j.get("changes").is_none()),
            Some(changes) => {
                assert_eq!(
                    j["changes"].as_object().unwrap().len(),
                    changes.registries.len()
                );
                for (registry, c) in &changes.registries {
                    let r = &j["changes"][registry];
                    for (name, list) in [
                        ("replaced", &c.replaced),
                        ("added", &c.added),
                        ("merged", &c.merged),
                        ("hidden", &c.hidden),
                        ("shadowed", &c.shadowed),
                        ("unreadable", &c.unreadable),
                    ] {
                        assert_eq!(r[name], serde_json::json!(list), "{registry} {name}");
                    }
                }
            }
        }
    }
    let dimensions = json["dimensions"].as_array().unwrap();
    assert_eq!(dimensions.len(), details.dimensions.len());
    for (j, d) in dimensions.iter().zip(&details.dimensions) {
        assert_eq!(j["dimension"], d.dimension.id());
        assert_eq!(j["answerable"], d.answerable);
        let h = d.holdings.as_ref().unwrap();
        let jh = &j["holdings"];
        for (name, n) in [
            ("chunks", h.chunks),
            ("full", h.full),
            ("proto", h.proto),
            ("unreadable", h.unreadable),
            ("unreadable_files", h.unreadable_files),
        ] {
            assert_eq!(jh[name], n, "{name}");
        }
        assert_eq!(jh["inhabited_ticks"], h.inhabited_ticks);
        assert_eq!(jh["bounds"], serde_json::json!(h.bounds));
        let versions: Vec<Value> = h
            .versions
            .iter()
            .map(|v| serde_json::json!({"data_version": v.data_version, "version": v.version, "chunks": v.chunks}))
            .collect();
        assert_eq!(jh["versions"], serde_json::json!(versions));
        for (name, counts) in [
            ("starts", &h.starts),
            ("dropped_starts", &h.dropped_starts),
            ("older_starts", &h.older_starts),
            ("sealed_containers", &h.sealed_containers),
        ] {
            let list: Vec<Value> = counts
                .iter()
                .map(|(id, count)| serde_json::json!({"id": id, "count": count}))
                .collect();
            assert_eq!(jh[name], serde_json::json!(list), "{name}");
        }
    }
    let warnings = json["warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), details.warnings.len());
    for (j, w) in warnings.iter().zip(&details.warnings) {
        assert_eq!(j["kind"], w.kind());
        assert_eq!(j["message"], w.to_string());
        let about = &j["about"];
        match w.subject() {
            SaveSubject::Setting(s) => {
                assert_eq!(about["kind"], "setting");
                assert_eq!(about["setting"], s.kind());
            }
            SaveSubject::Pack(p) => {
                assert_eq!(about["kind"], "pack");
                assert_eq!(about["pack"], p.as_str());
            }
            SaveSubject::Dimension(d) => {
                assert_eq!(about["kind"], "dimension");
                assert_eq!(about["dimension"], d.id());
            }
            SaveSubject::Holdings(d) => {
                assert_eq!(about["kind"], "holdings");
                assert_eq!(about["dimension"], d.id());
            }
        }
    }
}

/// A launcher save by folder name.
fn launcher_save(name: &str) -> Option<PathBuf> {
    spyglass::launcher_saves()
        .into_iter()
        .map(|s| s.dir)
        .find(|d| d.file_name().is_some_and(|n| n == name))
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

#[test]
#[ignore = "reads the saves on this machine"]
fn the_command_and_the_library_give_the_same_report() {
    // Chunks from an older version.
    let older = launcher_save("Modrinth Latest upgraded").expect("the save");
    let (json, _, details) = both(&older);
    same(&json, &details);
    let overworld = &details.dimensions[0].holdings.as_ref().unwrap();
    assert!(overworld.versions.len() > 1, "{:?}", overworld.versions);
    assert!(details.warnings.iter().any(|w| w.kind() == "older_chunks"));
    println!("{}: the same", older.display());

    // A 26.3 save with a data pack: a copy of one, with Terralith enabled.
    let pack = root().join("packs/Terralith_v2.6.5+26.3.zip");
    assert!(pack.is_file(), "put Terralith in packs/");
    let original = launcher_save("Portal Link Test").expect("the save");
    let tmp = tempfile::tempdir().unwrap();
    let copy = tmp.path().join("with-pack");
    copy_dir(&original, &copy);
    std::fs::create_dir_all(copy.join("datapacks")).unwrap();
    std::fs::copy(&pack, copy.join("datapacks/terralith.zip")).unwrap();
    let level = copy.join("level.dat");
    let (name, mut root_tag) = nbt::read_compressed(&std::fs::read(&level).unwrap()).unwrap();
    if let Tag::Compound(root) = &mut root_tag
        && let Some(Tag::Compound(data)) = root.get_mut("Data")
        && let Some(Tag::Compound(packs)) = data.get_mut("DataPacks")
        && let Some(Tag::List(enabled)) = packs.get_mut("Enabled")
    {
        enabled.push(Tag::String("file/terralith.zip".into()));
    } else {
        panic!("no Data.DataPacks.Enabled");
    }
    std::fs::write(&level, nbt::write_compressed(&name, &root_tag)).unwrap();
    let (json, report, details) = both(&copy);
    same(&json, &details);
    let terralith = details
        .packs
        .iter()
        .find(|p| p.name == "file/terralith.zip")
        .unwrap();
    let biomes = &terralith.changes.as_ref().unwrap().registries["worldgen/biome"];
    // What the zip holds, counted from its entries: 39 vanilla biomes and 95 of its own, 29
    // structures and 183 templates. 18 more structure files are `.json.disabled`, which the game
    // doesn't load.
    assert_eq!(biomes.replaced.len(), 39);
    assert_eq!(biomes.added.len(), 95);
    let registries = &terralith.changes.as_ref().unwrap().registries;
    assert_eq!(registries["worldgen/structure"].added.len(), 29);
    let templates = &registries["structure"];
    assert_eq!(templates.added.len() + templates.replaced.len(), 183);
    // Its old templates can't be opened yet, in any dimension: errors under the pack.
    let errors: Vec<&spyglass::SaveWarning> = details
        .warnings
        .iter()
        .filter(|w| w.kind() == "pack_error")
        .collect();
    assert_eq!(errors.len(), 3);
    for error in errors {
        assert_eq!(
            error.subject(),
            SaveSubject::Pack("file/terralith.zip".into())
        );
        assert!(
            matches!(error, spyglass::SaveWarning::PackError { file: Some(f), .. }
                if f == "data/terralith/structure/mage/tower.nbt"),
            "{error:?}"
        );
    }
    println!(
        "{} with Terralith: the same; {} packs applied; warnings: {:#?}",
        original.display(),
        report.packs.len(),
        details
            .warnings
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
    );
}
