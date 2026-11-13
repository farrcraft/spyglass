//! A save's enabled data packs, read as far as the save holds them: the `file/` packs in its
//! `datapacks/` folder (`FolderRepositorySource`). Built-in feature packs are in the version's
//! bundle, and mods' packs in the mods, so those are only named here.

use std::fs;
use std::io::{self, Read, Seek};
use std::path::{Component, Path, PathBuf};

use spyglass_data::pack::{MAX_ENTRIES, MCMETA, Pack, PackError, PackReader};

use crate::Save;

/// What spyglass knows about one enabled data pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackContents {
    /// The game's own data.
    Vanilla,
    /// A `file/` pack in the save's `datapacks/` folder, read.
    Files {
        /// The pack, named by its id in the save (`file/<name>`).
        pack: Box<Pack>,
    },
    /// A `file/` pack whose zip or folder is missing, or whose name isn't a single file name
    /// (the game only lists the folder's own entries, so it can't find such a pack).
    Missing {
        /// Where it should be.
        path: PathBuf,
    },
    /// A `file/` pack the game doesn't load either, so it can't have changed the world: a folder
    /// without `pack.mcmeta`, a file not named `.zip`, or metadata the game skips the pack for.
    Skipped {
        /// The zip or folder.
        path: PathBuf,
        /// Why the game skips it.
        reason: String,
    },
    /// A `file/` pack spyglass couldn't read: larger than it reads, not a zip, or an I/O error.
    Unreadable {
        /// The zip or folder.
        path: PathBuf,
        /// Why.
        reason: String,
    },
    /// Not `vanilla` or a `file/` pack: a built-in feature pack (in the bundle) or a mod's pack.
    Other,
}

impl Save {
    /// What each enabled data pack contains, in load order.
    pub fn packs(&self) -> Vec<(String, PackContents)> {
        self.enabled_packs
            .iter()
            .map(|name| {
                let contents = match name.strip_prefix("file/") {
                    _ if name == "vanilla" => PackContents::Vanilla,
                    None => PackContents::Other,
                    Some(file) => file_pack(&self.dir.join("datapacks"), name, file),
                };
                (name.clone(), contents)
            })
            .collect()
    }
}

/// The `file/<file>` pack in `datapacks`, as `FolderRepositorySource` finds it: a folder with a
/// `pack.mcmeta` file, or a file named `*.zip`.
fn file_pack(datapacks: &Path, id: &str, file: &str) -> PackContents {
    let path = datapacks.join(file);
    let mut parts = Path::new(file).components();
    let single = matches!(
        (parts.next(), parts.next()),
        (Some(Component::Normal(_)), None)
    );
    // The entry's own type: a symbolic link isn't followed.
    let kind = match fs::symlink_metadata(&path) {
        Ok(meta) if single => meta.file_type(),
        _ => return PackContents::Missing { path },
    };
    let skipped = |reason: &str| PackContents::Skipped {
        path: path.clone(),
        reason: reason.to_string(),
    };
    if kind.is_dir() && !path.join(MCMETA).is_file() {
        return skipped("it's a folder without pack.mcmeta");
    }
    if kind.is_file() && !file.ends_with(".zip") {
        return skipped("it's a file not named .zip");
    }
    if !kind.is_dir() && !kind.is_file() {
        return skipped("it's neither a folder nor a file");
    }
    match open_pack(&path) {
        Ok(pack) => PackContents::Files {
            pack: Box::new(pack.with_name(id)),
        },
        Err(PackError::Meta { message, .. }) => skipped(&format!("pack.mcmeta {message}")),
        Err(PackError::Io { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
            PackContents::Missing { path }
        }
        Err(e) => PackContents::Unreadable {
            reason: e.to_string(),
            path,
        },
    }
}

/// Reads the data pack at `path`, a folder or a zip, named by its file name, under the limits
/// `PackReader` keeps (`docs/specs/data-packs.md#reading-packs`). Only `pack.mcmeta` and the files
/// under `data/` at the root or one folder down are read.
pub fn open_pack(path: impl AsRef<Path>) -> Result<Pack, PackError> {
    let path = path.as_ref();
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let mut reader = PackReader::new(path);
    let metadata = fs::metadata(path).map_err(|e| reader.io_error("", e))?;
    if metadata.is_dir() {
        read_folder(path, &mut reader)?;
    } else {
        let file = fs::File::open(path).map_err(|e| reader.io_error("", e))?;
        read_zip(file, &mut reader)?;
    }
    reader.finish(name)
}

/// Reads a pack folder's wanted files. Symbolic links aren't followed, so a cycle can't loop,
/// as for a save's packs.
fn read_folder(root: &Path, reader: &mut PackReader) -> Result<(), PackError> {
    let mut stack = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, prefix)) = stack.pop() {
        let entries = fs::read_dir(&dir).map_err(|e| reader.io_error(&prefix, e))?;
        for entry in entries {
            let entry = entry.map_err(|e| reader.io_error(&prefix, e))?;
            reader.entry()?;
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            let path = format!("{prefix}{name}");
            let kind = entry.file_type().map_err(|e| reader.io_error(&path, e))?;
            if kind.is_dir() {
                // Descend only where a wanted file could be: `data/...`, or `<overlay>/data/...`.
                let mut parts = path.split('/');
                let (first, second) = (parts.next(), parts.next());
                if second.is_none() || first == Some("data") || second == Some("data") {
                    stack.push((entry.path(), format!("{path}/")));
                }
            } else if kind.is_file() && PackReader::wanted(&path) {
                let file = fs::File::open(entry.path()).map_err(|e| reader.io_error(&path, e))?;
                let size = file.metadata().map_or(0, |m| m.len());
                reader.add(&path, size, file)?;
            }
        }
    }
    Ok(())
}

