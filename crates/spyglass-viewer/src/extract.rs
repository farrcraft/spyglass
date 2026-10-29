//! Extracting bundles from the viewer (ADR 0042): the game versions to offer, and an extraction
//! on its own thread. The extracting is `spyglass_extract`'s, as the `spyglass-extract` command
//! does it.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

use spyglass::CHECKED;
use spyglass_extract::cache::JarCache;
use spyglass_extract::extract::{self, ExtractError, Step};
use spyglass_extract::manifest::{self, VersionManifest};

/// A game version that can be extracted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    pub id: String,
    pub release: bool,
    /// Whether this build was checked against it (ADR 0039).
    pub checked: bool,
}

/// The versions worth offering, newest first: every release and snapshot from the newest down
/// to the oldest checked one. Older versions need an extractor this build doesn't have. If the
/// manifest doesn't list the oldest checked version, only the checked versions it lists.
pub fn offers(manifest: &VersionManifest) -> Vec<Offer> {
    let oldest = CHECKED.first().map(|c| c.id);
    let end = manifest
        .versions
        .iter()
        .position(|v| Some(v.id.as_str()) == oldest);
    let checked = |id: &str| CHECKED.iter().any(|c| c.id == id);
    manifest
        .versions
        .iter()
        .take(end.map_or(manifest.versions.len(), |i| i + 1))
        .filter(|v| v.kind == "release" || v.kind == "snapshot")
        .filter(|v| end.is_some() || checked(&v.id))
        .map(|v| Offer {
            id: v.id.clone(),
            release: v.kind == "release",
            checked: checked(&v.id),
        })
        .collect()
}

/// Downloads Mojang's version list and picks the [`offers`] from it.
pub fn fetch_offers() -> Result<Vec<Offer>, String> {
    let manifest = manifest::fetch_manifest()
        .map_err(|e| format!("couldn't get the list of game versions: {e}"))?;
    Ok(offers(&manifest))
}

/// The offer to preselect: the newest checked release, else the newest checked version, else
/// the newest.
pub fn preferred(offers: &[Offer]) -> Option<&Offer> {
    offers
        .iter()
        .find(|o| o.checked && o.release)
        .or_else(|| offers.iter().find(|o| o.checked))
        .or(offers.first())
}

/// An extraction on its own thread.
pub struct Running {
    pub version: String,
    /// The step under way, for a person.
    step: Arc<Mutex<String>>,
    done: Receiver<Result<(), String>>,
}

impl Running {
    /// Starts extracting `version` into `out`; `repaint` is called whenever there's news.
    pub fn start(version: String, out: PathBuf, repaint: impl Fn() + Send + 'static) -> Running {
        let step = Arc::new(Mutex::new(format!("Starting {version}")));
        let (send, done) = channel();
        {
            let (step, version) = (step.clone(), version.clone());
            thread::spawn(move || {
                let result = run(&version, out, |s| {
                    *step.lock().unwrap_or_else(PoisonError::into_inner) = s;
                    repaint();
                });
                let _ = send.send(result);
                repaint();
            });
        }
        Running {
            version,
            step,
            done,
        }
    }

    /// The step under way.
    pub fn step(&self) -> String {
        self.step
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The outcome, once it's finished.
    pub fn poll(&self) -> Option<Result<(), String>> {
        match self.done.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err("the extraction failed".to_string())),
        }
    }
}

fn run(version: &str, out: PathBuf, mut step: impl FnMut(String)) -> Result<(), String> {
    let cache = JarCache::open_default().map_err(|e| e.to_string())?;
    let options = extract::Options {
        version,
        cache: &cache,
        java: None,
        out: &out,
        replace: false,
    };
    extract::extract(&options, |s| {
        step(match s {
            Step::Resolving(v) => format!("Looking up {v}"),
            Step::Jar(v) => format!("Downloading the {v} server"),
            Step::Java(j) => format!("Found Java {}", j.version),
            Step::Datagen => "Running the game's data generator (a minute or two)".to_string(),
            Step::Writing(_) => "Writing the bundle".to_string(),
        });
    })
    .map(|_| ())
    .map_err(|e| match &e {
        ExtractError::Java(not_found) => {
            format!(
                "No Java {} runtime found. {}",
                not_found.required,
                not_found.advice()
            )
        }
        _ => e.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use spyglass_extract::manifest::{Latest, VersionEntry};

    fn manifest(versions: &[(&str, &str)]) -> VersionManifest {
        VersionManifest {
            latest: Latest {
                release: String::new(),
                snapshot: String::new(),
            },
            versions: versions
                .iter()
                .map(|(id, kind)| VersionEntry {
                    id: (*id).to_string(),
                    kind: (*kind).to_string(),
                    url: String::new(),
                    sha1: String::new(),
                })
                .collect(),
        }
    }

    #[test]
    fn offers_stop_at_the_oldest_checked_version() {
        let oldest = CHECKED[0].id;
        let newest = CHECKED.last().unwrap().id;
        let found = offers(&manifest(&[
            ("99.1-snapshot-1", "snapshot"),
            (newest, "snapshot"),
            (oldest, "release"),
            ("1.21", "release"),
            ("b1.7", "old_beta"),
        ]));
        let ids: Vec<&str> = found.iter().map(|o| o.id.as_str()).collect();
        assert_eq!(ids, ["99.1-snapshot-1", newest, oldest]);
        assert!(!found[0].checked);
        assert!(found[2].checked && found[2].release);
        // The newest checked release, not the newest version.
        assert_eq!(preferred(&found).unwrap().id, oldest);
    }

    #[test]
    fn without_the_oldest_checked_version_only_checked_ones() {
        let newest = CHECKED.last().unwrap().id;
        let found = offers(&manifest(&[
            ("99.1", "release"),
            (newest, "snapshot"),
            ("1.21", "release"),
        ]));
        let ids: Vec<&str> = found.iter().map(|o| o.id.as_str()).collect();
        assert_eq!(ids, [newest]);
        assert_eq!(preferred(&found).unwrap().id, newest);
        assert_eq!(preferred(&[]), None);
    }
}
