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

## When answers may differ

`world info` says whether spyglass can answer for a world, and warns about what makes its answers inexact:

- **No bundle for the world's version.** [Extract one](game-data.md).
- **A world preset other than Default,** such as Large Biomes or Amplified.
- **Data packs or mods** that may change generation.
- **Chunks an older version generated.** Those were made by that version's rules, and stay as they were when the world was upgraded.

It reads every region file to count those chunks, which takes a few seconds for a large world. `--quick` skips that.

Loot in a world can differ too: a chest you've already opened holds whatever you left in it.
