//! Mojang's version manifest and per-version JSON. See `docs/specs/data-sources.md`.
//!
//! Only the fields spyglass uses are modelled; everything else is ignored.

use serde::Deserialize;

/// Mojang's list of every release and snapshot.
pub const MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

/// Why the manifest or a version JSON couldn't be fetched or understood.
#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    /// The request failed or returned an error status.
    #[error("HTTP request failed: {0}")]
    Http(#[from] ureq::Error),
    /// The response wasn't the JSON expected.
    #[error("malformed JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// The manifest has no version with this id.
    #[error("unknown version {0:?}")]
    UnknownVersion(String),
    /// A download's SHA-1 isn't the one the manifest gives for it.
    #[error("{url} has SHA-1 {actual}, but the manifest says {expected}")]
    Sha1 {
        /// What was downloaded.
        url: String,
        /// The manifest's hash.
        expected: String,
        /// The download's.
        actual: String,
    },
}

/// The HTTP client for every download: HTTPS only, redirects included, so a download can't be
/// sent to or through plain HTTP. What's downloaded is then checked against Mojang's SHA-1s,
/// except the manifest itself, whose only check is TLS (`docs/specs/data-sources.md`).
pub fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .https_only(true)
        .build()
        .into()
}

/// `version_manifest_v2.json`.
#[derive(Debug, Clone, Deserialize)]
pub struct VersionManifest {
    /// The newest release and snapshot ids.
    pub latest: Latest,
    /// Every version, newest first.
    pub versions: Vec<VersionEntry>,
}

/// The manifest's `latest` object.
#[derive(Debug, Clone, Deserialize)]
pub struct Latest {
    /// Newest release id, e.g. `26.3`.
    pub release: String,
    /// Newest snapshot id.
    pub snapshot: String,
}

/// One version in the manifest.
#[derive(Debug, Clone, Deserialize)]
pub struct VersionEntry {
    /// Version id, e.g. `26.3`.
    pub id: String,
    /// `release`, `snapshot`, `old_beta` or `old_alpha`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Where the version JSON is.
    pub url: String,
    /// SHA-1 of the version JSON.
    pub sha1: String,
}

/// A version's own JSON, linked from its [`VersionEntry`].
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionJson {
    /// Version id, e.g. `26.3`.
    pub id: String,
    /// Jars and mappings for this version.
    pub downloads: Downloads,
    /// The Java runtime the game needs. Missing on very old versions.
    pub java_version: Option<JavaVersion>,
}

/// The downloads spyglass uses from a version JSON.
#[derive(Debug, Clone, Deserialize)]
pub struct Downloads {
    /// The server jar (a bundler; see [`crate::bundler`]).
    pub server: Option<Download>,
    /// Official mappings for an obfuscated server jar. Absent from 26.x, which isn't obfuscated.
    pub server_mappings: Option<Download>,
}

/// One downloadable file.
#[derive(Debug, Clone, Deserialize)]
pub struct Download {
    /// Expected SHA-1, lower-case hex.
    pub sha1: String,
    /// Expected size in bytes.
    pub size: u64,
    /// Where to download it.
    pub url: String,
}

/// The Java runtime a version needs.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaVersion {
    /// The launcher's runtime name, e.g. `java-runtime-epsilon`.
    pub component: String,
    /// Required major version, e.g. 25.
    pub major_version: u32,
}

impl VersionManifest {
    /// Looks up a version id. `latest` means the latest release and `latest-snapshot` the latest
    /// snapshot.
    pub fn resolve(&self, id: &str) -> Result<&VersionEntry, ManifestError> {
        let id = match id {
            "latest" => self.latest.release.as_str(),
            "latest-snapshot" => self.latest.snapshot.as_str(),
            other => other,
        };
        self.versions
            .iter()
            .find(|v| v.id == id)
            .ok_or_else(|| ManifestError::UnknownVersion(id.to_string()))
    }
}

fn get(url: &str) -> Result<Vec<u8>, ManifestError> {
    Ok(agent().get(url).call()?.body_mut().read_to_vec()?)
}

