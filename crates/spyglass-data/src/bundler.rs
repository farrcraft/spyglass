//! The server jar's bundler layout.
//!
//! The downloaded server jar is a launcher: the game itself is at `META-INF/versions/<path>` and
//! its libraries at `META-INF/libraries/<path>`, each listed in `META-INF/versions.list` and
//! `META-INF/libraries.list` as `sha256 <TAB> id <TAB> path` lines. See
//! `docs/specs/data-sources.md`.

use std::fs::{self, File};
use std::io::{self, Read, Seek};
use std::path::{Component, Path, PathBuf};

use zip::ZipArchive;
use zip::result::ZipError;

/// Why a bundler couldn't be read or unpacked.
#[derive(Debug, thiserror::Error)]
pub enum BundlerError {
    /// Reading or writing a file failed.
    #[error("I/O error on {path}: {source}")]
    Io {
        /// The file, or for an entry inside a jar, its name.
        path: PathBuf,
        /// The underlying error.
        source: io::Error,
    },
    /// The jar isn't a valid zip, or an entry is missing.
    #[error("not a readable jar: {0}")]
    Zip(#[from] ZipError),
    /// A line of `versions.list` or `libraries.list` isn't three tab-separated fields, or its
    /// path would escape the destination.
    #[error("malformed {list} line: {line:?}")]
    BadList {
        /// Which list.
        list: &'static str,
        /// The offending line.
        line: String,
    },
    /// The lists don't describe a server jar, e.g. there isn't exactly one game jar.
    #[error("{0}")]
    Layout(String),
}

/// One line of `versions.list` or `libraries.list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// SHA-256 of the jar, as listed.
    pub sha256: String,
    /// Version id or Maven coordinates, e.g. `com.mojang:brigadier:1.3.11`.
    pub id: String,
    /// Relative to `META-INF/versions/` or `META-INF/libraries/`.
    pub path: String,
}

/// A bundler unpacked into plain jars, ready to be a classpath.
#[derive(Debug, Clone)]
pub struct Classpath {
    /// The game jar, e.g. `server-26.3.jar`.
    pub game: PathBuf,
    /// The game's libraries, in list order.
    pub libraries: Vec<PathBuf>,
}

impl Classpath {
    /// Every jar, game first.
    pub fn jars(&self) -> impl Iterator<Item = &Path> {
        std::iter::once(self.game.as_path()).chain(self.libraries.iter().map(PathBuf::as_path))
    }
}

/// An open server jar, read as a bundler.
pub struct Bundler<R> {
    archive: ZipArchive<R>,
}

impl Bundler<File> {
    /// Opens a server jar on disk.
    pub fn open(path: &Path) -> Result<Self, BundlerError> {
        let file = File::open(path).map_err(|source| BundlerError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Self::new(file)
    }
}

impl<R: Read + Seek> Bundler<R> {
    /// Reads a server jar from any seekable source.
    pub fn new(reader: R) -> Result<Self, BundlerError> {
        Ok(Bundler {
            archive: ZipArchive::new(reader)?,
        })
    }

    fn read_list(&mut self, list: &'static str) -> Result<Vec<Entry>, BundlerError> {
        let mut text = String::new();
        self.archive
            .by_name(&format!("META-INF/{list}"))?
            .read_to_string(&mut text)
            .map_err(|source| BundlerError::Io {
                path: list.into(),
                source,
            })?;
        text.lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| match line.split('\t').collect::<Vec<_>>()[..] {
                [sha256, id, path] if is_relative_and_contained(path) => Ok(Entry {
                    sha256: sha256.to_string(),
                    id: id.to_string(),
                    path: path.to_string(),
                }),
                _ => Err(BundlerError::BadList {
                    list,
                    line: line.to_string(),
                }),
            })
            .collect()
    }

    /// The entries of `META-INF/versions.list`: the game jar.
    pub fn versions(&mut self) -> Result<Vec<Entry>, BundlerError> {
        self.read_list("versions.list")
    }

    /// The entries of `META-INF/libraries.list`.
    pub fn libraries(&mut self) -> Result<Vec<Entry>, BundlerError> {
        self.read_list("libraries.list")
    }

