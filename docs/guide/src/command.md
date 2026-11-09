# The `spyglass` command

`spyglass` answers questions about a seed from a terminal: where things are, what's in the chests, maps and searches. Every command has `--help`, and `--json` for scripts ([its format](https://github.com/farrcraft/spyglass/blob/main/docs/specs/cli-json.md)).

On Windows, in PowerShell or cmd, use double quotes where these examples use single quotes: `"#minecraft:village"`.

## Locate

`spyglass locate` answers as the game's `/locate` would ([how](https://github.com/farrcraft/spyglass/blob/main/docs/specs/locate.md)):

```sh
spyglass locate structure '#minecraft:village' --seed 12345
spyglass locate structure minecraft:stronghold --seed spyglass --from 1000,64,-2000
spyglass locate structure minecraft:bastion_remnant --seed 12345 --dimension nether
spyglass locate biome minecraft:cherry_grove --seed 12345
spyglass locate slime --seed 12345 --radius 4
spyglass locate spawn --seed 12345
```

A structure is an id (`minecraft:mansion`) or a tag (`#minecraft:village`).

| Option                | Default        | What it does                                                                              |
| --------------------- | -------------- | ----------------------------------------------------------------------------------------- |
| `--seed <seed>`       | required       | A number, or text hashed as the world-creation screen does.                               |
| `--world <save>`      | none           | A world save instead of `--seed`: its folder, or its folder name in a launcher's saves.   |
| `--from <x,y,z>`      | `0,64,0`       | Where to search from, in blocks; `x,z` means y 64; `player` is `--world`'s single player. |
| `--dimension <d>`     | Overworld      | `overworld`, `nether` or `end`. With `--from player`, the player's dimension by default.  |
| `--radius <n>`        | as `/locate`   | Placement-cell rings (structures), blocks (biomes) or chunks (slime).                     |
| `--nearest`           | off            | Structures: the nearest start, not `/locate`'s answer, which isn't always the nearest.    |
| `--bounds`            | off            | Structures: each certain jigsaw structure's or mineshaft's bounds and piece count.        |
| `--unconfirmed`       | off            | Structures: don't decide candidates with the seed's terrain. Faster, but less certain.    |
| `--game-version <id>` | newest release | Which bundle to use. Snapshots are used only when named.                                  |
| `--bundle <dir>`      | none           | Use this bundle folder instead.                                                           |
| `--bundles <dir>`     | bundles folder | Where bundles are ([game data](game-data.md#where-bundles-go)).                           |
| `--json`              | off            | Print JSON for scripts.                                                                   |

Slime chunks and the world spawn are only in the Overworld.

## Features

The game has no `/locate` for dungeons (`minecraft:monster_room`), amethyst geodes, desert wells, fossils and lava lakes. spyglass finds them by placing each chunk as the game decorates it:

```sh
spyglass locate feature minecraft:monster_room --seed 12345
spyglass features minecraft:amethyst_geode --seed 12345 --min -200,-200 --max 200,200
```

That takes tens of milliseconds a chunk, so `locate feature` searches 8 chunks around `--from` unless `--radius` says more. Each answer is **certain**, or a **candidate** with the reason ([what that means](answers.md#certain-and-candidate)). `--all` adds the candidates that didn't place on spyglass's blocks, which are nearly every try.

Certain means certain for a chunk decorated before its neighbours. The order the game decorates chunks in depends on how the world is explored, and a neighbour decorated first can change what a chunk holds, though it rarely does: a few chunks in a thousand. `--neighbours` also checks whether each answer holds whatever its neighbours do, at about twice the time.

## Loot

`spyglass loot` is what's in the chests: the containers structures and dungeons place, with their contents as the seed generates them.

```sh
spyglass loot find minecraft:enchanted_golden_apple --seed 12345 --radius 16
spyglass loot find minecraft:enchanted_book --enchantment minecraft:mending --seed 12345
spyglass loot in --seed 12345 --min -200,-200 --max 200,200 --item minecraft:diamond
spyglass loot at 33,-5,-88 --seed 12345
```

- `loot find <item>` finds the nearest container holding an item or an item `#tag`, with `--enchantment` and `--level` for an enchanted book or tool.
- `loot in --min x,z --max x,z` lists the containers in a region, and `--item` keeps those holding an item.
- `loot at x,y,z` lists one container, slot by slot.

Placing a container's chunk takes about a tenth of a second. Each container and its contents are certain or a candidate, as features are. A candidate's contents may be another seed's. In a world save, a chest already opened, or in a chunk an older version generated, may hold something else.

## Heights and blocks

```sh
spyglass heights --seed 12345 --min 0,0 --max 15,15
spyglass blocks --seed 12345 --column 100,-37
```

`spyglass heights` prints the terrain heights for one column (`--at x,z`) or a region (`--min x,z --max x,z`, up to 256 × 256 columns): one above the highest block before structures, carvers, features and surface blocks change it, which is what structures are placed by. `--heightmap ocean-floor` ignores water. In the Nether, heights find the solid ceiling, about 128 everywhere.

`spyglass blocks` prints the blocks the game generates before structures and features: one block (`--at x,y,z`), a column top down (`--column x,z`), or how many of each block a chunk has (`--chunk x,z`, in chunks; `--json` for every block). That's the terrain with its surface (grass, sand, terracotta bands, icebergs), ore veins, and the caves and ravines carvers cut. `--no-carvers` leaves those out.

## More

- [Maps](maps.md): `spyglass render`.
- [Seed search](seed-search.md): `spyglass search`.
- [Searching within a seed](search-in-seed.md): `spyglass find`.
- [Your own worlds](your-worlds.md): `spyglass world`, and `--world` on every command.
