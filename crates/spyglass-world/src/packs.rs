//! What a save's enabled data packs contain, as far as spyglass can see.

use std::io;
use std::path::{Path, PathBuf};

use crate::Save;

/// What spyglass knows about one enabled data pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackContents {
    /// The game's own data.
    Vanilla,
    /// A `file/` pack in the save's `datapacks/` folder, with how many of its files are under
    /// `data/<namespace>/worldgen/` or `data/<namespace>/tags/worldgen/` (overlays included).
    /// Zero means it can't change worldgen.
    Files {
        /// The zip or folder.
        path: PathBuf,
        /// Worldgen files.
        worldgen_files: usize,
    },
    /// A `file/` pack whose zip or folder is missing or unreadable.
    Missing {
        /// Where it should be.
        path: PathBuf,
    },
    /// A built-in feature pack or a mod's pack: its files aren't in the save.
    Uninspectable,
}

/// Whether a path inside a pack is worldgen data: `data/<ns>/worldgen/...` or
/// `data/<ns>/tags/worldgen/...`, at the root or in an overlay folder.
fn is_worldgen(path: &str) -> bool {
    let parts: Vec<&str> = path.split(['/', '\\']).collect();
    parts.windows(3).enumerate().any(|(i, w)| {
        w[0] == "data"
            && (w[2] == "worldgen" || (w[2] == "tags" && parts.get(i + 3) == Some(&"worldgen")))
    })
}

fn folder_worldgen_files(root: &Path) -> io::Result<usize> {
    let mut count = 0;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let path = entry?.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .strip_prefix(root)
                .ok()
                .and_then(Path::to_str)
                .is_some_and(is_worldgen)
            {
                count += 1;
            }
        }
    }
    Ok(count)
}

fn zip_worldgen_files(path: &Path) -> io::Result<usize> {
    let file = std::fs::File::open(path)?;
    let archive = zip::ZipArchive::new(file).map_err(io::Error::other)?;
    archive.file_names().try_fold(0, |count, name| {
        let name = name.map_err(io::Error::other)?;
        Ok(count + usize::from(!name.ends_with('/') && is_worldgen(&name)))
    })
}

impl Save {
    /// What each enabled data pack contains, in load order.
    pub fn packs(&self) -> Vec<(String, PackContents)> {
        self.enabled_packs
            .iter()
            .map(|name| {
                let contents = match name.strip_prefix("file/") {
                    _ if name == "vanilla" => PackContents::Vanilla,
                    None => PackContents::Uninspectable,
                    Some(file) => {
                        let path = self.dir.join("datapacks").join(file);
                        let count = if path.is_dir() {
                            folder_worldgen_files(&path)
                        } else {
                            zip_worldgen_files(&path)
                        };
                        match count {
                            Ok(worldgen_files) => PackContents::Files {
                                path,
                                worldgen_files,
                            },
                            Err(_) => PackContents::Missing { path },
                        }
                    }
                };
                (name.clone(), contents)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worldgen_paths() {
        assert!(is_worldgen(
            "data/minecraft/worldgen/noise_settings/overworld.json"
        ));
        assert!(is_worldgen("data/mine/tags/worldgen/biome/is_ocean.json"));
        assert!(is_worldgen(
            "overlay_48/data/minecraft/worldgen/biome/plains.json"
        ));
        assert!(!is_worldgen("data/minecraft/function/tick.mcfunction"));
        assert!(!is_worldgen("data/minecraft/tags/block/logs.json"));
        assert!(!is_worldgen("assets/worldgen/x.json"));
    }
}
