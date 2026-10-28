//! `cargo xtask linux-check`: a fresh clone, `scripts/setup.sh` and the extractor on a clean
//! Ubuntu machine, in Docker. See docs/adr/0007-dev-container-and-linux-check.md.

use std::process::Output;
use std::time::Instant;

use anyhow::{Context, Result, bail, ensure};
use clap::Args;
use spyglass_extract::extract;

use crate::{command, first_line, repo_root};

/// The image built from the Dockerfile's `base` stage.
const IMAGE: &str = "spyglass-linux-check";
/// One check at a time; a leftover container from an interrupted run is replaced.
const CONTAINER: &str = "spyglass-linux-check";
/// The base image's non-root user, so the check runs as a contributor would.
const USER: &str = "ubuntu";
const CLONE: &str = "/home/ubuntu/spyglass";
/// Where setup.sh puts rustup and uv, ahead of the system path.
const PATH: &str = "/home/ubuntu/.cargo/bin:/home/ubuntu/.local/bin:\
                    /usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

#[derive(Args)]
pub struct LinuxCheck {
    /// Game version to extract: an id, `latest` or `latest-snapshot`.
    #[arg(long, default_value = "latest")]
    game_version: String,
    /// Also run `cargo xtask check` (every hook, including clippy, tests, MSRV and cargo-deny).
    #[arg(long)]
    full: bool,
    /// Leave the container running afterwards, to look around in it.
    #[arg(long)]
    keep: bool,
}

/// The running container. Removed on drop unless kept.
struct Container {
    keep: bool,
}

