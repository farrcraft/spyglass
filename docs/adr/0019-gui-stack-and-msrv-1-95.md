# 0019. The viewer's GUI stack, and MSRV 1.95

- Status: Accepted
- Date: 2026-10-25

## Context

The viewer (milestone 05) is a native window: a pannable, zoomable map drawn from tiles that render in the background, a side panel of controls, file dialogs for bundle folders, saves and exported images, and settings kept between runs. It must run on Windows and Linux, and on macOS where possible. The architecture has named `egui` with `wgpu` since the start ([architecture.md](../architecture.md)), but nothing had been added.

The current eframe, 0.37, needs Rust 1.95. The workspace promised 1.88 ([ADR 0005](0005-extra-rust-checks.md)), and the last eframe that builds on 1.88 is 0.33, about a year older.

The GUI stack is large. It brings licences the workspace hadn't allowed: BSD-2-Clause (`arrayref`), BSL-1.0 (`clipboard-win`, `error-code`), and OFL-1.1 and the Ubuntu Font Licence for egui's bundled fonts. It also brings four crates with "unmaintained" advisories and no upgrade yet: `ttf-parser`, through winit's Wayland window decorations, and `zbus_names`, `zvariant` and `zvariant_derive`, through AccessKit's Linux screen-reader bridge.

## Decision

We will build `spyglass-viewer` on:

- **`eframe` 0.37** with its default renderer, `wgpu`, and its `persistence` feature for settings.
- **`rfd`** 0.17 for native file and folder dialogs.

We will raise the workspace's `rust-version` to **1.95**, for every crate, rather than give the viewer its own.

In `deny.toml` we allow the four licences above, and ignore the four unmaintained-crate advisories with a reason each, to be removed when eframe moves off them.

Only the viewer depends on these crates; the library and the CLI don't.

## Consequences

- The viewer starts on the current egui and can follow its releases.
- Library users need Rust 1.95, which the library doesn't itself need. One `rust-version` is simpler to state and check (`cargo xtask msrv` is unchanged), and nothing is published yet.
- The setup scripts and `cargo xtask doctor` take the toolchain from `Cargo.toml`, so contributors get 1.95 on their next setup run.
- A clean build compiles a few hundred more crates for the viewer. `cargo build -p spyglass` and `-p spyglass-cli` don't.
- On Linux the viewer needs the windowing libraries listed in getting-started; the dev container has them.
- The ignored advisories need revisiting when eframe updates.

## Alternatives considered

- **A viewer-only MSRV of 1.95**, keeping 1.88 for the library: it keeps the library's promise, but `cargo xtask msrv` would build the workspace in two parts, and contributors would have two toolchains for one repository.
- **Pinning eframe 0.33**, the last on 1.88: no policy change, but the viewer would start a year behind and stay pinned until the MSRV moved.
- **`glow` (OpenGL) instead of `wgpu`:** smaller and builds faster, but `wgpu` is eframe's default, and the planned GPU search ([roadmap](../roadmap/README.md)) would use it anyway.
- **Another toolkit (`iced`, `slint`, a web front end):** egui's immediate mode suits a single map with a panel. The architecture had already chosen it, and a web front end would need a server, against the local-only goal.
- **Turning off AccessKit** to drop the zbus crates: the advisories are about maintenance, not security, and screen-reader support is worth keeping.
