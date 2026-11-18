# m27. Measuring the decoration order around the spawn

- Status: Completed
- Milestone: [27. Decoration order around the spawn](../../roadmap/completed/27-decoration-order.md)
- Started: 2026-11-14
- Completed: 2026-11-15

## Goal

Measure the order the game decorates chunks in around the spawn when a world is created, measure how many feature and container answers that order changes, and decide in an ADR whether spyglass gives a third answer, "as decorated when the world is created".

This plan covers the milestone's first two exit criteria: the measurements in `docs/research/`, and the ADR. If the ADR accepts a third answer, a second plan builds it (the third criterion). If it rejects one, the milestone closes with this plan.

The baseline is [ADR 0056](../../adr/0056-two-certainties-on-its-own-and-whatever-the-neighbours.md)'s: with all 8 neighbours decorated first, 4 of 26.3's 2,684 golden chunks differ, about 1 in 700 ([oracle.md](../../specs/oracle.md#feature-golden-files)). Horizon's lead ([prior art](../../research/prior-art.md)) is that two vanilla runs differ on about 6% of neighbouring chunk pairs, and that a model of the scheduler agrees with vanilla about as well. Both numbers are its claims, measured around a standing player, so this plan measures for itself.

## Approach

Three parts, each needing the one before it: what the game does at creation, recordings of it, and what the recorded orders change.

### What the game generates at creation

Before recording anything, read each supported version's server code (decompiled locally, never committed) for:

- which chunks reach the `FEATURES` status while a new world starts, before any player joins, and from which tickets;
- whether a player's first join, at the spawn, decorates more chunks, and in what order relative to startup;
- whether two chunks' `FEATURES` steps can run at the same time on different worker threads, or whether the step's write radius serialises chunks whose regions overlap. If they can overlap, "the order" is a partial order, and the recordings have to show which pairs overlapped.

What's found goes in the research doc, by version. It decides what the recordings can and can't measure: the oracle's server has no player, so if a join decorates chunks, the measurement covers startup only, and the doc says so.

### Recording the order: `cargo xtask oracle order`

A new oracle command, beside `locate` ([ADR 0015](../../adr/0015-locate-checks-on-a-real-server.md)), with its EULA opt-in ([ADR 0033](../../adr/0033-the-oracle-accepts-the-eula-only-when-asked.md)). The first part found that startup decorates only about 48 blocks, so the recording joins a stand-in player ([ADR 0073](../../adr/0073-the-oracle-joins-a-stand-in-player.md)):

- `spyglass.oracle.Join` starts the stock server in the harness's JVM with `--jfrProfile`, joins one player through the game's `PrepareSpawnTask`, waits for the view distance to load, and stops the server, which writes `debug/*.jfr`. `--no-join` records startup alone.
- The xtask reads the recording with the JDK's own `jfr print --json --events minecraft.ChunkGeneration`, parsed in Rust.
- Runs each seed twice by default (`--runs N`), and optionally with a different processor count (`-XX:ActiveProcessorCount`, `--cpus N`), since the order may depend on how many workers there are.
- Writes `oracle-out/<version>/order/<seed>-<run>.json`: the player's chunk and the view distance, and for each chunk that reached `FEATURES`, its position, the event's start time, duration and thread, its index in start order, and whether it came before the join. The file also records the version, seed, processor count and run.

It's slow (a server start and world creation per run), so like `locate` it isn't part of `oracle golden` or CI.

### What the order changes: `oracle replay`

The features harness ([ADR 0053](../../adr/0053-the-oracle-decorates-chunks-on-their-own.md)) already decorates a chunk on its own (`tries`) and after all 8 neighbours (`after_neighbours`). `oracle replay` replays a recorded order instead: every chunk of the order and its neighbours generated once to the step before features, then each chunk decorated in full in the recorded order over those shared chunks, recording every chunk's target calls (`replayed`) beside its calls on its own (`tries`).

- Decorating over shared chunks, rather than each target over a fresh neighbourhood, follows the recorded order exactly, neighbours' neighbours included, and generates each chunk once. It needs one adapter method, `GameApi.decorateInOrder`, in 26.2 and 26.3 (26.4 builds on 26.3).
- The recordings show `FEATURES` steps never overlap, so each order is replayed as recorded.
- The output is `oracle-out/<version>/order-features/<seed>/<run>.json`, in the feature files' form ([oracle.md](../../specs/oracle.md#replayed-orders)).

`cargo xtask order-report` (beside `math-report`) then reads the order files and the replays and prints:

- how stable the order is: for each pair of neighbouring decorated chunks, which went first, and how often two runs of a seed agree, by version, seed and processor count;
- how many answers it changes: target calls and containers that differ between the replay and `tries`, and between two runs' replays, per decorated chunk, against the 1 in 700 baseline;
- where the differences are: distance from the spawn, and which features.

### A model, if the order is stable

Only if the recordings show the order at creation is the same across runs, or nearly so: prototype the scheduler's order as an `order-report` module (tooling, so it can be thrown away), and compare its order with the recordings in the same report. Horizon's `worldgen::decoration_order` (MIT) can be read as a lead; spyglass's model is checked against its own recordings, not against Horizon. If the ADR accepts a third answer, the next plan moves the model into the library.

If the order isn't stable, there's no model to build, and the ADR rejects the third answer on the measurement.

## Tasks

- [x] Read each supported version's server code for what's decorated at creation, from which tickets, and whether `FEATURES` steps can overlap. Write it up in `docs/research/decoration-order.md`.
- [x] Check that the game's `minecraft.ChunkGeneration` events are recorded during spawn preparation with `--jfrProfile`, on 26.2, 26.3 and 26.4.
- [x] `Join`: the stock server in the harness's JVM, with a player joined through `PrepareSpawnTask` on a stand-in connection ([ADR 0073](../../adr/0073-the-oracle-joins-a-stand-in-player.md)).
- [x] Find how long the server keeps a player whose keep-alives go unanswered (15 to 30 s), and answer them, so loading needn't fit inside it.
- [x] `cargo xtask oracle order`: run `Join` per seed and run; `Join` writes the order file from the recording with `jdk.jfr.consumer`.
- [x] Record the golden seeds twice each on every supported release; on 26.3, also at 2 processors and 50 further seeds. Narrowed from 50 seeds and 2 processors on every release: 26.2 and 26.4 matched 26.3 on the golden seeds, and the extra runs couldn't make the order stable.
- [x] `oracle replay`: replay each recorded order over the area, writing a feature file per run.
- [x] Check the replay against the existing runs: a chunk with nothing within 2 chunks decorated before it reproduces `tries`, and an order with its 8 neighbours first reproduces `after_neighbours` (26.3's 4 golden chunks that neighbours change).
- [x] `cargo xtask order-report`: order stability between runs, seeds, versions and processor counts; answers changed against `tries` and between runs.
- [x] Record the measurements in `docs/research/decoration-order.md`.
- [x] If the order is stable: prototype the model. Not needed: the order isn't stable (about 8% of neighbouring pairs flip between runs, and the spawn chunk moves).
- [x] ADR 0074: a third answer, "as decorated when the world is created", accepted or rejected, from the measurements ([ADR 0074](../../adr/0074-no-third-answer-for-the-order-at-creation.md): rejected).
- [x] Update [oracle.md](../../specs/oracle.md) for `Join`, `order`, the order files and `replay`; CLAUDE.md's command list for `order-report`.
- [x] Update the milestone: tick its first two exit criteria, and either list the implementation plan or, if rejected, strike the third criterion with a pointer to the ADR.

## Verification

- `oracle order` run twice on one seed: every chunk the server decorates appears once at `FEATURES` in each file.
- The replay reproduces `tries` and `after_neighbours` exactly for the orders that should give them (above), on the golden seeds, for every supported version.
- `order-report`'s pair counts checked by hand on one small recording.
- `cargo xtask check` passes, and `cargo xtask oracle-test` where the feature files changed format.

## Durable outputs

- [x] `docs/research/decoration-order.md`: what the game decorates at creation, by version; the measurements; the model's agreement, if built.
- [x] [oracle.md](../../specs/oracle.md): the `order` command and its files, and `replay`.
- [x] [ADR 0073](../../adr/0073-the-oracle-joins-a-stand-in-player.md): the oracle joins a stand-in player.
- [x] [ADR 0074](../../adr/0074-no-third-answer-for-the-order-at-creation.md), accepting or rejecting a third answer. If it supersedes or amends [ADR 0056](../../adr/0056-two-certainties-on-its-own-and-whatever-the-neighbours.md), that ADR's status says so.
- [x] [prior-art.md](../../research/prior-art.md): Horizon's decoration-order lead updated with what spyglass measured.

## Notes

Newest first.

- 2026-11-14: The measurement ([research](../../research/decoration-order.md#measurements)), 80 seeds over 26.2, 26.3 and 26.4-snapshot-8:
  - Startup is 9 chunks every time; the join decorates up to 729.
  - Two runs join in the same chunk only about 1 time in 3, and where they overlap, 7.9 to 9.4% of neighbouring pairs flip; 4.8% at 2 processors.
  - The order changes an answer from on its own in 94 of 125,125 chunks (1 in 1,330), nearly all dungeons and lava lakes; two runs differ in 34 of 84,224 shared chunks.

  The order isn't stable, so there's no model to build, and ADR 0074 rejects the third answer.

- 2026-11-14: A prototype of `Join` on 26.3, seed 1, two runs at view distance 10, 16 workers:
  - Startup decorated exactly the 3 × 3 around the spawn origin chunk (10, 10); the spawn search stopped at its first chunk.
  - The player joined at chunk (10, 10) in one run and (10, 9) in the other: the spawn task's position in the spawn radius differs between runs. The join decorated 641 and 729 chunks.
  - `FEATURES` events came from 15 worker threads, and none overlapped.
  - Over the 641 chunks both runs decorated, 237 of 2,412 neighbouring pairs (9.8%) were decorated in the opposite order, and 366 chunks (57%) had a different set of neighbours decorated before them.

  One seed only, but it already points away from a stable order. The full measurement still decides, and still has to show how many answers that changes.

- 2026-11-14: What the code says ([research](../../research/decoration-order.md#what-the-game-decorates-at-creation)), the same in 26.2, 26.3 and 26.4-snapshot-8:
  - Without a player, a new world decorates only the 3 × 3 around the spawn origin chunk and around each chunk of the spawn spiral it visits before finding a spawn. There's no spawn-area ticket.
  - The area the spawn searches ask about is decorated when the first player joins, by the spawn-search and player-loading tickets, out to the view distance.
  - A dimension's `FEATURES` steps run one at a time on its `worldgen` mailbox, so the order is total. Replaying overlapping pairs both ways isn't needed unless the recordings disagree.
  - `--jfrProfile` records from before the world loads, and every version names the step `features`.

  This settles the open question below the wrong way for the plan as written: startup alone covers too little to be worth a third answer. Measuring the join needs a player in the oracle's server.

- 2026-11-14: The plan stops at the ADR. Building the third answer is only worth planning once the measurement says it's worth it, which is how the milestone frames it.
- 2026-11-14: Open questions for the first task: whether the versions supported still prepare a spawn area at startup at all, and whether startup without a player decorates enough to answer the dungeon and loot searches near the spawn that motivate this. If startup decorates little and a player's join does most of it, the oracle needs a way to imitate the join, which isn't planned here; the research doc says what's missing and the ADR decides on what's measured.
