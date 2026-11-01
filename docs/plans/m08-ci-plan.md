# m08. Oracle CI planned by cost

- Status: Draft
- Milestone: [08. Nether, End and multiple versions](../roadmap/completed/08-dimensions-and-versions.md), whose versions this keeps checkable as they're added
- Started: 2026-10-31
- Completed:

## Goal

Make every oracle job fit its hour again, and keep it that way as versions are added, as [ADR 0058](../adr/0058-oracle-ci-planned-by-cost.md) decides. That means a planned matrix in place of the hand-written one, pull requests checking the newest versions in full and the others in part, and a shared Rust build cache. The 0.2.0 release pull request (#4), whose 26.2 and 26.4-snapshot-6 `exclude heights` jobs timed out, must pass.

## Approach

**Units.** A unit is one oracle output for a set of seeds, or a group of bundle-only tests, for one version. `xtask/src/ci_plan.rs` lists a version's units:

- `probe`, `biomes`, `structures`, `spawn` and `locate`, each one unit for all seeds (each is a few minutes at most);
- `heights`, `blocks` and `features`, one unit per golden seed (features: per feature seed, `56` included);
- the bundle-only tests in groups by test binary, since `feature_search` alone runs 2 to 9 minutes.

**Costs.** `xtask/ci-costs.toml`, committed: per version, minutes per unit kind and per seed where the seeds differ much, plus a per-job setup cost (checkout, JDK, cached build, bundle extraction, harness compile). A version without an entry uses the newest version's. These first figures come from PR #4's logs:

| Unit (26.2)                  | Minutes         |
| ---------------------------- | --------------- |
| Overworld features, per seed | 2.5 to 3.3      |
| Overworld heights, per seed  | 8 to 12         |
| biomes, all                  | ~2              |
| structures, blocks, spawn    | ~1.5, ~1, ~0.3  |
| locate                       | 1.5 to 3        |
| feature tests, per seed      | ~1 (to measure) |
| `feature_search` (bundle)    | 2.5 to 9        |
| cold release build           | 4 to 6          |

**Packing.** First-fit decreasing per version, into jobs whose estimate is under 40 minutes, setup included. Jobs never mix versions, since each extracts one bundle. A test runs in the job that makes all its inputs:

- Tests that read a per-seed output run in every job holding some of its seeds, against those seeds' files, as `--shard` heights do now.
- Tests that need a particular seed (`two_dungeon_seeds_are_the_game_s` needs `56`) say so with a new `Input`, so they run only in the job holding that seed. `blocks_through_the_api_are_the_game_s` reads both block runs, which one `blocks` unit makes together.

**Running a job.** `cargo xtask ci` gains `--units <list>`, which replaces `--only`, `--exclude` and `--shard`. With no flags it still runs the whole version locally. The test filter becomes "every input of the test is made in this job". Each unit's actual time is printed as `ci: <unit> took <m>`, so estimates can be checked against the logs.

**Modes.** `cargo xtask ci-plan --mode full|pr [--base <ref>] [--label <name>...]` prints `matrix={"include":[...]}` for `$GITHUB_OUTPUT`. Each entry has `version`, `units` and a `name` such as `oracle 26.2 features 1/2`, and the plan prints a summary of jobs and their estimates. In `pr` mode, the newest release and any newer snapshot (newest by data version in `CHECKED`; snapshot by id, as [bundle-format.md](../specs/bundle-format.md) decides it) get every unit. Every other version gets a smoke set, `probe`, `biomes`, `structures`, `spawn`, plus `heights`, `blocks` and `features` for seed `0`, and its bundle-only tests, unless the pull request changes `oracle/adapters/<version>/` or carries the `oracle: full` label.

**Workflow.** The `scope` job runs `ci-plan` after `ci-scope` (it already has `fetch-depth: 0`). The `oracle` job's matrix is `fromJSON(needs.scope.outputs.matrix)`, and its name comes from the entry. `pull_request` gains `types: [opened, synchronize, reopened, labeled]`, so adding the label reruns. `push` and `workflow_call` (release) pass `--mode full`.

**Build cache.** `actions/cache` (already pinned in `ci.yml`) for `~/.cargo/registry`, `~/.cargo/git` and `target/`, with the key `oracle-build-${{ runner.os }}-${{ hashFiles('Cargo.lock', 'rust-toolchain.toml') }}` and a restore key on the prefix. It's saved only from `main`, so pull requests don't fill the 10 GB limit with their own copies.

## Tasks

- [ ] `ci-costs.toml` with 26.2, 26.3 and 26.4-snapshot-6 figures from PR #4's logs; snapshot-5 falls back to the newest
- [ ] `ci_plan.rs`: units per version, packing, `pr`/`full` modes, JSON output, with tests (every unit in exactly one job; every test's inputs in one job; each job under budget; a version not in the cost file uses the newest's costs)
- [ ] `ci.rs`: `--units` in place of `--only`/`--exclude`/`--shard`, per-unit timing, the test filter by made inputs; keep `two_jobs_run_each_test_once` as "a plan's jobs run each test once"
- [ ] `oracle_test.rs`: an `Input` for the dungeon seed's feature file, on `two_dungeon_seeds_are_the_game_s`
- [ ] `ci_scope.rs`: which versions' adapters a pull request changes, for `ci-plan` (or `ci-plan` diffs itself)
- [ ] `ci.yml`: the planned matrix, the label trigger, the build cache
- [ ] `versions.rs`: replace `checked_is_the_ci_matrix` with a test that `ci-costs.toml` names only `CHECKED` versions; `CHECKED`'s doc comment no longer mentions the matrix
- [ ] Run `ci-plan --mode full` and `--mode pr` locally and check the job lists by eye
- [ ] Push to PR #4's branch; every oracle job passes and the longest stays under 45 minutes
- [ ] Compare the actual unit times in the logs with the estimates; correct `ci-costs.toml`
- [ ] ADR 0058 to Accepted once CI passes on `main`

## Verification

- xtask unit tests for the planner's invariants (above), and for the `pr` mode's version choice with a release newest, a snapshot newest, and an adapter change.
- A full run on `main` after merging: every version, every job under 45 minutes.
- A pull request touching only `crates/spyglass-feature/` gets full jobs for 26.3 and 26.4-snapshot-6, and smoke jobs for 26.2 and 26.4-snapshot-5.
- A second run of the same pull request restores the build cache, and its test build takes under a minute.

## Durable outputs

- [ ] [ADR 0058](../adr/0058-oracle-ci-planned-by-cost.md) accepted
- [ ] [oracle.md](../specs/oracle.md#one-versions-ci-job): `cargo xtask ci --units`, and how CI plans the jobs
- [ ] [adding-a-version.md](../adding-a-version.md): replace the matrix and shard step with adding `CHECKED` and a `ci-costs.toml` entry once the first run's times are known
- [ ] [bundle-format.md](../specs/bundle-format.md#checked-versions): `CHECKED` is the list CI plans from, not a copy of the matrix
- [ ] [getting-started.md](../getting-started.md): `cargo xtask ci-plan` in the command table; CLAUDE.md's command list likewise

## Notes

- 2026-10-31: PR #4's logs: 26.2 `exclude heights` spent 38 minutes on goldens (33 on Overworld features), 4 building tests, 9 on `feature_search`, and was cancelled in `features.rs` at 60. 26.3's passed at 55.
