# 0020. A line-oriented query language for seed search

- Status: Accepted
- Date: 2026-10-25

## Context

Seed search takes a description of a world: structures counted in a region, clusters of structures, biomes present or covering an area, slime chunks, the spawn, distances between features. Each item is either required or preferred, with a weight. The same description has to work in three places:

- typed on a command line (`spyglass search -q '…'`) or kept in a file;
- edited in the viewer, with errors shown as the user types;
- echoed back in `--json` output and in results, so a search can be repeated.

So the format has to be short enough to type, readable without a manual, precise enough to have one meaning, and parseable with good error positions. It's a format users will keep in files, so changing it later costs them.

## Decision

We will use a small line-oriented language of our own, with a hand-written parser in `spyglass-search` and no new dependency.

- One constraint per line: `require` or `prefer`, then the constraint, then an optional `weight N` for preferred ones. `#` starts a comment.
- Constraints read as phrases: `require 2 minecraft:swamp_hut within 600 of spawn`, `prefer biome minecraft:jungle covers 25% in -500,-500 to 500,500`.
- Every constraint names a bounded region, so every check a search runs is finite.
- Errors give a line and column.
- Every query has a canonical text that parses back to the same query, used wherever a front end echoes one.

The grammar and the meaning of each constraint are in [specs/search.md](../specs/search.md).

## Consequences

- Queries fit on a command line and read aloud as what they mean; the viewer's editor is a plain text box with an error line.
- The parser, its errors and its tests are ours to write and keep (a few hundred lines). Fuzzed input must give errors, never panics.
- Adding a constraint means extending the grammar and the spec together. Within a format, new forms can be added; existing ones keep their meaning.
- Scripts that want to build queries generate text. A JSON form of the syntax tree can be added later if that proves awkward; the canonical text makes the two convertible.
- Structure ids and tags are written as `/locate structure` takes them, so users don't learn a second spelling.

## Alternatives considered

- **TOML or JSON constraint files:** no parser to write and well known, but verbose: a three-constraint query becomes a dozen lines of tables, unusable on a command line, and errors point at syntax rather than meaning.
- **A general expression language** (boolean `and`/`or`/`not` over predicates): more powerful, but harder to explain, to score (what does a preferred `or` earn?) and to stage cheapest first. Required plus preferred covers the milestone's queries; nesting can come later.
- **A parser generator or combinator crate** (`pest`, `nom`, `chumsky`): a dependency and its build cost for a grammar of a dozen rules, and error messages we'd still have to shape.
- **CLI flags only** (`--require-structure …`): no file form, no way to edit a query in the viewer, and awkward for regions and anchors.