/// Downloads the version manifest.
pub fn fetch_manifest() -> Result<VersionManifest, ManifestError> {
    Ok(serde_json::from_slice(&get(MANIFEST_URL)?)?)
}

/// Downloads a version's JSON, which must have the SHA-1 the manifest lists for it: it gives
/// the server jar's URL and hash in turn.
pub fn fetch_version(entry: &VersionEntry) -> Result<VersionJson, ManifestError> {
    parse_version(entry, &get(&entry.url)?)
}

/// A version's JSON from `body`, checked against `entry`'s SHA-1.
fn parse_version(entry: &VersionEntry, body: &[u8]) -> Result<VersionJson, ManifestError> {
    let actual = spyglass_data::bundle::sha1_hex(body).expect("reading a slice doesn't fail");
    if !actual.eq_ignore_ascii_case(&entry.sha1) {
        return Err(ManifestError::Sha1 {
            url: entry.url.clone(),
            expected: entry.sha1.clone(),
            actual,
        });
    }
    Ok(serde_json::from_slice(body)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Hand-written, trimmed to the fields spyglass reads.
    const MANIFEST: &str = r#"{
        "latest": { "release": "26.3", "snapshot": "26.4-snapshot-5" },
        "versions": [
            { "id": "26.4-snapshot-5", "type": "snapshot", "url": "https://example.invalid/s.json",
              "time": "x", "releaseTime": "x", "sha1": "aa", "complianceLevel": 1 },
            { "id": "26.3", "type": "release", "url": "https://example.invalid/r.json",
              "time": "x", "releaseTime": "x", "sha1": "bb", "complianceLevel": 1 }
        ]
    }"#;

    const VERSION: &str = r#"{
        "id": "26.3",
        "javaVersion": { "component": "java-runtime-epsilon", "majorVersion": 25 },
        "downloads": {
            "client": { "sha1": "cc", "size": 1, "url": "https://example.invalid/client.jar" },
            "server": { "sha1": "dd", "size": 2, "url": "https://example.invalid/server.jar" }
        }
    }"#;

    #[test]
    fn resolves_ids_and_aliases() {
        let manifest: VersionManifest = serde_json::from_str(MANIFEST).unwrap();
        assert_eq!(manifest.resolve("latest").unwrap().id, "26.3");
        assert_eq!(
            manifest.resolve("latest-snapshot").unwrap().kind,
            "snapshot"
        );
        assert_eq!(manifest.resolve("26.3").unwrap().sha1, "bb");
        assert!(
            matches!(manifest.resolve("1.0"), Err(ManifestError::UnknownVersion(id)) if id == "1.0")
        );
    }

    #[test]
    fn parses_version_json() {
        let version: VersionJson = serde_json::from_str(VERSION).unwrap();
        let java = version.java_version.unwrap();
        assert_eq!(
            (java.component.as_str(), java.major_version),
            ("java-runtime-epsilon", 25)
        );
        let server = version.downloads.server.unwrap();
        assert_eq!((server.sha1.as_str(), server.size), ("dd", 2));
        assert!(version.downloads.server_mappings.is_none());
    }

    #[test]
    fn version_json_must_match_the_manifests_hash() {
        let manifest: VersionManifest = serde_json::from_str(MANIFEST).unwrap();
        let mut entry = manifest.resolve("26.3").unwrap().clone();
        entry.sha1 = spyglass_data::bundle::sha1_hex(VERSION.as_bytes())
            .unwrap()
            .to_ascii_uppercase();
        assert_eq!(
            parse_version(&entry, VERSION.as_bytes()).unwrap().id,
            "26.3"
        );
        let tampered = VERSION.replace("server.jar", "other.jar");
        assert!(matches!(
            parse_version(&entry, tampered.as_bytes()),
            Err(ManifestError::Sha1 { .. })
        ));
    }

    #[test]
    fn downloads_refuse_plain_http() {
        // Refused before any connection is made, so this needs no network.
        let err = agent()
            .get("http://example.invalid/x.json")
            .call()
            .unwrap_err();
        assert!(matches!(err, ureq::Error::RequireHttpsOnly(_)), "{err}");
    }
}
