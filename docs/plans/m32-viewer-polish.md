# m32. Viewer polish

- Status: In progress
- Milestone: [32. Viewer polish](../roadmap/32-viewer-polish.md)
- Started: 2026-11-16
- Completed:

## Goal

Make the viewer clear about what each zoom can show, stop work the view no longer needs, compute nothing twice when the user zooms, and turn the side panel, all five of its tabs including m31's **World** tab, into clear sections that fold.

This plan covers all seven of the milestone's exit criteria:

- every layer with a zoom limit greyed past it, with hover text, and kept on but paused;
- running jobs stopped within half a second of zooming out past their limit, measured, with a test that the stop reaches the library;
- no chunk placed twice zooming from 1:2 to 1:1 with features on, and the zoom timing scenarios faster, none slower;
- tiles built from cached ones identical to tiles computed directly, a test per layer that reuses;
- side panel sections that fold, remember it across a restart, and say what's on when folded;
- every single-line text box padded the same, in a menu or not, and checkboxes in menus drawn as in a panel;
- the guide's [viewer](../guide/src/viewer.md) page showing the new panel, zoom limits and folding.

It changes no worldgen. The new library code reorders no arithmetic and no random draws: stopping only ends a loop early, and reuse copies pixels the spec already says are equal. So no oracle comparison is needed beyond `cargo xtask check`. Each reuse rule gets a test against direct computation instead.

## What the viewer does today

From `crates/spyglass-viewer/src/` and [render.md](../specs/render.md):

