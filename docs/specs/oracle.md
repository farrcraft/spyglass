# Differential oracle

Status: draft.

Correctness is tested against the real game, not assumed.

## Design

A small server-side harness (likely a Fabric mod) loads a seed and calls the game's own functions directly. This is much faster than `/locate`. It dumps golden files:

- biome grids
- structure starts and bounds
- heightmaps

CI runs the Rust crates against these for a fixed set of seeds per supported version. Golden files are generated locally into `/oracle-out` (gitignored), since they are derived from the game.

## Open questions

- Golden file format.
- Seed set: how many, and which edge cases (negative seeds, seeds with interesting high bits).
- How CI gets a server jar without redistributing it.
