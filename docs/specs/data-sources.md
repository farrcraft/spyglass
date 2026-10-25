# Data sources

Status: draft.

Everything spyglass knows about a game version comes from Mojang's own distribution.

## Sources

1. **Version manifest:** `https://piston-meta.mojang.com/mc/game/version_manifest_v2.json` lists releases and snapshots with links to server jars and official mappings.
2. **Jar data:** since 1.18 most worldgen lives in JSON under `data/minecraft/worldgen/` (noise settings, density functions, noise parameters, structure sets, biome tags).
3. **Data generator:** `java -DbundlerMainClass=net.minecraft.data.Main -jar server.jar --all` dumps registries and reports, including the overworld biome parameter list in recent versions.
4. **Readable code:** decompile with Vineflower. From 26.x the server jar ships unobfuscated (confirmed for 26.3: real class names, and no `server_mappings` in the version JSON), so no mappings step is needed. Older versions need the official mappings.

The server jar is a bundler; the game itself is at `META-INF/versions/<id>/server-<id>.jar` inside it.

## Licensing

Mojang's data and code are never committed or redistributed. The extractor runs locally against a jar the user downloads themselves, the same approach Amidst used. Generated bundles go in `/bundles`, which is gitignored.

## Open questions

- Bundle format and layout on disk.
- Which data-generator reports are needed vs which can be read straight from the jar.
- ~~Does the extractor need a Java runtime?~~ **Yes.** The overworld biome parameter list isn't in the jar's JSON (`multi_noise_biome_source_parameter_list/overworld.json` is only `{"preset": "minecraft:overworld"}`). It is built in code by `OverworldBiomeBuilder`, so it has to come from the data generator. The required Java version comes from the version JSON's `javaVersion.majorVersion` (25 for 26.3).
