# Reading the answers

spyglass's answers match the game's exactly, for the game versions it was checked against (`spyglass --version` lists them). Where it can't be sure of an answer, it says so.

## Certain and candidate

Each answer is **certain** or a **candidate**:

- **Certain** means the game generates it there, exactly as shown.
- A **candidate** is where the game tries to put something, but spyglass can't decide whether it generates there. The answer says why.

**Structures.** `locate` decides each structure with the seed's terrain, as the game does, so its answers are almost always certain, and then they are exactly the game's `/locate` answer. A candidate is left where the game's choice depends on a tied biome cell or on a stronghold position that can differ between platforms. Then the game's answer is the first of the candidates it generates, usually the first listed.

Without terrain (`--unconfirmed`, and maps and searches unless they `--confirm`), most surface structures, such as villages, temples and monuments, are candidates.

**Features and loot.** A feature or container is a candidate when something spyglass doesn't place could change it, such as a mineshaft's pieces. A candidate container's contents may be another seed's.

In a search, a constraint that candidates meet is **possibly** met. `--certain` keeps only seeds whose required constraints are met for certain.

## Search order and the nearest

Answers are listed in the game's search order, which isn't always nearest first: `/locate` can skip a nearer structure for one it scans first. `--nearest` answers with the nearest instead, and lists nearest first. The viewer's right-click locate shows both when they differ.

## JSON

With `--json`, every command prints JSON instead of text, for scripts. The [format is specified](https://github.com/farrcraft/spyglass/blob/main/docs/specs/cli-json.md), field by field.