/// Reads a pack zip's wanted entries, as `FilePackResources` finds them: by name, overlays being
/// prefixes.
fn read_zip(file: impl Read + Seek, reader: &mut PackReader) -> Result<(), PackError> {
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| reader.archive_error(e.to_string()))?;
    if archive.len() > MAX_ENTRIES {
        return Err(reader.limit(format!("more than {MAX_ENTRIES} entries")));
    }
    for i in 0..archive.len() {
        reader.entry()?;
        let entry = archive
            .by_index(i)
            .map_err(|e| reader.archive_error(e.to_string()))?;
        if entry.is_dir() {
            continue;
        }
        let name = entry
            .name()
            .map_err(|e| reader.archive_error(e.to_string()))?
            .into_owned();
        if PackReader::wanted(&name) {
            let size = entry.size();
            reader.add(&name, size, entry)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};

    use spyglass_data::pack::MAX_FILE;

    use super::*;

    /// A zip of `files`, deflated.
    fn zip(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in files {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    const MCMETA_JSON: &[u8] =
        br#"{"pack": {"description": "", "min_format": 121, "max_format": 121}}"#;

    #[test]
    fn zips_and_folders_read_alike() {
        let tmp = tempfile::tempdir().unwrap();
        let files: &[(&str, &[u8])] = &[
            ("pack.mcmeta", MCMETA_JSON),
            ("data/x/worldgen/noise/n.json", b"{}"),
            ("readme.txt", b"not read"),
        ];
        let zipped = tmp.path().join("p.zip");
        fs::write(&zipped, zip(files)).unwrap();
        let folder = tmp.path().join("p");
        for (name, bytes) in files {
            let path = folder.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }
        let (a, b) = (open_pack(&zipped).unwrap(), open_pack(&folder).unwrap());
        assert_eq!(a.name(), "p.zip");
        assert_eq!(b.name(), "p");
        assert_eq!(
            a.files().collect::<Vec<_>>(),
            ["data/x/worldgen/noise/n.json"]
        );
        assert_eq!(a.hash(), b.hash());
        assert!(matches!(
            open_pack(tmp.path().join("missing")),
            Err(PackError::Io { .. })
        ));
        fs::write(tmp.path().join("bad.zip"), b"PK not a zip").unwrap();
        assert!(matches!(
            open_pack(tmp.path().join("bad.zip")),
            Err(PackError::Zip { .. })
        ));
    }

    #[test]
    fn zips_are_read_within_limits() {
        let tmp = tempfile::tempdir().unwrap();
        let open = |files: &[(&str, &[u8])]| {
            let path = tmp.path().join("p.zip");
            fs::write(&path, zip(files)).unwrap();
            open_pack(&path)
        };
        for escaping in [
            "data/../../x.json",
            "data/x//y.json",
            "data/x/./y.json",
            "o/data/../x",
        ] {
            assert!(
                matches!(
                    open(&[("pack.mcmeta", MCMETA_JSON), (escaping, b"{}")]),
                    Err(PackError::Limit { .. })
                ),
                "{escaping}"
            );
        }
        // Zeros deflate to almost nothing; the size after inflating is what's limited.
        let bomb = vec![0; usize::try_from(MAX_FILE).unwrap() + 1];
        assert!(matches!(
            open(&[("pack.mcmeta", MCMETA_JSON), ("data/x/big.json", &bomb)]),
            Err(PackError::Limit { .. })
        ));
        // Large files outside the folders read are never inflated.
        assert!(open(&[("pack.mcmeta", MCMETA_JSON), ("assets/big.bin", &bomb)]).is_ok());
    }
}
