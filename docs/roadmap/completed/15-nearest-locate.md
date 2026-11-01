# 15. Nearest locate

- Status: Completed
- Depends on: [07](07-terrain-and-validity.md)
- Plans: [m15-nearest-locate](../../plans/completed/m15-nearest-locate.md)

## Goal

"Where's the nearest X?" gets the nearest X. Today spyglass answers only as the game's `/locate` does, and `/locate` doesn't always return the nearest structure: it scans rings of placement cells and keeps the first hit in each ring's scan order ([locate.md](../../specs/locate.md)). For seed 12345 in 26.3, `/locate minecraft:ancient_city` from the origin gives −288, −288, 407 blocks away, though a city at 176, −176 is 249 blocks away and generates. Keep `/locate`'s answer, since matching it is how players check a seed, and add the nearest as a second answer that front ends offer side by side.

## Scope

- **The library:** a locate mode, `/locate`'s (the default, as now) or nearest. Nearest searches until no unsearched cell can hold anything nearer than the best certain start, over the same radius limits, and reports candidates nearer than that start the same way `/locate`'s mode does. Biome locate is unaffected: `/locate biome` already searches outward by distance.
- **Distance:** to the `/locate` position, horizontally, as `/locate` measures. Ties are broken in a stated order, so answers are deterministic.
- **Concentric rings and mixed targets:** strongholds, and tags whose sets differ in spacing, need the same "nothing nearer is left" stopping rule.
- **The CLI:** `spyglass locate structure --nearest`, with `--json` saying which mode answered.
- **The viewer:** the Locate tab shows the nearest and `/locate`'s answer when they differ, labelled, with a pin for each.
- **The spec:** locate.md gains the nearest mode, its stopping rule and its tie order.

Not in scope:

- Changing `/locate`'s answers. They stay the game's, and stay checked against it.
- The nearest by path or travel distance. This is straight-line distance.

## Exit criteria

- [x] On the real bundle, for each golden seed and every structure the locate golden cases ask about, the nearest mode's answer is a certain start, and `structures_in` over the disc it claims holds no nearer certain start.
- [x] Where the two modes differ in those cases, the nearest is no farther than `/locate`'s answer. Every difference is listed in the plan, with the ring rule that explains it.
- [x] `/locate` mode's answers are unchanged: `cargo xtask oracle-test` passes for every supported version.
- [x] `cargo xtask check` passes.
