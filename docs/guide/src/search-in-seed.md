# Searching within a seed

`spyglass find` searches one seed, or one of your worlds with `--world`, for places that fit a query, and lists them nearest first. The viewer's **Search** tab does the same in its **This seed** mode, with presets to start from.

The query's first line starts with `find` and names what to look for. The lines after it are [constraints](seed-search.md#queries), checked around each place found. In them, `here` means the place being checked:

```sh
spyglass find --seed 12345 -q 'find minecraft:pillager_outpost within 4096
require flat 4 within 48 of here step 4'
spyglass find --world "My World" --from player -q 'find biome minecraft:mushroom_fields within 4096 step 32'
```

The first finds pillager outposts within 4,096 blocks that have flat ground around them. The second finds mushroom fields near your player, checking every 32 blocks.

## What to find

| After `find`             | Each place is                                       |
| ------------------------ | --------------------------------------------------- |
| a structure id or `#tag` | a structure                                         |
| `feature F`              | a feature, such as a dungeon                        |
| `loot I`                 | a container holding an item, or an item in a `#tag` |
| `biome B`                | a spot in the biome, checked on a grid              |
| `point`                  | every spot on a grid, judged by what's around it    |

`within R` on the `find` line searches around `--from` (`0,0` by default), and `step` sets how far apart the grid's spots are.

Biome tags such as `#minecraft:is_ocean`, and the terrain conditions `height A to B` and `flat D`, work here as in [seed searches](seed-search.md). Terrain conditions look at the ground as the game first shapes it, before structures and features change it.

## Options

Matches are listed nearest first, up to `--top` (20 by default). `--confirm`, `--certain`, `--threads`, `--time` and `--json` work as for [`search`](seed-search.md#options), and the seed, world, pack and bundle options as for [`locate`](command.md#locate). With `--from player`, your player's position is converted into the dimension of what you're finding.
