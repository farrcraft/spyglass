# Roadmap

## Milestones

1. Extractor + oracle harness for one current version.
2. RNG, noise and biomes, bit-exact against the oracle.
3. Structure placement, slime chunks, spawn point.
4. Viewer MVP.
5. Seed search with a filter language.
6. Terrain heights, structure validity, jigsaw.
7. Nether and End, then multiple versions.

## Stretch

- GPU (`wgpu` compute) structure-placement search over the lower 48 bits of the seed, with CPU expansion of the top 16 bits for biome checks.

## Out of scope for now

- **Bedrock Edition:** closed-source, with a different RNG and placement.
- **Pre-1.18 versions:** a different biome system.
