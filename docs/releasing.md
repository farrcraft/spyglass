# Releasing

How a version of spyglass gets from `main` to a download. What a release is and why is in [ADR 0037](adr/0037-versions-and-releases.md); how its archives are built, in [ADR 0038](adr/0038-how-release-archives-are-built.md).

A release is a `vX.Y.Z` tag on `main`. Pushing the tag starts the release workflow ([`.github/workflows/release.yml`](../.github/workflows/release.yml)):

1. It checks the tag.
2. It runs all of CI on the tagged commit, every oracle job included.
3. It builds and smoke-tests an archive per target.
4. It leaves a **draft** release.

Nothing is public until you publish the draft.

## Before

- `main` is green in CI.
- The [changelog](../CHANGELOG.md)'s **Unreleased** section says what changed, for people who use spyglass.
- Every game version the release should support is in CI's oracle matrix and in `versions::CHECKED` ([adding a version](adding-a-version.md)). The release notes list them.
- Choose the version ([ADR 0037](adr/0037-versions-and-releases.md#decision)). While it's `0.x`:
  - a breaking change to the API, the CLI's flags or its `--json` output is a minor release;
  - a new game version is a minor release;
  - a patch release is fixes only.

## Cut it

```sh
cargo xtask release 0.2.0
```

It refuses before changing anything if the tree isn't clean, the branch isn't `main`, the tag exists, the version isn't after the current one, or Unreleased is empty. Then it:

1. sets the workspace version and refreshes `Cargo.lock`;
2. turns Unreleased into `## [0.2.0] - <today>` with the game versions this build was checked against, opens a new empty Unreleased, and updates the links at the end of the changelog;
3. runs `cargo xtask check`, every hook on every file, which takes several minutes;
4. commits `Release 0.2.0` and makes the annotated tag `v0.2.0`.

If a check fails, the changes are left uncommitted. Fix the problem, `git checkout .`, and run it again.

Nothing is pushed. Read the commit, then push the branch and the tag together:

```sh
git push --atomic origin main v0.2.0
```

## Watch it

```sh
gh run watch   # pick the Release run
```

Expect about an hour, most of it the oracle jobs and the dist builds:

- **Tag and release notes:** the tag equals the workspace version, its commit is on `main`, and the changelog has a section for it. The notes are that section, with relative links made absolute to the tag.
- **CI:** all of [`ci.yml`](../.github/workflows/ci.yml), called on the tagged commit.
- **build:** for each target, `cargo xtask dist --target <triple>`, then `cargo xtask smoke` on the archive. On Linux the smoke test also extracts 26.3 with the packaged extractor and locates and renders with it. The bundle stays on the runner.
- **Draft release:** `SHA256SUMS`, a provenance attestation per archive, and the draft with the notes and every archive.

## Publish it

Open the draft on the repository's Releases page and check it:

- The notes read well. The title is `spyglass 0.2.0`.
- It has four archives (Windows, Linux, macOS on Apple silicon and on Intel) and `SHA256SUMS`.
- Download one for your machine, unpack it, and follow its `GETTING-STARTED.txt`.

Then publish it in the page, or:

```sh
gh release edit v0.2.0 --draft=false
```

## Try the workflow first: a pre-release

A pre-release version (`0.2.0-rc.1`) runs everything the same way, but doesn't touch the changelog: its notes are the Unreleased section. GitHub marks its draft as a pre-release.

```sh
cargo xtask release 0.2.0-rc.1
git push --atomic origin main v0.2.0-rc.1
```

If you don't publish it, delete the draft and its tag when you're done. The release commit can stay, and `cargo xtask release 0.2.0` follows on from it:

```sh
gh release delete v0.2.0-rc.1 --cleanup-tag --yes
git tag -d v0.2.0-rc.1
```

## When something fails

**Before the draft is published**, nothing is public, and the same version can be retried:

1. Delete the draft, if there is one, and the tag:

   ```sh
   gh release delete v0.2.0 --cleanup-tag --yes   # or, with no draft: git push --delete origin v0.2.0
   git tag -d v0.2.0
   ```

2. Fix the problem on `main` as usual. The release commit has already set the version and dated the changelog, so don't run `cargo xtask release` again.
3. Tag the fixed commit by hand, and push:

   ```sh
   git tag --annotate v0.2.0 -m "spyglass 0.2.0"
   git push --atomic origin main v0.2.0
   ```

Some failures need no fix in the code:

- **The tag job fails:** the tag doesn't match `Cargo.toml`, its commit isn't on `main`, or the changelog has no section for it. Tagging by hand is the usual cause; `cargo xtask release` gets all three right.
- **An oracle job fails** where `main` passed: often Mojang's download servers. Re-run the failed jobs from the run's page.

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
