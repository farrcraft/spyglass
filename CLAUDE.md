# CLAUDE.md

Guidance for Claude Code when working in this repository.

## Project

spyglass is a local-only seed explorer for Minecraft: Java Edition: ranked constraint search over seeds, map preview with selectable layers, locate queries on seeds and on the user's own world saves, and all three dimensions. It's library-first: the CLI and GUI are thin front ends over the `spyglass` public API (see the README's Goals and [docs/architecture.md](docs/architecture.md#front-ends)). The goal is **bit-exact** parity with the game on current versions. Start with [docs/architecture.md](docs/architecture.md), [docs/roadmap/](docs/roadmap/README.md) and [docs/development-process.md](docs/development-process.md).

## Commands

```sh
cargo build                                   # build the workspace
cargo test                                    # run all tests (hooks use cargo nextest run)
cargo test -p spyglass-rng                    # test one crate
cargo fmt --all                               # format
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p spyglass-viewer                  # run the viewer
cargo xtask check                             # every check, whole repo (as CI)
cargo xtask oracle golden                     # regenerate the game's answers (oracle --help for the rest)
cargo xtask oracle-test                       # ignored comparisons with the game, where inputs exist
cargo xtask ci --game-version <v>             # one version's CI job: extract, regenerate, compare
cargo xtask bench                             # time the public API's main operations
cargo xtask dist [--target <triple>]          # one target's release archive, in target/archives/
cargo xtask release <version>                 # release commit on a release/v<version> branch, for a PR
cargo xtask release-tag <version>             # tag the merged release commit on main (never pushes)
cargo xtask doctor                            # is this machine ready? (check-only)
cargo xtask links                             # every relative Markdown link and anchor resolves
cargo xtask fuzz <nbt|region>                 # fuzz a reader of untrusted files (nightly, cargo-fuzz)
cargo xtask msrv                              # build with the minimum supported Rust version
cargo xtask linux-check [--full]              # fresh clone + setup + extract on Ubuntu, in Docker
cargo xtask version-diff <old> <new> [--code] # what changed between two bundles
```

All checks are defined in `.pre-commit-config.yaml` and run as git hooks (commit: hygiene, Mojang-data blocks, Markdown links, fmt, unused deps, typos, TOML/JSON/YAML/Markdown formatting, shellcheck; push: clippy, nextest and doctests, cargo doc, MSRV build, cargo-deny). `cargo xtask check` runs all of them against the whole repo, as CI should. When a formatter hook modifies files, review and stage the changes, then commit again. Never bypass hooks with `--no-verify`. See [ADR 0002](docs/adr/0002-git-hooks-for-local-checks.md) and [ADR 0005](docs/adr/0005-extra-rust-checks.md). Lint levels live in `[workspace.lints]`; new crates opt in with `[lints] workspace = true`. A new dependency license must be allowed in `deny.toml`.

## Layout

- `crates/` holds one crate per layer (rng → noise → density → biome/terrain → structure → search, with `data` feeding versioned parameters and `viewer` on top). Keep dependencies flowing in that direction; lower crates must not depend on higher ones.
- `docs/` holds getting-started, architecture, the development process, `specs/` (behavior the code must match), `adr/` (decision records), `research/` (prior art, open questions), `roadmap/` (one file per milestone) and `plans/` (implementation plans). `roadmap/` and `plans/` each have a `completed/` subdirectory.
- `xtask/` holds project commands (`cargo xtask <cmd>`; [ADR 0003](docs/adr/0003-cargo-xtask-for-project-commands.md)). It's tooling: no crate may depend on it. Add new project commands here, in Rust, rather than as scripts.
- `fuzz/` holds cargo-fuzz targets for the readers of untrusted files, a workspace of its own that needs nightly ([ADR 0036](docs/adr/0036-fuzzing-with-cargo-fuzz.md)).
- `scripts/` holds only the setup scripts, in PowerShell and POSIX sh, because they run before Rust exists. Keep the pair in sync.

## Rules

- **Never commit Mojang code or data.** No jars, decompiled sources, extracted JSON or generated bundles. `/bundles`, `/oracle-out` and `*.jar` are gitignored; keep it that way. Test fixtures derived from the game (golden files) must also be generated locally, not committed, unless a spec explicitly says otherwise.
- **Library first.** Worldgen logic belongs in the library crates, never in `spyglass-cli` or `spyglass-viewer`. If a front end needs a result, add it to the API.
- **Algorithms in code, parameters in data.** If a value changes between game versions, it belongs in an extracted bundle, not a Rust constant. Behavior that changes between versions without showing in the data is a named switch in `spyglass_data::versions`, never a data-version check in the code ([ADR 0026](docs/adr/0026-how-game-versions-coexist.md)).
- **Bit-exact means bit-exact.** Match Java semantics precisely: wrapping integer arithmetic, `i32`/`i64` widths, signed shifts, `f64` vs `f32`, and operation order in floating-point expressions. Do not "simplify" math in a way that could change results.
- **Correctness is checked against the oracle** (see [docs/specs/oracle.md](docs/specs/oracle.md)), not against other third-party tools.
- New behavior that has a spec should cite the spec; if the spec is wrong or missing, update it in the same change.
- **Follow the development process** in [docs/development-process.md](docs/development-process.md): milestones → implementation plans → code. Write a plan in `docs/plans/` before starting milestone work, keep its task list current, and `git mv` it to `docs/plans/completed/` when done (milestones likewise to `docs/roadmap/completed/`). Fix relative links after any move.
- **Code must never reference `docs/roadmap/` or `docs/plans/`**, by path or by milestone/plan name, in source, comments, tests, `Cargo.toml` or scripts. They are temporary process docs. Put durable explanations in a spec or ADR and reference that. Specs, ADRs and architecture docs may link to the roadmap and plans indexes, not to individual files.
- Significant decisions (dependencies, formats, crate boundaries, policies) get an ADR in `docs/adr/`; use the `adr` skill. Use the `rust-review` skill to review Rust changes.
- Edition 2024, MSRV in the workspace `Cargo.toml`. Shared metadata and inter-crate dependencies go through `[workspace.package]` / `[workspace.dependencies]`.
