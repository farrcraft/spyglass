//! Where launchers keep world saves, at each platform's default location.

use std::path::{Path, PathBuf};

use directories::BaseDirs;

use crate::Save;

/// A launcher's saves folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavesDir {
    /// Which launcher, e.g. `Minecraft Launcher` or `Modrinth (Fabric 26.3)`.
    pub launcher: String,
    /// The `saves` folder.
    pub dir: PathBuf,
}

/// The saves folders that exist: the official launcher's (`.minecraft/saves`; on macOS
/// `~/Library/Application Support/minecraft/saves`) and each Modrinth App profile's
/// (`ModrinthApp/profiles/<profile>/saves` in the platform's data folder).
pub fn saves_dirs() -> Vec<SavesDir> {
    let Some(base) = BaseDirs::new() else {
        return Vec::new();
    };
    let mut found = Vec::new();
    let official = if cfg!(target_os = "windows") {
        base.data_dir().join(".minecraft")
    } else if cfg!(target_os = "macos") {
        base.data_dir().join("minecraft")
    } else {
        base.home_dir().join(".minecraft")
    };
    let saves = official.join("saves");
    if saves.is_dir() {
        found.push(SavesDir {
            launcher: "Minecraft Launcher".into(),
            dir: saves,
        });
    }
    let profiles = base.data_dir().join("ModrinthApp").join("profiles");
    let mut profiles: Vec<PathBuf> = std::fs::read_dir(profiles)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .collect();
    profiles.sort();
    for profile in profiles {
        let saves = profile.join("saves");
        if saves.is_dir() {
            let name = profile.file_name().unwrap_or_default().to_string_lossy();
            found.push(SavesDir {
                launcher: format!("Modrinth ({name})"),
                dir: saves,
            });
        }
    }
    found
}

/// The saves in a saves folder: subfolders with a `level.dat`, sorted by name.
pub fn saves_in(dir: &Path) -> Vec<PathBuf> {
    let mut saves: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| Save::is_save(p))
        .collect();
    saves.sort();
    saves
}
