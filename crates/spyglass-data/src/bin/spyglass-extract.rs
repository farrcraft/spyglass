//! Builds a version bundle from the user's own server jar.

use clap::Parser;

#[derive(Parser)]
#[command(version, about)]
struct Args {}

fn main() -> anyhow::Result<()> {
    let _args = Args::parse();
    println!("spyglass extract: not implemented yet");
    Ok(())
}
