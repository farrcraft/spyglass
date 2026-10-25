# 0007. Dev container and a containerized Linux check

- Status: Accepted
- Date: 2026-10-25

## Context

spyglass is developed on Windows, and contributors may use Windows, macOS or Linux. The setup scripts ([ADR 0002](0002-git-hooks-for-local-checks.md)) and `cargo xtask` ([ADR 0003](0003-cargo-xtask-for-project-commands.md)) are meant to work the same on all three, but until 2026-10-25 nothing had run on Linux. The first run, by hand in a throwaway `ubuntu:24.04` container, found two real bugs. On a fresh rustup, `setup.sh` made the MSRV toolchain the global default. The extractor's no-Java hint suggested `winget` on Linux. That kind of drift will keep happening unless Linux runs are routine.

Two needs pull in different directions:

- **Development:** a ready-made Linux environment (Rust, Java 25 for the extractor and oracle, pre-commit, hooks) that opens in VS Code with one click and is the same for everyone. Pre-installing tools is the point.
- **Testing the setup path:** proving that a fresh clone plus `scripts/setup.sh` works on a clean machine. Pre-installed tools would hide exactly what this is meant to catch.

There's no CI yet (the repo isn't on GitHub), so the Linux check has to run locally. Docker is available on the dev machine; macOS can't be containerized.

## Decision

We will add **one Dockerfile with two stages**, used by a VS Code dev container and by a new `cargo xtask linux-check` command.

**`.devcontainer/Dockerfile`**, based on `ubuntu:24.04`:

- `base` stage: only what [getting-started](../getting-started.md) assumes a clean machine has: `git`, `curl`, `ca-certificates`, `build-essential`. It installs no Rust and no Java.
- `dev` stage: `base` plus a JDK of the current target's Java major (`openjdk-25-jdk-headless` for 26.3; a JDK covers both the extractor and the oracle) and the viewer's Linux build dependencies.

**`.devcontainer/devcontainer.json`** builds the `dev` stage and runs `scripts/setup.sh` as its post-create command. The dev container therefore installs Rust, the cargo tools, pre-commit and the hooks through the same script a contributor runs by hand, and every rebuild exercises it. Named volumes keep `~/.cargo` and `~/.rustup` between rebuilds, so recreating the container doesn't redo a five-minute setup. It recommends the rust-analyzer extension.

**`cargo xtask linux-check`** builds the `base` stage and, in a new container, checks the fresh-clone path. Each step runs through `docker exec` and is checked in Rust, not in a shell script:

1. clone the repo's committed `HEAD` from a read-only mount (uncommitted changes aren't tested, and the command says so when the tree is dirty);
2. run `scripts/setup.sh`, then check that the default toolchain is the pinned channel;
3. run `spyglass-extract` without Java, and expect the not-found error;
4. install the Java runtime package, then `cargo xtask doctor` must pass;
5. extract the target version, `verify` it, and check that `git status --porcelain` is empty.

`--full` also runs `cargo xtask check` in the container, which covers clippy, tests, MSRV and cargo-deny on Linux. `--keep` leaves the container for debugging. A run takes about six minutes, mostly compiling cargo-nextest and cargo-shear. `cargo xtask doctor` gains a `warn`-level line for Docker, which only this command needs.

## Consequences

- Linux runs become a single command, and a future CI job can call the same command.
- Development and testing share one image definition, so the dev container can't drift from what the check assumes a clean machine has.
- The dev container also checks `setup.sh` on every rebuild, though only partly: its cached volumes mean later rebuilds skip the slow installs.
- Docker becomes an optional dependency. Nothing outside `linux-check` and the dev container needs it.
- The JDK major is written in the Dockerfile, so moving to a game version with a new Java major means editing it. The check's doctor step fails loudly if it's forgotten.
- The viewer can be built in the dev container but not easily run, since it needs a display. GUI work stays on the host.
- macOS remains unchecked: containers can't run it. It needs a real Mac or a hosted CI runner later.
- The check needs network access (rustup, crates.io, Mojang, Ubuntu packages), so it isn't hermetic. That's intentional, because the real setup path uses the network too.

## Alternatives considered

- **Microsoft's `devcontainers/rust` image:** quicker to start, but it ships Rust pre-installed, so the dev container would never exercise `setup.sh`. It's also Debian, not the Ubuntu the check uses.
- **Separate images for development and the check:** each simpler, but they would drift. The point is that both describe the same clean machine.
- **A shell script for the check**, like the one used by hand: shorter, but it breaks the rule that project commands are Rust in `xtask` ([ADR 0003](0003-cargo-xtask-for-project-commands.md)), and it can't share the Java and manifest logic.
- **GitHub Actions now:** the repo has no remote yet. When it does, CI should call `cargo xtask linux-check` rather than reimplementing it in YAML.
- **Nix or a pinned toolchain image:** reproducible, but a large new tool for contributors, and it would replace rather than test the setup scripts.
