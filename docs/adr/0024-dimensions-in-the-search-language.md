# 0024. Dimensions in the search language

- Status: Accepted
- Date: 2026-10-26

## Context

The search language ([ADR 0020](0020-search-query-language.md), [search.md](../specs/search.md)) describes one dimension's world: every count, place and anchor is in Overworld coordinates, and `spawn` is the Overworld's spawn origin. Milestone 08 adds the Nether and the End, and the queries users want cross them. Typical examples:

- "a fortress within 100 Nether blocks of the spawn's portal";
- "a bastion and a fortress close together";
- "an end city near the main island".

The forces:

- **Most targets belong to one dimension.** Fortresses, bastions and nether fossils are the Nether's; end cities the End's; each biome id is one dimension's. Making users say what's already implied adds noise and a new way to be wrong.
- **Some targets don't decide.** `spawn` and points have no dimension, and a tag can name structures in more than one (`#minecraft:ruined_portal` holds the Nether's portal and the Overworld's).
- **Coordinates differ by dimension.** The game links the Overworld and the Nether by the dimension types' `coordinate_scale` (1 and 8): a portal at Overworld `(x, z)` leads to Nether `(x / 8, z / 8)`. The End isn't linked by coordinates: its portals lead to a fixed platform.
- **Existing queries must keep their meaning.** Every 26.3 query written so far is Overworld-only.

## Decision

We will give each constraint a dimension, inferred from what it counts, and convert positions between linked dimensions by their coordinate scales.

- **Inferred.** A structure or tag's dimension is the one whose biome source can place all its structures. A biome's is the one whose source has it. Slime chunks and `spawn` constraints are the Overworld's.
- **Explicit when needed.** `overworld`, `nether` and `end` are new keywords, written after `require`/`prefer`. They're required when the target doesn't decide (a tag spanning dimensions), and an error when they contradict the target. The canonical text writes the dimension only when it isn't inferred.
- **Positions in the constraint's dimension.** Points and boxes are in the constraint's dimension's coordinates.
- **Converted anchors.** `spawn` is the Overworld spawn's position converted into the constraint's dimension. A structure anchor in another dimension is converted the same way. Conversion is the game's teleport scaling: multiply by `from.coordinate_scale / to.coordinate_scale` in `double` and round down, as `BlockPos.containing` does. So in the Nether, `spawn` is the Overworld spawn divided by 8, rounded down.
- **The End stands alone.** Anchors between the End and another dimension, and `spawn` in an End constraint, are errors.
- **Overworld-only features stay there.** Slime chunks and concentric-ring structures (strongholds) are errors in other dimensions.

The grammar and meaning are in [search.md](../specs/search.md). This extends ADR 0020's language; its decisions stand.

## Consequences

- Every existing query parses to the same tree and has the same canonical text: all its targets infer the Overworld.
- Most Nether and End queries read as they would in English, with no dimension written.
- Inference depends on the bundle: a datapack that let fortresses generate in the Overworld would make `fortress` ambiguous and need `nether`. Errors say so and name the choices.
- `overworld`, `nether` and `end` become keywords, so ids with those paths need their namespace (`minecraft:end`), as other keywords already do.
- Evaluation needs one session per dimension per seed. The engine binds a dimension only when a constraint needs it, and the prefilter runs on every dimension's structure placement, which depends only on the seed's lower 48 bits in all three ([ADR 0021](0021-search-engine.md)).
- Rounding down when converting means a Nether radius around `spawn` is measured from one block, not the eight Overworld blocks that map to it. That's how portals are placed, so it's the useful answer, but the spec has to say it.

## Alternatives considered

- **A dimension on every constraint:** simplest to specify, but most queries would repeat what their targets imply, and every existing query's canonical text would change.
- **A dimension per query, with cross-dimension anchors only:** can't express "a fortress near spawn and a village near spawn" in one query, which is the commonest cross-dimension wish.
- **Prefixed targets (`nether:fortress`) instead of keywords:** collides with namespaced ids (`minecraft:fortress`), which the language already uses.
- **Exact conversion (Overworld anchors as 8 × 8 Nether areas):** truer to the geometry, but no portal or player position works that way, and it would make distances fractional.
