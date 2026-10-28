//! The command line: commands, their options, and `--from`.

use std::path::PathBuf;
use std::time::Duration;

use clap::{Args, Parser, Subcommand, ValueEnum};
use spyglass::{DEFAULT_MAP_Y, Dimension, Heightmap, LOCATE_BIOME_RADIUS, LOCATE_RADIUS};

#[derive(Parser)]
#[command(
    name = "spyglass",
    version = spyglass::build::version(),
    long_version = spyglass::build::long_version(),
    about = "Explore Minecraft: Java Edition seeds locally."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Find structures, biomes, slime chunks or the spawn for a seed, as `/locate` would.
    Locate {
        #[command(subcommand)]
        what: Locate,
    },
    /// Your world saves: list them, or see what spyglass makes of one.
    World {
        #[command(subcommand)]
        what: WorldCmd,
    },
    /// Draw a map of a seed or world save as a PNG: biomes, and the layers you choose.
    Render(Render),
    /// Find the seeds that fit a query best: required and preferred structures, biomes, slime
    /// chunks and spawn, in any dimension.
    Search(Search),
    /// Terrain heights before structures, carvers and features, as structures are placed by:
    /// one column or a region.
    Heights(Heights),
}

/// `spyglass heights` (`docs/specs/terrain.md`).
#[derive(Args)]
pub struct Heights {
    /// One column, `x,z`.
    #[arg(long, allow_hyphen_values = true, value_parser = xz, required_unless_present = "min", conflicts_with = "min")]
    pub at: Option<[i32; 2]>,
    /// A region's corner, `x,z`, with `--max`: every column between them, both included.
    #[arg(long, allow_hyphen_values = true, value_parser = xz, requires = "max")]
    pub min: Option<[i32; 2]>,
    /// The region's other corner, `x,z`.
    #[arg(long, allow_hyphen_values = true, value_parser = xz, requires = "min")]
    pub max: Option<[i32; 2]>,
    /// Which height: the top of anything but air (fluids count), or of solid ground.
    #[arg(long, value_enum, default_value_t = HeightmapArg::WorldSurface)]
    pub heightmap: HeightmapArg,
    #[command(flatten)]
    pub common: Common,
}

/// The heightmaps `--heightmap` takes.
#[derive(Clone, Copy, ValueEnum)]
pub enum HeightmapArg {
    /// `WORLD_SURFACE_WG`: any block but air.
    WorldSurface,
    /// `OCEAN_FLOOR_WG`: blocks that block motion, so not fluids.
    OceanFloor,
}

impl HeightmapArg {
    pub fn heightmap(self) -> Heightmap {
        match self {
            HeightmapArg::WorldSurface => Heightmap::WorldSurfaceWg,
            HeightmapArg::OceanFloor => Heightmap::OceanFloorWg,
        }
    }
}

/// `spyglass search` (`docs/specs/search.md`).
#[derive(Args)]
pub struct Search {
    /// The query, e.g. `require 2 minecraft:swamp_hut within 600 of spawn`; one constraint per
    /// line.
    #[arg(
        short,
        long,
        conflicts_with = "query_file",
        required_unless_present = "query_file"
    )]
    pub query: Option<String>,
    /// A file with the query.
    #[arg(long)]
    pub query_file: Option<PathBuf>,
    /// How many seeds to keep.
    #[arg(long, default_value_t = 10, value_parser = clap::builder::RangedU64ValueParser::<usize>::new().range(1..=10_000))]
    pub top: usize,
    /// Search this many seeds, then stop. Without it or `--time`, every seed is searched until
    /// Ctrl-C.
    #[arg(long)]
    pub count: Option<u64>,
    /// Stop after this long: e.g. `90s`, `10m` or `2h`.
    #[arg(long, value_parser = duration)]
    pub time: Option<Duration>,
    /// The index in the seed source to start at: a search's `next` continues it.
    #[arg(long, default_value_t = 0)]
    pub from: u64,
    /// Search the seeds from A to B, both included, e.g. `-2147483648..2147483647` for every
    /// text seed. Without it or `--seeds`, every 64-bit seed is searched.
    #[arg(long, allow_hyphen_values = true, value_parser = range, conflicts_with = "seeds")]
    pub range: Option<[i64; 2]>,
    /// Search the seeds in a file, one per line, numbers or text as `--seed` takes them.
    #[arg(long)]
    pub seeds: Option<PathBuf>,
    /// Worker threads; all cores by default.
    #[arg(long)]
    pub threads: Option<usize>,
    /// Report only seeds whose required constraints are met whatever terrain decides, and rank
    /// by the worst case.
    #[arg(long)]
    pub certain: bool,
    /// Decide structure starts with each seed's terrain, as the game does. Slower, but only
    /// ties and platform-dependent results stay uncertain.
    #[arg(long)]
    pub confirm: bool,
    /// Game version of the bundle to use, from `--bundles`. Defaults to the newest release there;
    /// snapshots are used only when named.
    #[arg(long)]
    pub game_version: Option<String>,
    /// A bundle directory to use instead of choosing one from `--bundles`.
    #[arg(long, conflicts_with = "game_version")]
    pub bundle: Option<PathBuf>,
    /// Where bundles are, one directory per version. `SPYGLASS_BUNDLES` sets the default.
    #[arg(long, default_value_os_t = spyglass::default_bundles_dir())]
    pub bundles: PathBuf,
    /// Print JSON (docs/specs/cli-json.md) instead of text.
    #[arg(long)]
    pub json: bool,
}

