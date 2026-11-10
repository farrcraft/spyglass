# 0066. A user guide on GitHub Pages, built with mdBook

- Status: Accepted
- Date: 2026-11-09
- See also: [0038](0038-how-release-archives-are-built.md)

## Context

Someone who downloads spyglass and never clones it finds its documentation in three places, each written for a different reader:

- the README's Install section, a paragraph;
- `xtask/dist/GETTING-STARTED.txt`, about 100 lines of plain text, seen only after downloading an archive;
- [getting-started.md](../getting-started.md) §8–10 (game data, the `spyglass` command, the viewer), which comes after 250 lines on rustup, git hooks, the oracle and dev containers.

The same material is written more than once and drifts. A user-facing feature, such as loot in searches ([ADR 0065](0065-loot-in-the-search-language.md)), has no obvious home: getting-started.md is a contributor's document, and the specs say what the code must match, not how to use it. The repo is public and has no homepage, so the releases page and the README have nothing to link a user to.

Two things make this harder than writing a page:

- **Versions.** `main` runs ahead of the latest release. A guide published from `main` describes flags and layers that the latest download doesn't have.
- **Local only.** spyglass has no server and makes no network calls except for Mojang's server jar ([README](../../README.md#goals)). A website must not become something the programs depend on, and the archive must stay usable offline.

## Decision

We will write a user guide in `docs/guide/` as an [mdBook](https://rust-lang.github.io/mdBook/) and publish it to GitHub Pages.

- **Readers:** people who use the released programs: the viewer, the `spyglass` command, game data, world saves, searches. Building from source is covered only as far as an install option. Contributor material stays in getting-started.md, architecture, specs and ADRs.
- **Published per release.** A workflow builds the guide and deploys it when a release is published, from that release's tag, so the site describes the download. CI builds the guide on every pull request, and `cargo xtask check` builds it too, so a broken guide fails before merge.
- **One source.** getting-started.md's user sections move into the guide, and getting-started.md keeps contributor setup. `GETTING-STARTED.txt` shrinks to an offline quick start (unpack, Java, first run, unsigned-binary warnings) that links to the guide.
- **Links.** The guide's links to its own pages are relative, and `cargo xtask links` checks them. Links to specs, ADRs or code outside `docs/guide/` are absolute GitHub URLs, because only the guide is published.
- **The CLI's JSON** stays specified in [cli-json.md](../specs/cli-json.md). The guide links to it and doesn't copy it.
- mdBook is a pinned build tool, installed by the setup scripts and checked by `cargo xtask doctor`, like the other tools. No crate depends on it.

## Consequences

- Users get one place to read, with navigation and search, linked from the README, the releases page and the archive.
- getting-started.md becomes a contributor's document only, and shorter.
- Changing user-visible behaviour now includes updating the guide. Nothing enforces that beyond review. A missing page is less harmful than a wrong one, and building from release tags keeps the site from getting ahead of the download.
- A fix to the guide alone waits for the next release, unless the deploy workflow is run by hand from the release tag with the fix cherry-picked. The workflow allows a manual run for this.
- One more tool for the setup scripts and CI, and one more workflow with the permissions Pages needs (`pages: write`, `id-token: write`).
- Pages has to be turned on in the repo's settings once, with GitHub Actions as its source.
- The site is hosted by GitHub. The programs never fetch from it.

## Alternatives considered

- **Markdown in the repo, read on GitHub:** no tool and no workflow, but no search or navigation, and GitHub shows `main`, not the release.
- **A wiki:** edited outside review and the hooks, not versioned with the code, and not checked by `cargo xtask links`.
- **MkDocs or Docusaurus:** a Python or Node toolchain in a repo that has only Rust, plus Java for the oracle. mdBook installs with `cargo install`.
- **Publishing from `main`:** the guide would always be current with the code, and always ahead of what people can download.
- **Generating `GETTING-STARTED.txt` from the guide:** one source in theory, but a converter to maintain for 100 lines. A short quick start that links out is easier to keep right.
