# 0037. Versions and releases

- Status: Accepted
- Date: 2026-10-28
- See also: [0028](0028-supported-versions-and-ci.md), [0038](0038-how-release-archives-are-built.md)

## Context

Until now the only way to run spyglass was to clone it and build it with Rust. Every crate is at version `0.0.0` with `publish = false`, there are no tags, and nothing records what changed between two commits. People who just want to find a seed shouldn't need a Rust toolchain, and a bug report should name a version rather than a commit.

A release needs a version that means something. spyglass ships three binaries (`spyglass`, `spyglass-viewer`, `spyglass-extract`) and a public API. The CLI's `--json` output is an interface for scripts. Each can break separately, but they're built from one tree and move together.

Game versions add a second axis. Supported game versions are permanent ([ADR 0028](0028-supported-versions-and-ci.md)), and a new one changes which bundles a build accepts and how it answers for them. Correctness is only known for commits whose oracle jobs passed, and those jobs run only in CI, since nothing game-derived may be kept between runs ([ADR 0008](0008-game-derived-test-values-stay-local.md)).

## Decision

We will release the workspace as one product with one version:

1. **One SemVer version** in `[workspace.package]`, shared by every crate, starting at `0.1.0`. Crates stay `publish = false`. While the major version is `0`, a minor bump may break the API, the CLI's flags or its `--json` output. A patch never does.
2. **A new supported game version is a minor release**, never a patch, because it changes what the binaries accept.
3. **A release is an annotated `vX.Y.Z` tag on `main`** that equals the workspace version. `cargo xtask release <version>` prepares the commit and the tag locally, and never pushes.
4. **`CHANGELOG.md`**, in Keep a Changelog form. Changes add to its `Unreleased` section. Each release's section lists the game versions it supports and becomes the release notes.
5. **Releases are built only from green CI.** The release workflow runs the full CI, every check and every oracle job, on the tagged commit before it builds anything.
6. **CI creates a draft release, and the owner publishes it by hand.** Nothing reaches users without a person looking at it.

## Consequences

- Users get downloadable versions, and bug reports can name one.
- Every user-visible change needs a changelog entry. Forgetting one isn't caught by any check, so it relies on review. `cargo xtask release` refuses an empty `Unreleased`, which catches only the worst case.
- One version means a CLI-only fix also bumps the library's version. That's harmless while nothing is published to crates.io.
- Game versions arrive often (26.4 alone has weekly snapshots), so minor versions will climb quickly. That's expected, not a problem.
- A release takes as long as a full CI run, about 20 minutes wall-clock, and depends on Mojang's download servers being up, as CI already does.
- Publishing is a manual step. It's one click, but someone has to do it.

## Alternatives considered

- **A version per crate:** precise, but there's no external user of individual crates to benefit, and keeping a dozen versions straight is overhead with no payoff.
- **CalVer, or versions named after game versions:** a spyglass release supports many game versions at once, so tying the number to one of them misleads. CalVer says nothing about compatibility.
- **Building releases without the oracle jobs:** faster, but it would ship builds whose correctness, the project's main promise, wasn't checked.
- **Publishing automatically on tag:** one fewer step, but a mistake in the notes or the archives would be public immediately.