/// `A..B`, both included.
fn range(text: &str) -> Result<[i64; 2], String> {
    text.split_once("..")
        .and_then(|(a, b)| Some([a.trim().parse().ok()?, b.trim().parse().ok()?]))
        .filter(|[a, b]| a <= b)
        .ok_or_else(|| format!("expected A..B with A at most B, got {text:?}"))
}

/// A number and a unit: `ms`, `s`, `m` or `h`; seconds without one.
fn duration(text: &str) -> Result<Duration, String> {
    let split = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let n: u64 = number
        .parse()
        .map_err(|_| format!("expected a duration like 90s, 10m or 2h, got {text:?}"))?;
    let times = |unit: u64| {
        n.checked_mul(unit)
            .map(Duration::from_secs)
            .ok_or_else(|| format!("{text:?} is too long"))
    };
    Ok(match unit {
        "ms" => Duration::from_millis(n),
        "" | "s" => Duration::from_secs(n),
        "m" => times(60)?,
        "h" => times(3600)?,
        _ => {
            return Err(format!(
                "unknown unit {unit:?} in {text:?}: use ms, s, m or h"
            ));
        }
    })
}

/// `spyglass render` (`docs/specs/render.md`).
#[derive(Args)]
pub struct Render {
    /// The PNG file to write.
    #[arg(short, long)]
    pub output: PathBuf,
    /// The block at the image's centre, `x,z`, or `player` for the single player of `--world`.
    /// Defaults to the player with `--world` (or else the save's spawn), otherwise 0,0.
    #[arg(long, visible_alias = "center", allow_hyphen_values = true, value_parser = centre, conflicts_with = "min")]
    pub centre: Option<Centre>,
    /// The block at the image's top-left corner, `x,z`, instead of `--centre`. Must be a
    /// multiple of `--scale`.
    #[arg(long, allow_hyphen_values = true, value_parser = xz)]
    pub min: Option<[i32; 2]>,
    /// Width and height in pixels, e.g. `1920x1080`.
    #[arg(long, default_value = "1024x1024", value_parser = size)]
    pub size: [u32; 2],
    /// Blocks per pixel: 1, 2, 4 and so on up to 256.
    #[arg(long, default_value_t = 4)]
    pub scale: u32,
    /// The height biomes are drawn at. The map is a slice at this height, not the surface.
    #[arg(long, default_value_t = DEFAULT_MAP_Y, allow_hyphen_values = true)]
    pub y: i32,
    /// Structures to mark: ids or `#tags`, separated by commas.
    #[arg(long, value_delimiter = ',')]
    pub structures: Vec<String>,
    /// Biomes to keep bright, separated by commas; the rest are dimmed.
    #[arg(long, value_delimiter = ',')]
    pub highlight: Vec<String>,
    /// Shade slime chunks (scales up to 16).
    #[arg(long)]
    pub slime: bool,
    /// Mark the world spawn's origin.
    #[arg(long)]
    pub spawn: bool,
    /// Show tied biome cells as a checker of the biomes the game may give.
    #[arg(long)]
    pub ties: bool,
    /// Leave biomes out: a plain background under the other layers, or grey by height with
    /// `--relief`.
    #[arg(long)]
    pub no_biomes: bool,
    /// Decide structure markers with the seed's terrain, so candidates are only ties and
    /// platform-dependent results.
    #[arg(long)]
    pub confirm: bool,
    /// Shade by terrain height (scales up to 4).
    #[arg(long)]
    pub relief: bool,
    /// Draw biomes at the surface instead of at `--y` (scales up to 4).
    #[arg(long, conflicts_with = "no_biomes")]
    pub surface: bool,
    #[command(flatten)]
    pub common: Common,
}

