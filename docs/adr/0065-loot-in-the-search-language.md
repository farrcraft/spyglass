# 0065. Loot in the search language

- Status: Accepted
- Date: 2026-11-09

## Context

Loot containers ([loot.md](../specs/loot.md), [locate.md](../specs/locate.md#loot)) are answers in the API, the CLI and the viewer, but not in either search. The milestone's question is a search: "an enchanted golden apple within 1,000 blocks of spawn". The language already counts structures and features ([ADR 0020](0020-search-query-language.md), [ADR 0057](0057-features-in-the-search-language.md)). What makes loot different:

- **Cost.** An item a dungeon can hold places every chunk with its neighbours' carved blocks, at about 15 ms a chunk on 16 threads (`containers_in` over 8 × 8 chunks takes 0.8 to 1.0 s in 26.3). A radius of 1,000 blocks is about 12,300 chunks, so a seed that fails takes minutes. An item only structures can hold places only their chunks.
- **What's counted.** A container holds an item or doesn't, and its contents may not be known: a candidate container, a seed that isn't known, a tied biome its table reads. `Session::nearest_container` already tells these apart: a container certainly holds an item when it and its contents are certain (`FoundContainer::contents_certain`) and it holds the item; otherwise it's a candidate if it holds the item or has no contents but a table that could hold it.
- **Items and enchantments.** An item is an id or an item tag, and a book is wanted for what's stored in it: `minecraft:enchanted_book` with `minecraft:mending`. `World::item_query` already takes both.
- **Dimensions.** Every dimension's containers have the same items to choose from, so the item doesn't say which dimension is meant, as a structure or a feature does. Which tables a dimension's containers can have is spread over its jigsaw templates and its piece generators' code.
- **A chest beside its chunk.** A dungeon's or a ruined portal's chest can be in a neighbour of the chunk that placed it.

## Decision

We will add loot to the language with a `loot` keyword: `[count] loot ITEM place` and `find loot ITEM place`, where `ITEM` is an item id or `#` and an item tag, optionally followed by `with ENCHANTMENT [LEVEL]`, as `spyglass loot find` takes them. `with` becomes a keyword.

- A count counts containers, not items.
- The best case counts every container `Session::containers_holding` answers: those that certainly hold the item and the candidates. The worst case counts those that certainly hold it. `no` is the other way round, as for structures and features.
- A container's position is its block (a chest minecart's rail); places are horizontal. A chunk is placed when it or a neighbour is in the place, since a chest can be beside the chunk that placed it.
- A loot place is at most 2,048 blocks on a side (a radius at most 1,023), so the milestone's question can be asked.
- Loot is the Overworld's unless a dimension is written. A dimension written must place containers.
- Loot constraints are checked last, after features. The fast path stops placing a required one as soon as it's decided, as for features. There are no loot clusters and no loot anchors.

The rules are in [search.md](../specs/search.md#outcomes).

## Consequences

- Every query written before keeps its meaning and canonical text. `loot` and `with` become keywords, so an id spelled either needs its namespace.
- The milestone's question can be written, and a seed with a match near the start of the scan is decided in seconds. Measured on 16 threads in 26.3, the apple within 1,000 blocks took about 53 s a seed, and each reported seed about 2.5 minutes more to evaluate in full for its matches. So a loot search is a search of tens of seeds, not millions; cheaper constraints checked before it are what make a search fast.
- Searching is no faster than `containers_holding` over the same chunks: an item dungeons can't hold places only the chunks container structures reach.
- A Nether item, such as a netherite upgrade in a bastion, needs `nether` written; without it the search looks in the Overworld, where no container can hold it, and finds none.
- Containers of unlikely dungeons aren't counted, as unlikely features aren't.

## Alternatives considered

- **The feature limit, 1,024 blocks on a side:** consistent with features, but the milestone's own question couldn't be written.
- **A limit by the item's tables:** 1,024 when a dungeon could hold the item, the usual 65,536 when only structures can. Most flexible, but the limit would depend on the bundle and be checked after parsing, so a query's validity would change between versions.
- **Inferring the dimension from the tables that could hold the item:** right for Nether-only items, but it needs every table a dimension's containers can have, which isn't listed anywhere spyglass reads: it's in jigsaw templates, processors and piece generators' code.
- **Counting items rather than containers** ("three diamonds"): closer to what a player wants for common items, but a stack count depends on how the game splits stacks, and a count of containers answers the milestone's question.
- **Nearest-first placing** for the fast path, in rings from the place's centre: when matches are spread evenly, the order chunks are placed in doesn't change how many are placed before the first match, and rows share the carved chunks of three rows as features do.
