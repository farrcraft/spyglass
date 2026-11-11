# 0067. Older snapshots are checked weekly and at release, not on every change

- Status: Accepted
- Date: 2026-11-10
- Amends: point 4 of [0028](0028-supported-versions-and-ci.md), the schedule; points 2 and 3 of [0058](0058-oracle-ci-planned-by-cost.md), which versions run in full and in part
- See also: [0008](0008-game-derived-test-values-stay-local.md), [0041](0041-ci-runs-what-a-change-can-affect.md)

## Context

Every supported version stays supported, and every snapshot is a version ([ADR 0028](0028-supported-versions-and-ci.md)), so the oracle matrix only grows: four 26.4 snapshots by 26.4-snapshot-8, with more each week until 26.4's release, and the same again for the next. [ADR 0058](0058-oracle-ci-planned-by-cost.md) plans the oracle jobs by cost. A pull request checks the newest release and the newest snapshot in full and every other version by a smoke set; a push to `main` and the release workflow check every version in full.

With 26.4-snapshot-7 as the newest version, `cargo xtask ci-plan` gave a pull request 27 jobs and a push to `main` 74, past GitHub's 20 jobs at once, so a push queued for several rounds. Each new snapshot adds a smoke job to every pull request and 6 to 7 full jobs to every push.

Most of that repeats work for snapshots no one changes. A snapshot's behaviour is fixed once the next one ships: its switches' ranges are closed, its harness adapter is closed, and a change to the shared code that breaks it usually breaks the newest snapshot too, which is still checked. The regressions only an old snapshot shows are those in a switch's old arm or an old dispatch arm, and those are rare and slow to matter: a player on an old snapshot isn't hurt by a regression found a week later, before a release ships it.

26.2 is a cost of its own: its separate `f64` evaluator makes every unit slower, 48 of the 74 jobs. It's a release, though, and this decision doesn't change how releases are checked.

## Decision

We will check the current versions on every change, and every version weekly and at release:

1. **The current versions** are the newest release and the newest snapshot, if it's newer than that release. Pull requests and pushes to `main` check them in full.
2. **Older releases** are checked by ADR 0058's smoke set on pull requests and pushes to `main`, as now.
3. **Older snapshots** (every snapshot but the newest) are not checked on pull requests or pushes to `main`. A pull request that changes a version's harness adapter checks that version in full, and one with the `oracle: full` label checks every version in full, as ADR 0058 has it.
4. **Every version in full** runs in the release workflow, as now, and in a weekly scheduled run on `main`. A failure in the weekly run is a regression to fix like any other.
5. **Support stays permanent.** Older snapshots keep their bundles, switches, dispatch arms, adapters and comparisons ([ADR 0028](0028-supported-versions-and-ci.md)); only how often CI runs them changes.

`cargo xtask ci-plan --mode` takes `pr` (with `--base`), `main` and `full` for the three schedules.

## Consequences

- **Pushes to `main` cost what pull requests do.** With 26.4-snapshot-8 newest, a pull request and a push to `main` each plan 24 jobs (26.2 in part 11, 26.3 6, 26.4-snapshot-8 7), where ADR 0058's schedule planned about 28 and 81. A new snapshot changes neither: it replaces the last as the current snapshot, which drops to no jobs. The weekly run and the release plan all 81.
- **A regression only an older snapshot shows can sit on `main` for up to a week,** where ADR 0058 found it on the push that merged it. Shipping it is still ruled out, since the release workflow checks everything. A change aimed at an old snapshot's arm or switch should use the `oracle: full` label, as ADR 0058 already asks.
- **A failure in the weekly run has no pull request to blame.** It names the version and the comparison, and the commits since the last green weekly run are the suspects.
- **26.2 still costs most of every run,** 11 smoke jobs on every pull request and push and 48 in full. Making 26.2 cheaper is a separate decision.
- **Local runs don't change.** The hooks don't run the oracle; `cargo xtask oracle-test` and `cargo xtask ci` run the versions a machine has, and a contributor can keep only the current ones ([ADR 0028](0028-supported-versions-and-ci.md)).

## Alternatives considered

- **Keep ADR 0058's schedule:** every push to `main` checks every version in full. Regressions show on the push, but `main`'s runs queue longer with each snapshot, for checks that almost never find anything the current versions miss.
- **Drop old snapshots from support** once the next one lands: ADR 0028 considered and rejected this, since players keep worlds in snapshots and those saves would lose checked answers. Checking them weekly keeps the promise at a fraction of the cost.
- **Smoke older snapshots on pull requests, nothing on `main`:** cheaper than now, but a pull request still pays a job per snapshot, a cost that grows every week, for the same rare finds the weekly run makes.
- **Check older snapshots nightly** instead of weekly: finds a regression sooner, at seven times the cost. Their code rarely changes in a week, and a release checks everything regardless.
