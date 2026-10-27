# 0028. Supported versions, snapshots and CI

- Status: Proposed
- Date: 2026-10-26
- See also: [0008](0008-game-derived-test-values-stay-local.md), [0026](0026-how-game-versions-coexist.md), [0027](0027-oracle-harness-across-versions.md)

## Context

spyglass is moving from one game version (26.3) to several. 26.2 and 26.3 are releases. 26.4 arrives as weekly snapshots before its release. Three things are undecided:

- **Which versions are supported, and for how long.** Snapshots aren't only a step towards a release. Each one is a version that people can choose to play and keep playing, for its particular behavior or for any other reason, and worlds made in it stay in it. Each supported version is checked against the oracle and costs a CI job. It also carries switches, dispatch arms and perhaps a harness adapter ([ADR 0026](0026-how-game-versions-coexist.md), [ADR 0027](0027-oracle-harness-across-versions.md)).
- **What the front ends pick by default.** `find_bundle` with no version picks the bundle with the highest data version. Once a snapshot is extracted, that's the snapshot. The CLI tests already had to pin 26.3 to avoid it. A bundle doesn't record whether it's a release.
- **How the oracle runs in CI.** Nothing derived from the game may be committed or cached for later ([ADR 0008](0008-game-derived-test-values-stay-local.md)). The goldens for one version take about 8 minutes on 4 threads, and locate about 1.5 more. The repository isn't on GitHub yet.

## Decision

1. **Support is permanent, and every snapshot is a version.** Once a version is supported, it stays supported: releases, snapshots, pre-releases and release candidates alike. Each keeps its bundle, its oracle comparisons, its CI job and its row in the guide's cost log.
   - New versions are added as they come out, from 26.2 and 26.4-snapshot-5 on.
   - Versions older than that, including 26.4's first four snapshots, can be added later by the same guide when someone wants them.
   - Each version is identified by its own data version, which is unique to it, snapshots included.
2. **Bundles record whether they're a release.** `bundle.json` gains a `kind` field, `release` or `snapshot`, copied from the version manifest's `type`. The bundle format stays at 1: a bundle without `kind` is a snapshot if its id has `-snapshot-`, `-pre` or `-rc`, and a release otherwise.
3. **Defaults prefer releases.** With no version given, `find_bundle`, the CLI and the viewer's first choice pick the newest _release_. A snapshot is used when it's asked for by name, or, for a world save, when the save's data version matches it ([world-saves.md](../specs/world-saves.md)). This decides only the default. Snapshots are as supported as releases.
4. **CI runs on GitHub Actions**, one job per supported version, each in a fresh runner:
   1. download the server jar from Mojang;
   2. extract the bundle;
   3. run `cargo xtask oracle golden` and `cargo xtask oracle locate`;
   4. run `cargo xtask oracle-test`.

   Nothing game-derived is cached between runs or uploaded as an artifact. A separate job runs `cargo xtask check`. Every supported version runs on every push and pull request until the matrix takes more than an hour of wall-clock time. At that point a new ADR picks a cheaper schedule, for example the full matrix nightly and a fixed subset on pull requests.

5. **`cargo xtask ci --game-version <v>` runs one version's job locally**, the same steps in the same order. The workflow only calls it, so the workflow can be tested before the repository is on GitHub.

## Consequences

- **The supported set only grows.** Each release cycle adds a release and every snapshot, pre-release and candidate before it.
- **The code never sheds versions.** Switches, dispatch arms, evaluator families and harness adapters accumulate; none is deleted because its versions are old ([ADR 0026](0026-how-game-versions-coexist.md)). Keeping them readable is the cost. The switch table and `version-diff` are what keep it manageable.
- **Each version stays checkable.** Behavior that changes in one snapshot and changes back in the next must be expressed exactly, so switches hold data-version ranges, not a single starting version ([ADR 0026](0026-how-game-versions-coexist.md)).
- **CI cost grows linearly.** Each version job takes an estimated 15 to 20 minutes on a hosted runner: the jar download, extraction, compiling the harness, about 10 minutes of goldens and locate, and the tests. Jobs run in parallel, so wall-clock time grows in steps of the runner concurrency limit. On GitHub's free plan for public repositories, standard runners cost nothing and run 20 jobs at once, at the time of writing. Each run also depends on Mojang's download servers for every version. That's the price of committing nothing derived from the game.
- **Local work grows too.** A full local oracle run needs each version's bundle and outputs, about 320 MB of outputs for 26.3. `cargo xtask oracle-test` runs whatever versions a machine has, so contributors can keep only the versions they work on.
- **A user who extracts a snapshot doesn't silently get it** for a new world. They have to name it. A save made in the snapshot opens with it.
- **Re-extraction is optional.** Old bundles without `kind` still work by their id. The extractor writes it from now on, and [bundle-format.md](../specs/bundle-format.md) documents it.
- **Exit criterion 2 waits for publishing.** CI can't run on GitHub until the repository is published there, which is the owner's decision. `cargo xtask ci` keeps the steps tested locally until then.

## Alternatives considered

- **Releases plus only the newest snapshot**, with older snapshots dropped as new ones land: a small, fixed supported set. But snapshots are versions people play and keep worlds in. Dropping one leaves those players and their saves without answers.
- **Releases only, no snapshots:** cheaper. But it fails snapshot players entirely, and the first sign of a hard update would come on release day instead of over the weeks before it.
- **Caching the jar or goldens between CI runs:** faster. But it keeps game-derived files on a service we don't control, which ADR 0008 rules out.
- **Inferring release or snapshot from the id alone:** no format change. But Mojang's naming has changed before, and the manifest already says. The id is kept only as the fallback for old bundles.
