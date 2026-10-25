//! Running the game's data generator for the reports a bundle needs. See
//! `docs/specs/data-sources.md`.

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The generator's main class, run through the server jar's bundler.
pub const MAIN_CLASS: &str = "net.minecraft.data.Main";

/// How many trailing lines of the generator's output to keep when it fails.
const OUTPUT_TAIL: usize = 20;

/// Why the data generator couldn't be run.
#[derive(Debug, thiserror::Error)]
pub enum DatagenError {
    /// Java couldn't be started, or the working directory couldn't be prepared.
    #[error("couldn't run {program}: {source}")]
    Io {
        /// The executable or directory involved.
        program: PathBuf,
        /// The underlying error.
        source: io::Error,
    },
    /// The generator exited unsuccessfully.
    #[error("the data generator failed ({status}); last output:\n{tail}")]
    Failed {
        /// How it exited.
        status: std::process::ExitStatus,
        /// The end of its combined output.
        tail: String,
    },
}

/// Runs `java -DbundlerMainClass=net.minecraft.data.Main -jar <server_jar> --reports` with
/// `work_dir` as the working directory, and returns the output directory (`work_dir/generated`).
///
/// The bundler unpacks the game and its libraries into the working directory and the game writes
/// `logs/` there, so `work_dir` should be a scratch directory.
pub fn run_reports(
    java: &Path,
    server_jar: &Path,
    work_dir: &Path,
) -> Result<PathBuf, DatagenError> {
    let io_err = |program: &Path| {
        let program = program.to_path_buf();
        move |source| DatagenError::Io { program, source }
    };
    std::fs::create_dir_all(work_dir).map_err(io_err(work_dir))?;
    // The working directory changes, so a relative jar path would break.
    let server_jar = std::path::absolute(server_jar).map_err(io_err(server_jar))?;
    let out = work_dir.join("generated");
    let output = Command::new(java)
        .arg(format!("-DbundlerMainClass={MAIN_CLASS}"))
        .arg("-jar")
        .arg(&server_jar)
        .arg("--reports")
        .arg("--output")
        .arg(&out)
        .current_dir(work_dir)
        .output()
        .map_err(io_err(java))?;
    if !output.status.success() {
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let lines: Vec<_> = text.lines().collect();
        let tail = lines[lines.len().saturating_sub(OUTPUT_TAIL)..].join("\n");
        return Err(DatagenError::Failed {
            status: output.status,
            tail,
        });
    }
    Ok(out)
}
