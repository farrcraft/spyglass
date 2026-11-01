# 0044. The viewer draws its own map icons

- Status: Accepted
- Date: 2026-10-29
- See also: [0045](0045-viewer-icons-are-svgs.md)

## Context

The library's `Image::draw_features` marks each structure start with a 9 × 9 square in its set's colour, hollow for a candidate ([render.md](../specs/render.md#layers)), and the viewer draws the same squares at the same pixels so the map and an export look alike. The squares are exact and cheap, and they suit a PNG that a script will read. In the viewer they're hard to tell apart: every structure is the same shape, many set colours are dark on dark biomes, and there's nothing to read up close.

A design pass on the viewer gave each structure family a pictogram on a badge in its set's colour, sized by zoom, with pins for locate hits, rings for search matches and a halo for the marker under the pointer. Drawing those in the library too would keep one picture of a map everywhere. But render.md defines exports pixel by pixel, and they're tested to be identical however they're tiled, so every icon at every size would have to be specified as pixels. Exports from every earlier build would also change.

## Decision

We will draw the new icons in the viewer only. The library's markers stay as render.md defines them, for `Session::render`, `spyglass render` and the viewer's "Export image". The viewer draws its icons at the centre pixel the library would mark, `floor(position / scale)` on the world-aligned grid, in the set's `structure_colour`, filled for a certain start and hollow for a candidate. Spawn, locate hits and search matches get the viewer's own markers too.

## Consequences

- The viewer can change its icons, sizes and hover states freely, without changing the library's output or its tests.
- What the viewer shows and what an export draws differ in shape, but not in place, colour or meaning. render.md says so, so nobody takes the difference for a bug.
- `Session::map_features` stays the single source of what's drawn, so both pictures mark the same starts.
- A user who wants icons in an image must screenshot the viewer. If that becomes a common request, a later ADR can bring the icons into the library as a layer of its own.

## Alternatives considered

- **Icons in the library too:** one picture everywhere. But it needs pixel-exact icons in the spec, a breaking change to every export, and a much larger test surface, for a gain most export users don't need.
- **Keep the squares in the viewer:** no work, and nothing fixed. The squares are what made the markers hard to read.
