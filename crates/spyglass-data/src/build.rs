//! Which build of spyglass this is, for `--version`, the viewer and bug reports. Release builds
//! name their commit and its date; `cargo xtask dist` sets them (ADR 0038). Any other build is a
//! dev build.

use std::sync::OnceLock;

use crate::versions::CHECKED;

/// spyglass's version, e.g. `0.1.0`: the workspace's, shared by every crate (ADR 0037).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The commit built, from `SPYGLASS_BUILD_COMMIT` at compile time.
pub const COMMIT: Option<&str> = option_env!("SPYGLASS_BUILD_COMMIT");

/// The commit's date (`YYYY-MM-DD`), from `SPYGLASS_BUILD_DATE` at compile time.
pub const DATE: Option<&str> = option_env!("SPYGLASS_BUILD_DATE");

/// The version with where it was built from: `0.1.0 (1a2b3c4d5e6f 2026-10-28)`, or
/// `0.1.0 (dev build)`.
pub fn version() -> &'static str {
    static LINE: OnceLock<String> = OnceLock::new();
    LINE.get_or_init(|| line(VERSION, COMMIT, DATE))
}

/// [`version`], then the game versions this build was checked against, on a second line.
pub fn long_version() -> &'static str {
    static LINES: OnceLock<String> = OnceLock::new();
    LINES.get_or_init(|| {
        let checked: Vec<&str> = CHECKED.iter().map(|c| c.id).collect();
        format!("{}\nchecked against: {}", version(), checked.join(", "))
    })
}

fn line(version: &str, commit: Option<&str>, date: Option<&str>) -> String {
    match (commit, date) {
        (Some(commit), Some(date)) => format!("{version} ({commit} {date})"),
        (Some(commit), None) => format!("{version} ({commit})"),
        (None, _) => format!("{version} (dev build)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines() {
        assert_eq!(
            line("0.1.0", Some("1a2b3c4d5e6f"), Some("2026-10-28")),
            "0.1.0 (1a2b3c4d5e6f 2026-10-28)"
        );
        assert_eq!(
            line("0.1.0", Some("1a2b3c4d5e6f"), None),
            "0.1.0 (1a2b3c4d5e6f)"
        );
        assert_eq!(line("0.1.0", None, Some("2026-10-28")), "0.1.0 (dev build)");
        assert!(long_version().starts_with(version()));
        assert!(long_version().contains(&format!("checked against: {}", CHECKED[0].id)));
    }
}
