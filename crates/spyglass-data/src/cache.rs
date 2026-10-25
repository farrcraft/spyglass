//! Per-user cache of downloaded server jars, verified by size and SHA-1.
//!
//! Jars are stored by content as `<cache>/jars/<sha1>.jar`, so a cache directory can be shared
//! between versions, tools and CI runs. Jars are never written inside the repo. See
//! `docs/specs/data-sources.md`.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use sha1::{Digest, Sha1};

use crate::manifest::Download;

/// Why a jar couldn't be provided.
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("no per-user cache directory on this platform; pass one explicitly")]
    NoCacheDir,
    #[error("I/O error on {path}: {source}")]
    Io { path: PathBuf, source: io::Error },
    #[error("download of {url} failed: {source}")]
    Http { url: String, source: ureq::Error },
    #[error("{url}: expected {expected}, got {actual}")]
    Mismatch {
        url: String,
        expected: String,
        actual: String,
    },
}

fn io_err(path: &Path) -> impl FnOnce(io::Error) -> CacheError + '_ {
    move |source| CacheError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// The default cache directory: the platform's per-user cache folder for spyglass.
pub fn default_dir() -> Result<PathBuf, CacheError> {
    directories::ProjectDirs::from("", "", "spyglass")
        .map(|dirs| dirs.cache_dir().to_path_buf())
        .ok_or(CacheError::NoCacheDir)
}

/// A directory of verified server jars, keyed by SHA-1.
pub struct JarCache {
    dir: PathBuf,
}

impl JarCache {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        JarCache { dir: dir.into() }
    }

    /// A cache in [`default_dir`].
    pub fn open_default() -> Result<Self, CacheError> {
        Ok(Self::new(default_dir()?))
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Where the jar for `download` is (or would be) stored.
    pub fn path_for(&self, download: &Download) -> PathBuf {
        self.dir
            .join("jars")
            .join(format!("{}.jar", download.sha1.to_ascii_lowercase()))
    }

    /// The verified jar for `download`, downloading it first if it's missing or doesn't verify.
    pub fn get(&self, download: &Download) -> Result<PathBuf, CacheError> {
        self.get_with(download, |out| {
            let mut response =
                ureq::get(&download.url)
                    .call()
                    .map_err(|source| CacheError::Http {
                        url: download.url.clone(),
                        source,
                    })?;
            let mut body = response.body_mut().with_config().limit(u64::MAX).reader();
            io::copy(&mut body, out)
                .map(drop)
                .map_err(|source| CacheError::Http {
                    url: download.url.clone(),
                    source: source.into(),
                })
        })
    }

    /// [`get`](Self::get) with the download step supplied by the caller, so tests need no network.
    fn get_with(
        &self,
        download: &Download,
        fetch: impl FnOnce(&mut dyn Write) -> Result<(), CacheError>,
    ) -> Result<PathBuf, CacheError> {
        let path = self.path_for(download);
        if path.is_file() && verify(&path, download).is_ok() {
            return Ok(path);
        }
        let parent = path.parent().expect("jar paths have a parent");
        fs::create_dir_all(parent).map_err(io_err(parent))?;

        // Download beside the final path, then rename, so a half-written jar is never used.
        let partial = path.with_extension("jar.part");
        let mut file = File::create(&partial).map_err(io_err(&partial))?;
        fetch(&mut file)?;
        file.flush().map_err(io_err(&partial))?;
        drop(file);
        if let Err(err) = verify(&partial, download) {
            let _ = fs::remove_file(&partial);
            return Err(err);
        }
        fs::rename(&partial, &path).map_err(io_err(&path))?;
        Ok(path)
    }
}

/// Checks a file's size and SHA-1 against `download`.
pub fn verify(path: &Path, download: &Download) -> Result<(), CacheError> {
    let mismatch = |expected: String, actual: String| CacheError::Mismatch {
        url: download.url.clone(),
        expected,
        actual,
    };
    let size = fs::metadata(path).map_err(io_err(path))?.len();
    if size != download.size {
        return Err(mismatch(
            format!("{} bytes", download.size),
            format!("{size} bytes"),
        ));
    }
    let sha1 = sha1_hex(path)?;
    if !sha1.eq_ignore_ascii_case(&download.sha1) {
        return Err(mismatch(
            format!("SHA-1 {}", download.sha1),
            format!("SHA-1 {sha1}"),
        ));
    }
    Ok(())
}

/// Lower-case hex SHA-1 of a file.
pub fn sha1_hex(path: &Path) -> Result<String, CacheError> {
    let mut file = File::open(path).map_err(io_err(path))?;
    let mut hasher = Sha1::new();
    let mut buf = vec![0; 1 << 16];
    loop {
        let n = file.read(&mut buf).map_err(io_err(path))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONTENT: &[u8] = b"not really a jar";
    // Not CONTENT's SHA-1.
    const WRONG_SHA1: &str = "ed3ac4ac1e1fc4a1a3a6d0e0ee6c7d1a1e2d3d1b";

    fn download(sha1: &str) -> Download {
        Download {
            sha1: sha1.to_string(),
            size: CONTENT.len() as u64,
            url: "https://example.invalid/server.jar".to_string(),
        }
    }

    fn write_content(out: &mut dyn Write) -> Result<(), CacheError> {
        out.write_all(CONTENT).unwrap();
        Ok(())
    }

    fn real_sha1() -> String {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("f");
        fs::write(&path, CONTENT).unwrap();
        sha1_hex(&path).unwrap()
    }

    #[test]
    fn hashes_files() {
        // Known answer: SHA-1("abc").
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("abc");
        fs::write(&path, b"abc").unwrap();
        assert_eq!(
            sha1_hex(&path).unwrap(),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
    }

    #[test]
    fn downloads_once_then_reuses() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = JarCache::new(tmp.path());
        let dl = download(&real_sha1());

        let path = cache.get_with(&dl, write_content).unwrap();
        assert_eq!(fs::read(&path).unwrap(), CONTENT);
        assert_eq!(path, cache.path_for(&dl));

        let again = cache
            .get_with(&dl, |_| panic!("a verified jar must not be fetched again"))
            .unwrap();
        assert_eq!(again, path);
    }

    #[test]
    fn redownloads_a_corrupt_jar() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = JarCache::new(tmp.path());
        let dl = download(&real_sha1());
        let path = cache.path_for(&dl);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"corrupt jar data").unwrap();

        cache.get_with(&dl, write_content).unwrap();
        assert_eq!(fs::read(&path).unwrap(), CONTENT);
    }

    #[test]
    fn rejects_a_bad_download_and_leaves_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = JarCache::new(tmp.path());
        let dl = download(WRONG_SHA1);

        let err = cache.get_with(&dl, write_content).unwrap_err();
        assert!(matches!(err, CacheError::Mismatch { .. }), "{err}");
        assert!(!cache.path_for(&dl).exists());
        assert!(!cache.path_for(&dl).with_extension("jar.part").exists());
    }
}
