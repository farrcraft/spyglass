# Flower farms

Bone meal on grass grows flowers, and which flower depends on where. In a flower forest, the flower at each block comes from a pattern that covers the biome in bands, one flower to a band. Plains split tulips from the other flowers the same way. That pattern is three-dimensional, so the bands move as you go up or down: a farm's platform at y 100 grows different flowers from the ground below it.

spyglass works out the flower for any block, at any height, so you can find a band of the flower you want, and the height where it's widest, before you build.

## What decides the flower

When you use bone meal on grass, it spreads over the grass around the block you click, and a few of the blocks it reaches get a flower. Each of those flowers depends only on its own block:

- **The biome at that block**, at its height. Most biomes always grow poppies and dandelions, at random. Flower forests, plains, meadows, swamps, cherry groves, birch forests and pale gardens have their own flowers.
- **Its position**, in flower forests, plains and meadows. A flower forest grows exactly one flower at each block. Plains grow tulips in some places, at random among the four colours, and poppies, azure bluets, oxeye daisies, cornflowers and dandelions elsewhere, also at random. A meadow mixes its flowers by position, but half the time grows wildflowers instead.
- **Chance**, everywhere else: which of the biome's flowers grows is random.

So in a flower forest you can choose your flower exactly, by choosing where to build. Elsewhere, spyglass gives you each flower's chance.

Deep dark and dripstone caves grow plains flowers, so an underground farm there works like one in plains.

## Finding a field

Draw the flowers on a map at the height your platform will be, the height of the flower's block, one above the grass:

```sh
spyglass render --seed 12345 --centre 1000,-2000 --scale 1 --size 512x512 --y 71 --flowers -o flowers.png
spyglass render --seed 12345 --centre 1000,-2000 --scale 1 --size 512x512 --y 71 --flower minecraft:cornflower -o cornflowers.png
```

With `--flowers`, each block shows its most likely flower: solid where it's certain, and fainter the less likely it is. With `--flower`, only that flower is drawn, and every block that can't grow it is dimmed. Try a few heights with `--y` to see the bands move.

## Checking a block

```sh
spyglass flower at 1012,71,-1988 --seed 12345
```

This gives each flower that can grow at the block, with its chance, and why: the biome there, and whether its flowers go by position or by chance. Give the flower's block, one above the grass. "100%" means bone meal always grows that flower there.

The chances are for one flower at that block. Each block bone meal reaches gets a flower only about 1 time in 8, and grass the rest of the time.

## Choosing the height

A band that's narrow at one height can be wide at another. To find the best height for an area:

```sh
spyglass flower heights minecraft:cornflower --min 980,-2020 --max 1040,-1960 --y 60..140 --seed 12345
```

It lists, for each height, the share of the area where the flower is certain and where it can grow, and marks the best. Use `--step` to try every few heights, and `--scale` for a large area.

## Finding a seed

A seed search can ask for a field of a flower: a patch of blocks at one height where bone meal grows it.

```sh
spyglass search -q "require flower minecraft:cornflower field 300 within 1000 of spawn at y 64 to 96 step 8" --count 1000
```

`field 300` asks for at least 300 blocks, joined side to side, at one height. `at y 64 to 96` tries heights from 64 to 96, every 4 blocks; without it, only y 64. By default the flower must be certain at every block of the field. `chance 25%` accepts blocks where it grows at least a quarter of the time, for the flowers plains and meadows grow by chance:

```text
prefer flower minecraft:red_tulip field 100 chance 25% within 500 of spawn
```

The field is checked on a grid every `step` blocks (4 by default), so a smaller step is more exact and slower. The answer gives the field's first block and its height; check it with `spyglass flower at`, and choose the height with `spyglass flower heights`. The same constraint works in `spyglass find`, around each place it finds.

## Your own world

Give `--world` instead of `--seed` to ask about your own world, as with every command. The flowers come from your world's seed and version. A data pack that changes flower features changes the answers too.
