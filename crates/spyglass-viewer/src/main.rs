//! Native seed-map viewer. A front end over the `spyglass` library only (ADR 0014).

fn main() {
    println!("spyglass viewer: not implemented yet");
    match spyglass::bundles_in("bundles") {
        Ok(found) if !found.is_empty() => {
            for bundle in found {
                println!("bundle {} in {}", bundle.version, bundle.dir.display());
            }
        }
        _ => println!("no bundles in bundles/; extract one with spyglass-extract"),
    }
}
