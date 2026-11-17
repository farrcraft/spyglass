//! The command line: commands, their options, and `--from`.

use std::path::PathBuf;
use std::time::Duration;

use clap::{Args, Parser, Subcommand, ValueEnum};
use spyglass::{DEFAULT_MAP_Y, Dimension, Heightmap, LOCATE_BIOME_RADIUS, LOCATE_RADIUS, Pin};

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
    /// Find the seeds that fit a query best: required and preferred structures, features,
    /// biomes, slime chunks and spawn, in any dimension.
    Search(Search),
    /// Places in one seed or world save that fit a `find` query, nearest first: a target (a
    /// structure, a feature, a loot container holding an item, a biome or a grid point) and
    /// constraints around it.
    Find(Find),
    /// Terrain heights before structures, carvers and features, as structures are placed by:
    /// one column or a region.
    Heights(Heights),
    /// A chunk's blocks before features, as the game generates them, carvers included: one
    /// block, a column or a whole chunk.
    Blocks(Blocks),
    /// Features in a region: dungeons, geodes, desert wells, fossils and lava lakes, each
    /// certain or a candidate.
    Features(Features),
    /// What's in the chests: the loot containers structures and dungeons place, with their
    /// contents as the seed generates them.
    Loot {
        #[command(subcommand)]
        what: Loot,
    },
    /// The flower bone meal grows on grass: at a block, or the best height for a farm of one
    /// flower.
    Flower {
        #[command(subcommand)]
        what: Flower,
    },
    /// Where Nether portals lead: the portal a trip lands at, from a portal you give or from
    /// every portal in a world save. Needs no seed.
    Portal {
        #[command(subcommand)]
        what: PortalCmd,
    },
}

/// `spyglass portal` (`docs/specs/portals.md`).
#[derive(Subcommand)]
pub enum PortalCmd {
    /// Where a portal leads: from a portal at a position, the exit blocks a standing player
    /// aims at and the portal each lands at, among `--portal`s and the save's; or, with only
    /// `--world`, every portal in the save, with those that don't link back flagged.
    Link(PortalLink),
}

#[derive(Args)]
pub struct PortalLink {
    /// The portal's dimension: `overworld`, `nether` or `end`. With POSITION; leave both out
    /// to link a save's portals.
    #[arg(value_parser = dimension, requires = "position")]
    pub dimension: Option<Dimension>,
    /// The portal's lowest block, `x,y,z`: the least x, y and z of its portal blocks.
    #[arg(allow_hyphen_values = true, value_parser = xyz)]
    pub position: Option<[i32; 3]>,
    /// Which way the portal's blocks run: `x` or `z`.
    #[arg(long, default_value = "x", value_parser = axis)]
    pub axis: spyglass::Axis,
    /// The portal's size in blocks, `WIDTHxHEIGHT`: along its axis and up, inside the frame.
    #[arg(long, default_value = "2x3", value_parser = portal_size)]
    pub size: [i32; 2],
    /// Another portal to link to, `DIMENSION:x,y,z`, with `:AXIS` and `:WIDTHxHEIGHT` if not
    /// `x` and `2x3`, e.g. `overworld:100,64,-30:z:4x5`. Repeatable.
    #[arg(long = "portal", value_name = "PORTAL", allow_hyphen_values = true, value_parser = portal_spec)]
    pub portals: Vec<PortalSpec>,
    /// A world save: its portals are linked too, with its borders and version.
    #[arg(long, conflicts_with_all = ["bundle", "game_version", "packs"])]
    pub world: Option<String>,
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
    /// A data pack, a folder or zip, to apply over the bundle, for a dimension type's scale.
    /// Repeatable, lowest first.
    #[arg(long = "pack", value_name = "PACK")]
    pub packs: Vec<PathBuf>,
    /// Print JSON (docs/specs/cli-json.md) instead of text.
    #[arg(long)]
    pub json: bool,
}

/// A portal given with `--portal`.
#[derive(Clone, Copy)]
pub struct PortalSpec {
    pub dimension: Dimension,
    pub corner: [i32; 3],
    pub axis: spyglass::Axis,
    pub size: [i32; 2],
}

