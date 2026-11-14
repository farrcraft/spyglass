//! Reading a pack's files under the limits: which paths are read, and how much.

use std::io::{self, Read};
use std::path::{Path, PathBuf};

use super::{Contents, MAX_ENTRIES, MAX_FILE, MAX_TOTAL, MCMETA, Pack, PackError};

/// Reads a pack's files one at a time, under the limits, into a [`Pack`]: for readers of
/// folders and zips, which call [`entry`](Self::entry) for each entry they see and
/// [`add`](Self::add) for each [`wanted`](Self::wanted) file.
pub struct PackReader {
    /// The limits, and what's been read so far.
    budget: Budget,
    /// What's been read.
    contents: Contents,
}

impl PackReader {
    /// A reader for the pack at `path`, which names it in errors.
    pub fn new(path: impl AsRef<Path>) -> Self {
        PackReader {
            budget: Budget::new(path.as_ref()),
            contents: Contents::default(),
        }
    }

    /// The pack's path, as given.
    pub fn path(&self) -> &Path {
        &self.budget.pack
    }

    /// Whether a path in a pack is one spyglass reads: `pack.mcmeta`, or a file in a `data/`
    /// folder at the root or one level down (where overlay folders are) whose namespace and path
    /// an `Identifier` can hold, as the game reads them, with no empty, `.` or `..` segment.
    /// Readers skip the rest unread.
    pub fn wanted(path: &str) -> bool {
        wanted(path)
    }

    /// Counts one entry seen, read or not: more than [`MAX_ENTRIES`] is an error.
    pub fn entry(&mut self) -> Result<(), PackError> {
        self.budget.entry()
    }

    /// Reads the file at `path` in the pack, whose size is said to be `size`: a file larger than
    /// [`MAX_FILE`] (as said or as read) or a pack past [`MAX_TOTAL`] is an error. A path that
    /// isn't [`wanted`](Self::wanted) is skipped unread, as the game skips it.
    pub fn add(&mut self, path: &str, size: u64, reader: impl Read) -> Result<(), PackError> {
        if !wanted(path) {
            return Ok(());
        }
        if size > MAX_FILE {
            return Err(self
                .budget
                .limit(format!("{path} is larger than {MAX_FILE} bytes")));
        }
        let bytes = self.budget.read(path, reader)?;
        self.contents.insert(path.to_string(), bytes);
        Ok(())
    }

    /// An error about the pack: an I/O error on `file`, or its archive.
    pub fn io_error(&self, file: impl AsRef<Path>, source: io::Error) -> PackError {
        PackError::Io {
            path: self.budget.pack.join(file),
            source,
        }
    }

    /// An error saying the pack is past a limit, e.g. has too many entries.
    pub fn limit(&self, message: impl Into<String>) -> PackError {
        self.budget.limit(message.into())
    }

    /// An error saying the pack's archive can't be read.
    pub fn archive_error(&self, message: impl Into<String>) -> PackError {
        PackError::Zip {
            path: self.budget.pack.clone(),
            message: message.into(),
        }
    }

    /// The pack read, named `name`: its metadata parsed, and its files under `data/` at the root
    /// and in its overlays kept.
    pub fn finish(self, name: impl Into<String>) -> Result<Pack, PackError> {
        Pack::from_read(self.budget.pack, name.into(), self.contents)
    }
}

/// Whether a path in a pack is one spyglass reads ([`PackReader::wanted`]).
fn wanted(path: &str) -> bool {
    if path == MCMETA {
        return true;
    }
    // At the root, or in an overlay folder one level down.
    let in_data = path
        .strip_prefix("data/")
        .or_else(|| path.split_once('/')?.1.strip_prefix("data/"));
    let Some((namespace, resource)) = in_data.and_then(|rest| rest.split_once('/')) else {
        return false;
    };
    plain(path) && identifier_part(namespace, false) && identifier_part(resource, true)
}

