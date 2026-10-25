//! Building a version bundle end to end: manifest, verified server jar, Java runtime, data
//! generator, bundle. See `docs/specs/data-sources.md` and `docs/specs/bundle-format.md`.

use std::path::{Path, PathBuf};

use crate::bundle::{self, BundleError, Manifest};
use crate::bundler::{Bundler, BundlerError};
use crate::cache::{CacheError, JarCache};
use crate::datagen::{self, DatagenError};
use crate::java::{self, JavaInstall, JavaNotFound};
use crate::manifest::{self, ManifestError};

/// Why a bundle couldn't be extracted.
#[derive(Debug, thiserror::Error)]
pub enum ExtractError {
    /// The version manifest or version JSON.
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    /// The version has no server jar or no Java requirement, so it can't be extracted.
    #[error("version {0} can't be extracted: {1}")]
    Unsupported(String, &'static str),
    /// Downloading or verifying the server jar.
    #[error(transparent)]
    Cache(#[from] CacheError),
    /// No matching Java runtime.
    #[error(transparent)]
    Java(#[from] JavaNotFound),
    /// Unpacking the game jar.
    #[error(transparent)]
    Bundler(#[from] BundlerError),
    /// Running the data generator.
    #[error(transparent)]
    Datagen(#[from] DatagenError),
    /// Writing the bundle.
    #[error(transparent)]
    Bundle(#[from] BundleError),
    /// Creating the scratch directory.
    #[error("couldn't create a scratch directory: {0}")]
    Scratch(std::io::Error),
}

/// What to extract and where.
#[derive(Debug, Clone)]
pub struct Options<'a> {
    /// A version id, `latest` or `latest-snapshot`.
    pub version: &'a str,
    /// Where server jars are cached.
    pub cache: &'a JarCache,
    /// A Java home or executable to try first (`--java`).
    pub java: Option<&'a Path>,
    /// The bundles directory; the bundle goes in `<out>/<version id>`.
    pub out: &'a Path,
    /// Replace an existing bundle.
    pub replace: bool,
}

/// A step of [`extract`], reported as it starts.
#[derive(Debug)]
pub enum Step<'a> {
    /// Resolving the version in the manifest.
    Resolving(&'a str),
    /// Fetching (or reusing) the verified server jar.
    Jar(&'a str),
    /// Using this Java runtime.
    Java(&'a JavaInstall),
    /// Running the data generator.
    Datagen,
    /// Writing the bundle here.
    Writing(&'a Path),
}

/// A finished extraction.
#[derive(Debug, Clone)]
pub struct Extracted {
    /// The bundle directory.
    pub dir: PathBuf,
    /// Its manifest.
    pub manifest: Manifest,
}

/// Builds the bundle for `options.version`, calling `progress` as each step starts.
pub fn extract(
    options: &Options<'_>,
    mut progress: impl FnMut(Step<'_>),
) -> Result<Extracted, ExtractError> {
    progress(Step::Resolving(options.version));
    let entry = manifest::fetch_manifest()?
        .resolve(options.version)?
        .clone();
    let version = manifest::fetch_version(&entry)?;
    let server = version
        .downloads
        .server
        .as_ref()
        .ok_or(ExtractError::Unsupported(
            version.id.clone(),
            "no server jar",
        ))?;
    let required = version
        .java_version
        .as_ref()
        .ok_or(ExtractError::Unsupported(
            version.id.clone(),
            "no Java version in its version JSON",
        ))?
        .major_version;

    progress(Step::Jar(&version.id));
    let server_jar = options.cache.get(server)?;

    let java = java::find(options.java, required)?;
    progress(Step::Java(&java));

    let scratch = tempfile::Builder::new()
        .prefix("spyglass-extract-")
        .tempdir()
        .map_err(ExtractError::Scratch)?;
    let game_jar = Bundler::open(&server_jar)?.unpack_game(&scratch.path().join("game"))?;
    progress(Step::Datagen);
    let reports_dir = datagen::run_reports(
        &java.executable,
        &server_jar,
        &scratch.path().join("datagen"),
    )?;

    let dir = options.out.join(&version.id);
    progress(Step::Writing(&dir));
    let manifest = bundle::write(
        &dir,
        &bundle::Sources {
            game_jar: &game_jar,
            reports_dir: &reports_dir,
            version: &version.id,
            jar_sha1: &server.sha1,
            java_version: &java.version,
        },
        options.replace,
    )?;
    Ok(Extracted { dir, manifest })
}
