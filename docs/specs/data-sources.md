# Data sources

Status: draft.

Everything spyglass knows about a game version comes from Mojang's own distribution.

## Sources

1. **Version manifest:** `https://piston-meta.mojang.com/mc/game/version_manifest_v2.json` lists releases and snapshots with links to server jars and official mappings.
2. **Jar data:** since 1.18 most worldgen lives in JSON under `data/minecraft/worldgen/` (noise settings, density functions, noise parameters, structure sets, biome tags).
3. **Data generator:** `java -DbundlerMainClass=net.minecraft.data.Main -jar server.jar --all` dumps registries and reports, including the overworld biome parameter list in recent versions.
4. **Readable code:** decompile with Vineflower. From 26.x the server jar ships unobfuscated (confirmed for 26.3: real class names, and no `server_mappings` in the version JSON), so no mappings step is needed. Older versions need the official mappings.

## Server jar

### Jar cache

Downloaded server jars live in a per-user cache (`spyglass_data::cache`), never inside the repo: the platform cache folder for `spyglass` (`%LOCALAPPDATA%\spyglass\cache` on Windows, `~/Library/Caches/spyglass` on macOS, `~/.cache/spyglass` on Linux), or a directory given explicitly. Jars are stored by content as `jars/<sha1>.jar`. Before use, a jar's size and SHA-1 are checked against the version JSON's `downloads.server`. A missing or mismatched jar is downloaded again, into a `.part` file that is renamed only once it verifies. The extractor and the oracle share the cache.

### Bundler layout

The server jar is a bundler. The game itself and its libraries are jars inside it, listed in two files of tab-separated `sha256`, `id` and `path` lines:

- `META-INF/versions.list`: the game, one line, e.g. `26.3/server-26.3.jar`, stored at `META-INF/versions/26.3/server-26.3.jar`;
- `META-INF/libraries.list`: the libraries (39 for 26.3), under `META-INF/libraries/`.

`spyglass_data::bundler` reads both lists and unpacks the jars. List paths that are absolute or contain `..` are rejected. The outer jar's SHA-1 is verified, so the inner SHA-256 values aren't checked again.

## Java runtime

Running the data generator needs a Java runtime whose major version equals the version JSON's `javaVersion.majorVersion`. A runtime is enough; a JDK isn't needed. `spyglass_data::java` searches, in order, and uses the first exact match:

1. an explicit path (`--java`), either a Java home or the executable;
2. `JAVA_HOME`;
3. each `java` on `PATH`, following symlinks to the real home;
4. standard install locations, scanned up to six levels deep:
   - Windows: `%ProgramFiles%\` + `Eclipse Adoptium`, `Java`, `Microsoft`, `Zulu`, `Amazon Corretto`, `BellSoft`;
   - macOS: `/Library/Java/JavaVirtualMachines` and `~/Library/Java/JavaVirtualMachines`;
   - Linux: `/usr/lib/jvm`;
   - everywhere: `~/.sdkman/candidates/java` (SDKMAN) and `~/.jdks` (IntelliJ downloads);
5. runtimes downloaded by game launchers, scanned the same way:

   | Launcher                             | Windows                                                                                  | macOS                                                          | Linux                                           |
   | ------------------------------------ | ---------------------------------------------------------------------------------------- | -------------------------------------------------------------- | ----------------------------------------------- |
   | Minecraft Launcher                   | `%APPDATA%\.minecraft\runtime`                                                           | `~/Library/Application Support/minecraft/runtime`              | `~/.minecraft/runtime`                          |
   | Minecraft Launcher (Microsoft Store) | `%LOCALAPPDATA%\Packages\Microsoft.4297127D64EC6_8wekyb3d8bbwe\LocalCache\Local\runtime` | —                                                              | —                                               |
   | Modrinth App                         | `%APPDATA%\ModrinthApp\meta\java_versions`                                               | `~/Library/Application Support/ModrinthApp/meta/java_versions` | `~/.local/share/ModrinthApp/meta/java_versions` |

A directory counts as a Java home if it has `bin/java` and a `release` file. The version is read from the `JAVA_VERSION` line of `release`, never by running the executable. Majors follow both schemes: `25.0.3` → 25, `1.8.0_51` → 8. Duplicate homes (by canonical path) are reported once. A home with `bin/javac` is a JDK, which only oracle work needs ([ADR 0004](../adr/0004-oracle-standalone-harness.md)). The oracle uses the same search, filtered to JDKs.

The Windows rows were confirmed on a real machine on 2026-10-24. The macOS and Linux paths follow each launcher's documented data folder but haven't been confirmed yet.

## Licensing

Mojang's data and code are never committed or redistributed. The extractor runs locally against a jar the user downloads themselves, the same approach Amidst used. Generated bundles go in `/bundles`, which is gitignored.

## Open questions

- Bundle format and layout on disk.
- Which data-generator reports are needed vs which can be read straight from the jar.
- ~~Does the extractor need a Java runtime?~~ **Yes.** The overworld biome parameter list isn't in the jar's JSON (`multi_noise_biome_source_parameter_list/overworld.json` is only `{"preset": "minecraft:overworld"}`). It is built in code by `OverworldBiomeBuilder`, so it has to come from the data generator. The required Java version comes from the version JSON's `javaVersion.majorVersion` (25 for 26.3).
