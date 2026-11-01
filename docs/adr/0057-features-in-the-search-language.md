# 0057. Features in the search language

- Status: Accepted
- Date: 2026-10-31

## Context

Features ([features.md](../specs/features.md)) are answers in the API and on the map, but not yet in either search. Players ask for "two dungeons close enough for one spawner farm, near the spawn", and the milestone asks for exactly that search. The language ([ADR 0020](0020-search-query-language.md), [ADR 0047](0047-search-within-a-seed.md)) already has counts and clusters of structures. What makes features harder:

- **Cost.** Placing takes 25 to 30 ms a chunk, with its neighbours' carved blocks, where a structure start costs microseconds. A radius of 100 blocks is about 150 chunks.
- **Three kinds of answer.** A feature is certain, a likely candidate (it placed on spyglass's blocks, but something could change that), or an unlikely one (it declined on spyglass's blocks). Unlikely candidates are nearly every try: 1,531 of 1,541 dungeon answers in two of 26.3's golden squares.
- **Height.** Structure starts have only x and z. Features have a y, and for a farm it matters: two dungeons 10 blocks apart across can be 70 apart vertically.
- **Ids.** A feature id isn't a structure id, and `minecraft:desert_well` sounds like either.

## Decision

We will add features to the language with a `feature` keyword, as `biome` is: `[count] feature F place`, `cluster N feature F within D of each other place`, and `find feature F place`. F is a placed or configured feature id, as `spyglass locate feature` takes it. Its dimension is inferred as for structures.

- Counts and clusters use certain features in the worst case and certain and likely ones in the best. Unlikely candidates aren't counted in either case. Features don't change when the search confirms with terrain.
- A cluster's distance between features is three-dimensional. Places stay horizontal discs and boxes.
- A feature place is at most 1,024 blocks on a side.
- Feature constraints are checked last. The fast path stops placing a required one as soon as it's decided. Tries aren't filtered in pairs before placing.

The rules are in [search.md](../specs/search.md#outcomes).

## Consequences

- Every query written before keeps its meaning and canonical text. `feature` becomes a keyword, so an id spelled `feature` needs its namespace.
- "Two dungeons within 16 blocks" means within 16 blocks in 3D. That's much rarer than in 2D: 1 of 64 seeds in 26.3 against 9 of 64.
- Leaving unlikely candidates out narrows the best case, so the search can miss a pair the game has that spyglass's blocks don't show. That's the same trade the viewer and `locate feature` make by default.
- The search is slow: seconds a seed for a disc of 100 blocks, so a few seeds a second on 16 cores. It's fine for a search of hundreds of seeds, but not of millions.
- Clusters and the seed search share the grouping code, which now takes 2D or 3D points.

## Alternatives considered

- **Feature ids without a keyword**, told apart from structures by the registry: one fewer word, but a query's meaning would depend on the bundle, and an id can't be resolved in the parser.
- **Horizontal cluster distance**, as for structures: consistent, but it reports pairs no farm can use. Most of the 2D pairs near the spawn were tens of blocks apart vertically.
- **Counting unlikely candidates in the best case:** strictly "everything uncertain goes its way", but nearly every seed would be possibly met, and every chunk would have to be placed, with no ruling out.
- **Filtering pairs of tries before placing**, as the plan first proposed: after a dungeon's first try that the terrain doesn't rule out, its later tries' positions depend on what it drew, so a kept chunk could hold a pair anywhere. Tries rule out no more than ruling out whole chunks already does.
- **Feature anchors** (`within R of feature F`): not needed for the pair search, and each anchor would place its own place. Left for later.
