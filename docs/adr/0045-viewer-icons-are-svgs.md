# 0045. The viewer's icons are SVGs, drawn by egui_extras

- Status: Accepted
- Date: 2026-10-29
- See also: [0044](0044-the-viewer-draws-its-own-map-icons.md), [0019](0019-gui-stack-and-msrv-1-95.md)

## Context

[ADR 0044](0044-the-viewer-draws-its-own-map-icons.md) gives the viewer a pictogram per structure family, about 22 of them, drawn on badges at three sizes and at any display scale. They'll be redrawn as people suggest better ones, so it matters how easy one is to change.

egui can draw vector shapes itself (`Painter` paths), but a glyph written that way is Rust code: coordinates in a function, edited by reading numbers. egui can also show images through loaders, and `egui_extras` has one for SVG, built on `resvg`, which rasterises each icon at the size it's drawn.

## Decision

We will keep each icon as an SVG file in `crates/spyglass-viewer/icons/`, embedded with `include_bytes!`, and draw them with `egui_extras`' SVG loader (`egui_extras` 0.37, `default-features = false`, `features = ["svg"]`). Glyphs are white strokes on a 24 × 24 grid. The viewer tints them to the ink a badge needs and draws the badge itself.

## Consequences

- An icon is edited in any SVG editor and checked by opening the file. Adding one is a file and a line in the family table.
- The viewer gains `egui_extras`, `resvg`, `usvg`, `tiny-skia` and their dependencies: 17 crates, all MIT or Apache-2.0, already allowed by `deny.toml`. They bring a second `png` (0.17, beside our 0.18), which `cargo deny` reports as a duplicate but doesn't fail on. The library crates are unaffected.
- Each icon is rasterised once per size and cached as a texture. Rasterising costs a little at the first frame that shows a new size, and nothing after that.
- An SVG with features `resvg` doesn't support would draw wrongly. The icons keep to paths, lines and circles.

## Alternatives considered

- **`Painter` paths:** no dependency and crisp at any scale. But every glyph is code, and redrawing one means editing coordinates by hand.
- **An icon font:** one texture atlas, but making one needs a font toolchain, and colouring glyphs individually is awkward.
- **PNG sprites:** no new rendering code, but blurry or many-sized at the display scales egui supports.
