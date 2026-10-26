# 0005. Extra Rust checks: lints, dependencies, MSRV and spelling

- Status: Accepted
- Date: 2026-10-25
- See also: [0002](0002-git-hooks-for-local-checks.md), which this extends; [0019](0019-gui-stack-and-msrv-1-95.md), which raises the MSRV to 1.95

## Context

[ADR 0002](0002-git-hooks-for-local-checks.md) set up the hooks: hygiene, the Mojang-data blocks, formatters, then clippy and `cargo test` on push. Several common Rust checks were still missing:

- Clippy ran only its default lints, with no project-wide lint policy. Nothing required docs on public items or kept `unsafe` out.
- Nothing checked dependencies for security advisories, incompatible licenses or unexpected sources. spyglass will be distributed under MIT, so a dependency's license matters.
- The workspace declares `rust-version = "1.88"`, but the toolchain is `stable` (1.99 when this was written), so nothing checked that promise. Let-chains, used in xtask, only became stable in 1.88.
- Unused dependencies, spelling mistakes and broken doc links went unnoticed.

Two practical constraints shaped the details:

- pre-commit's `language: rust` hooks install tools with `cargo install` but **without `--locked`**. `cargo-nextest` refuses to build that way by design, and `cargo-shear` 1.14.0 failed to build that way because of a dependency version skew (2026-10-25).
- `cargo xtask check` runs pre-commit from inside `cargo run`, so every hook inherits the variables `cargo run` sets for xtask (`CARGO_PKG_NAME`, `CARGO_MANIFEST_DIR`, ...). `ring`'s build script reruns when those change, so nested cargo calls rebuilt `ring` and tried to relink the running `xtask.exe`, which Windows doesn't allow.

## Decision

We will add these checks, all defined in `.pre-commit-config.yaml` as before:

| Check                                                                                                                                   | Where                                                             | Stage         |
| --------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------- | ------------- |
| `[workspace.lints]`: `unsafe_code = "forbid"`, `missing_docs`, clippy `cast_lossless`, `dbg_macro`, `todo`; rustdoc link and HTML lints | `Cargo.toml`, every crate opts in with `[lints] workspace = true` | (clippy, doc) |
| `cargo doc --workspace --no-deps`, failing on the rustdoc lints above                                                                   | local hook                                                        | push          |
| `cargo-deny`: advisories, licenses (allow-list), bans, sources                                                                          | its own hook repo, config in `deny.toml`                          | push          |
| MSRV: `cargo +<rust-version> check --workspace --all-targets`                                                                           | `cargo xtask msrv`, which reads `rust-version` from `Cargo.toml`  | push          |
| `typos` spell checker, report only                                                                                                      | its own hook repo                                                 | commit        |
| `cargo-shear`: unused dependencies                                                                                                      | local hook                                                        | commit        |
| `cargo-nextest` instead of `cargo test`, plus `cargo test --doc` (nextest skips doctests)                                               | local hooks                                                       | push          |

Tools that build fine unlocked (cargo-deny, typos) come from their upstream hook repos, so pre-commit installs them. **nextest and cargo-shear are installed by the setup scripts** with `cargo install --locked` at pinned versions, and `cargo xtask doctor` checks for them. The setup scripts also install the MSRV toolchain.

xtask removes the variables `cargo run` injects (`CARGO_PKG_*`, `CARGO_MANIFEST_*`, `CARGO_CRATE_*`, `CARGO_BIN_*`, `CARGO_PRIMARY_PACKAGE`, `OUT_DIR`) from every process it spawns.

Clippy's `suboptimal_flops` and `imprecise_flops` stay off on purpose. They suggest `mul_add` and other rewrites that change floating-point results, which would break parity with the game.

## Consequences

- Public items in library crates must be documented, and the rustdoc lints catch broken links before they ship.
- License and advisory problems surface on push instead of at release. Adding a dependency with a new license means editing the allow-list in `deny.toml`, which is deliberate friction.
- The MSRV is checked rather than assumed. Raising it is a one-line change in `Cargo.toml`, and everything else follows.
- cargo-shear removed the stub crates' declared-but-unused dependencies on each other and on `md-5`. Cargo manifests now list what's actually used; the intended layering lives in `docs/architecture.md`, and each dependency is added back when code first uses it.
- **Costs:** setup takes longer the first time (nextest and cargo-shear compile for a few minutes, the 1.88 toolchain downloads, pre-commit builds cargo-deny). Two tools are pinned in both setup scripts, which have to stay in step. The push stage does more work: the MSRV check and `cargo doc` add about 10–20 s on an incremental build.
- `missing_docs` adds some boilerplate on error-variant fields and serde structs.

## Alternatives considered

- **`cargo-audit` instead of cargo-deny:** advisories only. cargo-deny covers advisories plus licenses, bans and sources in one tool.
- **`cargo-machete` or `cargo-udeps` instead of cargo-shear:** machete is regex-based and less precise; udeps needs nightly. cargo-shear parses the code and runs on stable.
- **Installing nextest and cargo-shear through pre-commit:** tried first; both failed to build without `--locked`.
- **Prebuilt binaries via `cargo-binstall`:** faster installs, but one more bootstrap tool, and downloading binaries rather than building from crates.io. Revisit if setup time becomes a problem.
- **Dropping the MSRV promise until publication:** simpler, but the declared `rust-version` would go on being unchecked. Checking it costs one toolchain download.
- **`clippy::pedantic` wholesale:** too noisy for the value. The few lints that matter here are enabled individually.
