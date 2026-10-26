//! Building a version bundle end to end: manifest, verified server jar, Java runtime, data
//! generator, bundle. See `docs/specs/data-sources.md` and `docs/specs/bundle-format.md`.

use std::path::{Path, PathBuf};

use spyglass_data::bundle::Manifest;

use crate::bundler::{Bundler, BundlerError};
use crate::cache::{CacheError, JarCache};
use crate::datagen::{self, DatagenError};
use crate::java::{self, JavaInstall, JavaNotFound};
use crate::manifest::{self, Download, ManifestError};
use crate::write::{self, WriteError};

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
    Write(#[from] WriteError),
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

/// A version's server jar and the Java runtime it needs, from the version manifest.
#[derive(Debug, Clone)]
pub struct Server {
    /// The version id, e.g. `26.3`.
    pub version: String,
    /// The server jar's download: URL, size and SHA-1.
    pub download: Download,
    /// `javaVersion.majorVersion`.
    pub java_major: u32,
}

/// Resolves `version` (an id, `latest` or `latest-snapshot`) to its server jar and Java
/// version. Downloads only the manifests, not the jar.
pub fn resolve(version: &str) -> Result<Server, ExtractError> {
    let entry = manifest::fetch_manifest()?.resolve(version)?.clone();
    let json = manifest::fetch_version(&entry)?;
    let download = json
        .downloads
        .server
        .clone()
        .ok_or(ExtractError::Unsupported(json.id.clone(), "no server jar"))?;
    let java_major = json
        .java_version
        .as_ref()
        .ok_or(ExtractError::Unsupported(
            json.id.clone(),
            "no Java version in its version JSON",
        ))?
        .major_version;
    Ok(Server {
        version: json.id,
        download,
        java_major,
    })
}

/// Builds the bundle for `options.version`, calling `progress` as each step starts.
pub fn extract(
    options: &Options<'_>,
    mut progress: impl FnMut(Step<'_>),
) -> Result<Extracted, ExtractError> {
    progress(Step::Resolving(options.version));
    let server = resolve(options.version)?;

    progress(Step::Jar(&server.version));
    let server_jar = options.cache.get(&server.download)?;

    let java = java::find(options.java, server.java_major)?;
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

    let dir = options.out.join(&server.version);
    progress(Step::Writing(&dir));
    let manifest = write::write(
        &dir,
        &write::Sources {
            game_jar: &game_jar,
            reports_dir: &reports_dir,
            version: &server.version,
            jar_sha1: &server.download.sha1,
            java_version: &java.version,
        },
        options.replace,
    )?;
    Ok(Extracted { dir, manifest })
}
