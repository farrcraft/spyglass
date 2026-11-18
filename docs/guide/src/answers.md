# Reading the answers

spyglass's answers match the game's exactly, for the game versions it was checked against (`spyglass --version` lists them). Where it can't be sure of an answer, it says so.

## Certain and candidate

Each answer is **certain** or a **candidate**:

- **Certain** means the game generates it there, exactly as shown.
- A **candidate** is where the game tries to put something, but spyglass can't decide whether it generates there. The answer says why.

**Structures.** Whether a structure really generates often depends on the ground where the game tries to build it. `locate` checks the ground, as the game does, so its answers are almost always certain, and then they're exactly what the game's `/locate` says. A few stay candidates: where two biomes fit a spot exactly as well, so the game's choice can't be predicted, or where a stronghold's position can differ between computers. Then the game's answer is the first of the candidates that generates, usually the first listed.

Checking the ground takes time. Without it (`--unconfirmed`, and maps and searches unless you give `--confirm`), most surface structures, such as villages, temples and monuments, are candidates.

**Features and loot.** A feature or container is a candidate when something spyglass doesn't generate could change it, such as the tunnels of a mineshaft. A candidate container's contents may be wrong.

In a search, a constraint that only candidates meet is **possibly** met. `--certain` keeps only seeds whose required constraints are met for certain.

## Search order and the nearest

The game's `/locate` doesn't always give the nearest structure. It searches outward in square rings of its structure grid, and stops at the first ring that has one. In that ring, it takes the first structure it scans, though one later in the ring, or in the next ring out, can be nearer. spyglass lists answers in the game's order, so its first answer is the game's. `--nearest` gives the nearest one instead, and lists nearest first. The viewer's right-click locate shows both when they differ.

## JSON

With `--json`, every command prints JSON instead of text, for scripts. The [format is specified](https://github.com/farrcraft/spyglass/blob/main/docs/specs/cli-json.md), field by field.
