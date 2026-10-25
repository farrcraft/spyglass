# Differential oracle

Status: draft.

Correctness is tested against the real game, not assumed.

## Design

A standalone Java harness compiled against the user's own server jar calls the game's own functions directly, following the same code paths the server uses. This is much faster than `/locate`. The approach is decided in [ADR 0004](../adr/0004-oracle-standalone-harness.md). It dumps golden files:

- biome grids
- structure starts and bounds
- heightmaps

CI runs the Rust crates against these for a fixed set of seeds per supported version. Golden files are generated locally into `/oracle-out` (gitignored), since they are derived from the game.

## Comparing biomes: exact, except ties

The game picks a biome by finding the parameter point with the minimum `Climate.ParameterPoint.fitness`, an integer distance. When two or more points tie at the minimum, the answer depends on lookup history: `Climate.RTree` starts from each thread's previous result and only replaces it with something strictly closer. Chunks generate on worker threads in no fixed order, so **the game can give a tied cell different biomes in different runs.** Measured in 26.3, about 1 cell in 2,000 is tied, and in two real worlds every disagreement with the harness (10,412 of 27.6 million cells) was a tie.

So for biomes:

- Golden data records, for each cell, the biome when the minimum is unique, or **the full set of tied biomes** when it isn't.
- spyglass passes a cell if it matches exactly where the minimum is unique, or returns any member of the tied set where it isn't.
- spyglass should be able to report a cell as tied, so users can see where the game itself is unpredictable.

## Independent check: real world saves

Every chunk in a save that has reached the biomes generation step stores the biomes the game generated, at quart resolution. Comparing the harness against saves made in the target version, with the Default preset and no worldgen datapacks, checks the harness independently of itself. Ties are allowed, as above.

## Open questions

- Golden file format.
- Seed set: how many, and which edge cases (negative seeds, seeds with interesting high bits).
- How CI gets a server jar without redistributing it.
