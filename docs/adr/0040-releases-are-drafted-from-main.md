# 0040. Releases are drafted from main by a workflow run by hand

- Status: Accepted
- Date: 2026-10-28
- Amends: point 3 of [0037](0037-versions-and-releases.md), how a release is made from `main`

## Context

[ADR 0037](0037-versions-and-releases.md) made a release an annotated `vX.Y.Z` tag on `main`, prepared by `cargo xtask release`. That command:

1. committed the version and changelog change on `main`;
2. tagged it;
3. asked for `git push --atomic origin main vX.Y.Z`.

Pushing the tag started the release workflow.

The same day, the repository was published to GitHub and `main` got a ruleset:

- every change needs a pull request;
- no force-pushes;
- `main` can't be deleted;
- nobody can bypass it.

A direct push of the release commit is now rejected. Work happens on branches and reaches `main` as a squash merge. A release should be cut from whatever `main` is when it's ready, which is always the last pull request's squash commit.

A tag has to be on that commit, which exists only after the merge. Tagging by hand afterwards works, but a mistyped version or the wrong commit only shows when the workflow rejects it. Making the release command branch, wait for the merge and then find the merged commit worked in a rehearsal, but it was more machinery than the job needs.

## Decision

We will split a release into a file change and a workflow run:

1. **`cargo xtask release <version>`** only changes files:
   - it sets the workspace version and refreshes `Cargo.lock`;
   - it turns the changelog's Unreleased section into the release's, dated and listing the game versions this build was checked against. A pre-release leaves the changelog alone.

   It doesn't branch, commit, tag or push. The change ships in a pull request like any other.

2. **The release workflow runs by hand**, from the Actions tab, and only on `main`. It releases the version in `main`'s `Cargo.toml`, from `main`'s head:
   1. it refuses a version whose tag already exists;
   2. it runs all of CI;
   3. it builds and smoke-tests each target;
   4. it leaves a **draft** release that records `main`'s commit.

   GitHub makes the `vX.Y.Z` tag on that commit when the owner publishes the draft. A rerun for the same version replaces its earlier draft, never a published release.

Everything else in ADR 0037 stands: one version, a changelog section per release, releases only from green CI, and drafts published by hand.

## Consequences

- **No tags by hand,** so no tag can be on the wrong commit or name the wrong version.
- **Nothing is public until publishing,** not even the tag, and that includes failed attempts.
- **Retrying is a rerun:** fix the problem through a pull request, then run the workflow again on the new `main`. There's no tag to delete.
- **A release is two steps:** the pull request that sets the version, then the run. Anything merged between them is in the release, since the run builds `main`'s head.
- **No tag while drafting:** the tag exists only once the draft is published. The draft's notes link to files at the tag, so those links work from publishing on.
- **One run at a time:** the workflow runs one at a time (a concurrency group), so two runs can't race to the same draft.

## Alternatives considered

- **Tag by hand after the merge,** in git or GitHub Desktop, with a tag-triggered workflow: works, but a wrong tag is caught only by the workflow, and a retry means deleting and remaking it.
- **`cargo xtask release` makes a release branch, and `release-tag` tags the merged commit:** built and rehearsed first. It needs more code and two commands, for what the workflow can do with the commit it already runs on.
- **Take the version from the tag, with no version change in a pull request:** any `main` could be released. But the released source would carry the previous version in `Cargo.toml` and an undated changelog, so building it gives a different `--version` than the downloads.
- **Start from GitHub's "Draft a new release" page:** the page can only make a tag by publishing, which would make the release public before its archives exist.