fn axis(text: &str) -> Result<spyglass::Axis, String> {
    match text {
        "x" | "X" => Ok(spyglass::Axis::X),
        "z" | "Z" => Ok(spyglass::Axis::Z),
        _ => Err(format!("expected x or z, got {text:?}")),
    }
}

fn portal_size(text: &str) -> Result<[i32; 2], String> {
    text.split_once(['x', 'X'])
        .and_then(|(w, h)| Some([w.trim().parse().ok()?, h.trim().parse().ok()?]))
        .ok_or_else(|| format!("expected WIDTHxHEIGHT in blocks, got {text:?}"))
}

/// `DIMENSION:x,y,z`, then optionally `:AXIS` and `:WIDTHxHEIGHT`, in either order.
fn portal_spec(text: &str) -> Result<PortalSpec, String> {
    // A dimension id has a colon of its own: `minecraft:the_nether:1,2,3`.
    let parts: Vec<&str> = text.split(':').collect();
    let at = parts
        .iter()
        .position(|p| p.contains(','))
        .ok_or_else(|| format!("expected DIMENSION:x,y,z, got {text:?}"))?;
    let mut spec = PortalSpec {
        dimension: dimension(&parts[..at].join(":"))?,
        corner: xyz(parts[at])?,
        axis: spyglass::Axis::X,
        size: [2, 3],
    };
    for part in &parts[at + 1..] {
        match axis(part) {
            Ok(a) => spec.axis = a,
            Err(_) => spec.size = portal_size(part)?,
        }
    }
    Ok(spec)
}

/// `spyglass flower` (`docs/specs/flowers.md`).
#[derive(Subcommand)]
pub enum Flower {
    /// What bone meal grows on grass at a block, `x,y,z`: the block above the grass. Each
    /// flower with its chance, and why: the biome there and its flower features.
    At {
        /// The flower's block, `x,y,z`, one above the grass.
        #[arg(allow_hyphen_values = true, value_parser = xyz)]
        position: [i32; 3],
        #[command(flatten)]
        common: Common,
    },
    /// How much of an area one flower covers at each height: where it's certain and where it
    /// can grow, for choosing a farm's height.
    Heights {
        /// The flower, e.g. `minecraft:cornflower`.
        flower: String,
        /// The area's corner, `x,z`, with `--max`: every block between them, both included.
        #[arg(long, allow_hyphen_values = true, value_parser = xz)]
        min: [i32; 2],
        /// The area's other corner, `x,z`.
        #[arg(long, allow_hyphen_values = true, value_parser = xz)]
        max: [i32; 2],
        /// The heights to try, `from..to`, both included: the flower's block, one above the
        /// grass.
        #[arg(long, default_value = "-64..320", allow_hyphen_values = true, value_parser = y_range)]
        y: [i32; 2],
        /// Try every this many heights.
        #[arg(long, default_value_t = 1)]
        step: u32,
        /// Sample every this many blocks across the area (a power of two): larger areas are
        /// faster at a coarser scale.
        #[arg(long, default_value_t = 1)]
        scale: u32,
        #[command(flatten)]
        common: Common,
    },
}

/// `spyglass loot` (`docs/specs/locate.md#loot`).
#[derive(Subcommand)]
pub enum Loot {
    /// The container at a block, `x,y,z`, slot by slot.
    At {
        /// The container's block, `x,y,z` (a chest minecart's rail).
        #[arg(allow_hyphen_values = true, value_parser = xyz)]
        position: [i32; 3],
        #[command(flatten)]
        common: Common,
    },
    /// The containers in a region, each with what it holds.
    In {
        /// A region's corner, `x,z` in blocks, with `--max`: every chunk they touch.
        #[arg(long, allow_hyphen_values = true, value_parser = xz)]
        min: [i32; 2],
        /// The region's other corner, `x,z`.
        #[arg(long, allow_hyphen_values = true, value_parser = xz)]
        max: [i32; 2],
        /// Only containers holding this item, an id or an item `#tag`.
        #[arg(long)]
        item: Option<String>,
        #[command(flatten)]
        enchantment: Enchantment,
        #[command(flatten)]
        common: Common,
    },
    /// The nearest container holding an item, an id or an item `#tag`, e.g.
    /// `minecraft:enchanted_golden_apple`.
    Find {
        /// The item, an id or an item `#tag`.
        item: String,
        #[command(flatten)]
        enchantment: Enchantment,
        #[command(flatten)]
        common: Common,
        #[command(flatten)]
        from: From,
        /// How far to search, in chunks. Each chunk a container structure or dungeon reaches
        /// takes a while to place.
        #[arg(long, default_value_t = 8)]
        radius: i32,
    },
}

