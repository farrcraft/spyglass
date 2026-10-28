# Contributing

Thanks for helping. This page is the short version; each section links the document that has the details.

## Set up

Run `scripts/setup.ps1` (Windows) or `scripts/setup.sh` (macOS, Linux), then `cargo xtask doctor` to see what's still missing. [Getting started](docs/getting-started.md) covers installing by hand, Java, the oracle and the dev container.

## The one rule that can't bend

**Never commit Mojang's code or data:** no jars, class files, decompiled sources, extracted JSON, generated bundles, or test values produced by running the game ([ADR 0008](docs/adr/0008-game-derived-test-values-stay-local.md)). The extractor and the oracle generate all of it locally, into gitignored folders, and commit hooks block the common cases. If you aren't sure whether something counts, ask first.

## How work flows

Milestones become implementation plans, and plans become code ([development process](docs/development-process.md)). Behavior the code must match is written down in [specs](docs/specs/README.md); decisions that will outlast the change get an [ADR](docs/adr/README.md). A change to behavior that has a spec updates the spec in the same change. A change people who use spyglass would notice gets a line under **Unreleased** in the [changelog](CHANGELOG.md), in the same change.

Worldgen code aims to match the game **bit for bit**: Java's integer widths and wrapping, signed and unsigned shifts, `f32` against `f64`, and the order of floating-point operations all matter. Correctness is checked against the real game through the [oracle](docs/specs/oracle.md), not against other tools. Worldgen belongs in the library crates; the CLI and viewer only call the public API ([architecture](docs/architecture.md)).

## Checks

Every check runs as a git hook ([ADR 0002](docs/adr/0002-git-hooks-for-local-checks.md)): formatting, spelling, Markdown links and the Mojang-data blocks on commit; clippy, tests, docs, the MSRV build and cargo-deny on push. `cargo xtask check` runs them all, as CI does, and `cargo xtask oracle-test` runs the comparisons with the game where you have their inputs. Don't bypass the hooks with `--no-verify`; if a formatter changes files, review them, stage them and commit again.

## Security

See [SECURITY.md](SECURITY.md) for what counts as a vulnerability and how to report one privately.
