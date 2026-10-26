# Seed search

Status: stable. Implemented in `spyglass-search` and `spyglass`, and checked by tests (these are spyglass's semantics, not the game's). Covers the query language, how each constraint is evaluated and scored, seed sources and their order, and what makes results deterministic. The language is [ADR 0020](../adr/0020-search-query-language.md), with dimensions from [ADR 0024](../adr/0024-dimensions-in-the-search-language.md) ([below](#dimensions), a draft until it's implemented); the engine is [ADR 0021](../adr/0021-search-engine.md).

A query describes a world as a list of constraints. A **required** constraint must be met for a seed to be reported; a **preferred** one adds to the seed's score. A search evaluates seeds from a source in a fixed order and keeps the best ones.

These are spyglass's own semantics, not the game's: the game has no seed search. They're built on answers that are the game's: structure starts and their certainty ([structure-placement.md](structure-placement.md)), `/locate` positions ([locate.md](locate.md)), biomes and ties ([biome-lookup.md](biome-lookup.md)), the spawn origin ([spawn.md](spawn.md)) and slime chunks.

## The language

UTF-8 text, one constraint per line. Blank lines are ignored.

### Tokens

- Tokens are separated by spaces or tabs.
- A **comment** runs from a `#` to the end of the line when the `#` is the first non-blank character of the line, or is followed by a space, a tab or the end of the line. Otherwise `#` begins a structure tag.
- **Keywords** are lowercase: `require`, `prefer`, `weight`, `no`, `cluster`, `within`, `of`, `each`, `other`, `in`, `to`, `spawn`, `slime`, `biome`, `covers`, `at`, `y`, `step`, and the dimensions `overworld`, `nether` and `end`. A keyword is never read as an id; write such an id with its namespace (`minecraft:spawn`).
- An **integer** is decimal digits, with a leading `-` where a signed value is allowed (coordinates and `y`).
- A **point** is `X,Z`: two integers and a comma, no spaces.
- A **percentage** is an integer followed by `%`.
- An **id** is a resource location, `namespace:path` or just `path` (namespace `minecraft`). A **structure** is an id or a `#`-prefixed tag, as `/locate structure` takes them. A **biome** is an id.

### Grammar

```text
line        = statement
statement   = "require" [ dimension ] constraint
            | "prefer" [ dimension ] constraint [ "weight" INT ]
dimension   = "overworld" | "nether" | "end"

constraint  = [ count ] STRUCTURE region
            | "cluster" INT STRUCTURE "within" INT "of" "each" "other" place
            | [ count ] "slime" region
            | [ "no" ] "biome" BIOME place [ sampling ]
            | "biome" BIOME "covers" PERCENT place [ sampling ]
            | "spawn" point-place

count       = INT | "no"
region      = place | "within" INT "of" STRUCTURE place
place       = "within" INT "of" ( "spawn" | POINT ) | "in" POINT "to" POINT
point-place = "within" INT "of" POINT | "in" POINT "to" POINT
sampling    = [ "at" "y" INT ] [ "step" INT ]
```

A missing count means 1. Structure anchors (`within R of STRUCTURE place`) are allowed only in structure and slime counts, and don't nest.

### Limits

- Coordinates and cluster distances: magnitude at most 30,000,000 (the world border).
- Places: a box at most 65,536 blocks on a side, a radius at most 32,767 (so a disc's bounding box fits the same limit), including the radius around a structure anchor. Every count a query asks for is then over a bounded area.
- Counts: 1 to 10,000. Cluster sizes: 2 to 64. Weights: 1 to 1,000. Percentages: 1 to 100. `step`: 1 to 4,096.
- `y`: within the dimension's build height (−64 to 319 in the 26.3 Overworld).
- A biome constraint's grid, counted over its place's bounding box, has at most 1,048,576 points.
- A query has at least one constraint.

Anything else is an error naming the line and column.

### Canonical form

Every query has a canonical text, which parses back to the same query: one constraint per line in the original order, single spaces, no comments, ids with their namespace, the count only when it isn't 1, `at y` and `step` only when they aren't the defaults, `weight` only when it isn't 1, the dimension only when it isn't the [inferred](#dimensions) one, and a newline after each line. Front ends show this text where they echo a query (`--json`, the viewer's results).

### Example

```text
# A witch-hut farm near spawn, with a village not far off.
require 2 minecraft:swamp_hut within 600 of spawn
require cluster 2 minecraft:swamp_hut within 200 of each other within 600 of spawn
require no minecraft:pillager_outpost within 300 of spawn
prefer 3 #minecraft:village within 400 of spawn weight 2
prefer biome minecraft:cherry_grove within 500 of spawn
prefer biome minecraft:jungle covers 25% within 1000 of 0,0 step 32
prefer 6 slime within 64 of spawn
prefer #minecraft:village within 300 of minecraft:ancient_city within 3000 of spawn
prefer spawn within 200 of 0,0
prefer biome minecraft:plains within 0 of spawn
```

The last line asks for the spawn to be in plains: a disc of radius 0 holds only its centre.

## Dimensions

Draft: decided in [ADR 0024](../adr/0024-dimensions-in-the-search-language.md), not yet implemented.

Every constraint is in one dimension: the Overworld, the Nether or the End of the Default preset.

- **Inferred.** A structure or tag's dimension is the one whose biome source can place every structure it names (its set applies there, [structure-placement.md](structure-placement.md)). A biome's is the one whose biome source has it. Slime counts and `spawn` constraints are the Overworld's.
- **Explicit.** A dimension written after `require` or `prefer` sets it. It's required when inference fails (a tag whose structures are placed in more than one dimension), and an error when it contradicts the target ("fortresses aren't placed in the Overworld").
- **Overworld only.** Slime chunks and structures placed on concentric rings (strongholds) are errors in another dimension.

Points, boxes, `at y` and every position a constraint counts are in its dimension's coordinates.

**Anchors in other dimensions** are converted into the constraint's dimension as the game scales a teleport (`DimensionType.getTeleportationScale`): each coordinate `v` becomes `floor(v · from / to)` in `double`, where `from` and `to` are the two dimension types' `coordinate_scale` (1 for the Overworld and the End, 8 for the Nether, from the bundle). So:

- `spawn` in a Nether constraint is the Overworld spawn's position divided by 8, rounded down: the block its portal links to.
- A structure anchor in another dimension is converted the same way, from its own place, which is in its own dimension's coordinates.
- The End isn't linked to the other dimensions by coordinates. An anchor between the End and another dimension, and `spawn` in an End constraint, are errors.

For example:

```text
# A fortress at the spawn's portal, with a bastion near it.
require minecraft:fortress within 100 of spawn
prefer minecraft:bastion_remnant within 200 of minecraft:fortress within 1000 of spawn
```

Both lines are in the Nether, which fortresses and bastions infer, and `spawn` there is the Overworld spawn's portal position. In `require nether #minecraft:ruined_portal within 300 of spawn`, the ruined portal tag names Overworld and Nether portals, so `nether` picks the Nether's.

Every query written before dimensions existed infers the Overworld throughout, so it keeps its meaning and canonical text.

## Positions and regions

Each feature is a point, in blocks, horizontal only:

- A **structure start**: its `/locate` position, the chunk's minimum block plus the set's locate offset.
- A **slime chunk**: its centre, `(16x + 8, 16z + 8)`.
- The **spawn**: the spawn origin's climate position, or the origin chunk's centre when the dimension has no spawn targets (`SpawnOrigin::map_block`). The game's final spawn block is somewhere in the 11 × 11 chunks around the origin chunk ([spawn.md](spawn.md)).

**Distance** between points is `d = isqrt(dx² + dz²)`, the integer square root rounded down, with `dx` and `dz` computed in 64-bit integers.

A **place** is a set of points:

- `within R of A`: the points with `dx² + dz² ≤ R²` from `A` (the spawn, or a point).
- `in X1,Z1 to X2,Z2`: the box between the two corners, both included, whichever way round they're written.

A feature is in a place when its point is. With a **structure anchor**, `within R of S P`, a feature is in the region when it's within R of some start of `S` in place `P`, other than the same start (same set and chunk) as the feature itself.

## Outcomes

Some answers aren't known: candidate structure starts (most, unless [confirmed](#confirmed-mode)), tied biome cells, a platform-dependent spawn. So each constraint is evaluated twice, in two **cases**:

- **Best case**: everything uncertain goes the constraint's way.
- **Worst case**: everything uncertain goes against it.

Its **outcome** is **met** if it holds in the worst case, **possibly met** if it holds only in the best case, and **failed** if it fails in both.

What's certain:

- A structure start is certain when `StructureStart::certain` is true; otherwise it's a candidate. A pair of a start and a structure anchor is certain when both are.
- A biome sample **certainly matches** when its cell isn't tied and its biome is the target, and **possibly matches** when the target is its biome or among its tied biomes.
- Slime chunks are always certain.
- If the spawn origin is platform-dependent, every constraint that uses the spawn (as an anchor, or `spawn` itself) is possibly met, and scores 0 in the worst case.

Evaluating counts in each case:

| Form                    | Best case counts                   | Worst case counts                 |
| ----------------------- | ---------------------------------- | --------------------------------- |
| `N S region` (at least) | every start, certain or candidate  | certain starts                    |
| `no S region`           | certain starts                     | every start                       |
| `cluster N S …`         | groups among every start           | groups among certain starts       |
| `N slime region`        | slime chunks (both cases the same) | slime chunks                      |
| `biome B place`         | possibly matching samples          | certainly matching samples        |
| `no biome B place`      | certainly matching samples         | possibly matching samples         |
| `biome B covers P% …`   | possibly matching samples          | certainly matching samples        |
| `spawn place`           | the spawn                          | the spawn (none if platform-dep.) |

A constraint **holds** in a case when:

- **At least N**: the count is at least N.
- **`no`**: the count is 0.
- **Cluster**: there are N starts every two of which are at most D apart (by `d` above).
- **Biome presence**: at least one sample matches.
- **Coverage**: `100 × matching ≥ P × samples`.
- **Spawn**: the spawn is in the place.

### Biome sampling

Biomes are sampled at height `y` (default 64, as [render.md](render.md)) on a grid of spacing `step` (default 16):

- `within R of A`: the points `A + step·(i, j)` for integers `i`, `j`, inside the disc.
- `in X1,Z1 to X2,Z2`: the points `(min X + step·i, min Z + step·j)` inside the box.

Each sample is the biome of the quart containing block `(x, y, z)`, as `Session::biome_at`. A small biome between grid points can be missed; a smaller `step` costs more samples.

## Scores

Each preferred constraint has a **satisfaction** from 0 to 1,000, in the case the search scores: the best case by default, the worst case in **certain** mode. A seed's **score** is the sum of `weight × satisfaction` over its preferred constraints, in unsigned 64-bit integers. Required constraints don't add to the score.

Every division below rounds down.

- **At least N** (structures, slime chunks, biome presence, the spawn): the matching features in that case, nearest first, earn points each:
  - in `within R of …`: `500 + 500 × (R − d) / R`, with `d` the distance to the anchor (the nearest qualifying anchor start for structure anchors, the nearest matching sample for biomes); 1,000 when R is 0;
  - in a box: 1,000.

  The satisfaction is the sum over the first `min(count, N)` matches, divided by N. So meeting the count with features at the edge earns 500, and more and closer features earn more. Biome presence and the spawn have N = 1.

- **`no`**: 1,000 if it holds in that case, else 0.
- **Cluster**: `1000 × min(G, N) / N`, where G is the size of the largest group whose members are every two at most D apart.
- **Coverage**: `min(1000, 100000 × matching / (P × samples))`.

### Confirmed mode

`confirm` (in `SearchOptions`, and an argument of `Session::evaluate`) decides structure starts with each seed's terrain, as [locate](locate.md#confirmed-mode) does. The query language doesn't change. Confirmed, a start is a candidate only behind a tied biome cell, a platform-dependent ring position, or a mineshaft whose check spyglass can't place in a column that's partly in its biomes; ties, a platform-dependent spawn and those starts are what keeps a constraint possibly met.

Terrain only removes candidates or makes them certain, so it only narrows each constraint: its confirmed best case is no better than its unconfirmed one, and its confirmed worst case no worse, in what holds and in satisfaction (cluster sizes included, since the largest group of a subset is no larger). The fast path relies on this. Confirmed, it first checks the required constraints unconfirmed, in their best case; a seed that fails there fails confirmed, and its terrain is never bound. The rest runs confirmed, and gives the same answer as the plain confirmed evaluation.

## Verdict and ranking

A seed's **verdict** is failed if any required constraint failed, possibly met if any is possibly met and none failed, and met otherwise (including when there are no required constraints). A search reports a seed when its verdict is met, or possibly met outside certain mode.

Reported seeds are ranked by score, highest first, then by their index in the seed source, lowest first. This is a total order. A search keeps the first `top` seeds in it.

## Seed sources

A source is a finite or full sequence of seeds, each with an **index**:

- **All**: every 64-bit seed. Index `i` (0 to 2⁶⁴ − 1) is the seed whose lower 48 bits are `i >> 16` and whose upper 16 are `i & 0xFFFF`. The 65,536 seeds that share their lower 48 bits are consecutive, so a check that depends only on those bits ([structure-placement.md](structure-placement.md#the-lower-48-bits)) settles them together.
- **Range** `A..=B`: the seeds from A to B in order; index `i` is `A + i`.
- **List**: given seeds in the given order; index `i` is the `i`-th. Duplicates are evaluated each time.

A search starts at index `from` and runs until its **budget** is spent:

- **A seed count** `n`: exactly the indices `from` to `from + n − 1`, or to the end of the source.
- **A time limit**: until the time is up, then stops at the next point where every index below some `next` has been evaluated.
- Both: whichever comes first.

Every search reports `next`, the first index it didn't evaluate, or that the source is finished. Starting again from `next` continues where it stopped.

## Determinism

The same query, world, source, `from` and seed count give the same ranked list, with the same scores, on every run, platform and thread count. A time-limited search reports `next`; a seed-count search over `from` to `next − 1` reproduces its list. Running a range in two parts, then merging the two lists in the order above, gives the same top list as running it whole.

Nothing in the semantics depends on floating point: positions, distances and scores are integers, and the answers underneath are the game's, which spyglass computes exactly.

## Testing

- **Language**: every example in this spec parses; canonical text parses back to the same query; malformed input gives an error with a line and column, never a panic.
- **Evaluation**: hand-checked answers for each constraint form on fixture bundles, including candidates, tied cells, a platform-dependent spawn and certain mode.
- **The fast path**: for every seed in test ranges, the placement prefilter never rejects a seed the plain evaluation reports, and the fast check's verdict and score equal the plain evaluation's, confirmed or not. Confirming must narrow every result, as above.
- **Determinism**: equal results for 1, 2 and 8 threads, and for a range run whole or in two parts.
