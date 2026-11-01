# m08. Oracle CI planned by cost

- Status: In progress
- Milestone: [08. Nether, End and multiple versions](../roadmap/completed/08-dimensions-and-versions.md), whose versions this keeps checkable as they're added
- Started: 2026-10-31
- Completed:

## Goal

Make every oracle job fit its hour again, and keep it that way as versions are added, as [ADR 0058](../adr/0058-oracle-ci-planned-by-cost.md) decides. That means a planned matrix in place of the hand-written one, pull requests checking the newest versions in full and the others in part, and a shared Rust build cache. The 0.2.0 release pull request (#4), whose 26.2 and 26.4-snapshot-6 `exclude heights` jobs timed out, must pass.

## Approach

**Units.** A unit is one piece of a version's oracle work (`ci_plan::Unit`):

- `probe`, `biomes`, `structures`, `spawn` and `locate`, each made for all seeds at once (a few minutes at most);
- `heights:<seed>` and `blocks:<seed>` per golden seed, and `features:<seed>` per feature seed, `56` included;
- `bundle`: every test that reads only the bundle, and the test pack where it loads.

**Costs.** `COSTS` in `xtask/src/ci_plan.rs`: per version, minutes per unit kind (per seed for the per-seed kinds), the comparisons that read it included, and a per-job setup cost. A version without an entry uses the newest older version's. Kept in Rust rather than a TOML file, which would need parsing for nothing. The first figures come from PR #4's logs:

| Unit                                     | 26.2 (minutes) | 26.3 and snapshots (minutes) |
| ---------------------------------------- | -------------- | ---------------------------- |
| Overworld features, per seed             | ~3.0           | ~2.5                         |
| heights, per seed                        | 3.6 to 5.6     | ~0.9                         |
| probe, biomes, structures, spawn, blocks | ~4.6 together  | ~5 together                  |
| locate                                   | 2.7            | 1.5                          |
| `features.rs` (mostly the dungeon test)  | –              | ~4.7                         |
| `placing`, all seeds                     | –              | ~2.8                         |
| `feature_search` (bundle)                | 8.9            | ~2.4                         |
| cold release build of the tests          | 4.1            | ~6.2                         |

**Packing.** Per version, as few jobs as fit a 40-minute budget with setup, balanced: units dealt costliest first to the least-loaded job. Jobs never mix versions, since each extracts one bundle. A test runs in each job that makes all its oracle inputs. One that reads a per-seed output checks the seeds that job made, as `--shard` heights did. `two_dungeon_seeds_are_the_game_s` reads a new `Input::DungeonSeed`, so it runs only where `features:56` is made.

**Running a job.** `cargo xtask ci --units <list>` replaces `--only`, `--exclude` and `--shard`. Without it, the job does everything, as locally. Each kind prints `ci: <kind> ... took <m> min`, so estimates can be checked against the logs.

**Modes.** `cargo xtask ci-plan --mode full|pr [--base <ref>]` prints `matrix={"include":[...]}` for `$GITHUB_OUTPUT`, each entry with `name`, `version` and `units`, and the plan with estimates to stderr. In `pr` mode, the newest release, a newer snapshot, and any version whose adapter chain the pull request changes get every unit. The others get the smoke set: everything made for all seeds at once, `bundle`, and the per-seed kinds for seed `0`. The workflow passes `--mode full` for the `oracle: full` label, a push and the release workflow.

**Build cache.** `actions/cache/restore` and `actions/cache/save` (the pinned `actions/cache`) for the cargo registry, `target/debug/` and `target/release/`, never `target/oracle/` (the game's unpacked jars). The key is `oracle-build-<os>-<hash of Cargo.lock and rust-toolchain.toml>`, with a restore key on the prefix. It's saved only from `main`.

## Tasks

- [x] `COSTS` with 26.2, 26.3 and 26.4-snapshot-6 figures from PR #4's logs; snapshot-5 uses 26.3's
- [x] `ci_plan.rs`: units, balanced packing, `pr`/`full` modes, JSON output, with tests (every unit in exactly one job; jobs under budget; every test runs, bundle-only ones once; the pull request's version choice; estimates fall back by data version)
- [x] `ci.rs`: `--units` in place of `--only`/`--exclude`/`--shard`, per-kind timing, the test filter by made inputs
- [x] `oracle_test.rs`: `Input::DungeonSeed` on `two_dungeon_seeds_are_the_game_s`
- [x] `oracle.rs`: `adapter_dirs` (an adapter and its bases) for the adapter rule; `golden_seed_shard` removed. `ci_scope.rs` shares `changed_files`
- [x] `ci.yml`: the planned matrix, the label, the build cache. No `labeled` trigger: adding a label would cancel the running checks through the concurrency group, so the label applies from the next push
- [x] `versions.rs`: `checked_is_the_ci_matrix` removed, since the workflow no longer lists versions; `CHECKED`'s doc comment says CI plans from it
- [x] Run `ci-plan --mode full` and `--mode pr` locally and check the job lists by eye: full is 13 jobs of ~31 to 40 minutes, a pull request touching no adapter 8
- [ ] Push to PR #4's branch; every oracle job passes and the longest stays under 45 minutes
- [ ] Compare the actual unit times in the logs with the estimates; correct `COSTS`
- [ ] ADR 0058 to Accepted once CI passes on `main`

## Verification

- xtask unit tests for the planner's invariants (above).
- PR #4 changes every adapter, so it plans every version in full: 13 jobs, all passing.
- A full run on `main` after merging: every job under 45 minutes, and the build cache saved.
- The next pull request restores the build cache, and its test build takes under a minute.

## Durable outputs

- [ ] [ADR 0058](../adr/0058-oracle-ci-planned-by-cost.md) accepted
- [x] [oracle.md](../specs/oracle.md#one-versions-ci-job): `cargo xtask ci --units`, and how CI plans the jobs
- [x] [adding-a-version.md](../adding-a-version.md): adding `CHECKED` is the CI step; a `COSTS` entry once the first run's times are known
- [x] [bundle-format.md](../specs/bundle-format.md#checked-versions): `CHECKED` is the list CI plans from
- [x] [getting-started.md](../getting-started.md) and CLAUDE.md: `cargo xtask ci-plan`

## Notes

- 2026-10-31: first-fit packing filled each job to the 40-minute budget and left a small one over; dealing units to the least-loaded job of the fewest that fit gives each job the same slack instead.
- 2026-11-01: the first planned run passed 12 of 13 jobs; 26.3 and the snapshots took 14 to 30 minutes, 26.2's 34 to 42. 26.2's job with seed `56` was cancelled at 60: `two_dungeon_seeds_are_the_game_s` took 22 minutes there (4 to 5 in later versions), estimated at 5 because the first run never finished it. `COSTS` now uses the slowest runner's per-seed times, and `setup` includes the comparisons every job runs on its own seeds (`features_in`, `placing`: about 5 minutes in 26.2), which were missing. The new plan gives 26.2 six jobs, one of them the dungeon seed alone, and 26.3 and 26.4-snapshot-5 two each: still 13 jobs, estimated at 30 to 37 minutes.
- 2026-10-31: PR #4's logs: 26.2 `exclude heights` spent 38 minutes on goldens (33 on Overworld features), 4 building tests, 9 on `feature_search`, and was cancelled in `features.rs` at 60. 26.3's passed at 55. 26.2's heights shards took 18 and 28 minutes for five seeds each.