/// Where a map is centred.
#[derive(Clone, Copy)]
pub enum Centre {
    At([i32; 2]),
    /// The single player of `--world`'s save.
    Player,
}

/// `overworld`, `nether` or `end`, or a dimension id such as `minecraft:the_nether`.
fn dimension(text: &str) -> Result<Dimension, String> {
    let keyword = match text {
        "overworld" => Some(Dimension::Overworld),
        "nether" => Some(Dimension::Nether),
        "end" => Some(Dimension::End),
        _ => None,
    };
    keyword
        .or_else(|| Dimension::from_id(text))
        .ok_or_else(|| format!("expected overworld, nether or end, got {text:?}"))
}

fn centre(text: &str) -> Result<Centre, String> {
    if text == "player" {
        Ok(Centre::Player)
    } else {
        xz(text).map(Centre::At)
    }
}

fn xz(text: &str) -> Result<[i32; 2], String> {
    match text
        .split(',')
        .map(|p| p.trim().parse::<i32>())
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(parts) if parts.len() == 2 => Ok([parts[0], parts[1]]),
        _ => Err(format!("expected x,z in blocks, got {text:?}")),
    }
}

fn size(text: &str) -> Result<[u32; 2], String> {
    text.split_once(['x', 'X'])
        .and_then(|(w, h)| Some([w.trim().parse().ok()?, h.trim().parse().ok()?]))
        .ok_or_else(|| format!("expected WIDTHxHEIGHT in pixels, got {text:?}"))
}

