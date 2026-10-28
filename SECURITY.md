# Security

spyglass runs locally and never sends your data anywhere. Its exposure is to what it reads and what it downloads, so those are what security reports are about.

## What counts

- **Files spyglass reads that someone else may have made:** world saves (`level.dat`, player files, region files) and the data packs inside them. Any such file that crashes spyglass, makes it allocate without bound, hang where the game wouldn't, or read or write outside the save is a vulnerability.
- **Downloads:** the extractor downloads Mojang's version manifest and server jar and checks the jar's SHA-1 against the manifest before using it ([data sources](docs/specs/data-sources.md)). A way to make spyglass use a file it didn't verify is a vulnerability.
- **Unpacking the jar:** a jar entry that is written outside the bundle or cache folder.

Out of scope: the game's own behavior (spyglass matches it on purpose, so data that hangs the game may hang spyglass too), and the oracle harness, which runs Mojang's server on your machine by design.

## Reporting

Report privately through GitHub's **Report a vulnerability** button on the repository's Security tab, not in a public issue. Include the spyglass commit, the command or API call, and the file that triggers it if you can share it.