impl Loot {
    /// The options every loot command has.
    pub fn common(&self) -> &Common {
        match self {
            Loot::At { common, .. } | Loot::In { common, .. } | Loot::Find { common, .. } => common,
        }
    }
}

/// An enchantment the item must have, with `--item` or `loot find`.
#[derive(Args)]
pub struct Enchantment {
    /// An enchantment the item must have, on it or stored in a book, e.g. `minecraft:mending`.
    #[arg(long)]
    pub enchantment: Option<String>,
    /// The enchantment's least level.
    #[arg(long, requires = "enchantment")]
    pub level: Option<i32>,
}

/// `spyglass features` (`docs/specs/locate.md#features`).
#[derive(Args)]
pub struct Features {
    /// Feature id: a placed feature (`minecraft:monster_room_deep`) or a configured one
    /// (`minecraft:monster_room`, both dungeons).
    pub target: String,
    /// A region's corner, `x,z` in blocks, with `--max`: every chunk they touch.
    #[arg(long, allow_hyphen_values = true, value_parser = xz)]
    pub min: [i32; 2],
    /// The region's other corner, `x,z`.
    #[arg(long, allow_hyphen_values = true, value_parser = xz)]
    pub max: [i32; 2],
    /// Also list candidates that didn't place on spyglass's blocks, which the game may still
    /// place.
    #[arg(long)]
    pub all: bool,
    /// Also check whether each answer holds whatever the neighbouring chunks do, if they're
    /// decorated first: about twice as slow.
    #[arg(long)]
    pub neighbours: bool,
    #[command(flatten)]
    pub common: Common,
}

