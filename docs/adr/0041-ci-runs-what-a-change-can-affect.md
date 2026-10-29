# 0041. CI runs what a change can affect, in jobs that fit an hour

- Status: Accepted
- Date: 2026-10-28
- Amends: point 4 of [0028](0028-supported-versions-and-ci.md), what runs on a pull request and how a version's job is split

## Context

[ADR 0028](0028-supported-versions-and-ci.md) runs every check and one oracle job per supported version on every push and pull request. Each oracle job regenerates that version's outputs from the game in a fresh runner, since nothing game-derived may be kept between runs ([ADR 0008](0008-game-derived-test-values-stay-local.md)). The first runs on GitHub, on 2026-10-28, showed two problems.

- **Most pull requests can't change what the oracle tests,** yet pay for it. Oracle jobs took 15 to 25 minutes each, and there's one per supported version, a set that only grows. A pull request that changes only docs, the viewer or the release tooling waited for all of them.
- **One version's job didn't fit its hour.** In 26.2 the game's own code computes Overworld heights about five times slower than in 26.3: 8 to 12 minutes per golden seed on a hosted runner's four cores. The height goldens took about 45 minutes, and with the rest of the job, 26.2 passed the 60-minute timeout. ADR 0028 anticipated wall-clock limits, but for the matrix as a whole, not for one job.

GitHub can skip a whole workflow by path (`paths:` on the trigger), but a skipped workflow leaves required status checks waiting forever. A job skipped by a condition counts as passing.

## Decision

1. **A pull request runs the jobs its files can affect.** A first job, `cargo xtask ci-scope`, compares the pull request with its base and sets which jobs run. The rules are in `xtask/src/ci_scope.rs`, with tests.
   - **Docs** (Markdown, `docs/`, images): only the commit-stage hooks (links, formatting, spelling), in a short `docs` job.
   - **Specs** (`docs/specs/`) also get the Linux check, because a test reads their headings.
   - **The oracle jobs** run for changes to the library crates, `oracle/`, the build files (`Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`), xtask's oracle code and `ci.yml` itself. The viewer is excluded: its tests compare it with the library, not with the game.
   - **The Windows check** runs for everything but docs, specs, Linux's setup script, the dev container and the other workflows.

   Jobs are skipped by condition, never by a workflow path filter.

2. **Everything runs on a push to `main` and in the release workflow,** whatever changed. A rule that skips too much on a pull request shows on the merge that follows.
3. **Each version's oracle work runs in parts.** `cargo xtask ci --part heights` makes the height goldens and runs their test; `--part rest` makes and tests everything else. Each part makes its own inputs, so nothing game-derived passes between jobs. Where a version's heights don't fit an hour, `--shard I/N` splits them by golden seed. 26.2's heights run as two shards of five seeds each. Locally, `cargo xtask ci` without `--part` still runs the whole job.

## Consequences

- **Faster pull requests:** a docs pull request takes a few minutes instead of an hour, and a viewer or tooling change skips the oracle jobs.
- **The rules can be wrong.** A file that should start the oracle jobs and doesn't is caught on `main`, after the merge, not before. The rules lean towards running: a file they don't recognise gets the Linux check, and changing `ci.yml` runs everything.
- **More jobs, each shorter.** Every version has at least two oracle jobs, and each extracts the bundle and compiles the harness and tests again. The total runner time is a little higher; wall-clock time is lower. On GitHub's free plan for public repositories, runner time costs nothing and 20 jobs run at once, at the time of writing. That's 10 versions at two jobs each before the matrix waits for a free runner.
- **Required checks:** if status checks are made required in the `main` ruleset, a skipped job counts as passing. The `scope` job decides which ran.
- **A slow new version** gets its heights sharded in the matrix with one `include` per shard. docs/adding-a-version.md says how.

## Alternatives considered

- **Path filters on the workflow trigger:** simpler, but a skipped workflow blocks required checks.
- **A third-party action to classify paths** (such as `dorny/paths-filter`): less code, but another action to trust, and the workflows otherwise only call `cargo xtask`.
- **A longer timeout for 26.2:** one line, but a 26.2 change would wait over an hour for its result, and the next slow version would need the same.
- **Run the full oracle matrix nightly instead of on pull requests,** as ADR 0028 suggested for when the matrix gets too slow: cheaper still, but a worldgen change would merge before anything checked it against the game.
- **Generate the goldens once and share them between jobs:** that means uploading game-derived files as artifacts, which ADR 0008 rules out.
