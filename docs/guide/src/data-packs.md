# Data packs

A data pack can change how a world generates: its terrain, biomes, structures, features and loot. spyglass applies data packs the way the game does, so its answers for a world with packs are the answers for that world.

## Your world's packs

With `--world`, and in the viewer, spyglass applies the packs your world has enabled, in the order the game loads them:

- packs in the world's `datapacks/` folder, zips and folders alike;
- the game's built-in packs, such as `trade_rebalance`, which you turn on under **Experiments** when you create a world.

`spyglass world info` lists the packs it applied, and warns about any it couldn't:

```text
My World (C:\Users\me\AppData\Roaming\.minecraft\saves\My World)
  version  26.3 (data version 5023)
  seed     -4893098694207956070
  packs    vanilla, file/terralith.zip, file/my-tweaks
  applied  file/terralith.zip (1203 worldgen files), file/my-tweaks (2 worldgen files)
  bundle   C:\Users\me\AppData\Local\spyglass\bundles\26.3
  answers  Overworld, Nether, End
```

What it may say:

- **Missing:** the world enables a pack that isn't in `datapacks/` any more. The world may have been generated with it, so answers can differ.
- **From a mod:** spyglass can't apply a mod's packs, or mods.
- **Couldn't be read:** the pack is damaged, or larger than spyglass reads.
- **Not loaded by the game either** (a note): a folder without `pack.mcmeta`, for example. It doesn't change the world, and spyglass doesn't apply it.
- **For other formats** (a note): the pack was made for another game version. The game loads it anyway, and so does spyglass.
- **Can't be applied in a dimension** (an error): the pack uses something spyglass doesn't support yet, such as a new kind of density function or feature placement. It names the file. spyglass won't answer in that dimension rather than answer wrongly.

A pack you put into `datapacks/` after the world was created loads the next time the game opens the world. Chunks generated before then don't have it, so spyglass applies only the packs the world lists as enabled.

## Packs for a seed

To see what a seed looks like with packs, give `--pack` with the pack's zip or folder, once for each pack, lowest first, as the game would load them:

```sh
spyglass render --seed 42 --pack Terralith.zip --out map.png
spyglass locate structure '#minecraft:village' --seed 42 --pack Terralith.zip --pack my-tweaks
spyglass search 'require minecraft:mansion within 2000 of spawn' --pack Terralith.zip
```

`--pack` works with every command that takes `--seed`. With `--world`, the world's own packs are used instead.

## Bundles from older versions of spyglass

Applying a pack needs the version's data pack format, which bundles extracted by older versions of spyglass don't record. spyglass says so; [extract the bundle again](game-data.md) to fix it.
