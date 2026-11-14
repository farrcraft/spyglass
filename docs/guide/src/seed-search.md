# Seed search

`spyglass search` finds the seeds that best fit a query. The viewer's **Search** tab takes the same queries.

## Queries

Each line of a query is a constraint:

- `require` constraints are ones a seed must meet;
- `prefer` constraints add to a seed's score, and `weight N` makes one count N times.

```sh
spyglass search --time 2m -q 'require 2 minecraft:swamp_hut within 600 of spawn
require cluster 2 minecraft:swamp_hut within 200 of each other within 600 of spawn
prefer 3 #minecraft:village within 800 of spawn
prefer biome minecraft:cherry_grove within 500 of spawn weight 2'
```

A constraint counts structures (ids or `#tags`), features, loot or slime chunks in a place, or asks for a biome, the spawn or terrain (`height A to B`, `flat D`) there. `no` asks for none, and `biome B covers 30%` for a share of the place. The place is:

- a distance from the spawn or a point: `within 600 of spawn`, `within 300 of 1000,-2000`;
- a rectangle: `in -500,-500 to 500,500`;
- for structures and slime chunks, a distance from a structure: `within 200 of minecraft:fortress within 1000 of spawn`;
- for a `cluster`, a distance of each other. The [search language's specification](https://github.com/farrcraft/spyglass/blob/main/docs/specs/search.md) has every form. A long query is easier to keep in a file: `--query-file huts.txt`.

## Examples

Two witch huts close together near the spawn, for a farm:

```text
require cluster 2 minecraft:swamp_hut within 200 of each other within 600 of spawn
```

A village and a cherry grove, both near the spawn:

```text
require #minecraft:village within 500 of spawn
prefer biome minecraft:cherry_grove within 500 of spawn
```

A fortress near the spawn's portal, with a bastion beside it:

```text
require minecraft:fortress within 100 of spawn
prefer minecraft:bastion_remnant within 200 of minecraft:fortress within 1000 of spawn
```

Two dungeons close enough for one spawner farm:

```text
require cluster 2 feature minecraft:monster_room within 16 of each other within 96 of spawn
```

An enchanted golden apple in a chest near the spawn:

```text
require loot minecraft:enchanted_golden_apple within 1000 of spawn
```

## Dimensions

Each constraint is in one dimension, worked out from what it counts: fortresses and bastions are the Nether's, end cities the End's, villages the Overworld's. Loot is the Overworld's unless you say otherwise.

In a Nether constraint, `spawn` is the block the Overworld spawn's portal leads to (its position divided by 8), and a structure in another dimension is converted the same way. To choose the dimension for a structure tag that spans more than one, write `nether`, `end` or `overworld` after `require` or `prefer`:

```text
require nether #minecraft:ruined_portal within 300 of spawn
```

## Options

| Option                          | Default    | What it does                                                                                                                      |
| ------------------------------- | ---------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `-q <text>` or `--query-file`   | required   | The query.                                                                                                                        |
| `--count <n>`, `--time <t>`     | none       | Stop after this many seeds, or this long (`90s`, `10m`, `2h`). Without either, Ctrl-C stops it and prints the best so far.        |
| `--range <a..b>`, `--seeds <f>` | every seed | Only seeds from a to b (both included), or those listed in a file. `--range -2147483648..2147483647` is every seed text can make. |
| `--from <index>`                | `0`        | Where to start; a search prints the index to continue from.                                                                       |
| `--top <n>`                     | `10`       | How many seeds to keep.                                                                                                           |
| `--confirm`                     | off        | Decide structure starts with each seed's terrain, as the game does.                                                               |
| `--certain`                     | off        | Only seeds whose required constraints hold whatever is left undecided.                                                            |
| `--threads <n>`                 | all cores  | Worker threads.                                                                                                                   |

The bundle options are as for [`locate`](command.md#locate). `--json` prints every hit, with what each constraint matched. The same query, range, start and count always give the same list, on any machine.

A search stops at `--count`, `--time` or Ctrl-C once the seeds being checked are done, and then evaluates the best seeds again for what each constraint matched. For structures and biomes that's instant; for features and loot each seed takes as long again as it did to check, so `--time` is a lower bound: the loot query above with `--time 20s` printed after 16 minutes on 16 threads. A second Ctrl-C skips evaluating the best seeds.

## What a search costs

- **Structures** are fast. Searching every seed checks structure placement first, which depends only on part of the seed, so seeds that can't place enough structures are skipped 65,536 at a time.
- **Confirming** with terrain costs little on top, since it runs only on seeds that pass everything else. Without `--confirm`, a constraint on most structures is usually **possibly** met rather than met: they're where the game tries them, and whether each one generates depends on the ground there.
- **Features** are slow. Each seed's dungeons are placed chunk by chunk, about a seed a second, and a feature's place is at most 1,024 blocks on a side.
- **Loot** is slower still. Each seed places every chunk of the place until it finds the item: about a minute a seed for a radius of 1,000. A loot place is at most 2,048 blocks on a side.

Cheaper constraints in a query are checked first, and rule seeds out before any chunk is placed, so put a structure or biome constraint beside a feature or loot one where you can. Use `--range` and `--count` to keep a slow search small.

Likely features and containers count only towards **possibly** met. `--certain` keeps only seeds whose answers are certain ([what that means](answers.md#certain-and-candidate)).
