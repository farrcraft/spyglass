# Data sources

Status: draft.

Everything spyglass knows about a game version comes from Mojang's own distribution.

## Sources

1. **Version manifest:** `https://piston-meta.mojang.com/mc/game/version_manifest_v2.json` lists releases and snapshots with links to server jars and official mappings.
2. **Jar data:** since 1.18 most worldgen lives in JSON under `data/minecraft/worldgen/` (noise settings, density functions, noise parameters, structure sets, biome tags).
3. **Data generator:** `java -DbundlerMainClass=net.minecraft.data.Main -jar server.jar --all` dumps registries and reports, including the overworld biome parameter list in recent versions.
4. **Readable code:** decompile with Vineflower plus the official mappings. (TODO: confirm whether 26.x jars ship unobfuscated.)

## Licensing

Mojang's data and code are never committed or redistributed. The extractor runs locally against a jar the user downloads themselves, the same approach Amidst used. Generated bundles go in `/bundles`, which is gitignored.

## Open questions

- Bundle format and layout on disk.
- Which data-generator reports are needed vs which can be read straight from the jar.
- Does the extractor need a Java runtime, or can it avoid running the data generator?
