# m27. Measuring the decoration order around the spawn

- Status: Draft
- Milestone: [27. Decoration order around the spawn](../roadmap/27-decoration-order.md)
- Started: 2026-11-14
- Completed:

## Goal

Measure the order the game decorates chunks in around the spawn when a world is created, measure how many feature and container answers that order changes, and decide in an ADR whether spyglass gives a third answer, "as decorated when the world is created".

This plan covers the milestone's first two exit criteria: the measurements in `docs/research/`, and the ADR. If the ADR accepts a third answer, a second plan builds it (the third criterion). If it rejects one, the milestone closes with this plan.

The baseline is [ADR 0056](../adr/0056-two-certainties-on-its-own-and-whatever-the-neighbours.md)'s: with all 8 neighbours decorated first, 4 of 26.3's 2,684 golden chunks differ, about 1 in 700 ([oracle.md](../specs/oracle.md#feature-golden-files)). Horizon's lead ([prior art](../research/prior-art.md)) is that two vanilla runs differ on about 6% of neighbouring chunk pairs, and that a model of the scheduler agrees with vanilla about as well. Both numbers are its claims, measured around a standing player, so this plan measures for itself.

## Approach

Three parts, each needing the one before it: what the game does at creation, recordings of it, and what the recorded orders change.

### What the game generates at creation

Before recording anything, read each supported version's server code (decompiled locally, never committed) for:

- which chunks reach the `FEATURES` status while a new world starts, before any player joins, and from which tickets;
- whether a player's first join, at the spawn, decorates more chunks, and in what order relative to startup;
- whether two chunks' `FEATURES` steps can run at the same time on different worker threads, or whether the step's write radius serialises chunks whose regions overlap. If they can overlap, "the order" is a partial order, and the recordings have to show which pairs overlapped.

What's found goes in the research doc, by version. It decides what the recordings can and can't measure: the oracle's server has no player, so if a join decorates chunks, the measurement covers startup only, and the doc says so.

### Recording the order: `cargo xtask oracle order`

A new oracle command, beside `locate` ([ADR 0015](../adr/0015-locate-checks-on-a-real-server.md)), reusing its `Server` and its EULA opt-in ([ADR 0033](../adr/0033-the-oracle-accepts-the-eula-only-when-asked.md)):

- Starts the dedicated server on a fresh world per seed, with the game's JFR events recording from the start of the JVM (the server's own JFR option, or `-XX:StartFlightRecording` if that doesn't catch startup; whichever the first part finds records `minecraft.ChunkGeneration` during spawn preparation).
- Stops the server after `Done (`, and reads the recording with the JDK's own `jfr print --json --events minecraft.ChunkGeneration`, parsed in Rust. No new Java.
- Runs each seed twice by default (`--runs N`), and optionally with a different processor count (`-XX:ActiveProcessorCount`, `--cpus N`), since the order may depend on how many workers there are.
- Writes `oracle-out/<version>/order/<seed>-<run>.json`: for each chunk that reached `FEATURES`, its position, the event's start and end time and thread, and its index in start order. The file also records the version, seed, processor count and run.

It's slow (a server start and world creation per run), so like `locate` it isn't part of `oracle golden` or CI.

### What the order changes: `oracle features --order`

The features harness ([ADR 0053](../adr/0053-the-oracle-decorates-chunks-on-their-own.md)) already decorates a chunk on its own (`tries`) and after all 8 neighbours (`after_neighbours`). A third mode replays a recorded order: generate the area to the step before features, then decorate its chunks one at a time in the recorded order, each over its own 3 × 3, recording the target features' calls and the containers for every chunk.

- The replay lives in the oracle's core, over the adapters' existing decorate-one-chunk call, so no adapter needs a new method. If one does, it's added to every adapter.
- If the first part finds that `FEATURES` steps overlap, the replay runs overlapping pairs in both orders and reports both.
- The output is a feature file per run, in the existing format plus the order it was replayed from, so the existing comparisons read it.

`cargo xtask order-report` (beside `math-report`) then reads the order files and the replays and prints:

- how stable the order is: for each pair of neighbouring decorated chunks, which went first, and how often two runs of a seed agree, by version, seed and processor count;
- how many answers it changes: target calls and containers that differ between the replay and `tries`, and between two runs' replays, per decorated chunk, against the 1 in 700 baseline;
- where the differences are: distance from the spawn, and which features.

### A model, if the order is stable

Only if the recordings show the order at creation is the same across runs, or nearly so: prototype the scheduler's order as an `order-report` module (tooling, so it can be thrown away), and compare its order with the recordings in the same report. Horizon's `worldgen::decoration_order` (MIT) can be read as a lead; spyglass's model is checked against its own recordings, not against Horizon. If the ADR accepts a third answer, the next plan moves the model into the library.

If the order isn't stable, there's no model to build, and the ADR rejects the third answer on the measurement.

## Tasks

- [ ] Read each supported version's server code for what's decorated at creation, from which tickets, and whether `FEATURES` steps can overlap. Write it up in `docs/research/decoration-order.md`.
- [ ] Check that the game's `minecraft.ChunkGeneration` events are recorded during spawn preparation, and with which option, on 26.2, 26.3 and 26.4.
- [ ] `cargo xtask oracle order`: run the server with JFR per seed and run, convert the events to order files. Tests for parsing `jfr print --json` output from a fixture the test builds (no game data committed).
- [ ] Record the golden seeds and 50 further seeds per supported release, twice each, at the default processor count and at 2.
- [ ] `oracle features --order <file>`: replay a recorded order over the area, writing a feature file per run.
- [ ] Check the replay against the existing runs: an order with the target chunk first reproduces `tries`, and one with its 8 neighbours first reproduces `after_neighbours`.
- [ ] `cargo xtask order-report`: order stability between runs, seeds, versions and processor counts; answers changed against `tries` and between runs.
- [ ] Record the measurements in `docs/research/decoration-order.md`.
- [ ] If the order is stable: prototype the model in `order-report`, and add its agreement with the recordings to the report and the research doc.
- [ ] ADR 0073: a third answer, "as decorated when the world is created", accepted or rejected, from the measurements.
- [ ] Update [oracle.md](../specs/oracle.md) for `order`, the order files and `features --order`; CLAUDE.md's command list for `order-report`.
- [ ] Update the milestone: tick its first two exit criteria, and either list the implementation plan or, if rejected, strike the third criterion with a pointer to the ADR.

## Verification

- `oracle order` run twice on one seed: every chunk the server decorates appears once at `FEATURES` in each file.
- The replay reproduces `tries` and `after_neighbours` exactly for the orders that should give them (above), on the golden seeds, for every supported version.
- `order-report`'s pair counts checked by hand on one small recording.
- `cargo xtask check` passes, and `cargo xtask oracle-test` where the feature files changed format.

## Durable outputs

- [ ] `docs/research/decoration-order.md`: what the game decorates at creation, by version; the measurements; the model's agreement, if built.
- [ ] [oracle.md](../specs/oracle.md): the `order` command and its files, and `features --order`.
- [ ] ADR 0073, accepting or rejecting a third answer. If it supersedes or amends [ADR 0056](../adr/0056-two-certainties-on-its-own-and-whatever-the-neighbours.md), that ADR's status says so.
- [ ] [prior-art.md](../research/prior-art.md): Horizon's decoration-order lead updated with what spyglass measured.

## Notes

Newest first.

- 2026-11-14: The plan stops at the ADR. Building the third answer is only worth planning once the measurement says it's worth it, which is how the milestone frames it.
- 2026-11-14: Open questions for the first task: whether the versions supported still prepare a spawn area at startup at all, and whether startup without a player decorates enough to answer the dungeon and loot searches near the spawn that motivate this. If startup decorates little and a player's join does most of it, the oracle needs a way to imitate the join, which isn't planned here; the research doc says what's missing and the ADR decides on what's measured.
