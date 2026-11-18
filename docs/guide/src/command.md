# The `spyglass` command

`spyglass` answers questions about a seed from a terminal: where things are, what's in the chests, maps, and searches. Every command has `--help`. Every command also takes `--json`, which prints the answer as JSON for scripts ([the format](https://github.com/farrcraft/spyglass/blob/main/docs/specs/cli-json.md)).

The examples put ids and queries in single quotes, which works in macOS and Linux terminals and in PowerShell. In the Windows Command Prompt (cmd), use double quotes instead: `"#minecraft:village"`.

## Locate

`spyglass locate` finds the nearest structure, biome, slime chunk or the world spawn, as the game's `/locate` command does ([how](https://github.com/farrcraft/spyglass/blob/main/docs/specs/locate.md)):

```sh
spyglass locate structure '#minecraft:village' --seed 12345
spyglass locate structure minecraft:stronghold --seed spyglass --from 1000,64,-2000
spyglass locate structure minecraft:bastion_remnant --seed 12345 --dimension nether
spyglass locate biome minecraft:cherry_grove --seed 12345
spyglass locate slime --seed 12345 --radius 4
spyglass locate spawn --seed 12345
```

Name a structure by its id (`minecraft:mansion`) or by a tag, which starts with `#` and covers a group (`#minecraft:village` is every kind of village).