/// `spyglass blocks` (`docs/specs/material.md`).
#[derive(Args)]
#[command(group(clap::ArgGroup::new("what").required(true).args(["at", "column", "chunk"])))]
pub struct Blocks {
    /// One block, `x,y,z`.
    #[arg(long, allow_hyphen_values = true, value_parser = xyz)]
    pub at: Option<[i32; 3]>,
    /// One column, `x,z`, from the bottom of the world up.
    #[arg(long, allow_hyphen_values = true, value_parser = xz)]
    pub column: Option<[i32; 2]>,
    /// A whole chunk, `x,z` in chunks.
    #[arg(long, allow_hyphen_values = true, value_parser = xz)]
    pub chunk: Option<[i32; 2]>,
    /// The blocks before carvers cut caves and ravines.
    #[arg(long)]
    pub no_carvers: bool,
    #[command(flatten)]
    pub common: Common,
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
    /// A data pack, a folder or zip, to apply over the bundle as a world that enables it does.
    /// Repeatable, lowest first.
    #[arg(long = "pack", value_name = "PACK")]
    pub packs: Vec<PathBuf>,
    /// Print JSON (docs/specs/cli-json.md) instead of text.
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct Find {
    /// The query: a `find` line, e.g. `find minecraft:swamp_hut within 4096`, then
    /// constraints around `here`, one per line.
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
    /// The world seed: a number, or text hashed as the world-creation screen does.
    #[arg(long, allow_hyphen_values = true, required_unless_present = "world")]
    pub seed: Option<String>,
    /// A world save instead of a seed: its folder, or its folder name in a launcher's saves.
    #[arg(long, conflicts_with_all = ["seed", "bundle", "game_version"])]
    pub world: Option<String>,
    /// What `within R` alone is around: `x,z` in the target's dimension, or `player` for the
    /// single player of `--world`, converted into the target's dimension.
    #[arg(long, default_value = "0,0", allow_hyphen_values = true, value_parser = find_origin)]
    pub from: FindOrigin,
    /// How many matches to find.
    #[arg(long, default_value_t = 20, value_parser = clap::builder::RangedU64ValueParser::<usize>::new().range(1..=10_000))]
    pub top: usize,
    /// Stop after this long: e.g. `90s` or `10m`.
    #[arg(long, value_parser = duration)]
    pub time: Option<Duration>,
    /// Worker threads; all cores by default.
    #[arg(long)]
    pub threads: Option<usize>,
    /// Report only matches whose required constraints are met whatever terrain decides, at
    /// targets that certainly are what's looked for.
    #[arg(long)]
    pub certain: bool,
    /// Decide structure starts with the seed's terrain, as the game does.
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
    /// A data pack, a folder or zip, to apply over the bundle as a world that enables it does.
    /// Repeatable, lowest first.
    #[arg(long = "pack", value_name = "PACK", conflicts_with = "world")]
    pub packs: Vec<PathBuf>,
    /// Print JSON (docs/specs/cli-json.md) instead of text.
    #[arg(long)]
    pub json: bool,
}

/// What a `find` query's `within R` alone is around.
#[derive(Clone, Copy)]
pub enum FindOrigin {
    At([i32; 2]),
    /// The single player of `--world`'s save.
    Player,
}

fn find_origin(text: &str) -> Result<FindOrigin, String> {
    if text == "player" {
        return Ok(FindOrigin::Player);
    }
    let parse = |s: &str| {
        s.trim()
            .parse::<i32>()
            .map_err(|_| format!("not a block: {s:?}"))
    };
    match text.split_once(',') {
        Some((x, z)) => Ok(FindOrigin::At([parse(x)?, parse(z)?])),
        None => Err("expected `x,z` or `player`".to_string()),
    }
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
    let count: u64 = number
        .parse()
        .map_err(|_| format!("expected a duration like 90s, 10m or 2h, got {text:?}"))?;
    let times = |unit_seconds: u64| {
        count
            .checked_mul(unit_seconds)
            .map(Duration::from_secs)
            .ok_or_else(|| format!("{text:?} is too long"))
    };
    Ok(match unit {
        "ms" => Duration::from_millis(count),
        "" | "s" => Duration::from_secs(count),
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
    /// Features to mark (scales up to 2): placed or configured feature ids, separated by
    /// commas, such as `minecraft:monster_room`. Each chunk they could be in is placed, which
    /// takes a while.
    #[arg(long, value_delimiter = ',')]
    pub features: Vec<String>,
    /// Ring the loot containers holding this item, an id or an item `#tag` (scales up to 2),
    /// and the candidates that might. Each chunk a container could be in is placed, which takes
    /// a while.
    #[arg(long)]
    pub loot: Option<String>,
    /// Biomes to keep bright, separated by commas; the rest are dimmed.
    #[arg(long, value_delimiter = ',')]
    pub highlight: Vec<String>,
    /// Shade slime chunks (scales up to 16).
    #[arg(long)]
    pub slime: bool,
    /// Shade the chunks `--world`'s save has generated: magenta, fainter where generated
    /// partway, amber where an older version generated them (scales up to 16).
    #[arg(long)]
    pub generated: bool,
    /// With `--generated`: shade each chunk by the time players spent near it, from faint
    /// (passed through) to strong (lived in), instead of by whether it's finished.
    #[arg(long, requires = "generated")]
    pub inhabited: bool,
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
    /// Shade by terrain height. Slow: each pixel needs its column's height.
    #[arg(long)]
    pub relief: bool,
    /// Draw biomes at the surface instead of at `--y`. Slow: each pixel needs its column's
    /// height.
    #[arg(long, conflicts_with = "no_biomes")]
    pub surface: bool,
    /// Tint copper and iron ore veins at `--y` (scales up to 4; the Overworld).
    #[arg(long)]
    pub veins: bool,
    /// Tint noise caves at `--y`: the caves in the terrain itself, before carvers (scales up
    /// to 4).
    #[arg(long)]
    pub caves: bool,
    /// Tint the caves and ravines carvers cut at `--y` (scales up to 4).
    #[arg(long)]
    pub carvers: bool,
    /// Draw the flower bone meal grows on grass at `--y`, the block above the grass: opaque
    /// where it's certain, fainter as it's less likely, and dimmed where none grows.
    #[arg(long)]
    pub flowers: bool,
    /// With `--flowers` implied: show only this flower, e.g. `minecraft:cornflower`, dimming
    /// the blocks that can't grow it.
    #[arg(long, value_name = "ID")]
    pub flower: Option<String>,
    /// A pin at block `x,z`, with a colour `#rrggbb` after another comma (amber if none).
    /// Repeat it for more; they're drawn last, in order.
    #[arg(long = "pin", allow_hyphen_values = true, value_parser = pin)]
    pub pins: Vec<Pin>,
    #[command(flatten)]
    pub common: Common,
}

/// A pin's colour when `--pin` gives none: the viewer's first marker colour.
const PIN_COLOUR: [u8; 3] = [0xf0, 0xb0, 0x30];

/// `x,z` or `x,z,#rrggbb`.
fn pin(text: &str) -> Result<Pin, String> {
    let bad = || format!("expected x,z or x,z,#rrggbb, got {text:?}");
    let (at, colour) = match text.rsplit_once(',') {
        Some((at, colour)) if colour.trim_start().starts_with('#') => (at, Some(colour.trim())),
        _ => (text, None),
    };
    let position = xz(at).map_err(|_| bad())?;
    let colour = match colour {
        None => PIN_COLOUR,
        Some(hex) => {
            let digits = hex
                .strip_prefix('#')
                .filter(|d| d.len() == 6 && d.bytes().all(|b| b.is_ascii_hexdigit()));
            let digits = digits.ok_or_else(bad)?;
            [0, 2, 4].map(|i| u8::from_str_radix(&digits[i..i + 2], 16).unwrap_or(0))
        }
    };
    Ok(Pin { position, colour })
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

fn xyz(text: &str) -> Result<[i32; 3], String> {
    match text
        .split(',')
        .map(|p| p.trim().parse::<i32>())
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(parts) if parts.len() == 3 => Ok([parts[0], parts[1], parts[2]]),
        _ => Err(format!("expected x,y,z in blocks, got {text:?}")),
    }
}

/// `from..to`, both included.
fn y_range(text: &str) -> Result<[i32; 2], String> {
    let parse = |s: &str| s.trim().parse::<i32>();
    match text.split_once("..").map(|(a, b)| (parse(a), parse(b))) {
        Some((Ok(from), Ok(to))) if from <= to => Ok([from, to]),
        _ => Err(format!(
            "expected from..to in blocks, from at most to, got {text:?}"
        )),
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
    /// What spyglass read from a save: its settings and where each came from, its data packs and
    /// what they change, what each dimension holds, and anything that makes answers inexact.
    Info {
        /// The save's folder, or its folder name in a launcher's saves.
        save: String,
        /// Where bundles are, one directory per version. `SPYGLASS_BUNDLES` sets the default.
        #[arg(long, default_value_os_t = spyglass::default_bundles_dir())]
        bundles: PathBuf,
        /// Skip reading every region file, so leave out what each dimension holds and the chunks
        /// older versions generated.
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
        /// Show each certain jigsaw structure's or mineshaft's bounds and piece count
        /// (villages, outposts, ancient cities, trial chambers, mineshafts and others). The first
        /// use for a jigsaw loads the template pools.
        #[arg(long)]
        bounds: bool,
        /// Answer with the nearest start, not `/locate`'s: the game's `/locate` takes the first
        /// it finds in its search order, which isn't always the nearest.
        #[arg(long)]
        nearest: bool,
    },
    /// The nearest feature: dungeons, geodes, desert wells, fossils or lava lakes, by placed
    /// feature (`minecraft:monster_room_deep`) or configured feature (`minecraft:monster_room`).
    Feature {
        /// Feature id.
        target: String,
        #[command(flatten)]
        common: Common,
        #[command(flatten)]
        from: From,
        /// How far to search, in chunks. Each chunk with a try takes a while to place.
        #[arg(long, default_value_t = 8)]
        radius: i32,
        /// Also list candidates that didn't place on spyglass's blocks, which the game may
        /// still place.
        #[arg(long)]
        all: bool,
        /// Also check whether each answer holds whatever the neighbouring chunks do, if
        /// they're decorated first: about twice as slow.
        #[arg(long)]
        neighbours: bool,
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
            | Locate::Feature { common, .. }
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
    /// A data pack, a folder or zip, to apply over the bundle as a world that enables it does.
    /// Repeatable, lowest first.
    #[arg(long = "pack", value_name = "PACK", conflicts_with = "world")]
    pub packs: Vec<PathBuf>,
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
