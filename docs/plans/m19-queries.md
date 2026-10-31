# m19. Features in the API, the CLI, the viewer and search

- Status: In progress
- Milestone: [19. Features](../roadmap/19-features.md)
- Started: 2026-10-30
- Completed: YYYY-MM-DD

## Goal

Features as answers everywhere structures are: in an area, nearest, in `--json`, on the map, and in both kinds of search. It meets the milestone's second and third exit criteria. `spyglass locate feature minecraft:monster_room` answers on a real seed, the viewer draws dungeons up close (1:2 and nearer; 1:4 was the aim, see the notes), and a seed search for two dungeons within 16 blocks of each other near spawn returns seeds the game confirms.

## Approach

**Queries** in the `spyglass` API, as [locate.md](../specs/locate.md) has them for structures: `Session::features_in(query, area)` and `Session::nearest_feature(query, from, radius)`. Each answer has its position, whether it's certain, and a candidate's reason from [m19-placing](completed/m19-placing.md). Features are named by placed feature (`minecraft:monster_room`, `minecraft:monster_room_deep`) or configured feature (`minecraft:monster_room` covers both). The spec decides which.

**Cost first.** Placing needs a chunk's blocks, about 20 to 30 ms each, but tries are cheap. Each query works from tries first, and places only the chunks whose tries could answer it. The plan measures a 1:4 viewer tile of dungeons and a nearest dungeon from the origin.

**The CLI:** `spyglass locate feature <id>` and `spyglass features --min --max`, with `--json` (cli-json.md).

**The viewer:** dungeon, geode, well, fossil and lava lake icons in the shared style ([ADR 0044](../adr/0044-the-viewer-draws-its-own-map-icons.md)), filled for certain and hollow for candidates. Feature layers in the Layers tab, drawn at scales up to a limit set from cost. A Locate tab entry. `render.md`'s markers for exports.

**Search:** features as targets and conditions in `find` queries ([search.md](../specs/search.md#searching-within-a-seed)), and as conditions in the seed search. "Two dungeons within 16 blocks" is a pair condition over one feature, so the query language gains it if it has none. A seed search filters seeds on tries first, then places only the chunks of the pairs that are left.

## Tasks

- [x] **API:** `features_in` and `nearest_feature`, naming, and the answers' certainty.
- [x] **CLI:** `locate feature`, `features`, cli-json.md.
- [ ] **Viewer:** icons, layers, Locate tab, view text names ([user-data.md](../specs/user-data.md)). Done: a Features section in the Layers tab (dungeons, geodes, wells, fossils, lava lakes), one job per tile at 1:1 and 1:2, diamonds with tooltips, and image export. Left: the Locate tab, share-text names, and marking features done.
- [x] **Render:** feature markers in render.md, with tiling tests. `Layers::features` at scales up to 2, `Session::render_with` so a front end finds them once, `render --features`.
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

- 2026-10-31: **Dungeons at 1:4 aren't within reach; the viewer draws them up close.** At 4 blocks a pixel a full-HD screen is about 130,000 chunks. Measured on 26.3's golden squares (512 chunks):
  - Chunk generation was the cost, and is now 3 to 5 times faster with the same blocks (the game comparisons pass in every version). A chunk's terrain is one volume instead of 256 columns, ore veins share their cells' corners (they were 38 ms of a chunk), and carvers ask the aquifer only where they carve. A 26.3 carved chunk went from about 80 ms to 17 to 30 ms, a 26.4 one from 50 ms to 20; 26.2's is unchanged at about 100 ms.
  - A dungeon chunk now takes about 29 ms, from 71, with the chunk's own feature bounds limited to the steps before the targets.
  - The pre-filter ([features.md](../specs/features.md#ruling-out)) is sound: it gives exactly the certain and likely answers of placing every chunk, in 26.3 and both 26.4 snapshots. It rules out 246 of 512 chunks at about 7 ms a chunk. But each kept chunk needs its 8 neighbours' carved blocks, so nearly every carved chunk is still built, and it saves little (27 ms a chunk against 29). Most kept chunks have a try beside a real cave.
  - Decided (with the user): commit what's measured, and draw dungeons only at 1:1 and 1:2 (about 8,000 to 30,000 chunks a screen), progressively. Unlikely candidates are left out unless asked (`FeatureOptions::unlikely`, `--all`). Further ideas, not pursued: skip placing ruled-out tries inside kept chunks, and build neighbours only when something reads them.

- 2026-10-30: **First answers.** `spyglass locate feature minecraft:monster_room --seed 0` gives the game's nearest dungeon (a certain `monster_room_deep` at 59 −12 −13) in 17 s. In two golden squares every dungeon the game placed is an answer and every certain one is the game's (`spyglass/tests/features.rs`), at about 175 ms a chunk; most of the 1,532 candidates are unlikely (they declined on spyglass's blocks).

- 2026-10-30: **Design before code.** A feature argument names every target placed feature whose id, or whose configured feature's id, is the argument (`minecraft:monster_room` both dungeons; `minecraft:fossil_coal` the upper fossils); one spyglass doesn't place is an error saying so. Each answer: the placed feature, the position, whether it's certain on its own and its reason if not, and whether it holds whatever the neighbours do ([ADR 0056](../adr/0056-two-certainties-on-its-own-and-whatever-the-neighbours.md)). Tries placed by spyglass and candidates are answers; a candidate that declined on spyglass's blocks is one too, marked unlikely. `nearest_feature` goes out in rings of chunks and places only those where the target's tries reach their feature.
