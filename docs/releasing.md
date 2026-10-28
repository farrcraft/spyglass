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

`main` only takes changes through pull requests, so a release is a pull request, then a tag on what it merged ([ADR 0040](adr/0040-releases-go-through-a-pull-request.md)).

### 1. The release's pull request

From `main`, up to date with `origin/main`:

```sh
cargo xtask release 0.2.0
```

It refuses before changing anything if:

- the tree isn't clean;
- you're not on `main`, or `main` differs from `origin/main`;
- the tag or the branch `release/v0.2.0` exists;
- the version isn't after the current one;
- Unreleased is empty.

Otherwise, on a new branch `release/v0.2.0`, it:

1. sets the workspace version and refreshes `Cargo.lock`;
2. turns Unreleased into `## [0.2.0] - <today>` with the game versions this build was checked against, opens a new empty Unreleased, and updates the links at the end of the changelog;
3. runs `cargo xtask check`, every hook on every file, which takes several minutes;
4. commits `Release 0.2.0`.

If a check fails, the changes are left uncommitted on the branch. Fix them and commit there, or start again with `git checkout . && git switch main && git branch -D release/v0.2.0`.

Nothing is pushed. Read the commit, then push the branch and open the pull request:

```sh
git push --set-upstream origin release/v0.2.0
gh pr create --fill
```

Merge it once CI passes, by any merge method.

### 2. The tag

```sh
cargo xtask release-tag 0.2.0
git push origin v0.2.0
```

`release-tag` fetches `main` and finds the commit that set the workspace version to 0.2.0: the merged pull request, whichever way it was merged. It then makes the annotated tag `v0.2.0` there. If other pull requests have merged since, it says so; the tag stays on the release commit, so their changes aren't in this release. Pushing the tag starts the release workflow.

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
git push --set-upstream origin release/v0.2.0-rc.1
gh pr create --fill
# merge it, then:
cargo xtask release-tag 0.2.0-rc.1
git push origin v0.2.0-rc.1
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

2. Fix the problem through a pull request as usual. The release commit has already set the version and dated the changelog, so don't run `cargo xtask release` again.
3. Once the fix is merged, tag `main`'s new head by hand: `release-tag` would find the release commit, from before the fix. Then push the tag:

   ```sh
   git fetch origin main
   git tag --annotate v0.2.0 origin/main -m "spyglass 0.2.0"
   git push origin v0.2.0
   ```

Some failures need no fix in the code:

- **The tag job fails:** the tag doesn't match `Cargo.toml`, its commit isn't on `main`, or the changelog has no section for it. Tagging by hand is the usual cause; `cargo xtask release-tag` gets all three right.
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
