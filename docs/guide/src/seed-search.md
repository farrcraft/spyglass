# Seed search

`spyglass search` goes through seeds and keeps the ones that best fit what you ask for. You ask in a **query**. The viewer's **Search** tab takes the same queries.

## Queries

A query is one or more lines, and each line is a **constraint**: something you want in the seed.

- A `require` line is something a seed must have. Seeds without it are skipped.
- A `prefer` line is something you'd like. It adds to a seed's score, and the best-scoring seeds are kept. Add `weight N` to make it count N times as much.

```sh
spyglass search --time 2m -q 'require 2 minecraft:swamp_hut within 600 of spawn
require cluster 2 minecraft:swamp_hut within 200 of each other within 600 of spawn
prefer 3 #minecraft:village within 800 of spawn
prefer biome minecraft:cherry_grove within 500 of spawn weight 2'
```

Each constraint says **what** you want and **where**.

What you can ask for:

- a number of structures: `2 minecraft:swamp_hut`, or a group with a tag, `#minecraft:village`;
- features, such as `feature minecraft:monster_room` (a dungeon);
- a container holding an item: `loot minecraft:diamond`;
- slime chunks: `slime`;
- a biome: `biome minecraft:cherry_grove`, or a share of the place, `biome minecraft:cherry_grove covers 30%`;
- terrain: `height 60 to 90` for ground at those heights, or `flat 4` for ground that varies by no more than 4 blocks;
- a field of a flower: `flower minecraft:cornflower field 300` (see [Flower farms](flower-farms.md#finding-a-seed));
- none of something: `no minecraft:pillager_outpost`;
- several of a structure close together: `cluster 2 minecraft:swamp_hut within 200 of each other`.

Where:

- within a distance of the spawn or a point: `within 600 of spawn`, `within 300 of 1000,-2000`;
- in a rectangle: `in -500,-500 to 500,500`;
- for structures and slime chunks, near a structure: `within 200 of minecraft:fortress within 1000 of spawn`.

The [search language's specification](https://github.com/farrcraft/spyglass/blob/main/docs/specs/search.md) has every form. A long query is easier to keep in a file, passed with `--query-file huts.txt`.

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

A field of 300 blocks where bone meal always grows cornflowers, near the spawn, at a height from 64 to 96:

```text
require flower minecraft:cornflower field 300 within 1000 of spawn at y 64 to 96 step 8
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
| `--count <n>`, `--time <t>`     | none       | Stop after this many seeds, or this long (`90s`, `10m`, `2h`). Without either, a search of every seed runs until Ctrl-C.          |
| `--range <a..b>`, `--seeds <f>` | every seed | Only seeds from a to b (both included), or those listed in a file. `--range -2147483648..2147483647` is every seed text can make. |
| `--from <index>`                | `0`        | Where to start; a search prints the index to continue from.                                                                       |
| `--top <n>`                     | `10`       | How many seeds to keep.                                                                                                           |
| `--confirm`                     | off        | Decide structure starts with each seed's terrain, as the game does.                                                               |
| `--certain`                     | off        | Only seeds whose required constraints hold whatever is left undecided.                                                            |
| `--threads <n>`                 | all cores  | Worker threads.                                                                                                                   |

The pack and bundle options work as for [`locate`](command.md#locate). `--json` prints the same seeds, with everything each constraint matched. The same query, range, start and count always give the same list, on any computer.

When a search stops, at `--count`, `--time` or Ctrl-C, it first finishes the seeds it's checking. Then it looks at the best seeds again, to list what each constraint matched. For structures and biomes that's instant. For features and loot, each seed takes as long again as it did the first time, so a search can run well past its `--time`: the loot example above, with `--time 20s`, printed after 16 minutes on 16 threads. Press Ctrl-C a second time to skip that step.

## What a search costs

- **Structures** are fast. Where structures can go depends on only part of the seed, so spyglass can rule out 65,536 seeds at a time when they can't have enough of them.
- **Confirming** with terrain (`--confirm`) adds little, since it runs only on seeds that pass everything else. Without it, most structure constraints are only **possibly** met: the structures are where the game tries to build them, but whether each one generates depends on the ground there.
- **Features** are slow: spyglass generates each seed's chunks one by one, about a seed a second for dungeons. A feature constraint's area can be at most 1,024 blocks across.
- **Loot** is slower still. spyglass generates every chunk in the area until it finds the item: about a minute a seed within 1,000 blocks. A loot constraint's area can be at most 2,048 blocks across.

spyglass checks the cheapest constraints first, and they rule seeds out before any chunk is generated. So where you can, add a structure or biome constraint alongside a feature or loot one. Use `--range` and `--count` to keep a slow search small.

Features and containers that are only likely count as **possibly** met. `--certain` keeps only seeds whose answers are certain ([what that means](answers.md#certain-and-candidate)).
