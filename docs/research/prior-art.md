# Prior art

## Seed-map tools

- [cubiomes](https://github.com/cubitect/cubiomes) / [Cubiomes Viewer](https://github.com/cubitect/cubiomes-viewer): the standard open-source reference implementation for algorithms and features. Per-version tables are hand-ported, so support for new releases lags. That gap is spyglass's main reason to exist.
- **Chunkbase:** the most widely used web tool, closed-source. Useful as a feature checklist only.
- [slime_seed_finder](https://github.com/Badel2/slime_seed_finder): Rust seed finder with a WebAssembly biome viewer. Reference for search architecture and the 48→64-bit seed-extension trick.
- **Amidst:** older Java map viewer that ran against the user's own jar. Precedent for spyglass's local-extraction licensing approach.

## Rust Minecraft projects

- [Pumpkin](https://github.com/Pumpkin-MC/Pumpkin): Rust server whose `pumpkin-world` reportedly ports vanilla's noise router and density functions. Assess its parity and whether parts can be reused before writing terrain from scratch.
- [Azalea](https://github.com/azalea-rs/azalea) / [Valence](https://github.com/valence-rs/valence): templates for generating per-version code and data from the game.
- `fastanvil`: reads region files, which is useful for comparing against real saves.
