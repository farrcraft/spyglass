# 0036. Fuzzing the readers of untrusted files with cargo-fuzz

- Status: Accepted
- Date: 2026-10-28

## Context

spyglass reads files other people make: world saves, whose `level.dat`, player files and region files may come from anywhere, and the data packs inside them ([world-saves.md](../specs/world-saves.md)). A crafted file that makes a reader panic, allocate without bound or overflow the stack takes down the CLI or viewer that opened it. The repo audit of 2026-10-28 found one such bug in the NBT reader: a list header that reserved gigabytes before reading anything.

The readers already had hand-written defenses and tests: size caps, a depth limit, every truncation and a byte-flip test. Those check the cases someone thought of. Coverage-guided fuzzing finds the ones nobody did. The standard Rust tool, `cargo-fuzz` (libFuzzer), needs a nightly compiler for its sanitizer flags. The project otherwise builds on a pinned stable release ([ADR 0034](0034-an-exact-toolchain-bumped-deliberately.md)).

## Decision

We will fuzz the NBT reader and the region file scan with `cargo-fuzz` on nightly:

- **Where:** targets in `fuzz/`, a workspace of its own, so stable builds and the hooks never see it.
- **How it runs:** `cargo xtask fuzz <nbt|region> [--seconds N]` writes a small corpus of valid files to start from, then runs. On Windows it finds Visual Studio's AddressSanitizer runtime and puts it on PATH.
- **When:** a weekly CI workflow runs each target for 30 minutes and keeps any crashing input as an artifact. Fuzzing isn't part of `cargo xtask check` or the hooks.

## Consequences

- Readers of untrusted files get tested on inputs nobody wrote by hand. A crash leaves the input that reproduces it.
- Contributors need nightly and `cargo-fuzz` to fuzz, and on Windows the MSVC build tools' sanitizer runtime. The checks everyone runs don't need any of them.
- `fuzz/` has its own `Cargo.lock`. It isn't covered by `cargo-deny`, `cargo-shear`, clippy or the `cargo fmt` hook, which all run on the main workspace. It has two small files and no dependencies beyond `libfuzzer-sys` and the crates it tests.
- The weekly job uses a floating nightly. A nightly that breaks the build fails that job, not the checks that gate changes.
- The density loader, the other reader of untrusted input, isn't fuzzed yet. Its nesting limit is tested directly ([density-functions.md](../specs/density-functions.md#loading)), and a target for it can follow the same pattern.

## Alternatives considered

- **A seeded property test on stable:** runs in the hooks with no new tooling, but mutates blindly. It finds far less than coverage guidance does in the same time.
- **`proptest` or `arbitrary`-based generators:** structured inputs reach deeper, but they need generators written and kept in step with the formats. That's more code than the two targets, for a format whose own length fields already steer libFuzzer well once seeded.
- **No fuzzing:** relies on the hand-written tests, which missed the reservation bug.
