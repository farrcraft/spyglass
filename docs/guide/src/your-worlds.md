# Your own worlds

Every command takes `--world` instead of `--seed`, and the viewer opens saves too. spyglass reads a save's seed, game version and player from it, and never writes to it.

```sh
spyglass world list
spyglass world info "My World"
spyglass locate structure '#minecraft:village' --world "My World" --from player
```

- `world list` lists the saves in the official Minecraft Launcher and the Modrinth App.
- `world info` shows a world's seed, version, spawn and player, and anything that makes spyglass's answers inexact.
- `--world` takes a save's folder, or its folder name in a launcher's saves. `--from player` searches from where your player is, in the player's dimension.

## What the world already holds

Where your world has already generated a chunk, the game answers from what it saved there, not from the seed, and so does spyglass. A structure in a chunk the world has saved is marked **saved**: it's there, even if an older version, a data pack or a mod placed it. A chunk the world saved with no structure has none, whatever the seed would put there. Everywhere else, spyglass predicts from the seed as usual. Chunks saved before 1.18 are predicted too.

```text
#village from 0 64 0 (My World, seed -4893098694207956070, Minecraft 26.3)
        272 ~ 384           470 blocks  saved      minecraft:village_plains
```

## When answers may differ

`world info` says whether spyglass can answer for a world, and warns about what makes its answers inexact:

- **No bundle for the world's version.** [Extract one](game-data.md).
- **A world preset other than Default,** such as Large Biomes or Amplified.
- **Data packs or mods** that may change generation.
- **Chunks an older version generated.** Those were made by that version's rules, and stay as they were when the world was upgraded.

It reads every region file to count those chunks, which takes a few seconds for a large world. `--quick` skips that.

Loot in a world can differ too: a chest you've already opened holds whatever you left in it.
