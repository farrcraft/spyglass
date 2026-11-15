# Maps

`spyglass render` draws a map as a PNG: biomes, and the layers you ask for. It's the same image the viewer's **Export → Image of this view…** saves for the same view, and **Export → Copy the command that draws it** gives you the command for it.

```sh
spyglass render --seed 12345 -o map.png
spyglass render --seed 12345 --centre 1000,-2000 --size 1920x1080 --scale 16 \
    --structures '#minecraft:village,minecraft:stronghold' --slime --spawn -o map.png
spyglass render --world "My World" --highlight minecraft:cherry_grove -o map.png
spyglass render --seed 12345 --scale 2 --loot minecraft:diamond -o diamonds.png
```

| Option                            | Default            | What it does                                                                                                                                                         |
| --------------------------------- | ------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `-o <file>`                       | required           | The PNG to write.                                                                                                                                                    |
| `--centre <x,z>` or `--min <x,z>` | 0,0, or the player | The block at the image's centre, or at its top-left corner.                                                                                                          |
| `--size <WxH>`                    | `1024x1024`        | Pixels.                                                                                                                                                              |
| `--scale <n>`                     | `4`                | Blocks per pixel: 1, 2, 4 and so on up to 256.                                                                                                                       |
| `--y <y>`                         | `64`               | The height biomes and the underground layers are drawn at: a slice, not the surface.                                                                                 |
| `--surface`                       | off                | Biomes at the surface instead of at `--y`. Slow, as `--relief` is.                                                                                                   |
| `--relief`                        | off                | Shade by terrain height. Slow: per 256 × 256, about a second at scale 1, 5 s at 4 and 15–20 s from 32 out.                                                           |
| `--veins`, `--caves`, `--carvers` | off                | Ore veins, noise caves, and carvers' caves and ravines at `--y` (scales up to 4).                                                                                    |
| `--flowers`                       | off                | The flower bone meal grows on grass at `--y`, the block above the grass: solid where it's certain, fainter as it's less likely. See [Flower farms](flower-farms.md). |
| `--flower <id>`                   | none               | Only this flower, such as `minecraft:cornflower`, with the blocks that can't grow it dimmed. Implies `--flowers`.                                                    |
| `--structures <ids>`              | none               | Structure ids or `#tags` to mark, separated by commas. Hollow markers are candidates.                                                                                |
| `--features <ids>`                | none               | Feature ids to mark (scales up to 2), such as `minecraft:monster_room`. Slow.                                                                                        |
| `--loot <item>`                   | none               | Ring the containers holding an item or item `#tag` (scales up to 2). Slow.                                                                                           |
| `--confirm`                       | off                | Decide structure markers with the seed's terrain, so nearly all are certain.                                                                                         |
| `--highlight <ids>`               | none               | Biomes to keep bright; the rest are dimmed.                                                                                                                          |
| `--slime`, `--spawn`, `--ties`    | off                | Slime chunks (scales up to 16), the spawn's origin, tied biome cells.                                                                                                |
| `--generated`                     | off                | With `--world`: the chunks your world has generated, magenta, fainter where generated partway, amber where an older version generated them (scales up to 16).        |
| `--inhabited`                     | off                | With `--generated`: shade each chunk by the time players spent near it, faint where they passed through and strong where they lived.                                 |
| `--no-biomes`                     | off                | A plain background under the other layers, or grey by height with `--relief`.                                                                                        |
| `--pin <x,z[,#rrggbb]>`           | none               | A pin at a block, in a colour (amber if none). Repeat it for more.                                                                                                   |

The seed, world, dimension and bundle options are as for [`locate`](command.md#locate). With `--world`, a map centred on the player is in the player's dimension. In the Nether, terrain layers aren't drawn, since its surface heights are the ceiling's, and slime chunks and the spawn are the Overworld's alone. With `--json` it prints the area and everything it drew.