| Option                | Default        | What it does                                                                                                     |
| --------------------- | -------------- | ---------------------------------------------------------------------------------------------------------------- |
| `--seed <seed>`       | required       | A number, or text, which is turned into a number as the game's Create New World screen does.                     |
| `--world <save>`      | none           | Use one of your world saves instead of `--seed`: its folder, or its folder name in a launcher's saves.           |
| `--from <x,y,z>`      | `0,64,0`       | Where to search from. `x,z` means y 64. With `--world`, `player` means your player's position.                   |
| `--dimension <d>`     | Overworld      | `overworld`, `nether` or `end`. With `--from player`, your player's dimension.                                   |
| `--radius <n>`        | see below      | How far to search: in the game's structure grid cells for structures, blocks for biomes, chunks for slime.       |
| `--nearest`           | off            | Structures: give the nearest one, which isn't always the game's `/locate` answer.                                |
| `--bounds`            | off            | Structures: the outline and piece count of villages, other structures built from pieces, and mineshafts.         |
| `--unconfirmed`       | off            | Structures: skip checking the ground. Faster, but answers may be [candidates](answers.md#certain-and-candidate). |
| `--pack <zip\|dir>`   | none           | Apply a [data pack](data-packs.md#packs-for-a-seed). Repeat it for more.                                         |
| `--game-version <id>` | newest release | Which game version to use. Snapshots are used only when you name them.                                           |
| `--bundle <dir>`      | none           | Use this bundle folder.                                                                                          |
| `--bundles <dir>`     | bundles folder | Where to look for bundles ([game data](game-data.md#where-bundles-go)).                                          |
| `--json`              | off            | Print JSON for scripts.                                                                                          |

`--radius` defaults to the game's own `/locate` range for structures and biomes, and to 8 chunks for slime chunks. Slime chunks and the world spawn are only in the Overworld.

## Features

The game has no `/locate` for dungeons (`minecraft:monster_room`), amethyst geodes, desert wells, fossils and lava lakes. These are **features**: things the game adds to each chunk after its terrain. spyglass finds them by generating each chunk's features as the game does:

```sh
spyglass locate feature minecraft:monster_room --seed 12345
spyglass features minecraft:amethyst_geode --seed 12345 --min -200,-200 --max 200,200
```

`locate feature` finds the nearest. Since every chunk takes work, it searches 8 chunks around `--from` unless you give a larger `--radius`. `features` lists every one in an area.

Each answer is **certain**, or a **candidate** with the reason ([what that means](answers.md#certain-and-candidate)). `--all` also lists the tries that failed, which is nearly every try.

One more thing can change an answer: the order the game generates chunks in. That depends on how players explore, and a neighboring chunk generated first can change what a chunk holds. That's rare, a few chunks in a thousand. "Certain" assumes the chunk was generated before its neighbors. `--neighbours` checks whether each answer holds whatever order the neighbors come in, and takes about twice as long.

## Loot

`spyglass loot` tells you what's in the chests: the containers that structures and dungeons generate, with their contents as the seed decides them.

```sh
spyglass loot find minecraft:enchanted_golden_apple --seed 12345 --radius 16
spyglass loot find minecraft:enchanted_book --enchantment minecraft:mending --seed 12345
spyglass loot in --seed 12345 --min -200,-200 --max 200,200 --item minecraft:diamond
spyglass loot at 33,-5,-88 --seed 12345
```

- `loot find <item>` finds the nearest container holding an item, or any item in a `#tag`, within `--radius` chunks (8 by default). `--enchantment` and `--level` ask for an enchanted book or tool.
- `loot in --min x,z --max x,z` lists the containers in an area. `--item` keeps only those holding an item.
- `loot at x,y,z` lists one container's contents, slot by slot.

Each container takes about a tenth of a second to work out. Like features, each container and its contents are certain or a candidate. A candidate's contents can be wrong.

With `--world`, spyglass also reads your save. Where your world has already generated a container, it says whether it's still unopened, what it holds now if it was opened, or that it's gone. `loot find` goes by what's in it now. See [your own worlds](your-worlds.md#when-answers-may-differ).

## Flowers

```sh
spyglass flower at 165,94,-2074 --seed 12345
spyglass flower heights minecraft:cornflower --min 120,-2110 --max 260,-2010 --y 60..110 --seed 12345
```

`spyglass flower at x,y,z` says which flowers bone meal can grow at a block, with each one's chance, and why: the biome there and how it picks its flowers. Give the flower's own block, one above the grass.

`spyglass flower heights` tries each height in `--y` (every `--step` blocks) and says how much of an area grows a flower for certain, and how much can grow it. See [Flower farms](flower-farms.md).

## Portals

```sh
spyglass portal link nether 100,64,100 --portal overworld:800,64,800
spyglass portal link overworld 812,70,-400 --axis z --size 4x5 --world "My World"
spyglass portal link --world "My World"
```

`spyglass portal link DIMENSION x,y,z` says where a Nether portal leads and which portal you'll come out of. Give:

- the portal's dimension and its lowest block inside the frame;
- `--axis z` if it runs along z rather than x;
- `--size` for its inside size, if it isn't 2x3.

The portals it can lead to are the ones you list with `--portal DIMENSION:x,y,z` (add `:z` or a size such as `:4x5` if they differ), and, with `--world`, the portals in your save.

With only `--world`, it lists every portal in your save, where each leads, and which ones don't link back.

Portals don't depend on the seed, so `portal link` needs no `--seed`. See [Nether portals](portals.md).

## Heights and blocks

```sh
spyglass heights --seed 12345 --min 0,0 --max 15,15
spyglass blocks --seed 12345 --column 100,-37
```

`spyglass heights` prints the terrain height of one column (`--at x,z`), or of every column in an area (`--min x,z --max x,z`, up to 65,536 columns). This is the ground as the game first shapes it, before caves, structures and decorations change it, and it's what the game uses to place structures. `--heightmap ocean-floor` ignores water. In the Nether it finds the solid roof, about 128 everywhere.

`spyglass blocks` prints the blocks the game generates before structures and features:

- `--at x,y,z`: one block;
- `--column x,z`: a column, from the top down;
- `--chunk x,z`: how many of each block a chunk has, with chunk coordinates (`--json` lists every block).

That's the terrain with its surface (grass, sand, terracotta bands, icebergs), ore veins, and the caves and ravines cut into it. `--no-carvers` leaves out the caves and ravines.

## More

- [Maps](maps.md): `spyglass render`.
- [Flower farms](flower-farms.md): `spyglass flower`, and the flower layer.
- [Seed search](seed-search.md): `spyglass search`.
- [Searching within a seed](search-in-seed.md): `spyglass find`.
- [Your own worlds](your-worlds.md): `spyglass world`, and `--world` on the other commands.
- [Data packs](data-packs.md): `--pack`, and your world's packs.
