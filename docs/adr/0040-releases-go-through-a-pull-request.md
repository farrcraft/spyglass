# 0040. Releases go through a pull request

- Status: Accepted
- Date: 2026-10-28
- Amends: point 3 of [0037](0037-versions-and-releases.md), how a release commit reaches `main`

## Context

[ADR 0037](0037-versions-and-releases.md) made a release an annotated `vX.Y.Z` tag on `main`, prepared by `cargo xtask release`. The command committed the version and changelog change on `main`, tagged it, and asked for `git push --atomic origin main vX.Y.Z`.

The same day, the repository was published to GitHub and `main` got a ruleset:

- every change needs a pull request;
- no force-pushes;
- `main` can't be deleted;
- nobody can bypass it.

A direct push of the release commit to `main` is now rejected. Tags aren't covered by the ruleset.

With a pull request, the commit that lands on `main` isn't necessarily the one made locally. A squash or rebase merge makes a new commit, so a tag made before the merge would point at a commit that isn't on `main`. The release workflow's first job checks that the tagged commit is on `main`, so such a tag fails the release.

## Decision

We will release in two steps, with a pull request between them:

1. **`cargo xtask release <version>`** starts from a `main` that matches `origin/main`. It makes the branch `release/v<version>`, commits the version and changelog change there after every check passes, and stops. The release commit goes through a pull request like any other change.
2. **`cargo xtask release-tag <version>`**, after the merge, finds the commit on `origin/main` that set the workspace version to `<version>`, and tags it. It finds that commit by reading the version before and after each commit that touched `Cargo.toml`, so it works for any merge method, and with other pull requests merged after the release's. Pushing the tag starts the release workflow, as before.

Neither command pushes. Everything else in ADR 0037 stands.

## Consequences

- Releases follow the same review path as every other change, and the ruleset needs no exception.
- A release takes one more step: the release's pull request, then the tag.
- The tag is on the release commit itself, not on whatever `main` has by the time it's made. A change merged in between isn't in the release. `release-tag` says when there is one.
- Retrying a failed release still means a new tag on a later commit. `release-tag` finds only the commit that set the version, so a retry is tagged by hand, on the fixed commit ([releasing.md](../releasing.md#when-something-fails)).
- If tags are ever protected too, `release-tag`'s push needs that ruleset to allow it.

## Alternatives considered

- **A bypass for releases in the ruleset:** keeps one step, but it's an exception to "every change goes through a pull request" for the commit that matters most.
- **Tag the release branch's commit before merging:** with a squash or rebase merge, that commit never reaches `main`, and the release workflow rejects the tag.
- **Let the release workflow tag on merge:** automatic, but a merged pull request would start a release with no deliberate last step. A tag pushed by hand is that step.
