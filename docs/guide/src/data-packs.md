# Data packs

A data pack can change how a world generates: its terrain, biomes, structures, features and loot. spyglass applies data packs the way the game does, so its answers for a world with packs are right for that world.

## Your world's packs

With `--world`, and in the viewer, spyglass applies the packs your world has enabled, in the order the game loads them:

- packs in the world's `datapacks/` folder, zips and folders alike;
- the game's built-in packs, such as `trade_rebalance`, which you turn on under **Experiments** when you create a world.

`spyglass world info` lists every pack the world enables, whether it applied each, and what an applied pack changes, by kind of file, and which of spyglass's answers that touches ([what spyglass read](your-worlds.md#what-spyglass-read)):

```text
Data packs
  vanilla                      the bundle's own data
  file/my-tweaks               applied from datapacks/: changes biomes, terrain and portals
                                 worldgen/noise_settings: 1 replaced
                                 recipe: 3 added (not read by spyglass)
  file/old-pack.zip            not applied
    warning: data pack file/old-pack.zip is enabled but missing from datapacks/, so spyglass can't apply it
```

Packs load in order, each on top of the ones before it. For each kind of file, the counts say:

- **replaced:** a pack below it (or the game's own data) has the same file, and this one takes its place;
- **added:** no pack below has it;
- **merged:** a tag combined with the same tag below it (unless the tag says to replace them);
- **hidden below:** files of the packs below that this pack's filter removes;
- **shadowed above:** files of this pack that a pack above it replaces or hides, so they never take effect;
- **unreadable:** files spyglass couldn't read.

What it may warn about, under the pack:

- **Missing:** the world enables a pack that isn't in `datapacks/` any more. The world may have been generated with it, so answers can differ.
- **From a mod:** spyglass can't apply a mod's packs, or mods.
- **Couldn't be read:** the pack is damaged, or larger than spyglass reads.
- **Not loaded by the game either** (a note): a folder without `pack.mcmeta`, for example. It doesn't change the world, and spyglass doesn't apply it.
- **For other formats** (a note): the pack was made for another game version. The game loads it anyway, and so does spyglass.
- **Can't be applied in a dimension** (an error): the pack uses something spyglass doesn't support yet, such as a new kind of density function or feature placement. It names the file. spyglass won't answer in that dimension rather than answer wrongly.

A pack you put into `datapacks/` after the world was created loads the next time the game opens the world. Chunks generated before then don't have it, so spyglass applies only the packs the world lists as enabled.

## Packs for a seed

To see what a seed looks like with packs, give `--pack` with each pack's zip or folder, in the order the game would load them, lowest first:

```sh
spyglass render --seed 42 --pack my-biomes.zip -o map.png
spyglass locate structure '#minecraft:village' --seed 42 --pack my-biomes.zip --pack my-tweaks
spyglass search -q 'require minecraft:mansion within 2000 of spawn' --pack my-biomes.zip
```

`--pack` works with every command that takes `--seed`, and with `search` and `portal link`. With `--world`, spyglass uses the world's own packs instead.

A pack made for another game version may not load. Large worldgen packs such as Terralith often include structure files saved by older versions, which spyglass can't read yet, so it says which file it stopped at.

## Bundles from older versions of spyglass

Applying a pack needs the version's data pack format, which bundles extracted by older versions of spyglass don't record. spyglass says so; [extract the bundle again](game-data.md) to fix it.
