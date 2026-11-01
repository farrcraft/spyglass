# 0058. Oracle CI planned by cost, in full on main and in part on pull requests

- Status: Proposed
- Date: 2026-10-31
- Amends: point 4 of [0028](0028-supported-versions-and-ci.md), the schedule; point 3 of [0041](0041-ci-runs-what-a-change-can-affect.md), how a version's oracle work is split into jobs
- See also: [0008](0008-game-derived-test-values-stay-local.md)

## Context

Every supported version stays supported, snapshots included, so the oracle matrix only grows ([ADR 0028](0028-supported-versions-and-ci.md)). Each version's job regenerates its oracle outputs from the game in a fresh runner, since nothing game-derived may be cached or shared between jobs ([ADR 0008](0008-game-derived-test-values-stay-local.md)). [ADR 0041](0041-ci-runs-what-a-change-can-affect.md) split each version into an `only heights` job and an `exclude heights` job, with a hand-written `--shard` where a version's heights were slow.

The 0.2.0 release pull request (2026-11-01) showed that this doesn't hold:

- **Features broke the hour.** The Overworld feature goldens take 2.5 to 3.3 minutes per seed on a hosted runner, 11 seeds one after another: about 33 minutes. With the other goldens, `locate`, a cold release build of the tests (4 to 6 minutes) and the feature tests (up to 9 minutes for `feature_search` alone), 26.2's and 26.4-snapshot-6's `exclude heights` jobs passed the 60-minute timeout. 26.3's took 55 minutes.
- **Hand-sharding doesn't scale.** Each new slow output needs its own `--only`/`--exclude` split and its own shards, repeated for every version, in the matrix by hand. At 2 to 4 jobs a version, a few more snapshots pass GitHub's limit of 20 jobs at once, and the wall-clock time doubles.
- **Most of the cost is repeated for versions that a change rarely affects differently.** A pull request that changes a shared algorithm is checked as thoroughly against 26.4-snapshot-5 as against the newest release, though a regression usually shows in every version at once.

ADR 0028 anticipated this: when the matrix takes more than an hour, a new ADR picks a cheaper schedule. ADR 0041 rejected running the full matrix only nightly, because a worldgen change would merge before anything checked it against the game.

## Decision

1. **Oracle work is planned, not written into the matrix.** `cargo xtask ci-plan` divides each version's work into units: one output for a set of seeds (`features` for seed `0`), or a group of tests that read only the bundle. It packs the units into jobs that fit a budget of 40 minutes, using estimated minutes per unit plus a fixed setup cost per job, and prints the jobs as the workflow's matrix (`fromJSON`). Each job runs `cargo xtask ci` for one version with the units it was given, and makes the outputs its tests read, as before. A test that reads several outputs, or a particular seed, runs in the job that has all of them. The estimates are committed in the planner, `xtask/src/ci_plan.rs`, per version and unit kind. A version without its own uses the newest older version's. The versions come from `spyglass_data::versions::CHECKED`, so adding a version no longer edits `ci.yml`.
2. **Pull requests check the newest versions in full and the others in part.** On a pull request that runs the oracle jobs (ADR 0041's scope rules are unchanged), the newest release and the newest snapshot that's newer than it run in full. Every other version runs a smoke set: the outputs that are cheap for every seed, one golden seed for heights, blocks and features, and its bundle-only tests. A change under a version's harness adapter (`oracle/adapters/<version>/`) runs that version in full too, and so does a pull request with the `oracle: full` label. The label is read when the checks start, so one added later applies from the next push.
3. **Pushes to `main` and the release workflow run every version in full.** They may queue past the 20-job limit; their wall-clock time matters less than a pull request's.
4. **Oracle jobs cache the Rust build:** the cargo registry, `target/debug/` and `target/release/`, keyed on `Cargo.lock` and `rust-toolchain.toml`, through `actions/cache`, which CI already uses. The build doesn't depend on the game version, so all oracle jobs share one cache, saved only from `main`. It holds only spyglass's own compiled code and its dependencies. `target/oracle/`, where the harness unpacks the game's jars and merges test-pack bundles, is never cached (ADR 0008).

## Consequences

- **Adding a version doesn't need CI tuning.** A slow version gets more jobs from the planner, with no shards written by hand. If a job runs long, the fix is its estimate, not the workflow.
- **Pull request time stops growing with versions** beyond one smoke job each, about 15 minutes, which run in parallel.
- **A regression only in an older version can merge.** It shows on the push to `main` that follows, as ADR 0041's scope rules already allow for files they misjudge. The smoke set, the adapter rule and the label narrow this. A change aimed at an old version should use the label.
- **The estimates drift.** Runners vary by about 30% between runs, and code changes speed. The 40-minute budget leaves room for that; a job that nears the timeout means updating the estimates. The planner prints each job's estimate, and `cargo xtask ci` prints the actual time of each unit, so they can be compared.
- **More jobs and more setup.** Every job extracts the bundle and builds the harness again, and a version's goldens may be split across jobs. The build cache saves most of the setup, but total runner time rises. On the free plan it costs nothing, at the time of writing.
- **The cache can go stale or be evicted.** The 10 GB repository limit holds a few builds. A miss costs one cold build, about 5 minutes, and nothing else.
- **The test that keeps `CHECKED` equal to the matrix** changes to check the planner, since the workflow no longer lists versions.

## Alternatives considered

- **More hand-written splits:** a `features` job and feature shards per version. This fixes 0.2.0, but each version needs 3 to 4 jobs tuned by hand, and the next slow output needs the same again.
- **The full matrix nightly, and none on pull requests,** as ADR 0028 suggested: cheapest, but a worldgen change merges unchecked (ADR 0041). Here the newest versions are still checked in full on every pull request. A nightly run on top of `main`'s would recheck an unchanged commit.
- **Cache the oracle outputs**, keyed on the game version, the harness source and the seed list: most pull requests would only run tests. But that keeps game-derived files in a cache other workflows read, which ADR 0008 rules out.
- **Larger runners:** fewer jobs, but paid, and the hour still runs out as versions are added.
- **Fewer golden seeds for every version:** cheaper everywhere, but it weakens the full check that `main` and releases rely on.