    /// Copies `META-INF/<kind>/<entry.path>` to `dest/<entry.path>`.
    fn extract(&mut self, kind: &str, entry: &Entry, dest: &Path) -> Result<PathBuf, BundlerError> {
        let out = dest.join(&entry.path);
        let io_err = |path: &Path| {
            let path = path.to_path_buf();
            move |source| BundlerError::Io { path, source }
        };
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).map_err(io_err(parent))?;
        }
        let mut zipped = self
            .archive
            .by_name(&format!("META-INF/{kind}/{}", entry.path))?;
        let mut file = File::create(&out).map_err(io_err(&out))?;
        io::copy(&mut zipped, &mut file).map_err(io_err(&out))?;
        Ok(out)
    }

    /// Unpacks the single game jar and every library under `dest`.
    pub fn unpack(&mut self, dest: &Path) -> Result<Classpath, BundlerError> {
        let versions = self.versions()?;
        let [game] = &versions[..] else {
            return Err(BundlerError::Layout(format!(
                "expected one game jar in versions.list, found {}",
                versions.len()
            )));
        };
        let game = self.extract("versions", game, &dest.join("versions"))?;
        let libraries = self
            .libraries()?
            .iter()
            .map(|lib| self.extract("libraries", lib, &dest.join("libraries")))
            .collect::<Result<_, _>>()?;
        Ok(Classpath { game, libraries })
    }
}

/// Rejects absolute paths and `..`, so a list can't write outside the destination.
fn is_relative_and_contained(path: &str) -> bool {
    !path.is_empty()
        && Path::new(path)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use zip::write::SimpleFileOptions;

    fn bundler(files: &[(&str, &[u8])]) -> Bundler<Cursor<Vec<u8>>> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, content) in files {
            zip.start_file(*name, SimpleFileOptions::default()).unwrap();
            zip.write_all(content).unwrap();
        }
        Bundler::new(zip.finish().unwrap()).unwrap()
    }

    #[test]
    fn unpacks_game_and_libraries() {
        let mut b = bundler(&[
            (
                "META-INF/versions.list",
                b"aa\t26.3\t26.3/server-26.3.jar\n",
            ),
            (
                "META-INF/libraries.list",
                b"bb\tcom.example:a:1\tcom/example/a/1/a-1.jar\ncc\tcom.example:b:2\tb-2.jar\n",
            ),
            ("META-INF/versions/26.3/server-26.3.jar", b"game"),
            ("META-INF/libraries/com/example/a/1/a-1.jar", b"lib a"),
            ("META-INF/libraries/b-2.jar", b"lib b"),
        ]);
        assert_eq!(
            b.versions().unwrap(),
            [Entry {
                sha256: "aa".into(),
                id: "26.3".into(),
                path: "26.3/server-26.3.jar".into()
            }]
        );

        let tmp = tempfile::tempdir().unwrap();
        let cp = b.unpack(tmp.path()).unwrap();
        assert_eq!(fs::read(&cp.game).unwrap(), b"game");
        assert!(cp.game.ends_with("versions/26.3/server-26.3.jar"));
        let contents: Vec<_> = cp.libraries.iter().map(|p| fs::read(p).unwrap()).collect();
        assert_eq!(contents, [b"lib a".to_vec(), b"lib b".to_vec()]);
        assert_eq!(cp.jars().count(), 3);
    }

    #[test]
    fn rejects_escaping_paths() {
        for path in ["../evil.jar", "/abs.jar", "a/../../b.jar", ""] {
            let list = format!("aa\tid\t{path}\n");
            let mut b = bundler(&[("META-INF/versions.list", list.as_bytes())]);
            assert!(
                matches!(b.versions(), Err(BundlerError::BadList { .. })),
                "{path:?}"
            );
        }
    }

    #[test]
    fn requires_exactly_one_game_jar() {
        let mut b = bundler(&[
            ("META-INF/versions.list", b""),
            ("META-INF/libraries.list", b""),
        ]);
        let tmp = tempfile::tempdir().unwrap();
        assert!(matches!(b.unpack(tmp.path()), Err(BundlerError::Layout(_))));
    }
}