/// Whether `path` is plain: not empty or absolute, with no `\` or `:`, and no empty, `.` or `..`
/// segment. Files are looked up by path, never joined onto the filesystem, so no path could
/// reach outside the pack either way; a path that isn't plain is skipped so that lookups by
/// path stay unambiguous. (The game would take a `.` or `..` segment into the id.)
fn plain(path: &str) -> bool {
    !path.is_empty()
        && !path.contains('\\')
        && !path.contains(':')
        && path
            .split('/')
            .all(|seg| !seg.is_empty() && seg != "." && seg != "..")
}

/// Whether a namespace (`[a-z0-9_.-]`), or with `slashes` a path (`[a-z0-9/._-]`), is one an
/// `Identifier` can hold. The game skips a pack's file whose namespace or path isn't ("Invalid
/// path in pack", "Non [a-z0-9_.-] character in namespace") and loads the rest: uppercase
/// letters included.
fn identifier_part(text: &str, slashes: bool) -> bool {
    text.chars().all(|c| {
        c.is_ascii_lowercase() || c.is_ascii_digit() || "_-.".contains(c) || (slashes && c == '/')
    })
}

/// The limits on one pack's reading, and what's been read so far.
struct Budget {
    pack: PathBuf,
    entries: usize,
    total: u64,
}

impl Budget {
    fn new(pack: &Path) -> Self {
        Budget {
            pack: pack.to_path_buf(),
            entries: 0,
            total: 0,
        }
    }

    fn limit(&self, message: String) -> PackError {
        PackError::Limit {
            path: self.pack.clone(),
            message,
        }
    }

    /// Counts one entry seen, read or not.
    fn entry(&mut self) -> Result<(), PackError> {
        self.entries += 1;
        if self.entries > MAX_ENTRIES {
            return Err(self.limit(format!("more than {MAX_ENTRIES} entries")));
        }
        Ok(())
    }

    /// Counts `len` bytes read for `file`.
    fn take(&mut self, file: &str, len: u64) -> Result<(), PackError> {
        if len > MAX_FILE {
            return Err(self.limit(format!("{file} is larger than {MAX_FILE} bytes")));
        }
        self.total += len;
        if self.total > MAX_TOTAL {
            return Err(self.limit(format!("more than {MAX_TOTAL} bytes of data")));
        }
        Ok(())
    }

    /// Reads at most [`MAX_FILE`] bytes of `reader`, counting them.
    fn read(&mut self, file: &str, reader: impl Read) -> Result<Vec<u8>, PackError> {
        let mut bytes = Vec::new();
        reader
            .take(MAX_FILE + 1)
            .read_to_end(&mut bytes)
            .map_err(|source| PackError::Io {
                path: self.pack.join(file),
                source,
            })?;
        self.take(file, bytes.len() as u64)?;
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_that_arent_plain_are_skipped_not_errors() {
        let mut reader = PackReader::new("pack");
        let mcmeta = br#"{"pack": {"description": "", "pack_format": 48}}"#;
        reader.add(MCMETA, 0, &mcmeta[..]).unwrap();
        for odd in [
            "data/a/../b.json",
            "data/a/./b.json",
            "data\\a\\b.json",
            "data/a//b.json",
            "data/a:b/c.json",
            "/data/a/b.json",
            // Seen in 26.3: "Invalid path in pack: minecraft:worldgen/noise/Bad_Name.json".
            "data/minecraft/worldgen/noise/Bad_Name.json",
            "data/Mod/worldgen/noise/a.json",
            "overlay/data/minecraft/worldgen/noise/a b.json",
        ] {
            assert!(!PackReader::wanted(odd), "{odd}");
            reader.add(odd, 2, &b"{}"[..]).unwrap();
        }
        reader.add("data/a/b.json", 2, &b"{}"[..]).unwrap();
        let pack = reader.finish("pack").unwrap();
        assert_eq!(pack.files().collect::<Vec<_>>(), ["data/a/b.json"]);
    }
}
