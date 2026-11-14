# 0071. Answers from a save, where the game reads it

- Status: Accepted
- Date: 2026-11-11

## Context

spyglass answers a world save's questions by predicting every chunk from the seed. A save that has played for a while has generated much of its world already, and what it holds can differ from the prediction: chunks an older version generated, data packs or mods that were removed, containers players have opened. spyglass reads the save's region files already, but only to count chunks by version and warn ([world-saves.md](../specs/world-saves.md#region-files)).

The game doesn't predict generated chunks either. Every structure check, `/locate`'s included, asks storage first (`StructureCheck.tryLoadFromStorage`): a chunk on disk is answered from its saved `structures.starts`, data-fixed to the current version, whatever its status or the version that saved it; only a chunk not on disk is predicted ([world-saves.md](../specs/world-saves.md#structure-starts-in-saves)). A container keeps its `LootTable` until it's first filled, and from then on holds its saved `Items` ([world-saves.md](../specs/world-saves.md#containers-in-saves)). So for a save, the save is the game's own answer wherever it has the chunk, and a prediction there can be wrong.

Reading a save is not free: a whole chunk is a tree of up to megabytes, against the root fields the scan reads now, and saves are untrusted input. And spyglass's sessions don't know about saves: a `World` and a seed define one.

## Decision

We will answer from a save wherever the game does, and only there:

- **Structures:** a chunk the save holds with a current-format `structures.starts` decides which structures start in it, certainly. A chunk it doesn't hold, or holds without starts, is predicted as before. Chunks saved before 1.18 (`Level.Structures.Starts`), which the game reads through data fixers that rename structures by biome, and chunks that can't be read, are predicted, and the older-chunks warning stands for them.
- **Containers:** a predicted container in a chunk the save holds gets the save's state: unopened, with whether its stored seed is the predicted one; filled, with the saved items; or gone.
- **How:** a `SaveChunks` for one dimension, made from a `SaveReport` (keeping `Scan::Chunks`'s per-chunk index, so a save is scanned once), and attached to a `Session` with `Session::with_save`. A session without one answers exactly as before. `spyglass-structure` takes stored starts through a trait (`StoredStarts`), so it doesn't depend on `spyglass-world`. Chunks are read on demand, each once per `SaveChunks`, under the region reader's limits.
- **Certainty:** an answer from the save is certain about the save as it is. Every answer that came from the save says so, so front ends can tell a saved start from a predicted one.

## Consequences

- `locate structure --world`, the viewer and the map give the game's answer in generated chunks, even ones an older version, a removed data pack or a mod placed, and say where it came from.
- Loot answers stop presenting an opened chest's prediction as its contents.
- The saves on a machine become a check of the predictions: in a 26.3 save no older version touched, every saved start and every unopened container's seed should be the predicted one.
- **Costs:** reading whole chunks, each a few milliseconds, for every candidate chunk a query visits in a save; a cache per `SaveChunks`, and the per-chunk index (a few MB for a 4.6 GB save) held with the report. A save is read as it was when opened; a running game changes it underneath, and a later query doesn't see that. Structures in pre-1.18 chunks stay predicted, though the game knows better.

## Alternatives considered

- **Predict everything, as before:** cheapest, and wrong wherever the save differs, in exactly the long-played worlds a seed explorer's users have.
- **Read every chunk's starts up front:** simpler to query, but a whole-tree read of every chunk is many times the scan's cost (8 s on the largest save), for answers most sessions never ask for.
- **Data-fix pre-1.18 starts as the game does:** would answer old chunks too, but needs the game's fixers' biome-based renames ported and kept, for chunks the warning already flags.
- **Make the save part of `World`:** a `World` is a bundle's dimension and is shared across seeds; a save is one seed's state. A session is the unit that already binds one seed.
