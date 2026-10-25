//! Mojang's version manifest and per-version JSON. See `docs/specs/data-sources.md`.
//!
//! Only the fields spyglass uses are modelled; everything else is ignored.

use serde::Deserialize;

pub const MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] ureq::Error),
    #[error("malformed JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unknown version {0:?}")]
    UnknownVersion(String),
}

#[derive(Debug, Clone, Deserialize)]
pub struct VersionManifest {
    pub latest: Latest,
    pub versions: Vec<VersionEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Latest {
    pub release: String,
    pub snapshot: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VersionEntry {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub url: String,
    pub sha1: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionJson {
    pub id: String,
    pub downloads: Downloads,
    pub java_version: Option<JavaVersion>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Downloads {
    pub server: Option<Download>,
    pub server_mappings: Option<Download>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Download {
    pub sha1: String,
    pub size: u64,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaVersion {
    /// The launcher's runtime name, e.g. `java-runtime-epsilon`.
    pub component: String,
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

fn get_json<T: serde::de::DeserializeOwned>(url: &str) -> Result<T, ManifestError> {
    let body = ureq::get(url).call()?.body_mut().read_to_string()?;
    Ok(serde_json::from_str(&body)?)
}

pub fn fetch_manifest() -> Result<VersionManifest, ManifestError> {
    get_json(MANIFEST_URL)
}

pub fn fetch_version(entry: &VersionEntry) -> Result<VersionJson, ManifestError> {
    get_json(&entry.url)
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
}
