# Releasing

How a version of spyglass gets from `main` to a download. What a release is and why is in [ADR 0037](adr/0037-versions-and-releases.md); how it's made from `main`, in [ADR 0040](adr/0040-releases-are-drafted-from-main.md); how its archives are built, in [ADR 0038](adr/0038-how-release-archives-are-built.md).

A release takes two steps:

1. A pull request sets the version and dates the changelog.
2. Once it's merged, the **Release** workflow, run by hand on `main`, builds `main`'s head and leaves a **draft** release.

Nothing is public, not even the `vX.Y.Z` tag, until you publish the draft. GitHub makes the tag then, on the commit the workflow built.

## Before

- `main` is green in CI.
- The [changelog](../CHANGELOG.md)'s **Unreleased** section says what changed, for people who use spyglass.
- Every game version the release should support is in CI's oracle matrix and in `versions::CHECKED` ([adding a version](adding-a-version.md)). The release notes list them.
- Choose the version ([ADR 0037](adr/0037-versions-and-releases.md#decision)). While it's `0.x`:
  - a breaking change to the API, the CLI's flags or its `--json` output is a minor release;
  - a new game version is a minor release;
  - a patch release is fixes only.

## 1. Set the version

On a branch, as for any change:

```sh
cargo xtask release 0.2.0
```

It refuses if the version isn't after the current one, or if Unreleased is empty. Otherwise it:

- sets the workspace version and refreshes `Cargo.lock`;
- turns Unreleased into `## [0.2.0] - <today>` with the game versions this build was checked against;
- opens a new empty Unreleased, and updates the links at the end of the changelog.

It doesn't touch git. Commit the change, push the branch, and merge its pull request once CI passes, by any merge method. Other pull requests can go in before you release; everything on `main` when the workflow runs is in the release.

## 2. Run the Release workflow

On GitHub: **Actions > Release > Run workflow**, on `main`. Or:

```sh
gh workflow run release.yml --ref main
gh run watch   # pick the Release run
```

Expect about an hour, most of it the oracle jobs and the dist builds:

- **Version and release notes:** the run is on `main`, the version in its `Cargo.toml` has no tag yet, and the changelog has a section for it. The notes are that section, with relative links made absolute to the coming tag.
- **CI:** all of [`ci.yml`](../.github/workflows/ci.yml), on the same commit.
- **build:** for each target, `cargo xtask dist --target <triple>`, then `cargo xtask smoke` on the archive. On Linux the smoke test also extracts 26.3 with the packaged extractor and locates and renders with it. The bundle stays on the runner.
- **Draft release:**
  1. `SHA256SUMS`, and a provenance attestation per archive;
  2. the draft, with the notes and every archive, recording the commit it was built from;
  3. if this version already has a draft from an earlier run, it's replaced. A published release never is.

## 3. Publish it

Open the draft on the repository's Releases page and check it:

- The notes read well. The title is `spyglass 0.2.0`, and the tag to be made is `v0.2.0`.
- It has four archives (Windows, Linux, macOS on Apple silicon and on Intel) and `SHA256SUMS`.
- Download one for your machine, unpack it, and follow its `GETTING-STARTED.txt`.

Then publish it in the page, or:

```sh
gh release edit v0.2.0 --draft=false
```

Publishing makes the tag `v0.2.0` on the commit the workflow built.

## Trying it first: a pre-release

A pre-release version (`0.2.0-rc.1`) goes the same way, but `cargo xtask release` leaves the changelog alone, and its notes are the Unreleased section. GitHub marks its draft as a pre-release.

```sh
cargo xtask release 0.2.0-rc.1   # then commit, pull request, merge
gh workflow run release.yml --ref main
```

If you don't want to publish it, delete the draft. There's no tag to clean up. `cargo xtask release 0.2.0` follows on from the pre-release's version.

```sh
gh release delete v0.2.0-rc.1 --yes
```

## When something fails

**Before publishing**, nothing is public, so retrying is a rerun:

1. Fix the problem through a pull request as usual. The version stays as it is, so don't run `cargo xtask release` again.
2. Run the workflow again on `main`. It replaces the earlier draft for the version.

Some failures need no fix in the code:

- **An oracle job fails** where `main` passed: often Mojang's download servers. Rerun the failed jobs from the run's page.
- **The first job refuses because the tag exists:** that version is already released. Set a new one with `cargo xtask release`.

**After publishing**, never move or reuse the tag: people may have downloaded it, and its checksums and attestations name those bytes. Fix forward with a patch release.

## Checking an archive locally

```sh
cargo xtask dist                                                # this machine's target
cargo xtask smoke target/archives/spyglass-<version>-<target>.zip --extract 26.3
```

`dist` needs `cargo-about`. `scripts/setup.ps1 -ReleaseTools` or `scripts/setup.sh --release-tools` installs it. The archive's binaries name the commit, with `-dirty` if the tree has changes.

## Checking a download

Anyone can check that an archive is the one this workflow built:

```sh
sha256sum -c SHA256SUMS --ignore-missing
gh attestation verify spyglass-<version>-<target>.tar.gz --repo farrcraft/spyglass
```