#[derive(Subcommand)]
pub enum WorldCmd {
    /// Saves in the known launchers' folders.
    List {
        /// Where bundles are, to say which saves spyglass can answer for. `SPYGLASS_BUNDLES` sets
        /// the default.
        #[arg(long, default_value_os_t = spyglass::default_bundles_dir())]
        bundles: PathBuf,
        /// Print JSON (docs/specs/cli-json.md) instead of text.
        #[arg(long)]
        json: bool,
    },
    /// A save's seed, version, spawn and player, and anything that makes answers inexact.
    Info {
        /// The save's folder, or its folder name in a launcher's saves.
        save: String,
        /// Where bundles are, one directory per version. `SPYGLASS_BUNDLES` sets the default.
        #[arg(long, default_value_os_t = spyglass::default_bundles_dir())]
        bundles: PathBuf,
        /// Skip counting chunks generated by older versions (which reads every region file).
        #[arg(long)]
        quick: bool,
        /// Print JSON (docs/specs/cli-json.md) instead of text.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum Locate {
    /// The nearest structure: an id (`minecraft:mansion`) or a tag (`#minecraft:village`).
    Structure {
        /// Structure id or `#tag`.
        target: String,
        #[command(flatten)]
        common: Common,
        #[command(flatten)]
        from: From,
        /// Rings of placement cells to search; the game's `/locate` searches 100.
        #[arg(long, default_value_t = LOCATE_RADIUS)]
        radius: i32,
        /// Don't decide candidates with the seed's terrain: faster, but most surface
        /// structures stay candidates.
        #[arg(long)]
        unconfirmed: bool,
        /// Show each certain jigsaw structure's bounds and piece count (villages, outposts,
        /// ancient cities, trial chambers and others). The first use loads the template pools.
        #[arg(long)]
        bounds: bool,
    },
    /// The nearest biome, searched in 3D from `--from`, as `/locate biome` does.
    Biome {
        /// Biome id, e.g. `minecraft:cherry_grove`.
        biome: String,
        #[command(flatten)]
        common: Common,
        #[command(flatten)]
        from: From,
        /// Search radius in blocks; the game's `/locate` searches 6400.
        #[arg(long, default_value_t = LOCATE_BIOME_RADIUS)]
        radius: i32,
    },
    /// Slime chunks near `--from`, nearest first.
    Slime {
        #[command(flatten)]
        common: Common,
        #[command(flatten)]
        from: From,
        /// Search radius in chunks.
        #[arg(long, default_value_t = 8)]
        radius: i32,
    },
    /// The chunk the game's world-spawn search starts from.
    Spawn {
        #[command(flatten)]
        common: Common,
    },
}

impl Locate {
    /// The options every locate command has.
    pub fn common(&self) -> &Common {
        match self {
            Locate::Structure { common, .. }
            | Locate::Biome { common, .. }
            | Locate::Slime { common, .. }
            | Locate::Spawn { common } => common,
        }
    }
}

#[derive(Args)]
pub struct Common {
    /// The world seed: a number, or text hashed as the world-creation screen does.
    #[arg(long, allow_hyphen_values = true, required_unless_present = "world")]
    pub seed: Option<String>,
    /// A world save instead of a seed: its folder, or its folder name in a launcher's saves.
    /// Its seed and version are used, and anything that makes answers inexact is shown.
    #[arg(long, conflicts_with_all = ["seed", "bundle", "game_version"])]
    pub world: Option<String>,
    /// The dimension: `overworld`, `nether` or `end`. Defaults to the player's when the position
    /// is the player's, otherwise the Overworld.
    #[arg(long, value_parser = dimension)]
    pub dimension: Option<Dimension>,
    /// Game version of the bundle to use, from `--bundles`. Defaults to the newest release there;
    /// snapshots are used only when named.
    #[arg(long)]
    pub game_version: Option<String>,
    /// A bundle directory to use instead of choosing one from `--bundles`.
    #[arg(long, conflicts_with = "game_version")]
    pub bundle: Option<PathBuf>,
    /// Where bundles are, one directory per version. `SPYGLASS_BUNDLES` sets the default.
    #[arg(long, default_value_os_t = spyglass::default_bundles_dir())]
    pub bundles: PathBuf,
    /// Print JSON (docs/specs/cli-json.md) instead of text.
    #[arg(long)]
    pub json: bool,
}

/// Where a search starts.
#[derive(Clone, Copy)]
pub enum Origin {
    At([i32; 3]),
    /// The single player of `--world`'s save.
    Player,
}

#[derive(Args)]
pub struct From {
    /// Where to search from: `x,y,z` or `x,z` (y 64), in blocks, or `player` for the single
    /// player of `--world`.
    #[arg(long, default_value = "0,64,0", allow_hyphen_values = true, value_parser = origin)]
    pub from: Origin,
}

fn origin(text: &str) -> Result<Origin, String> {
    if text == "player" {
        return Ok(Origin::Player);
    }
    let parts: Vec<i32> = text
        .split(',')
        .map(|p| p.trim().parse::<i32>())
        .collect::<Result<_, _>>()
        .map_err(|e| format!("{e} in {text:?}"))?;
    match parts[..] {
        [x, y, z] => Ok(Origin::At([x, y, z])),
        [x, z] => Ok(Origin::At([x, 64, z])),
        _ => Err(format!("expected x,y,z, x,z or player, got {text:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(duration("90"), Ok(Duration::from_secs(90)));
        assert_eq!(duration("90s"), Ok(Duration::from_secs(90)));
        assert_eq!(duration("250ms"), Ok(Duration::from_millis(250)));
        assert_eq!(duration("10m"), Ok(Duration::from_secs(600)));
        assert_eq!(duration("2h"), Ok(Duration::from_secs(7200)));
        // Too long for a u64 of seconds once multiplied: an error, not an overflow.
        assert!(duration(&format!("{}h", u64::MAX / 3600 + 1)).is_err());
        assert!(duration(&format!("{}m", u64::MAX)).is_err());
        for bad in ["", "s", "-1s", "1d", "1.5s", "1 s"] {
            assert!(duration(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn ranges() {
        assert_eq!(range("-5..5"), Ok([-5, 5]));
        assert_eq!(range("3..3"), Ok([3, 3]));
        assert_eq!(
            range("-9223372036854775808..9223372036854775807"),
            Ok([i64::MIN, i64::MAX])
        );
        for bad in [
            "5..-5",
            "1..",
            "..1",
            "1-2",
            "a..b",
            "9223372036854775808..9",
        ] {
            assert!(range(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn sizes_and_columns() {
        assert_eq!(size("1024x768"), Ok([1024, 768]));
        assert_eq!(size("8X8"), Ok([8, 8]));
        for bad in ["1024", "x8", "-1x8", "8x8x8"] {
            assert!(size(bad).is_err(), "{bad:?}");
        }
        assert_eq!(xz("-3, 7"), Ok([-3, 7]));
        for bad in ["1", "1,2,3", "1,a", "2147483648,0"] {
            assert!(xz(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn origins() {
        assert!(matches!(origin("player"), Ok(Origin::Player)));
        assert!(matches!(origin("1,2,3"), Ok(Origin::At([1, 2, 3]))));
        assert!(matches!(origin("-1,-2"), Ok(Origin::At([-1, 64, -2]))));
        for bad in ["", "1", "1,2,3,4", "a,b", "Player"] {
            assert!(origin(bad).is_err(), "{bad:?}");
        }
    }
}