- **Zoom limits** are checked in four places with three different behaviours:
  - Slime chunks and generated chunks (`MAX_SLIME_SCALE`, 16) are disabled with `add_enabled`, plus a hint line.
  - Underground (`MAX_UNDERGROUND_SCALE`, 4), features and loot (`MAX_FEATURE_SCALE`, 2) stay clickable, and add a hint line only once something is on.
  - Structure bounds (the viewer's own `MAX_BOUNDS_SCALE`, 4) are only mentioned in the hint under **Confirm with terrain**.
  - Each limit's text is typed out by hand ("Zoom to 1:4 or closer to see them.").
- **The pool** (`tiles.rs`) has one worker per core but one. `Pool::want` replaces the waiting list each frame. A running job always finishes, and its result goes into a cache even when nothing wants it any more. An error is cached like a result.
- **The library's map calls** (`biome_map`, `height_map`, `surface_biome_map`, `underground_map`, `carving_map`, `flower_map`, `map_features`, `containers_in`, `containers_holding`, `structure_layout`) take no stop signal. Only searches do (`SearchStop`, `spyglass_search::Stop`).
- **Caches** are least-recently-used, by count: 1,024 tiles (`CACHE_TILES`), 512 each of markers, features and loot (`CACHE_MARKERS`), 512 layouts, and 256 carving maps in the pool. Every key holds a `Tile`, which includes the scale. So nothing computed at one scale is used at another, except that a coarser tile is scaled up as a stand-in while a finer one loads.
- **Points** are kept per tile. A features or loot tile places every chunk its markers could come from, so neighbouring tiles place their shared edge twice. A 1:2 tile (16 × 16 chunks) and the four 1:1 tiles under it place the same chunks again.
- **The side panel** has five tabs: Layers, Search, Locate, Markers and World. World appears only while a save is shown, or one that couldn't be shown. The selected tab is marked by its text and a 2-pixel underline. Locate, Markers and World show counts; Search shows a dot while it runs. Sections are `style::heading` labels separated by `add_space(6.0)`. The World tab alone uses `egui::CollapsingHeader` (Settings, Data packs, Dimensions, This map so far), always open at first, and its state isn't remembered. Search has one collapsing **Seed range and start**.
- **Viewer timing** (`xtask/src/viewer_timing.rs`) has five cold-start scenarios. None of them zooms.

## Approach

Five parts: the zoom rule, stopping, reuse, the side panel, and the measurements and guide.

### One zoom rule

A new `app/zoom.rs` has one table, `ZOOM_LIMITS`, with one row per layer that has a limit:

| Layer                            | Limit                                   | From    |
| -------------------------------- | --------------------------------------- | ------- |
| Ore veins, noise caves, carvers  | `spyglass::MAX_UNDERGROUND_SCALE` (1:4) | library |
| Features (each kind)             | `spyglass::MAX_FEATURE_SCALE` (1:2)     | library |
| Loot containers, ringed items    | `spyglass::MAX_FEATURE_SCALE` (1:2)     | library |
| Slime chunks                     | `spyglass::MAX_SLIME_SCALE` (1:16)      | library |
| Generated chunks, by time played | `spyglass::MAX_SLIME_SCALE` (1:16)      | library |
| Structure bounds                 | `MAX_BOUNDS_SCALE` (1:4)                | viewer  |

Structure bounds are the viewer's own limit, since the library draws layouts at any scale. Generated chunks share slime chunks' constant today. If they should have their own, the library gains `MAX_GENERATED_SCALE` with the same value, so the table never copies a number.

`LayerState::of(limit, on, scale) -> {Shown, Paused, Off}`, together with `limit.hint() -> "Zoom to 1:2 or closer"` built from the constant, is the only place the rule lives. Every control with a limit goes through one helper, `zoom::checkbox(ui, &mut on, label, limit, scale)`. It works the same way for every layer:

- Past the limit the control is greyed (`add_enabled(false, …)`), but it keeps its value. egui draws a disabled checkbox's tick, so a layer that's on stays visibly on.
- The hover text, disabled hover included, says how far to zoom in.
- A paused layer shows "paused" in the muted colour beside its label.
- The section's header gets the same note ([below](#the-side-panel)).
- Zooming back in draws the layer again. Nothing in `Settings` changes, so the choice is kept across a restart too.

The existing "Only in the Overworld" and "Open a world to see its chunks" reasons are other kinds of unavailability. They stay as they are, but go through the same helper, so a control that's unavailable for both reasons gives the dimension's reason first.

`Frame`'s `featuring`, `looting` and `underground` flags, and the slime and generated checks in `compute`, read `LayerState` too. So the map, the panel and the jobs can't disagree.

### Stopping work

**In the library.** An ADR records the API shape. The proposal:

- `spyglass_search::Stop` becomes the crate's general stop handle, re-exported as `spyglass::Stop`. `SearchStop` stays one release as a deprecated alias.
- `Session::stopping(&self, stop: &Stop) -> Stopping<'_, 'w>` returns a borrowed view of the session with the same map calls: `biome_map`, `height_map`, `surface_biome_map`, `underground_map`, `carving_map`, `flower_map`, `map_features`, `containers_in`, `containers_holding` and `structure_layout`.
- Inside, each call's private implementation takes an `Option<&Stop>`, and the existing public calls pass `None`. So callers that don't stop see no change.
- A stopped call returns `Error::Stopped`, a new variant; `Error` is already `#[non_exhaustive]`.

The calls check the stop where they already loop:

- per chunk in `each_feature` and the container walks (about 30 ms each);
- per chunk of heights (a few ms);
- per chunk carved;
- per row of pixels in the biome, underground and flower maps.

The slowest unit today is a 1:64 height tile's row, about 160 ms ([render.md](../specs/render.md#heights)), well inside half a second. The parallel row in `each_feature` checks the stop before it starts each chunk, so a stop doesn't wait for a whole row of chunks.

**In the pool.** `Queue::running` becomes `HashMap<JobKey, Stop>`. `Pool::want(jobs)` raises the stop of every running job whose key isn't among `jobs`, comparing against the list as given, before it filters out what's running. A job is unwanted when:

- the view zoomed past its layer's limit;
- the view moved so far that its tile isn't in view or among the coarse stand-ins;
- its layer was switched off;
- the seed, world or selection changed.

All of these already drop the job from the frame's list, so the one rule covers every case. A worker whose job returns `Error::Stopped` sends it back marked as stopped, and `take_finished` drops it: a stopped job is never cached, unlike a real error.

With `SPYGLASS_VIEWER_TIMING=1` the pool logs `timing: stopped <kind> after <ms> ms`, the time from the stop being raised to the worker being free. That gives the exit criterion's measurement.

### Reuse across zoom levels

#### Points

Features, loot containers and structure starts are kept per **chunk region**, not per tile. A region is a square of chunks, independent of the view's scale. Each point belongs to the region of the chunk that placed or started it, so each is in exactly one region and is drawn once. This replaces today's "the tile its centre is on" rule.

- **Features and loot:** regions of 16 × 16 chunks, the size of a 1:2 tile. A 1:2 tile is one region and a 1:1 tile a quarter of one, so 1:2 → 1:1 needs nothing new. (Drafted as 8 × 8; measured slower, see the notes.) A region's job places only its own chunks: `features_in` and `containers_in` over the region's chunk range. Chunks that reach in from outside are other regions' jobs.
  - This depends on what each call returns. `features_in` returns what the chunks in range placed. `loot_marks` today widens the range by one chunk, because a chest can be beside the chunk that places it. A task checks that `containers_in` and `containers_holding` give each container under the chunk that placed it, not by its position. If they give it by position, they gain a by-placing-chunk form.
  - Drawing asks for the regions under the view plus one ring, so a marker whose centre is just outside still shows where it reaches in.
- **Structure starts:** regions of 32 × 32 chunks, a 1:4 tile. Starts are cheap per chunk, but confirming them with terrain isn't, and 1:4 → 1:1 is the common zoom while confirming. At scales coarser than 1:4, each tile is one region of its own chunks, with no ring, so a 1:256 view doesn't queue thousands of region jobs. (Drafted as 1:16; measured too many regions for the cache, see the notes.)
- **Layouts** are already keyed by start chunk, not scale, and are unchanged.

`MarkerKey`'s and `FeatureKey`'s `tile` becomes a `Region { size, x, z }`. Region jobs are ordered nearest the centre first, as tiles are.

**Past a layer's limit.** The milestone asks the plan to decide whether points already computed should be drawn further out than the limit, with the area they cover shaded. This plan builds it behind a viewer constant and decides by eye with the `run-viewer` skill:

- Past the limit, only regions already cached are drawn, up to 4× the limit (1:8 for features and loot).
- Each drawn region is outlined, and the rest of the view gets a faint hatch, so an empty region reads as "nothing here" and an unknown one as "not looked at".
- Nothing new is queued.
- The layer's control still shows "paused: zoom in", with a hover line saying that what's drawn was found earlier.

If two people can't tell at a glance which places were searched, it's switched off and the note says why. Either way the decision goes into the render spec's viewer notes, not only into this plan.

#### Pixels

[render.md](../specs/render.md) says every biome, underground and flower pixel shows its corner block alone. A pixel at scale `s` with corner `(x, z)` is the same pixel at scale `s/2`, `2s` or any scale where `(x, z)` is a pixel corner. So a 1:2 tile's pixels are every other pixel, on both axes, of the four 1:1 tiles under it. The milestone excluded relief and surface biomes as depending on neighbouring columns. The spec says otherwise:

- Heights are each pixel's own column (`WORLD_SURFACE_WG` at the corner block).
- Surface biomes are the biome at that column's height.
- Relief is drawn at texture time (`Style::relief`), from the height map's pixels plus one row and column beyond its west and north edges.

So all of them reuse, provided the edge row is carried. A coarser tile's edge row is the column `2s` to the west, which is a pixel of the neighbouring finer tile. The edge row is taken from there when it's cached, and computed (one row, a few chunks) when not.

The one rule, in the library so both front ends can rely on it:

- `spyglass::Known` holds pixel values already known for an area, gathered from maps of the same `y` and layers at other scales.
- `Known::from_finer(area, &[maps])` subsamples finer maps.
- `Known::from_coarser(area, &map)` copies the quarter of a coarser map the area shares.
- The map calls gain `…_with(area, &known)` forms that compute only the pixels `known` lacks. A map that `known` covers whole is copied and nothing is sampled.

This covers biome maps (the tie of each pixel included), height maps (the edge row included), surface biome maps, underground maps, flower maps and carving maps. The carving maps are by far the most expensive part of the underground layers, and are also per pixel.

The viewer looks for known pixels before queueing a tile:

- **Zooming out:** a coarse tile whose four finer tiles are cached is assembled on the UI thread's budget, with no job (a copy of 128 × 128 values per layer). One whose children are partly cached becomes a job that computes only the rest. (Done as a job at the front of the queue instead, and no tile starts from its parent: see the notes.)
- **Zooming in:** a fine tile whose parent is cached starts from the parent's quarter, a quarter of its pixels.

Whether that start is worth a job's bookkeeping depends on the layer. Biome lookups are cached per quart, and heights are sampled a chunk at a time, so skipping one column in four may save little. A task measures each layer's saving for the 1:2 → 1:1 start. Layers that save less than 15% skip the finer-from-coarser path, and the spec says why. Assembling coarser from finer is kept for every layer, since it skips the work entirely.

The coarse stand-ins (`view::coarse`) are unchanged: a scaled-up parent stays the placeholder while the exact tile is built.

### Memory

The caches stay bounded by count, and the counts are set from measurements, not guessed. A task measures:

- `TileData`'s size per layer combination (biomes, plus heights, underground, flowers, carving maps);
- a region's points at 1:1 in a dense area (features in a badlands–dripstone border, loot near a village and a trial chamber);
- the viewer's resident memory after the zoom scenarios.

Two candidate changes are measured and kept only if they help:

- `CACHE_TILES` becomes a byte budget (`Cache::with_budget`, each entry's `size()` counted), since an underground tile is several times a biome tile.
- The region caches get their own counts, sized so that the view at 1:2, plus one zoom level either way, fits at 4K.

The limits and the measured sizes go into the render spec, not only this plan.

### The side panel

**Tabs.** All five, in today's order: Layers, Search, Locate, Markers, World.

- The selected tab gets a filled `style::RAISED` background and the accent underline. The others are muted text with no frame, so the selection reads at a glance and not only by its weight.
- Counts go in a small rounded badge after the name, not in brackets:
  - Search: the finds in the seed shown, or hits of the last seed search, with a spinner while either runs instead of today's dot.
  - Locate: hits.
  - Markers: live markers in this dimension.
  - World: warnings, in the warning colour, with notes not counted, as today.
- The row wraps to a second line when the panel is narrower than the five tabs. A test at the panel's minimum width checks that no tab is cut off.
- The World tab keeps its rules from m31: shown only while `save_report()` is some, and the panel falls back to Layers when it goes.

**Sections.** One helper, `style::section(ui, id, title, summary, |ui| body)`, replaces both `style::heading` + `add_space` and the World tab's `CollapsingHeader`s:

- The title is drawn larger and stronger than any control's label.
- The section has a fold chevron, and a 1-pixel `style::BORDER` rule above it.
- The body is indented by the same amount everywhere, with one spacing constant between sections and another between controls.

The sections, by tab:

- **Layers:** Biomes, Overlays, Underground, Flowers, Features, Loot, Structures, and **Confirm with terrain** as a section of its own instead of a footer.
- **Search:** Query, Options, and Seed range and start (folded by default, as now).
- **Locate:** Nearest, /locate gives.
- **Markers:** This world, Markers, Done.
- **World:** Settings, Data packs, Dimensions, This map so far.

The Markers tab's Saved and Recent headings are in the worlds menu, not the panel, and aren't sections.

**Folding.** `Settings` gains `folded: BTreeSet<String>`, keyed by stable section ids (`layers.underground`, `world.packs`, …). It's saved with the rest through eframe's storage, so it survives a restart. Old settings files without the field load with nothing folded (`#[serde(default)]`). The World tab's sections share one fold state across saves, since its ids name the section and not the save.

**What a folded section says.** Each section supplies a `summary` shown after its title when folded, and also when open where it helps:

- **Layers:**
  - how many of its layers are on ("2 on");
  - which are paused ("paused: zoom to 1:2");
  - "on, paused" when every layer that's on is paused;
  - for Structures, the number chosen and in view;
  - for Biomes, the height and the highlighted count.
- **World:**
  - its warnings ("2 warnings"), in the warning colour, so a fold never hides a warning;
  - for This map so far, chunks read.
- **Search, Locate, Markers:** counts (finds, hits, markers, done).

When **What spyglass read** in the notice row opens the World tab, it unfolds the sections that hold a warning. The user asked to see them.

**Text boxes.** In a menu, a single-line text box's text sits in its top-left corner, against the border. egui takes a text box's padding from `spacing.button_padding`, and its menus shrink that to 2 × 0 (`egui::containers::menu::menu_style`). It shows in the three boxes inside menus: **Filter saves**, **Filter biomes** and the shared view's text. Boxes in panels are padded by the theme and look right.

One helper, `style::text_box(&mut String) -> TextEdit`, builds a single-line box with the theme's padding (`BUTTON_PADDING`, the one `style::apply` sets) and its text centered vertically, and all twelve single-line boxes go through it, so a box looks the same wherever it is. The two multi-line boxes (the query and the find) are in panels and unchanged.

**Checkboxes in menus.** The same menu style takes away widgets' resting fill and outline, so a menu's rows don't read as buttons, and that leaves an unchecked checkbox almost invisible: in **Highlight a biome** and the **Export** menu's **Include markers**. `style::menu_checkbox` draws a checkbox in a menu with the panel's widget fills and outlines, and both go through it.

### Measurements and the guide

**Viewer timing** gets four zoom scenarios. Each is a cold start, run until the window is full, then a zoom set by `SPYGLASS_VIEWER_TIMING_ZOOM=<scale>`, read once after the first `window full`. Timing then reports the zoom's own `window covered` and `window full`.

- 6: 1:2 → 1:1 with every feature kind on (exit criterion 3; the log's `features` job lines also count chunks placed).
- 7: 1:1 → 1:4 with biomes at the surface and relief (assembly from finer).
- 8: 1:4 → 1:1 with all three underground layers at y 0 (the finer-from-coarser start, and carving maps).
- 9: 1:2 → 1:8 with features and loot on (the stop: the `stopped` lines give the time from the stop to the free worker).

Scenarios 1 to 5 are run before the change and after it, three times each. Neither the cold starts nor the zooms may get slower. The numbers go into this plan's notes, in the form of [milestone 14's results](../roadmap/completed/14-viewer-performance.md), and a summary into render.md.

**Guide.** [viewer.md](../guide/src/viewer.md) gets:

- a short **The side panel** part: the tabs and their counts, sections and folding, and what a folded header says;
- a **Zoom limits** part with the table above in user terms, what "paused" means, and what's drawn past a limit if that ships;
- new screenshots, taken with the `run-viewer` skill from fixed settings (`viewer-overworld.png` with the new panel, and a new `viewer-zoom.png` of a paused layer). The before-and-after pair goes in this plan's notes.

## Tasks

- [ ] Spec first. [render.md](../specs/render.md) gets:
  - **Reuse across scales:** which maps reuse, the corner-block argument, the heights' edge row, and the 15% rule for finer-from-coarser;
  - **Stopping:** the stop handle, where each call checks it, and `Error::Stopped`.
  - The milestone's scope is reworded: relief and surface biomes do reuse, and the side panel names its five tabs.
- [x] ADR: map calls take a stop signal (`Session::stopping`, `spyglass::Stop`, `Error::Stopped`, `SearchStop` deprecated). Use the `adr` skill.
- [x] Library: `Stopping` and the stop checks in every call listed, with a test per call that a stop raised before the call returns `Stopped` at once, and one raised mid-call (from another thread, after 50 ms) returns it within 500 ms on a 1:64 heights tile, a 1:4 all-underground tile and a 1:2 features tile.
- [x] Viewer pool: running jobs' stops, `want` raising them, `Stopped` never cached, and the `stopped` timing line. A test with a real session: a 1:2 features job, then `want(vec![])`, and the job comes back stopped within 500 ms.
- [x] The zoom rule: `zoom.rs`, `ZOOM_LIMITS`, `LayerState`, `zoom::checkbox`, every limited control through it, and `Frame` and `compute` reading it. Unit tests: each layer at its limit, one step past, and back; a paused layer keeps its setting; the hint text from each constant. `MAX_GENERATED_SCALE` in the library if generated chunks get their own.
- [x] Check `containers_in` and `containers_holding`: whether each container comes under the chunk that placed it. Add a by-placing-chunk form if they go by position, with a test that a region's containers are the same as today's tiles' with the edges removed.
- [x] Point regions: `Region` keys for features, loot and starts, region jobs nearest the centre first, drawing from the regions under the view plus a ring, and per-tile starts past 1:16. Tests:
  - the markers drawn at 1:2 and 1:1 over one area are the same set as today's per-tile results;
  - zooming 1:2 → 1:1 queues no features or loot job;
  - no marker is drawn twice.
- [x] Measure the structure-region cut (1:16) on scenario 4 and the 1:256 view, and move it if needed.
- [x] `Known` and the `…_with` map calls for biomes, heights (edge row), surface biomes, underground, flowers and carving. A test per map, at several seeds, scales 1 to 16, both directions, and a tile only partly known:
  - assembled or started from known pixels == computed directly, pixel for pixel;
  - ties and the edge row included;
  - with every pixel known, nothing is sampled.
- [x] Viewer reuse: assembly on the UI thread's budget, partial jobs, and the finer-from-coarser start. Measure each layer's saving and drop the start where it's under 15%.
- [x] Past the limit: draw cached regions with outlines and the hatch, behind a constant. Check by eye and decide, with the decision and why in the notes and render.md.
- [x] Memory: measure tile and region sizes and resident memory, decide on a byte budget, set the counts, and record them in render.md.
- [ ] The side panel: `style::section`, the five tabs' new look and badges, wrapping, every section listed through the helper, `Settings::folded`, the summaries, and **What spyglass read** unfolding warned sections. Tests:
  - fold state survives a settings round trip, and old settings load;
  - each Layers summary for on, paused and off;
  - a World section with a warning says so folded.
- [x] Text boxes: `style::text_box`, and all twelve single-line boxes through it. A test that its padding in a menu is egui's in a panel, within a pixel, and one that egui's own box in a menu isn't. Checked by eye in **Filter saves** and **Filter biomes**.
- [x] Checkboxes in menus: `style::menu_checkbox` for **Highlight a biome** and **Include markers**, with a test that it restores every widget state's fill and outline. Checked by eye in **Highlight a biome**.
- [x] Viewer timing: `SPYGLASS_VIEWER_TIMING_ZOOM` and scenarios 6 to 9. Run 1 to 9 three times before the change and after, on the same machine and seed, and record the times.
- [ ] Check by eye with the `run-viewer` skill:
  - each limited layer greyed and paused past its limit, and back;
  - zooming out stops the spinner's jobs;
  - folds remembered across a restart;
  - the World tab's sections on "Modrinth Latest upgraded" and "Terralith Test" (from m31's notes);
  - before-and-after screenshots.
- [ ] Guide: [viewer.md](../guide/src/viewer.md)'s side panel and zoom limits, new screenshots. The changelog notes `spyglass::Stop`, `Session::stopping`, `Error::Stopped`, the `…_with` map calls and the deprecated `SearchStop`.
- [ ] Tick the milestone's exit criteria, and complete the plan and milestone.

## Verification

- **Zoom rule:** unit tests on `LayerState` per layer, and by eye on every limited control.
- **Stopping:**
  - library tests that each map call stops, within 500 ms mid-call on the slowest tiles;
  - a pool test that unwanting a running job stops it, and that a stopped result isn't cached;
  - scenario 9's `stopped` lines for the measured time.
- **Reuse:**
  - per map, assembled and started tiles equal to direct ones, pixel for pixel, ties and edge rows included;
  - region markers equal to today's per-tile ones;
  - scenario 6 placing no chunk twice (its job lines show no features job after the zoom).
- **No regressions:** viewer timing 1 to 5 before and after, none slower. `cargo xtask bench` for the map calls, since the stop checks sit in their loops, none slower beyond noise.
- **Memory:** resident memory after scenarios 6 to 9 recorded, and the caches' bounds tested (`Cache` with a byte budget drops the oldest first).
- `cargo xtask check` passes.

## Durable outputs

- [ ] [render.md](../specs/render.md): reuse across scales and why it's exact, stopping, the cache bounds, and what the viewer draws past a layer's limit.
- [ ] An ADR for stoppable map calls.
- [ ] The user guide's [viewer.md](../guide/src/viewer.md): the side panel, folding, zoom limits, and new screenshots.
- [ ] The changelog: the new public API and the deprecation.
- [x] The viewer-timing results for scenarios 1 to 9, before and after.

## Notes

Newest first.

- 2026-11-17: Viewer timing and memory. `SPYGLASS_VIEWER_TIMING_ZOOM=<scale>[,covered]` zooms once, when the window is first full or first covered; "full" now also waits for the feature and loot regions drawn, ring included, or scenario 6 would be full before its dungeons. `cargo xtask viewer-timing` has scenarios 6 to 9 and prints the viewer's working set at the end and its peak. Scenario 9 zooms once covered, since at full nothing runs to stop. 26.3, seed 0, 1280 × 800, this machine, three runs each; "before" is the m31 tip (`b4a0de7`) in a worktree, which has scenarios 1 to 5 only. A first set of runs had scenarios 2 and 3 at up to 10.7 and 16.0 s with each heights job at 2.2 to 3.4 s, the machine busy; rerun quiet:

  | Scenario                                 | Before: full  | After: covered | After: full   | Peak memory |
  | ---------------------------------------- | ------------- | -------------- | ------------- | ----------- |
  | 1: 1:4, biomes                           | 0.22–0.35 s   | 0.07–0.10 s    | 0.22–0.24 s   | 299 MB      |
  | 2: 1:4, at the surface                   | 8.66–8.99 s   | 0.06–0.07 s    | 8.26–8.60 s   | 329–335 MB  |
  | 3: + relief, swamp huts                  | 8.48–8.68 s   | 0.06–0.07 s    | 8.38–8.49 s   | 326–332 MB  |
  | 4: 1:16, villages                        | 0.25 s        | 0.08–0.11 s    | 0.24–0.27 s   | 302–311 MB  |
  | 5: 2 during a search                     | 15.54–34.32 s | 0.08–0.12 s    | 13.50–14.85 s | 325–332 MB  |
  | 6: 1:2, every feature                    | –             | 0.08–0.10 s    | 212–232 s     | 760–769 MB  |
  | 6: zoomed to 1:1                         | –             | 0.02 s         | 0.07 s        |             |
  | 7: 1:1, surface and relief               | –             | 2.18–2.28 s    | 2.20–2.28 s   | 379–414 MB  |
  | 7: zoomed to 1:4                         | –             | 0.07–0.12 s    | 8.39–9.04 s   |             |
  | 8: 1:4, every underground layer at y 0   | –             | 0.08–0.12 s    | 14.40–15.71 s | 421–436 MB  |
  | 8: zoomed to 1:1                         | –             | 0.08–0.10 s    | 1.85–2.12 s   |             |
  | 9: 1:2, features and loot, zoomed to 1:8 | –             | 0.07–0.09 s    | 0.22–0.24 s   | 378–380 MB  |

  None slower. Scenario 6's zoom placed no chunk: no `features` job after it. Scenario 7's zoom computed 46 to 49 of its tiles, since only the middle sixteenth of a 1:4 view was cached at 1:1. Scenario 9's 45 stops freed their workers 7 to 55 ms after the stop. Memory: the tile cache stays a count of 1,024 (a 4K screen is 510 tiles at one scale); a tile is about 100 KB with biomes and its texture, 320 KB with every layer (32 KB biomes at 1:4 and out, 65 KB heights, 96 KB underground, 64 KB flowers, 64 KB texture), so 100 to 330 MB at most. A byte budget would only bite with every layer on at 4K, and there it would drop tiles on screen, so it's not used. Regions measured with a scratch example on seed 12345: around a village, 36 features (4 KB) and 30 containers (104 KB); around a trial chamber, 21 features (2 KB) and 80 containers (127 KB), the containers counted as their debug text, so on the high side; 1,024 regions of each are kept. Peak memory is mostly the library's: 770 MB placing every feature kind at 1:2. Recorded in [render.md](../specs/render.md#reuse-across-scales).

- 2026-11-17: Past the limit, decided by eye: kept, to 1:4 rather than 1:8. `zoom::DRAW_FOUND` and `zoom::FOUND_REACH`. Past their limit, features and loot draw what the region caches hold, nothing new is asked for, regions looked at are outlined where they meet ones that aren't, and those are hatched; the controls' hover text and the section's line say what's drawn was found earlier. Checked with the `run-viewer` skill, seed 12345, dungeons at 1:2 with the ring still filling, then 1:4 and 1:8. At 1:4 the searched area reads at a glance: an outline round it, the hatch outside, and the outlined gaps between dungeons read as none there. At 1:8 the dungeons inside pile into one grey blob and the hatch covers nearly the whole map, so `FOUND_REACH` is 2. Recorded in [render.md](../specs/render.md#reuse-across-scales).
- 2026-11-17: Viewer reuse. Measured first, with a scratch example on 26.3 (three seeds, 128 × 128 tiles, each after its source in the same session, as the viewer has them). Started from the parent's quarter, a tile saved: 1:1 from 1:2, biomes −1%, heights 1%, surface −4%, flowers 9%, carving 2%, underground 5%; 1:2 from 1:4, biomes 12%, flowers −9%, carving −14%, the rest 2%; 1:4 from 1:8, all within 2%. Every layer is under 15%, so there's no finer-from-coarser start at all. Assembled from finer tiles, a coarse tile saved 88–100% per layer; its known pixels and maps took about 6 ms in all, flowers 5 of them. So assembling isn't done on the UI thread, as drafted: a tile or two a frame would take most of a second over a 70-tile view, where the workers do it in a few frames. A tile its finer tiles cover whole is a job at the front of the queue, ahead of the coarse stand-ins; one covered in part is a job with the pixels it has. Tile data is shared (`Arc<TileData>`) so a job can carry the tiles it takes pixels from; a tile serves if it has the layers needed (heights and flowers if needed, the same underground layers if any), looked up without marking it used. Carving maps are assembled the same way in the workers' own cache. Checked with the `run-viewer` skill, seed 12345, relief at 1:2, then zoomed out: the 1:4 tiles under the old view took 0.1 ms for heights and 0.2 ms for biomes, those in part about 15 ms, and the view was full in 9.2 s against 12 s for 1:2; no seams by eye. Tests: a 1:2 tile from all its finer tiles and from two equals the tile computed, at y and at the surface, heights with their edge row; a tile's carvings from the four under it equal its own; which cached tiles serve a key.
- 2026-11-17: `Known` and the `…_with` map calls ([render.md](../specs/render.md#reuse-across-scales)). `spyglass::Known<M>` is one type over the six maps, through a sealed trait, with `Known::new(area)` and `Known::add(map)` taking a map at any scale: each pixel whose corner block is one of the map's corners, a height map's edge row included. That covers finer, coarser and partly overlapping maps at once, so there's no `from_finer` or `from_coarser`. A map records its kind so `add` refuses another: biome maps their height or that they're at the surface, underground maps their height and layers, flower maps their height; heights and carvings go at any height. The existing calls are the `…_with` forms with nothing known, built through the same code, so a map is the same struct whatever was known (the biome grid at scales 1 and 2, a vein's ore and a flower's answer numbered in pixel order). Heights and probes still go a band of rows at a time, rows lacking the same columns together; carving visits only chunks under missing pixels. With every pixel known, nothing is computed and the stop is never checked, which is what the tests use to tell. Tests: on fixtures at three seeds, biomes, heights and surface biomes at 1 to 16, and carvings and underground at 1 to 4 at two heights, from a finer area with its edge, the west part of one and a coarser one, equal to direct, ties, veins, noise caves and carvers present; with every pixel known, made with a raised stop; kinds, areas and worlds that don't match refused. The same on 26.3 at two seeds, flowers included (ignored test). `cargo xtask bench`, before and after, twice in each order: the first pair ran on a busy machine (before slower everywhere, `World::open` 1.4 s against 0.3 s); the second, map calls within noise either way: heights 1:1 1.22 → 1.16 s, 1:4 6.42 → 6.56 s, underground 1:4 2.99 → 3.04 s, flowers 1:16 80 → 74 ms, biome_map 1:1 9 → 13 ms (17 → 15 ms in the first pair).
- 2026-11-17: Structure starts by region. Each region's job asks `structures_in` over its own chunks, so a start belongs to the region of its chunk. The library gained `Stopping::structures_in`, and `StructureQuery::with` so the chosen structures are asked as one query, as `map_features` asks them: asked one at a time, a start two of them name would be listed twice, and couldn't be known certain when only together they cover every try. The 1:16 cut didn't hold: a 1280 × 800 view at 1:16 is over 1,100 regions of 32 chunks with the ring, more than the 512 the markers cache held, so it evicted what it drew and scenario 4 never filled in 180 s. The cut moved to 1:4, the bounds' limit, where starts are confirmed and zoomed into. Past it each tile is one region of its own chunks, and there's no ring, as by tile before. `CACHE_MARKERS` went to 1,024 for each kind, since a 4K view at 1:2 is about 600 feature regions; the memory task revisits it. Three runs each, before and after, seed 0, 26.3: scenario 3 full in 7.62–8.87 s before and 8.26–8.55 s after, with markers 0.01 s of work either way, so terrain sets it; scenario 4 full in 0.24–0.26 s both. At 1:256 starts are still kept by tile, so it wasn't timed again. Tests: zooming 1:4 → 1:2 → 1:1 needs no new start region; past 1:4 each region is its tile's chunks; on a fixture at 1:2 and 1:32 no start is in two regions, and every start a tile drew before is in one. Zooming 1:2 → 1:1 asking for no new features or loot is the regions test from yesterday: a 1:1 view's regions are among a 1:2 view's, and the jobs are asked only for regions not cached.
- 2026-11-16: Features and loot by region. Each region's job runs `features_in` or `containers_in` over its own chunks, so each feature or container is in the region of the chunk that placed it, drawn once, and the viewer draws the regions under the view and a ring. Over a whole 1:2 view (80 tiles, seed 12345, dungeons), one job after another: 401 s of work by tile, 672 s in regions of 8 chunks, 357 s in regions of 16. A region's job carves the chunks around its own, so small regions repeat that. Regions are 16, a 1:2 tile; a 1:1 tile is a quarter of one, so zooming in needs nothing new. Checked by eye: dungeons drawn at 1:2 were all there at once at 1:1. Tests: a 1:1 view's regions are among a 1:2 view's; on 26.3 every feature `map_features` draws for a tile is in a region, and none is in two.
- 2026-11-16: The zoom rule. `zoom.rs` sits at the crate root, not under `app/`, since `tiles.rs` checks the slime and generated limits too. Generated chunks keep the library's `MAX_SLIME_SCALE`, which `map_features` checks them against, so no `MAX_GENERATED_SCALE`. A section with a paused layer says "Paused: zoom to 1:4 or closer." under it until the folding sections give it a header note. Structure bounds have no checkbox, so the **Confirm with terrain** hint takes its number from `zoom::BOUNDS` and says when bounds are paused. **By time played** is greyed with its own reason while generated chunks are off. Checked by eye at 1:2 → 1:16 → 1:2 with ore veins and dungeons on: greyed with "paused" beside them, the hover text says how far to zoom in, and both draw again back at 1:2.
- 2026-11-16: The pool stops running jobs. `Pool::want` raises the stop of every running job its list leaves out, so zooming past a limit, panning away, a layer switched off and a new seed all stop work the same way. A stopped job sends nothing, so nothing caches it, and logs `timing: stopped <kind> after <ms> ms`. Jobs compute through `Session::stopping`; layouts don't stop. On 26.3 a features job at 1:2 freed its worker well within 500 ms of being unwanted, three runs (ignored test `a_running_job_stops_within_half_a_second`).
- 2026-11-16: The library stops ([ADR 0078](../adr/0078-map-calls-can-be-stopped.md), proposed; [render.md](../specs/render.md#stopping)).
  - `Session::stopping(&Stop) -> Stopping`, `Error::Stopped`, and `SearchStop` renamed `Stop` with a deprecated alias. Calls on `Session` pass `Halt::NEVER` and are unchanged.
  - Heights and underground probes are computed a band of rows (one chunk row) at a time. The terrain crate already splits rows into the same runs, so answers are identical; the real-world tiling test passes.
  - Checking only before each chunk wasn't enough: features stopped 1.9 s late at first, because the first row's chunks all start in parallel and then carve their neighbours from a cold cache. The feature and loot caches now hold the halt and check before each neighbour they carve.
  - Then 4.4 s late once: the first chunk to need structure pieces loads the world's jigsaw template pools, with every core waiting. That's once per world, so it isn't interrupted; the ADR and spec say so, and the test warms it up.
  - Measured, release build, 26.3 Overworld, three runs: heights at 1:64 stopped 11–13 ms after the stop, every underground layer at 1:4 44–56 ms, features at 1:2 21–22 ms, containers in 17 × 17 chunks 16–31 ms.
  - `containers_in` and `containers_holding` already give each container under the chunk that placed it (`chunk_holding`); the viewer's extra ring of chunks is only for markers that reach a tile. So point regions need no new library call.
- 2026-11-16: Checkboxes in menus done: unchecked boxes in **Highlight a biome** now show their outline, checked by eye.
- 2026-11-16: Text boxes done. The first guess at the cause (the theme's taller controls with a top-aligned `TextEdit`) was wrong: a test showed egui's box in a panel already centered, 4 px above and 5 below. A screenshot of **Filter saves** showed the text against the box's top and left edges, which is egui's menu style shrinking `button_padding`, so only boxes in menus were affected. Before and after checked by eye with the `run-viewer` skill. The same screenshots showed the **Highlight a biome** menu's unchecked checkboxes almost invisible, since the menu style also drops the resting fill and outline; added as its own task.
- 2026-11-16: Added the text boxes' alignment, reported in **Filter saves**.
- 2026-11-16: Drafted.
  - The milestone predates m31's **World** tab. The plan brings it into the tab bar's new look (warnings as a badge) and into the folding sections, its four `CollapsingHeader`s becoming `style::section`s. A folded World section names its warnings.
  - The milestone left relief and surface biomes out of reuse. render.md says heights are each pixel's own column and relief is drawn from heights at texture time, so both reuse once the heights' edge row is carried.
  - Points are keyed by the chunk that placed them, in regions independent of scale. That's what makes "no chunk placed twice" hold, and it also ends today's double placing along tile edges.
