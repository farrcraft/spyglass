# m19. Features in the API, the CLI, the viewer and search

- Status: In progress
- Milestone: [19. Features](../roadmap/19-features.md)
- Started: 2026-10-30
- Completed: YYYY-MM-DD

## Goal

Features as answers everywhere structures are: in an area, nearest, in `--json`, on the map, and in both kinds of search. It meets the milestone's second and third exit criteria. `spyglass locate feature minecraft:monster_room` answers on a real seed, the viewer draws dungeons at 1:4, and a seed search for two dungeons within 16 blocks of each other near spawn returns seeds the game confirms.

## Approach

**Queries** in the `spyglass` API, as [locate.md](../specs/locate.md) has them for structures: `Session::features_in(query, area)` and `Session::nearest_feature(query, from, radius)`. Each answer has its position, whether it's certain, and a candidate's reason from [m19-placing](completed/m19-placing.md). Features are named by placed feature (`minecraft:monster_room`, `minecraft:monster_room_deep`) or configured feature (`minecraft:monster_room` covers both). The spec decides which.

**Cost first.** Placing needs a chunk's blocks, about 40 ms each, but tries are cheap. Each query works from tries first, and places only the chunks whose tries could answer it. The plan measures a 1:4 viewer tile of dungeons and a nearest dungeon from the origin.

**The CLI:** `spyglass locate feature <id>` and `spyglass features --min --max`, with `--json` (cli-json.md).

**The viewer:** dungeon, geode, well, fossil and lava lake icons in the shared style ([ADR 0044](../adr/0044-the-viewer-draws-its-own-map-icons.md)), filled for certain and hollow for candidates. Feature layers in the Layers tab, drawn at scales up to a limit set from cost. A Locate tab entry. `render.md`'s markers for exports.

**Search:** features as targets and conditions in `find` queries ([search.md](../specs/search.md#searching-within-a-seed)), and as conditions in the seed search. "Two dungeons within 16 blocks" is a pair condition over one feature, so the query language gains it if it has none. A seed search filters seeds on tries first, then places only the chunks of the pairs that are left.

## Tasks

- [ ] **API:** `features_in` and `nearest_feature`, naming, and the answers' certainty.
- [ ] **CLI:** `locate feature`, `features`, cli-json.md.
- [ ] **Viewer:** icons, layers, Locate tab, view text names ([user-data.md](../specs/user-data.md)).
- [ ] **Render:** feature markers in render.md, with tiling tests.
- [ ] **Search:** feature targets and conditions in both searches, pairs within a distance, and filtering on tries.
- [ ] **Oracle:** an ignored test that a seed search's two-dungeon seeds have both dungeons in the game's decoration.
- [ ] **Cost:** bench lines for a tile, a nearest query and the search's seeds per second. Check the viewer by eye with `run-viewer`.
- [ ] **Docs:** locate.md, search.md, render.md, getting-started, the changelog.
- [ ] `cargo xtask check` and `cargo xtask oracle-test`. Complete the plan, and the milestone.

## Verification

- `spyglass locate feature minecraft:monster_room` from the origin on a golden seed gives the game's nearest dungeon, or a candidate with its reason.
- Viewer tiles of features equal the library's, and a view equals its tiles.
- The two-dungeon search's results are confirmed by the oracle.

## Durable outputs

- [ ] locate.md, search.md, render.md, cli-json.md and user-data.md: features.
- [ ] getting-started: feature commands and layers.

## Notes

- 2026-10-30: **Design before code.** A feature argument names every target placed feature whose id, or whose configured feature's id, is the argument (`minecraft:monster_room` both dungeons; `minecraft:fossil_coal` the upper fossils); one spyglass doesn't place is an error saying so. Each answer: the placed feature, the position, whether it's certain on its own and its reason if not, and whether it holds whatever the neighbours do ([ADR 0056](../adr/0056-two-certainties-on-its-own-and-whatever-the-neighbours.md)). Tries placed by spyglass and candidates are answers; a candidate that declined on spyglass's blocks is one too, marked unlikely. `nearest_feature` goes out in rings of chunks and places only those where the target's tries reach their feature.
