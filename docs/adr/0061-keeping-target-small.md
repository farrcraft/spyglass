# 0061. Keeping `target/` small: line tables and a sweep of stale outputs

- Status: Accepted
- Date: 2026-11-04
- See also: [0005](0005-extra-rust-checks.md), [0058](0058-oracle-ci-planned-by-cost.md)

## Context

On 2026-11-04 the repo's `target/` reached 122 GB on a Windows machine and filled its disk, three weeks after the first commit. `target/debug` held 104 GB of it: 56 GB of incremental-compilation directories (4,975 of them) and 47 GB in `deps` (14,415 files, 938 of them executables). The release, dist, cross-target and oracle outputs together were under 12 GB.

Two things made it grow:

- **Every build leaves its outputs behind.** Cargo names an output after its crate and a hash of how it was built: profile, features, dependency versions. When the hash changes it writes a new copy and never deletes the old one. Here hashes change often: Dependabot merges bump dependencies, and the hooks build the workspace several ways (clippy, nextest, doctests, `cargo doc`, the xtask commands), each with its own feature sets. Nothing ever cleans up, short of `cargo clean`.
- **Each copy is big.** The dev profile had full debug info, and about 40 test binaries each link the whole worldgen stack. On Windows each one's `.pdb` was a few hundred MB: one stale viewer `.pdb` alone was 380 MB.

## Decision

We will:

1. **Build dev with line tables only** for the workspace's crates and no debug info for dependencies (`[profile.dev]` in `Cargo.toml`). Panics and backtraces keep their files and lines. A debugger session can turn full debug info back on with `CARGO_PROFILE_DEV_DEBUG=full`.
2. **Sweep stale outputs** with `cargo xtask sweep`. In each profile's `deps`, `incremental`, `build`, `.fingerprint` and `examples`, it keeps each name's newest copy and every copy written within the last 7 days (`--days`), and deletes the rest. `cargo xtask check` sweeps when its checks pass, and `cargo xtask doctor` warns past 5 GB of stale outputs.

## Consequences

- `target/` stays near the size of one build of each kind in use, instead of growing with every change in how the workspace is built.
- A sweep is always safe: cargo's freshness check sees a missing output and rebuilds it. The cost is time, when a variant not rebuilt for a week is used again and rebuilt once.
- Without full debug info, a debugger shows source lines but not variables, unless the variable above is set for that session.
- Cargo's own garbage collection of build outputs is unstable (`-Zgc`). When it's stable, the sweep can give way to it.
- Freshness is judged by modification time, since cargo doesn't touch an output it reuses. An output in steady use but never rebuilt counts as old; keeping each name's newest copy is what saves it.

## Alternatives considered

- **`cargo clean` now and then:** it throws away every build, and someone has to remember. The disk filled because nobody did.
- **`cargo-sweep`:** an external tool doing much the same, keyed on access times, which Windows doesn't keep up to date by default. One more tool for the setup scripts to install, for about 200 lines of xtask.
- **Disabling incremental compilation:** it would remove the biggest directory, but slow every edit-and-test cycle, which is where incremental pays.
- **Fewer test binaries,** one per crate: fewer copies to link and keep, but a restructuring of every crate's tests, and the stale copies would still pile up.
