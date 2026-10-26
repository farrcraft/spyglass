# 0025. Inferring dimensions in search queries

- Status: Proposed
- Date: 2026-10-26
- Supersedes: [0024](0024-dimensions-in-the-search-language.md)

## Context

[ADR 0024](0024-dimensions-in-the-search-language.md) gave each search constraint a dimension, inferred from its target and written (`overworld`, `nether`, `end`) only when the target doesn't decide. It inferred a structure or tag's dimension as "the one whose biome source can place all its structures", and promised that every existing query keeps its meaning.

Implementing it showed the two can't both hold. `#minecraft:ruined_portal` is a vanilla tag naming six Overworld portals and the Nether's. Under the "all" rule neither dimension places all of it, so `require #minecraft:ruined_portal within 300 of spawn`, a valid Overworld query since milestone 06, would become an error asking for a dimension. Tags are how users ask for families of structures, and any tag that mixes dimensions, vanilla or from a data pack, would break the same way.

Two more questions came up that 0024 left open:

- **Structure anchors** have no syntax for a dimension. "A bastion within 200 of a fortress" should anchor on the Nether's fortresses. But "a Nether ruined portal within 100 of a ruined portal" should anchor on Nether portals, which the anchor's own inference wouldn't pick.
- **One world.** `World::search_query` has only its own dimension's data. Inferring as a `Game` does needs every dimension's structure sets (0.35 to 0.7 s each to load for 26.3).

The forces are 0024's: existing queries keep their meaning, most Nether and End queries read without a dimension written, and nothing loads a dimension a query doesn't need more than inference requires.

## Decision

We will keep 0024's language, conversion and limits, and change how dimensions are inferred:

- **First that places it.** A structure or tag infers the first of the Overworld, the Nether and the End that places any of its structures: some structure it names has biomes in that dimension's source. A biome infers the first whose source has it. A target no dimension places is the Overworld's, where it's never found, as before. Slime chunks and the spawn are the Overworld's.
- **Written dimensions pick.** A written dimension must place some of the target, or it's an error. That's how a mixed tag's other dimension is reached: `require nether #minecraft:ruined_portal …`.
- **Anchors follow the constraint.** A structure anchor is in the constraint's dimension when that dimension places some of it, and otherwise in its own inferred dimension, converted by coordinate scale. Anchors between the End and another dimension stay errors.
- **A world resolves in its own dimension.** `World::search_query` puts every constraint in the world's dimension. A written dimension that differs, slime chunks or the spawn outside the Overworld, and anything that needs another dimension's data are errors that point to `Game::search_query`.

The rules are in [search.md](../specs/search.md#dimensions).

## Consequences

- Every query written before dimensions existed has the same meaning and canonical text in a `Game`: whatever the Overworld places infers the Overworld.
- Inference never fails, so there's no ambiguity error. The cost is a silent preference: a data pack that let fortresses generate in the Overworld would make `fortress` mean the Overworld's, and only `nether` reaches the others. The order is fixed, so the answer is predictable.
- Inferring a Nether target loads the Overworld's data to rule it out, and an End target loads all three. A `Game` keeps what it loads, so this happens once per game.
- A `World` and a `Game` can resolve the same text differently: a Nether world reads `#minecraft:ruined_portal` as the Nether's portals, a `Game` as the Overworld's. The canonical text is each one's own. Front ends that echo text should search through a `Game`.

## Alternatives considered

- **0024's "places all" rule:** the strictest reading, but it turns valid vanilla-tag queries into errors.
- **"Places all", falling back to the Overworld when it's ambiguous:** keeps old queries, but makes two rules where one does, and still errors for mixed tags that the Overworld doesn't place.
- **An error whenever more than one dimension places some:** honest, but it fails the same vanilla tags, and every query using them would need `overworld` written.
- **A dimension keyword on anchors:** fully explicit, but it adds syntax for a rare case that following the constraint's dimension already covers.
