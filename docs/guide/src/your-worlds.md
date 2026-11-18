# Your own worlds

Every command that takes `--seed` also takes `--world` instead, to answer for one of your world saves. The viewer opens saves too. spyglass reads the world's seed, game version and player position from the save, and never changes the save.

```sh
spyglass world list
spyglass world info "My World"
spyglass locate structure '#minecraft:village' --world "My World" --from player
```

- `world list` lists the saves in the official Minecraft Launcher and the Modrinth App.
- `world info` shows everything spyglass read from a world and how its answers use it ([below](#what-spyglass-read)).
- `--world` takes a save's folder, or its folder name in a launcher's saves. `--from player` searches from where your player is, in your player's dimension.

## What spyglass read

`world info` explains how spyglass answers for your world. It has three parts, and lists each warning under the thing it's about:

- **Settings:** every value spyglass took from the save, the file it came from, and what it decides. For example, the seed decides every prediction, the game version decides which bundle answers, and the world border decides where portals can go.
- **Data packs:** each pack the world uses, in the order the game loads them, whether spyglass applied it, and, if it did, what the pack changes and which answers that affects. A pack of only recipes or functions changes none of them.
- **Dimensions:** whether spyglass can answer in each dimension, and what your world has saved there: how many chunks, which game versions generated them, the structures the world recorded, and the loot containers nobody has opened yet.

For a 26.3 world with Terralith, shortened:

```text
Settings
  seed                         -5730496209435919920
                               from data/minecraft/world_gen_settings.dat data.seed; decides every prediction
  modded                       yes
                               from level.dat Data.WasModded; decides nothing, but mods can change generation
    warning: the world has been opened with a modded game; mods can change generation

Data packs
  vanilla                      the bundle's own data
  file/terralith.zip           applied from datapacks/: changes biomes, terrain, structures, features, loot, flowers and portals
                                 worldgen/biome: 39 replaced, 95 added
                                 worldgen/structure: 29 added
                                 function: 3 added (not read by spyglass)
    error: the Overworld can't be opened with the save's packs: terralith:mage/tower: DataVersion: unsupported template data version "Some(3839); only 5023 is read without data fixing"

Dimensions
  the Overworld                not answered; regions in dimensions\minecraft\overworld\region
                                 chunks    4318: 1898 full, 2420 proto, 0 unreadable
                                 versions  26.3 (5023) 4318
                                 starts    28 answered from the save: minecraft:mineshaft 12, minecraft:ocean_ruin_cold 5, …
                                 sealed    337 loot containers not yet opened: minecraft:decorated_pot 115, minecraft:chest 96, …
```

For a large world, reading all its chunks takes a few seconds. `--quick` skips that, and leaves out what each dimension holds. With `--json`, the same report is in `settings`, `packs` and `dimensions`, and each warning says what it's `about` ([the JSON format](https://github.com/farrcraft/spyglass/blob/main/docs/specs/cli-json.md#world-info)).

The viewer shows the same report in its **World** tab, with a count of what the map has taken from the save so far ([the viewer](viewer.md#your-worlds)). The report doesn't count the containers you've opened, since a save can't tell a structure's chest you opened from one you placed yourself. Instead, spyglass says how each container stands as you look at it ([below](#when-answers-may-differ)).

## What the world already holds

Once your world has generated a chunk, the game uses what it saved there, not the seed, and so does spyglass. A structure in a chunk your world has saved is marked **saved**: it's really there, even if an older version, a data pack or a mod put it there. A saved chunk with no structure has none, whatever the seed would put there. Everywhere else, spyglass predicts from the seed as usual. Chunks saved before 1.18.2 are predicted too, since the game renames their structures when it loads them.

```text
#village from 0 64 0 (My World, seed -4893098694207956070, Minecraft 26.3)
        272 ~ 384           470 blocks  saved      minecraft:village_plains
```

## Portals

`spyglass portal link --world "My World"` lists your world's Nether portals, where each one leads, and the ones that don't link back. The viewer draws them on the map. See [Nether portals](portals.md).

## When answers may differ

`world info` says whether spyglass can answer for a world, and warns about what makes its answers inexact. Each warning appears under what it's about:

- **No bundle for the world's version,** under the data version. [Extract one](game-data.md).
- **A world preset other than Default,** such as Large Biomes or Amplified, under that dimension's generator.
- **Structures turned off,** under the structures setting.
- **Data packs spyglass can't apply,** and mods' packs, under the pack. spyglass applies your world's data packs as the game does; see [data packs](data-packs.md) for what it can't. If spyglass applies a pack but then can't open a dimension with it (because the pack uses a kind of feature spyglass doesn't support yet, say), that's an error under the pack, or under the dimension when spyglass can't tell which pack caused it.
- **A world opened with a modded game,** under the modded setting.
- **Chunks an older version generated,** and files that can't be read, under the dimension. Older chunks were made by that version's rules, and stay as they were when the world was upgraded.
- **A world border that's still moving** is a note under the border: spyglass answers with the size it ends at.

Loot can differ too: a chest you've already opened holds whatever you left in it. With `--world`, each container in a chunk your world has generated says what the save holds: `in the save: unopened` (it holds what spyglass shows), `opened, now …` with what's in it now, or `gone` (broken, or something else in its place). `loot find` goes by what's in it now.
