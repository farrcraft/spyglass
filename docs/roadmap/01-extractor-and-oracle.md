# 01. Extractor and oracle harness

- Status: Planned
- Depends on: —
- Plans: [m01-extractor](../plans/m01-extractor.md), [m01-oracle](../plans/m01-oracle.md)

## Goal

Get trustworthy inputs and a trustworthy reference for one current game version, **26.3**, before writing any generation code.

## Scope

- `spyglass-extract` fetches the version manifest, downloads the server jar, verifies its SHA-1 and builds a version bundle from the jar's worldgen JSON (and data-generator reports if needed).
- Version bundle format defined in a spec.
- Oracle harness that loads a seed in the real game and dumps golden files (biome grids first; structure starts and heightmaps can follow in later milestones).
- Golden file format defined in a spec.

## Exit criteria

- [ ] The extractor produces a bundle for the chosen version from a clean machine, with nothing from Mojang committed.
- [ ] The oracle produces biome-grid golden files for a fixed seed set.
- [ ] [specs/data-sources.md](../specs/data-sources.md) and [specs/oracle.md](../specs/oracle.md) have their open questions resolved, and the bundle and golden-file formats each have a spec.
- [ ] ADRs record the bundle format and the oracle approach.

## Open questions

- ~~Which version is the first target?~~ **26.3**, the current release. Its jar is unobfuscated and needs Java 25.
- Fabric mod, or a standalone harness on the unobfuscated jar? Background is in [research/oracle-approaches.md](../research/oracle-approaches.md). A spike in [m01-oracle](../plans/m01-oracle.md) will answer it, and the result is recorded in an ADR.
