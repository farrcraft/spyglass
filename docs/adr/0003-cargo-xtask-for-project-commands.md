# 0003. cargo xtask for project commands

- Status: Accepted
- Date: 2026-10-24

## Context

spyglass needs project-level commands beyond `cargo build/test/run`, and the list is growing:

- **Now:** run every check as CI would (`scripts/check.ps1` / `check.sh`, which run the pre-commit config per [ADR 0002](0002-git-hooks-for-local-checks.md)).
- **Next:** a preflight `doctor` that reports whether a machine is ready: Rust toolchain and linker, uv and pre-commit, hooks installed, a Java runtime matching the target game version, a JDK for oracle work, extracted bundles.
- **Later:** regenerate oracle golden files, extract bundles for tests, compare Rust output against golden files, CI steps.

Every command so far exists twice, in PowerShell and POSIX sh, because contributors use Windows, macOS and Linux. Each new command doubles the work, and the pairs can drift apart.

Some commands need project logic, not just shell glue. `doctor` has to find Java the same way `spyglass-extract` does (`--java`, `JAVA_HOME`, `PATH`, then launcher runtime folders, matched against the version JSON). If `doctor` reimplements that in shell, the two will disagree sooner or later.

Constraints: no extra tools for contributors beyond what setup already installs; commands work identically on all three platforms; the setup scripts can't depend on the solution, because they run before Rust exists.

## Decision

We will use the [cargo xtask](https://github.com/matklad/cargo-xtask) pattern: an `xtask` binary crate in the workspace, run as `cargo xtask <command>` through an alias in `.cargo/config.toml`.

- `xtask/` lives at the repo root, not under `crates/`, so it's clearly tooling rather than part of the product. It's a workspace member with `publish = false`. No other crate may depend on it.
- It may depend on workspace crates (e.g. `spyglass-data` for Java discovery) and uses `clap` and `anyhow` like the other binaries.
- First commands: `cargo xtask check`, replacing `scripts/check.ps1` / `check.sh` and running both pre-commit stages across the repo; and `cargo xtask doctor`, check-only, never installing anything.
- `scripts/setup.ps1` / `setup.sh` stay as the only paired scripts, since they bootstrap Rust itself. They end by running `cargo xtask doctor`.
- `.pre-commit-config.yaml` stays the single definition of checks (ADR 0002). `cargo xtask check` invokes it; it doesn't duplicate it.

## Consequences

- One cross-platform implementation per command, in Rust, with no new tool to install.
- Commands can call project code directly, so `doctor` and the extractor share Java discovery and can't disagree.
- Commands are discoverable with `cargo xtask --help`.
- One more crate to maintain. The first `cargo xtask` run compiles it, which takes a few seconds; after that it's cached in the shared `target/`.
- `cargo xtask` is a convention, not a Cargo feature. New contributors may not know it, so getting-started and CLAUDE.md have to mention it.
- Trivial commands that are one line of shell become a few lines of Rust. Acceptable, since the trivial ones are few and the cross-platform guarantee is worth it.
- xtask is code, so the rule against referencing `docs/roadmap/` and `docs/plans/` applies to it. The pre-commit hook's `files` pattern must cover `xtask/` as well as `crates/`.
- `scripts/check.ps1` / `check.sh` are deleted. ADR 0002 mentions them as the thin wrapper; that role moves to `cargo xtask check` without changing ADR 0002's decision.

## Alternatives considered

- **Makefile:** Windows doesn't ship `make`, and recipes run in a platform shell, so they'd still need per-platform variants.
- **[just](https://github.com/casey/just) (1.59):** well liked in the Rust community, with good discoverability (`just --list`). But it's another binary every contributor has to install, and recipes still run in a shell, so the PowerShell/sh duplication remains. A good choice for projects whose commands are shell glue. Ours increasingly aren't.
- **[cargo-make](https://github.com/sagiegurari/cargo-make) (0.37.24):** cross-platform task runner with its own TOML recipe language. It's an extra install, adds a DSL to learn, and its last release was January 2025.
- **Keep paired PowerShell and sh scripts:** no new crate, but every command written twice, with drift, and no way to share Rust logic like Java discovery.