impl Container {
    fn start() -> Result<Self> {
        let _ = command("docker").args(["rm", "-f", CONTAINER]).output();
        let mount = format!(
            "type=bind,source={},target=/src,readonly",
            repo_root().display()
        );
        let out = command("docker")
            .args(["run", "-d", "--name", CONTAINER, "--user", USER])
            .args(["--mount", &mount, IMAGE, "sleep", "infinity"])
            .output()
            .context("failed to run docker")?;
        ensure!(
            out.status.success(),
            "couldn't start the container: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
        Ok(Container { keep: false })
    }

    fn exec(&self, user: &str, script: &str) -> std::process::Command {
        let mut cmd = command("docker");
        cmd.args(["exec", "--user", user, "--env"])
            .arg(format!("PATH={PATH}"))
            .args(["--workdir", "/home/ubuntu", CONTAINER, "sh", "-c"])
            .arg(format!("cd {CLONE} 2>/dev/null; {script}"));
        cmd
    }

    /// Runs a step with its output on the console. `Ok(false)` if it exits unsuccessfully.
    fn run(&self, script: &str) -> Result<bool> {
        let status = self
            .exec(USER, script)
            .status()
            .context("failed to run docker exec")?;
        Ok(status.success())
    }

    /// Runs a step that must succeed, with its output on the console.
    fn must(&self, script: &str) -> Result<()> {
        ensure!(self.run(script)?, "`{script}` failed");
        Ok(())
    }

    /// Runs a step and captures its output instead of printing it.
    fn capture(&self, user: &str, script: &str) -> Result<Output> {
        self.exec(user, script)
            .output()
            .context("failed to run docker exec")
    }
}

impl Drop for Container {
    fn drop(&mut self) {
        if self.keep {
            println!(
                "\nThe container is still running. Enter it with: docker exec -it {CONTAINER} bash\n\
                 Remove it with: docker rm -f {CONTAINER}"
            );
        } else {
            let _ = command("docker").args(["rm", "-f", CONTAINER]).output();
        }
    }
}

fn step(name: &str) {
    println!("\n===== {name} =====");
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// `channel` from rust-toolchain.toml.
fn pinned_channel() -> Result<String> {
    crate::toml_string("rust-toolchain.toml", &["toolchain", "channel"])
}

pub fn run(args: &LinuxCheck) -> Result<()> {
    let started = Instant::now();
    let target = preflight(&args.game_version)?;

    step("build image");
    let status = command("docker")
        .args(["build", "--target", "base", "--tag", IMAGE])
        .args(["--file", ".devcontainer/Dockerfile", ".devcontainer"])
        .status()
        .context("failed to run docker build")?;
    ensure!(status.success(), "docker build failed");
    let mut c = Container::start()?;
    c.keep = args.keep;

    clone_and_set_up(&c, &target)?;
    java_and_extraction(&c, &target)?;
    if args.full {
        step("cargo xtask check");
        c.must("cargo xtask check")?;
    }

    let minutes = started.elapsed().as_secs_f64() / 60.0;
    println!(
        "\nLinux check passed for {:.12} in {minutes:.1} min.",
        target.head
    );
    Ok(())
}

/// What the check runs against, settled before any container starts.
struct Target {
    /// The commit cloned.
    head: String,
    /// The game version extracted, and the Java major it needs.
    version: String,
    java: u32,
    /// `channel` from rust-toolchain.toml.
    channel: String,
}

fn preflight(game_version: &str) -> Result<Target> {
    if first_line("docker", &["version", "--format", "{{.Server.Version}}"]).is_none() {
        bail!("Docker isn't available; install Docker and make sure its daemon is running");
    }
    let Some(head) = first_line("git", &["rev-parse", "HEAD"]) else {
        bail!("not a git repository, or git not found");
    };
    if first_line("git", &["status", "--porcelain"]).is_some_and(|l| !l.is_empty()) {
        println!(
            "note: the working tree has uncommitted changes; only the committed HEAD is checked"
        );
    }
    let server = extract::resolve(game_version)?;
    Ok(Target {
        head,
        version: server.version,
        java: server.java_major,
        channel: pinned_channel()?,
    })
}

/// A fresh clone of the commit, the setup script, and the default toolchain it leaves.
fn clone_and_set_up(c: &Container, target: &Target) -> Result<()> {
    step(&format!("fresh clone of {:.12}", target.head));
    // The mount is owned by the host user, which git refuses to read from without an exception.
    c.must(&format!(
        "git config --global --add safe.directory /src/.git && git clone -q /src {CLONE} && cd {CLONE} && git checkout -q {} \
         && git log --oneline -1",
        target.head
    ))?;

    step("scripts/setup.sh");
    // Doctor, at the end of setup, fails because there's no Java yet. Setup has to have done
    // everything else, which the steps below check.
    if !c.run("./scripts/setup.sh")? {
        println!("(setup.sh exited unsuccessfully; expected while Java is missing)");
    }

    step("default toolchain");
    let channel = &target.channel;
    let out = c.capture(USER, "cd / && rustup default")?;
    let default = text(&out);
    print!("{default}");
    ensure!(
        out.status.success() && default.starts_with(&format!("{channel}-")),
        "the default toolchain should be the pinned channel ({channel})"
    );
    Ok(())
}

/// `text` as one POSIX shell word: single-quoted, with each `'` written as `'\''`.
fn sh_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// The extractor's error without Java, then with Java: doctor, extraction and a clean tree.
fn java_and_extraction(c: &Container, target: &Target) -> Result<()> {
    // The version id comes from Mojang's manifest, so it's quoted for the shell like any text.
    let (id, java) = (sh_quote(&target.version), target.java);
    step("extract without Java");
    let out = c.capture(
        USER,
        &format!("cargo run -q -p spyglass-extract -- {id} --out /tmp/no-java"),
    )?;
    let error = text(&out);
    print!("{error}");
    ensure!(
        !out.status.success() && error.contains(&format!("no Java {java} runtime found")),
        "expected extraction to fail with a Java-not-found error"
    );

    step(&format!("install a Java {java} runtime"));
    let out = c.capture(
        "root",
        &format!(
            "apt-get update -qq && \
             DEBIAN_FRONTEND=noninteractive apt-get install -y -qq openjdk-{java}-jre-headless"
        ),
    )?;
    ensure!(
        out.status.success(),
        "couldn't install openjdk-{java}-jre-headless:\n{}",
        text(&out)
    );
    println!("installed openjdk-{java}-jre-headless");

    step("cargo xtask doctor");
    c.must(&format!("cargo xtask doctor --game-version {id}"))?;

    step(&format!("extract {}", target.version));
    c.must(&format!(
        "cargo run -q -p spyglass-extract -- {id} \
         && cargo run -q -p spyglass-extract -- verify bundles/{id}"
    ))?;

    step("git status");
    let out = c.capture(USER, "git status --porcelain")?;
    let status = text(&out);
    ensure!(
        out.status.success() && status.trim().is_empty(),
        "the clone isn't clean after setup and extraction:\n{status}"
    );
    println!("clean");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_words_are_quoted() {
        assert_eq!(sh_quote("26.3"), "'26.3'");
        assert_eq!(sh_quote("a b; rm -rf /"), "'a b; rm -rf /'");
        assert_eq!(sh_quote("it's"), r"'it'\''s'");
    }
}
