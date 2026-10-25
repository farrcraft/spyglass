# CLAUDE.md

Guidance for Claude Code when working in this repository.

## Project

spyglass is a Rust-native, offline seed-map toolkit for Minecraft: Java Edition (biome maps, structure finders, seed search). The goal is **bit-exact** parity with the game on current versions. Start with [docs/architecture.md](docs/architecture.md) and [docs/roadmap.md](docs/roadmap.md).

## Commands

```sh
cargo build                                   # build the workspace
cargo test                                    # run all tests
cargo test -p spyglass-rng                    # test one crate
cargo fmt --all                               # format
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p spyglass-viewer                  # run the viewer
```

`scripts/check.ps1` / `scripts/check.sh` run fmt, clippy and tests the same way CI should.

## Layout

- `crates/` holds one crate per layer (rng → noise → density → biome/terrain → structure → search, with `data` feeding versioned parameters and `viewer` on top). Keep dependencies flowing in that direction; lower crates must not depend on higher ones.
- `docs/` holds getting-started, architecture, `specs/` (behavior the code must match), `adr/` (decision records), `research/` (prior art, open questions) and the roadmap.
- `scripts/` holds developer setup and check scripts, in both PowerShell and POSIX sh versions.

## Rules

- **Never commit Mojang code or data.** No jars, decompiled sources, extracted JSON or generated bundles. `/bundles`, `/oracle-out` and `*.jar` are gitignored; keep it that way. Test fixtures derived from the game (golden files) must also be generated locally, not committed, unless a spec explicitly says otherwise.
- **Algorithms in code, parameters in data.** If a value changes between game versions, it belongs in an extracted bundle, not a Rust constant.
- **Bit-exact means bit-exact.** Match Java semantics precisely: wrapping integer arithmetic, `i32`/`i64` widths, signed shifts, `f64` vs `f32`, and operation order in floating-point expressions. Do not "simplify" math in a way that could change results.
- **Correctness is checked against the oracle** (see [docs/specs/oracle.md](docs/specs/oracle.md)), not against other third-party tools.
- New behavior that has a spec should cite the spec; if the spec is wrong or missing, update it in the same change.
- Significant decisions (dependencies, formats, crate boundaries, policies) get an ADR in `docs/adr/`; use the `adr` skill. Use the `rust-review` skill to review Rust changes.
- Edition 2024, MSRV in the workspace `Cargo.toml`. Shared metadata and inter-crate dependencies go through `[workspace.package]` / `[workspace.dependencies]`.
