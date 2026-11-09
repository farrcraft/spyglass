//! `cargo xtask guide`: build the user guide, `docs/guide/`, with mdBook into `target/guide/`
//! (docs/adr/0066-a-user-guide-on-github-pages.md). A commit hook and the Pages workflow run it,
//! so a guide that builds with warnings fails before it's merged or published.

use anyhow::{Context, Result, bail};

use crate::{command, first_line};

/// The mdBook the setup scripts and the workflows install. A test keeps them in step.
pub const MDBOOK_VERSION: &str = "0.5.4";

/// The book's directory, holding `book.toml`, relative to the repo root.
const BOOK: &str = "docs/guide";

#[derive(clap::Args)]
pub struct Guide {
    /// Serve the guide locally, rebuilding it on every change, instead of building it once.
    #[arg(long)]
    serve: bool,
    /// Open the guide in a browser once it's built.
    #[arg(long)]
    open: bool,
}

pub fn run(args: &Guide) -> Result<()> {
    let installed = installed_version().with_context(|| {
        format!(
            "mdBook isn't installed; run scripts/setup.ps1 or scripts/setup.sh, or \
             cargo install --locked mdbook@{MDBOOK_VERSION}"
        )
    })?;
    if installed != MDBOOK_VERSION {
        eprintln!(
            "warning: mdBook {installed} is installed, but the guide is built with \
             {MDBOOK_VERSION}"
        );
    }
    let mut cmd = command("mdbook");
    cmd.arg(if args.serve { "serve" } else { "build" })
        .arg(BOOK);
    if args.open {
        cmd.arg("--open");
    }
    if args.serve {
        let status = cmd.status().context("failed to run mdbook")?;
        if !status.success() {
            bail!("mdbook serve failed");
        }
        return Ok(());
    }
    // mdBook logs to stderr, and exits with success after some problems it warns about, so its
    // warnings are read from the log. Links and images are `cargo xtask links`'s to check.
    let output = cmd.output().context("failed to run mdbook")?;
    let log = String::from_utf8_lossy(&output.stderr);
    eprint!("{log}");
    if !output.status.success() {
        bail!("the guide didn't build");
    }
    let warnings = log.lines().filter(|line| is_warning(line)).count();
    if warnings > 0 {
        bail!("the guide built with {warnings} warning(s)");
    }
    Ok(())
}

/// The installed mdBook's version (`0.5.4`), if it runs.
pub fn installed_version() -> Option<String> {
    let line = first_line("mdbook", &["--version"])?;
    Some(
        line.trim_start_matches("mdbook")
            .trim()
            .trim_start_matches('v')
            .to_string(),
    )
}

/// Whether a line of mdBook's log is a warning or an error.
fn is_warning(line: &str) -> bool {
    line.split_whitespace()
        .take(3)
        .any(|word| matches!(word, "WARN" | "ERROR"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warnings_in_the_log() {
        assert!(is_warning(
            " WARN mdbook_html::html_handlebars: missing image"
        ));
        assert!(is_warning(" ERROR failed to read chapter `missing.md`"));
        assert!(!is_warning(" INFO mdbook::book: Running the html backend"));
        assert!(!is_warning(
            " INFO a page mentioning WARN later in the line"
        ));
    }
}
