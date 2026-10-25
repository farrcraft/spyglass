//! Builds a version bundle from the user's own server jar.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use spyglass_data::bundle;
use spyglass_data::cache::JarCache;
use spyglass_data::extract::{self, ExtractError, Step};

#[derive(Parser)]
#[command(version, about, args_conflicts_with_subcommands = true)]
struct Args {
    #[command(subcommand)]
    command: Option<Cmd>,
    /// Game version id, `latest` or `latest-snapshot`.
    #[arg(value_name = "VERSION", default_value = "latest")]
    game_version: String,
    /// Server jar cache [default: the per-user cache folder].
    #[arg(long)]
    cache_dir: Option<PathBuf>,
    /// Java home or executable to try first.
    #[arg(long)]
    java: Option<PathBuf>,
    /// Bundles directory; the bundle is written to `<OUT>/<version>`.
    #[arg(long, default_value = "bundles")]
    out: PathBuf,
    /// Replace an existing bundle.
    #[arg(long)]
    force: bool,
}

#[derive(Subcommand)]
enum Cmd {
    /// Recheck a bundle's files against its bundle.json.
    Verify {
        /// The bundle directory, e.g. `bundles/26.3`.
        dir: PathBuf,
    },
}

fn main() -> ExitCode {
    let args = Args::parse();
    let result = match &args.command {
        Some(Cmd::Verify { dir }) => bundle::verify(dir)
            .map(|m| {
                println!(
                    "{}: bundle for {} is valid ({} files)",
                    dir.display(),
                    m.version,
                    m.files.len()
                );
            })
            .map_err(anyhow::Error::from),
        None => run_extract(&args),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            if let Some(ExtractError::Java(not_found)) = err.downcast_ref() {
                eprintln!(
                    "\nInstall a Java {} runtime (for example `winget install EclipseAdoptium.Temurin.{}.JRE`, \
                     or your platform's package manager), or pass --java <path>.",
                    not_found.required, not_found.required
                );
            }
            ExitCode::FAILURE
        }
    }
}

fn run_extract(args: &Args) -> anyhow::Result<()> {
    let cache = match &args.cache_dir {
        Some(dir) => JarCache::new(dir),
        None => JarCache::open_default()?,
    };
    let options = extract::Options {
        version: &args.game_version,
        cache: &cache,
        java: args.java.as_deref(),
        out: &args.out,
        replace: args.force,
    };
    let done = extract::extract(&options, |step| match step {
        Step::Resolving(v) => println!("Resolving {v}"),
        Step::Jar(v) => println!("Fetching the {v} server jar into {}", cache.dir().display()),
        Step::Java(j) => println!(
            "Using Java {} ({}: {})",
            j.version,
            j.source,
            j.home.display()
        ),
        Step::Datagen => println!("Running the data generator"),
        Step::Writing(dir) => println!("Writing {}", dir.display()),
    })?;
    println!(
        "Bundle for {} (data version {}): {} files in {}",
        done.manifest.version,
        done.manifest.data_version,
        done.manifest.files.len(),
        done.dir.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_is_well_formed() {
        Args::command().debug_assert();
    }

    #[test]
    fn verify_is_a_subcommand_and_versions_are_positional() {
        let args = Args::try_parse_from(["spyglass-extract", "verify", "bundles/26.3"]).unwrap();
        assert!(matches!(args.command, Some(Cmd::Verify { .. })));
        let args = Args::try_parse_from(["spyglass-extract", "26.3", "--force"]).unwrap();
        assert!(args.command.is_none());
        assert_eq!(args.game_version, "26.3");
        assert!(args.force);
        let args = Args::try_parse_from(["spyglass-extract"]).unwrap();
        assert_eq!(args.game_version, "latest");
    }
}
