# 0047. Search within a seed with `find` queries

- Status: Accepted
- Date: 2026-10-29

## Context

The seed search ([ADR 0020](0020-search-query-language.md), [ADR 0021](0021-search-engine.md)) judges whole seeds: constraints around fixed anchors (the spawn, a point, a structure's starts), over many seeds, ranked by score. Players also ask the opposite question about one seed they already have: "where in this world are two witch huts close enough to share an AFK spot?", "which ocean monument is surrounded only by deep ocean?", "where's a pillager outpost on flat ground?", "where's the nearest mushroom island?". Chunkbase offers this as its advanced search. It's the main gap in spyglass's seed search that its users would notice.

These questions are the seed search's constraints, asked around each candidate place instead of around the spawn. Some need things the language lacks:

- **Biome groups.** "Deep ocean" is four biome ids. The game's own monument check uses the tag `#minecraft:required_ocean_monument_surrounding`.
- **Terrain.** "Flat" and "at this height" need heights. The library computes `WORLD_SURFACE_WG` heights ([terrain.md](../specs/terrain.md)), but at a few milliseconds per chunk, far more than a biome lookup.

The format is one users keep in files, so additions must keep every existing query's meaning and canonical text. A second evaluator would have to be kept in step with the first, and the seed search's tests check only the first.

## Decision

We will extend the query language with a `find` line and the anchor `here`, and judge a `find` query in one seed by evaluating its constraints once per target, with `here` set to that target, using the seed search's evaluator.

- **`find TARGET [place] [sampling] [apart D]`**, at most one, before every constraint.
  - **Targets:** each start of a structure or `#tag`; `biome B` (each grid sample whose biome matches); or `point` (each grid point).
  - **The place** is as in constraints. `within R` with no `of` is around an origin the caller gives (the command's `--from`, the viewer's view centre), so saved queries and presets carry no coordinates.
  - **`apart D`** skips a match within D blocks of an earlier one. It defaults to 0 for structures and to 256 for biome and point targets.
- **`here`** is an anchor allowed only in a `find` query: the target's position. Like a structure anchor, a structure target's own start is never counted by a constraint on the same structure around `here`.
- **Order:** matches are the targets whose verdict a seed search would report (met, or possibly met outside certain mode). They're reported nearest first from the place's centre, ties by z then x, up to a `top` count. The score and each constraint's result are shown, but don't reorder them.
- **Biome tags** (`#namespace:path`) are allowed wherever a biome is, in both kinds of search. A sample possibly matches if any of its tied biomes is in the tag.
- **Terrain conditions** are allowed in both kinds of search, over `WORLD_SURFACE_WG` heights on a grid (`step`, default 16):
  - `height A to B place` holds when every sample is in `A..=B`.
  - `flat D place` holds when the highest and lowest samples differ by at most D.
  - Heights are certain, so these are met or failed. They're errors in a dimension with a ceiling.
- **Separate kinds:** a `find` query in the seed search, and `here` without `find`, are errors.

The double-hut preset asks for another hut within 200 blocks of `here`. A witch spawns only inside its hut and needs the player within 128 blocks (a sphere). A hut's floor spans about 7 × 9 blocks from its `/locate` position, so two huts whose positions are up to about 230 apart can share a point within 128 of both, at hut height. 200 leaves room for the height between the huts' floors and the player.

The full semantics go in [specs/search.md](../specs/search.md).

## Consequences

- **One evaluator.** Both searches share it, so everything the seed search's tests and goldens check holds for `find`. Biome tags and terrain conditions arrive in the seed search at the same time.
- **Plain text.** A `find` query is still text, and it round-trips through the canonical form. The viewer's editor, error line and echo work unchanged.
- **Cost per target.** Each target is a full evaluation. A wide `find` over a common structure (mineshafts are in about one chunk in 250) can mean many thousands of evaluations. `check`'s cheapest-first ordering and the target limits bound this, but a large area with terrain conditions is slow, and the docs must say so.
- **Terrain is expensive.** The terrain conditions' sample limit is far lower than biomes', and the cost order evaluates them last. Even so, a seed search with a terrain condition is much slower per seed than one without.
- **Some ids now error.** `#` before a biome id was an error before, and is now a tag: a query that used it as an id fails to resolve.
- **Pre-generation heights.** The heights are the generator's, before structures, carvers and features, as structures see them, so "flat" means flat terrain, not flat after decoration. The spec has to say so.
- **More to maintain.** The language grows by five forms, each with limits, errors, canonical text and fuzz cases.

## Alternatives considered

- **A separate language or file format for within-seed search:** clearer separation, but a second parser, a second evaluator to keep exactly in step with the first, and two ways to say "a hut within 200 blocks".
- **A mode flag on `spyglass search`** (`--in-seed`), with the first constraint's structure as the target: no new syntax, but the target would be implicit, and the same text would mean different things depending on a flag. Saved queries couldn't say what they are.
- **Ranking matches by score, as the seed search does:** good for "the best site", but it can't stream nearest first, and it can't stop early with a meaningful prefix. Nearest first is what players step through on a map. Ranking by score can come later as an option over the same matches.
- **Biome lists (`biome a|b|c`) instead of tags:** no data needed, but long to type, and the game already names the groups that matter (`is_deep_ocean`, `is_forest`). Tags also follow version changes without edits.
- **Final-surface heights for terrain conditions:** what a player sees, but they need surface rules and features (milestones 18 and 19 on the [roadmap](../roadmap/README.md)). Pre-generation heights are available now, exact, and checked against the game.
