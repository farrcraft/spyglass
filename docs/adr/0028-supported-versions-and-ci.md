# 0028. Supported versions, snapshots and CI

- Status: Proposed
- Date: 2026-10-26
- See also: [0008](0008-game-derived-test-values-stay-local.md), [0026](0026-how-game-versions-coexist.md), [0027](0027-oracle-harness-across-versions.md)

## Context

spyglass is moving from one game version (26.3) to several: 26.2, 26.3, and 26.4 as it moves through weekly snapshots. Three things are undecided:

- **Which versions are supported.** Each one is checked against the oracle, carries switches, dispatch arms and perhaps a harness adapter ([ADR 0026](0026-how-game-versions-coexist.md), [ADR 0027](0027-oracle-harness-across-versions.md)), and costs a CI job. A snapshot is replaced within a week. Most users play releases.
- **What the front ends pick by default.** `find_bundle` with no version picks the bundle with the highest data version. Once a snapshot is extracted, that's the snapshot. The CLI tests already had to pin 26.3 to avoid it. A bundle doesn't record whether it's a release.
- **How the oracle runs in CI.** Nothing derived from the game may be committed or cached for later ([ADR 0008](0008-game-derived-test-values-stay-local.md)). The goldens for one version take about 8 minutes on 4 threads, and locate about 1.5 more. The repository isn't on GitHub yet.

## Decision

1. **Supported versions are every release from 26.2 on, plus the newest snapshot of the next release.**
   - When a newer snapshot lands, it replaces the older one in CI and in the guide's cost log. The older one's switches and dispatch arms stay, since the next snapshot usually keeps them.
   - When the release ships, it replaces its snapshots.
   - Dropping an old release is a separate decision, made when its cost shows in the cost log, and recorded in the supported-versions list in the README.
2. **Bundles record whether they're a release.** `bundle.json` gains a `kind` field, `release` or `snapshot`, copied from the version manifest's `type`. The bundle format stays at 1: a bundle without `kind` is a snapshot if its id has `-snapshot-`, `-pre` or `-rc`, and a release otherwise.
3. **Defaults prefer releases.** With no version given, `find_bundle`, the CLI and the viewer's first choice pick the newest _release_. A snapshot is used only when asked for by name, or, for a world save, when the save's data version matches it ([world-saves.md](../specs/world-saves.md)).
4. **CI runs on GitHub Actions**, one job per supported version, each in a fresh runner:
   1. download the server jar from Mojang;
   2. extract the bundle;
   3. run `cargo xtask oracle golden` and `cargo xtask oracle locate`;
   4. run `cargo xtask oracle-test`.

   Nothing game-derived is cached between runs or uploaded as an artifact. A separate job runs `cargo xtask check`.

5. **`cargo xtask ci --game-version <v>` runs one version's job locally**, the same steps in the same order. The workflow only calls it, so the workflow can be tested before the repository is on GitHub.

## Consequences

- **The supported set stays small.** It's every release from 26.2 plus one snapshot, so a snapshot's CI cost doesn't pile up.
- **A user who extracts a snapshot doesn't silently get it.** They have to name it. A save made in the snapshot still opens with it.
- **Re-extraction is optional.** Old bundles without `kind` still work by their id. The extractor writes it from now on, and [bundle-format.md](../specs/bundle-format.md) documents it.
- **CI takes time and has a dependency.** Each version job takes an estimated 15 to 20 minutes on a hosted runner. That's the jar download, extraction, compiling the harness, about 10 minutes of goldens and locate, and the tests. Each run depends on Mojang's download servers. That's the price of committing nothing derived from the game.
- **Exit criterion 2 waits for publishing.** CI can't run on GitHub until the repository is published there, which is the owner's decision. `cargo xtask ci` keeps the steps tested locally until then.

## Alternatives considered

- **Every version and snapshot ever extracted:** no decisions to make, but the cost grows with every snapshot, for versions nobody plays.
- **Releases only, no snapshots:** cheaper. But the first sign of a hard update would come on release day instead of over the weeks before it, and following snapshots is how update costs are learned ([ADR 0026](0026-how-game-versions-coexist.md)).
- **Caching the jar or goldens between CI runs:** faster. But it keeps game-derived files on a service we don't control, which ADR 0008 rules out.
- **Inferring release or snapshot from the id alone:** no format change. But Mojang's naming has changed before, and the manifest already says. The id is kept only as the fallback for old bundles.
