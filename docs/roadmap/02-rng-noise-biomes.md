# 02. RNG, noise and biomes

- Status: In progress
- Depends on: [01](completed/01-extractor-and-oracle.md)
- Plans: [m02-rng-noise](../plans/completed/m02-rng-noise.md), [m02-density](../plans/m02-density.md), [m02-biomes](../plans/m02-biomes.md)

## Goal

Overworld biome lookup that matches the game bit for bit.

## Scope

- `spyglass-rng`: Java LCG, Xoroshiro128++, positional random factories, MD5 name seeding.
- `spyglass-noise`: Perlin/improved, octave and normal noise.
- `spyglass-density`: enough of the density-function graph interpreter to evaluate the climate parameters.
- `spyglass-biome`: Overworld climate sampler and nearest-biome lookup at quart (1:4) resolution.

## Exit criteria

- [ ] Biome grids match the oracle's golden files exactly for the full seed set, including negative seeds and coordinates.
- [ ] RNG and noise have unit tests with values taken from the game.
- [ ] Specs exist for the RNGs, noise and biome lookup.

## Not in this milestone

- Nether and End biomes ([08](08-dimensions-and-versions.md)).
- Performance work beyond what's needed to run the tests in reasonable time.
