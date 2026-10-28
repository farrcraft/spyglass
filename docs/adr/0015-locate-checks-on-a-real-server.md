# 0015. Check `/locate` answers on a real dedicated server

- Status: Accepted
- Date: 2026-10-25
- See also: [0033](0033-the-oracle-accepts-the-eula-only-when-asked.md), which makes the server's EULA an explicit opt-in

## Context

[ADR 0004](0004-oracle-standalone-harness.md) chose a standalone Java harness that calls the game's functions directly, without a server or a level. That covers biomes, placement, the structure biome check and the spawn origin.

`/locate structure` doesn't fit it. `ChunkGenerator.findNearestMapStructure` takes a `ServerLevel`: it decides whether a structure is "generated" by generating chunks to the structure-starts status through the level's chunk source and `StructureManager`. That includes terrain-dependent validity, which spyglass can't compute until milestone 07. Reproducing it in the harness would mean building a level, or copying the search loop and its chunk loading, which tests a copy rather than the game. The milestone's exit criterion is agreement with the game's `/locate`.

## Decision

We will spot-check locate answers with the game's own dedicated server: `cargo xtask oracle locate` starts the server from the same jar, on a throwaway world per seed, and sends `execute positioned x y z run locate ...` for a fixed case list on its console. Answers are written to `oracle-out/<version>/locate/` and compared by an ignored Rust test. The standalone harness stays the oracle for everything else.

## Consequences

- The comparison is against real `/locate`, terrain validity included, so it also measures how often a candidate spyglass can't decide is the game's answer.
- It's slow (a server start and world creation per seed, minutes in all), so it isn't part of `cargo xtask oracle golden`, and the case list stays small.
- Answers are parsed from the server's log text, which can change between versions; a format change fails loudly rather than silently.
- Worlds, logs and answers stay local, like other oracle output ([ADR 0008](0008-game-derived-test-values-stay-local.md)).

## Alternatives considered

- **Build a `ServerLevel` in the harness:** most of a server's startup without its stability; fragile across versions.
- **Copy the search loop into the harness:** fast, but tests a copy of the code under test, and still needs chunk generation to decide "generated".
- **Check by hand in the game:** not repeatable.
