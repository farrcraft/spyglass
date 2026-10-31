//! Running the game's data generator, and the block behaviour report (ADR 0054), for the reports
//! a bundle needs. See `docs/specs/data-sources.md`.

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::bundler::Classpath;

/// The generator's main class, run through the server jar's bundler.
pub const MAIN_CLASS: &str = "net.minecraft.data.Main";

/// How many trailing lines of the generator's output to keep when it fails.
const OUTPUT_TAIL: usize = 20;

/// The block behaviour report's class (ADR 0054), compiled from `extractor-java/BlockReport.java`
/// by `cargo xtask java-report`.
const BLOCK_REPORT_CLASS: &[u8] = include_bytes!("../../../extractor-java/BlockReport.class");

/// The block behaviour report, relative to the generator's output directory, as in the bundle.
pub const BLOCK_BEHAVIOUR: &str = "reports/block_behaviour.json";

/// Why the data generator or the block report couldn't be run.
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
    /// The program exited unsuccessfully.
    #[error("{what} failed ({status}); last output:\n{tail}")]
    Failed {
        /// Which program: the data generator or the block report.
        what: &'static str,
        /// How it exited.
        status: std::process::ExitStatus,
        /// The end of its combined output.
        tail: String,
    },
}

/// An I/O error running or preparing `program`, for `map_err`.
fn io_err(program: &Path) -> impl FnOnce(io::Error) -> DatagenError {
    let program = program.to_path_buf();
    move |source| DatagenError::Io { program, source }
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
    std::fs::create_dir_all(work_dir).map_err(io_err(work_dir))?;
    // The working directory changes, so a relative jar path would break.
    let server_jar = std::path::absolute(server_jar).map_err(io_err(server_jar))?;
    let out = work_dir.join("generated");
    let mut command = java_command(java);
    command
        .arg(format!("-DbundlerMainClass={MAIN_CLASS}"))
        .arg("-jar")
        .arg(&server_jar)
        .arg("--reports")
        .arg("--output")
        .arg(&out)
        .current_dir(work_dir);
    run(&mut command, java, "the data generator")?;
    Ok(out)
}

/// Runs the block behaviour report (ADR 0054) on the unpacked game, `classpath`, with
/// `work_dir` as the working directory, writing [`BLOCK_BEHAVIOUR`] under `reports_dir`.
pub fn run_block_report(
    java: &Path,
    classpath: &Classpath,
    work_dir: &Path,
    reports_dir: &Path,
) -> Result<(), DatagenError> {
    let classes = work_dir.join("classes");
    std::fs::create_dir_all(&classes).map_err(io_err(&classes))?;
    let class = classes.join("BlockReport.class");
    std::fs::write(&class, BLOCK_REPORT_CLASS).map_err(io_err(&class))?;
    let out = reports_dir.join(BLOCK_BEHAVIOUR);
    let parent = out.parent().unwrap_or(reports_dir);
    std::fs::create_dir_all(parent).map_err(io_err(parent))?;
    let jars = std::iter::once(classes.as_path()).chain(classpath.jars());
    let jars: Vec<PathBuf> = jars
        .map(|p| std::path::absolute(p).map_err(io_err(p)))
        .collect::<Result<_, _>>()?;
    let joined = std::env::join_paths(&jars).map_err(|e| DatagenError::Io {
        program: classes.clone(),
        source: io::Error::new(io::ErrorKind::InvalidInput, e),
    })?;
    let out = std::path::absolute(&out).map_err(io_err(&out))?;
    let mut command = java_command(java);
    command
        .arg("-cp")
        .arg(joined)
        .arg("BlockReport")
        .arg(&out)
        .current_dir(work_dir);
    run(&mut command, java, "the block report")
}

/// A command running `java`.
fn java_command(java: &Path) -> Command {
    let mut command = Command::new(java);
    #[cfg(windows)]
    {
        // No console window of its own: the viewer has none for it to share (ADR 0042).
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// Runs `command`, `what` for errors; a failure keeps the end of its output, which is where
/// Java puts the reason.
fn run(command: &mut Command, java: &Path, what: &'static str) -> Result<(), DatagenError> {
    let output = command.output().map_err(io_err(java))?;
    if !output.status.success() {
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let lines: Vec<_> = text.lines().collect();
        let tail = lines[lines.len().saturating_sub(OUTPUT_TAIL)..].join("\n");
        return Err(DatagenError::Failed {
            what,
            status: output.status,
            tail,
        });
    }
    Ok(())
}
