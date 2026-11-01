# 11. Releases

- Status: Completed, with two exit criteria carried as follow-ups ([below](#follow-ups))
- Depends on: [10](10-hardening.md)
- Plans: [m11-releases](../../plans/completed/m11-releases.md)

## Goal

Anyone can download spyglass and run it without installing Rust. A pushed version tag produces versioned, checksummed release archives of the `spyglass` command, the viewer and the extractor for Windows, Linux and macOS. They're built by CI from a commit that passed every check and oracle job, and they contain nothing of Mojang's.

## Scope

- One version for the workspace (SemVer, `0.x` for now), a version tag per release, and a changelog.
- `cargo xtask dist`: builds one target's archive locally or in CI, with the `dist` profile, the pinned toolchain and `--locked`.
- What's in an archive: the three binaries, README, LICENSE, third-party license notices and a short first-run guide. No jars, bundles or anything else derived from the game ([ADR 0008](../../adr/0008-game-derived-test-values-stay-local.md)).
- A release workflow on GitHub Actions. It runs on a pushed tag: the full CI first, then builds for each target, a smoke test of each archive, checksums, build provenance, and a **draft** GitHub release that the owner publishes.
- The changes binaries need to work outside the repository:
  - a default bundles folder per user;
  - `--version` everywhere, naming the commit;
  - the game versions a build was checked against, and a warning for bundles it wasn't.
- `cargo xtask release <version>` to prepare a release commit, and a durable guide for cutting one.
- ADRs for the decisions.

Not in scope:

- Code signing and macOS notarization. Both cost money or an account. Checksums and provenance come first; an ADR records the choice and when to revisit it.
- Package managers (winget, Homebrew, crates.io, cargo-binstall) and auto-update.
- New worldgen behavior.

## Exit criteria

- [x] A tag `v0.1.0` pushed to GitHub produces a draft release with an archive per target, `SHA256SUMS` and provenance attestations, with no manual step except publishing the draft. Since [ADR 0040](../../adr/0040-releases-are-drafted-from-main.md), the workflow is run by hand on `main` and GitHub makes the tag on publishing; v0.1.0 was released that way on 2026-10-29.
- [ ] Each archive, unpacked on a clean machine of its OS with no Rust installed, can extract a bundle and run `spyglass locate` and the viewer, following only its first-run guide. Checked on Windows; macOS and Linux are a follow-up.
- [ ] Building the same tag twice on the same runner image gives the same binaries (byte-identical), or the differences are listed with their causes in the ADR. Identical on Windows; Linux and macOS are a follow-up.
- [x] No archive contains a path from the build machine, or any file derived from the game.
- [x] `cargo xtask check` and `cargo xtask oracle-test` pass for every supported version.

## Follow-ups

Moved to completed on 2026-10-29 with the two unchecked criteria above still open. They need no plan, and are listed in the [roadmap](../README.md#follow-ups) until done:

- the first-run guide followed on a clean macOS and Linux machine (criterion 2);
- two builds of one commit compared on Linux and macOS, with the result in [ADR 0038](../../adr/0038-how-release-archives-are-built.md) (criterion 3).
