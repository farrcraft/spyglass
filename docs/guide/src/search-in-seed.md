# Searching within a seed

`spyglass find` searches one seed, or a world save with `--world`, for places that fit a query, nearest first. The viewer's **Search** tab does the same in its **This seed** mode, with presets to start from.

A query starts with a `find` line naming what to look for. The constraints after it are judged around each place found, which they call `here`:

```sh
spyglass find --seed 12345 -q 'find minecraft:pillager_outpost within 4096
require flat 4 within 48 of here step 4'
spyglass find --world "My World" --from player -q 'find biome minecraft:mushroom_fields within 4096 step 32'
```

The first finds pillager outposts within 4,096 blocks with flat ground around them. The second finds mushroom fields near your player, checking every 32 blocks.

## What to find

| Target                   | Each place is                              |
| ------------------------ | ------------------------------------------ |
| a structure id or `#tag` | a structure start                          |
| `feature F`              | a feature, such as a dungeon               |
| `loot I`                 | a container holding an item or `#tag`      |
| `biome B`                | a sample of the biome, on a grid           |
| `point`                  | a grid point, to judge by its surroundings |

`within R` alone is around `--from` (default `0,0`), and `step` sets the grid's spacing.

Biome tags such as `#minecraft:is_ocean`, and the terrain conditions `height A to B` and `flat D`, work here as in [seed searches](seed-search.md). Terrain conditions read the ground before structures and features change it.

## Options

Matches come nearest first, up to `--top` (default 20). `--confirm`, `--certain`, `--threads`, `--time` and `--json` are as for [`search`](seed-search.md#options), and the seed, world and bundle options as for [`locate`](command.md#locate).
