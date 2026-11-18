//! Where a Nether portal leads (`docs/specs/portals.md`): the exit block a trip aims at, and the
//! existing portal the game's search lands at from there.
//!
//! The search needs only the portals, not the world's blocks: the game looks up portal points
//! of interest near the exit and takes the nearest. So the answer is exact for the portals
//! given, and says when the game's choice between equally near portals can't be known.

use crate::{Dimension, Error, Game, Result};

/// The world border's largest size, and its default (`WorldBorder.MAX_SIZE`).
pub const DEFAULT_BORDER_SIZE: f64 = 5.999_996_8E7;

/// The most a border's bounds may reach from 0 by default (`absoluteMaxSize`, the server's
/// `max-world-size`).
pub const DEFAULT_ABSOLUTE_MAX: i32 = 29_999_984;

/// The most blocks a portal spans each way (`PortalShape.MAX_WIDTH` and `MAX_HEIGHT`).
pub const MAX_PORTAL_SIDE: i32 = 21;

/// How far `clampToBounds` keeps a position inside the border's high edge: the `float`
/// `1.0E-5f` widened, `9.999999747378752E-6`.
const BORDER_INSET: f64 = 9.999_999_747_378_752E-6;

/// The search's half-width in blocks into the Nether (`PortalForcer.NETHER_PORTAL_RADIUS`).
const NETHER_SEARCH_RADIUS: i32 = 16;

/// The search's half-width in blocks into any other dimension
/// (`PortalForcer.OVERWORLD_PORTAL_RADIUS`).
const OTHER_SEARCH_RADIUS: i32 = 128;

/// A dimension's world border, as static bounds (`docs/specs/portals.md#the-world-border`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Border {
    /// The centre's x and z (`center_x`, `center_z`).
    pub center: [f64; 2],
    /// The side's length in blocks (`size`). A moving border is given by where it's heading
    /// (`lerp_target`).
    pub size: f64,
    /// How far from 0 the bounds may reach (`absoluteMaxSize`).
    pub absolute_max: i32,
}

impl Default for Border {
    /// The border of a dimension that has never had one set: centred on 0, 0, as large as it
    /// gets.
    fn default() -> Self {
        Border {
            center: [0.0, 0.0],
            size: DEFAULT_BORDER_SIZE,
            absolute_max: DEFAULT_ABSOLUTE_MAX,
        }
    }
}

impl Border {
    /// `[min_x, max_x, min_z, max_z]`, as `WorldBorder.StaticBorderExtent` computes them.
    pub fn bounds(&self) -> [f64; 4] {
        let limit = f64::from(self.absolute_max);
        let half = self.size / 2.0;
        let [cx, cz] = self.center;
        [
            mth_clamp(cx - half, -limit, limit),
            mth_clamp(cx + half, -limit, limit),
            mth_clamp(cz - half, -limit, limit),
            mth_clamp(cz + half, -limit, limit),
        ]
    }

    /// `WorldBorder.clampToBounds(x, y, z)`: x and z clamped to just inside the border, then
    /// every coordinate floored (`BlockPos.containing`). y is left as it is.
    pub fn clamp_to_block(&self, [x, y, z]: [f64; 3]) -> [i32; 3] {
        let [min_x, max_x, min_z, max_z] = self.bounds();
        [
            floor(mth_clamp(x, min_x, max_x - BORDER_INSET)),
            floor(y),
            floor(mth_clamp(z, min_z, max_z - BORDER_INSET)),
        ]
    }

    /// `WorldBorder.isWithinBounds(BlockPos)`: whether a block's low corner is inside, with
    /// the low edges included and the high ones not.
    pub fn contains_block(&self, [x, _, z]: [i32; 3]) -> bool {
        let [min_x, max_x, min_z, max_z] = self.bounds();
        let (x, z) = (f64::from(x), f64::from(z));
        x >= min_x && x < max_x && z >= min_z && z < max_z
    }

    /// The blocks [`contains_block`](Self::contains_block) accepts along x and along z, as
    /// inclusive ranges: `[x_lo, x_hi, z_lo, z_hi]`. Empty ranges where the border is.
    fn block_ranges(&self) -> [i64; 4] {
        let [min_x, max_x, min_z, max_z] = self.bounds();
        // A whole number x is at least min when it's at least min's ceiling, and below max
        // when it's below max's ceiling. The bounds are within absolute_max, so they fit.
        let ceil = |v: f64| v.ceil() as i64;
        [ceil(min_x), ceil(max_x) - 1, ceil(min_z), ceil(max_z) - 1]
    }
}

/// `Mth.clamp(value, min, max)` for doubles: `value < min ? min : Math.min(value, max)`.
/// `Math.clamp`, which 26.4 uses, agrees for the finite values here.
fn mth_clamp(value: f64, min: f64, max: f64) -> f64 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

/// `Mth.floor`: rounded down to an `int`, saturating as Java's `(int)` cast does.
fn floor(value: f64) -> i32 {
    value.floor() as i32
}

/// Which way a portal's blocks run (its `axis` block state property).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Axis {
    /// Along x: the portal faces north and south.
    X,
    /// Along z: the portal faces east and west.
    Z,
}

/// A Nether portal: a rectangle of portal blocks standing in one column across its axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Portal {
    /// The dimension it's in.
    pub dimension: Dimension,
    /// Which way its blocks run.
    pub axis: Axis,
    /// Its lowest block: the least x, y and z of its blocks.
    pub corner: [i32; 3],
    /// How many blocks it spans along its axis, 1 to [`MAX_PORTAL_SIDE`].
    pub width: i32,
    /// How many blocks it spans up, 1 to [`MAX_PORTAL_SIDE`].
    pub height: i32,
}

impl Portal {
    /// A portal of `width` × `height` blocks from `corner`. Fails if a side is less than 1 or
    /// more than [`MAX_PORTAL_SIDE`], or the portal would reach past the `i32` coordinates.
    pub fn new(
        dimension: Dimension,
        axis: Axis,
        corner: [i32; 3],
        width: i32,
        height: i32,
    ) -> Result<Portal> {
        for (side, value) in [("width", width), ("height", height)] {
            if !(1..=MAX_PORTAL_SIDE).contains(&value) {
                return Err(Error::InvalidPortal(format!(
                    "a portal's {side} is 1 to {MAX_PORTAL_SIDE} blocks, not {value}"
                )));
            }
        }
        let portal = Portal {
            dimension,
            axis,
            corner,
            width,
            height,
        };
        if portal
            .extent()
            .iter()
            .any(|&[_, hi]| hi > i64::from(i32::MAX))
        {
            return Err(Error::InvalidPortal(format!(
                "a portal at {corner:?} would reach past the world's coordinates"
            )));
        }
        Ok(portal)
    }

    /// Its blocks' inclusive range in x, y and z: `[[x_lo, x_hi], [y_lo, y_hi], [z_lo, z_hi]]`.
    pub fn extent(&self) -> [[i64; 2]; 3] {
        let [x, y, z] = self.corner.map(i64::from);
        let (width, height) = (i64::from(self.width) - 1, i64::from(self.height) - 1);
        match self.axis {
            Axis::X => [[x, x + width], [y, y + height], [z, z]],
            Axis::Z => [[x, x], [y, y + height], [z, z + width]],
        }
    }

    /// Whether `block` is one of its blocks.
    pub fn contains(&self, block: [i32; 3]) -> bool {
        self.extent()
            .iter()
            .zip(block)
            .all(|(&[lo, hi], v)| (lo..=hi).contains(&i64::from(v)))
    }
}

/// Where a trip through a portal aims.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortalTarget {
    /// The dimension it leads to.
    pub dimension: Dimension,
    /// The exit block the search starts from.
    pub exit: [i32; 3],
}

/// The dimension a Nether portal in `from` leads to: the Overworld from the Nether, and the
/// Nether from anywhere else, the End included (`NetherPortalBlock.getPortalDestination`).
pub fn portal_destination(from: Dimension) -> Dimension {
    match from {
        Dimension::Nether => Dimension::Overworld,
        Dimension::Overworld | Dimension::End => Dimension::Nether,
    }
}

impl Game {
    /// `NetherPortalBlock.getPortalDestination`: where an entity at `pos` in a portal in
    /// `from` aims. Its x and z are scaled by the dimensions' coordinate scales and clamped to
    /// `border`, the target dimension's; y is left as it is
    /// (`docs/specs/portals.md#the-target`). Fails if the preset lacks either dimension.
    pub fn portal_target(
        &self,
        from: Dimension,
        [x, y, z]: [f64; 3],
        border: &Border,
    ) -> Result<PortalTarget> {
        let to = portal_destination(from);
        // DimensionType.getTeleportationScale: from's scale over to's.
        let scale = self.coordinate_scale(from)? / self.coordinate_scale(to)?;
        Ok(PortalTarget {
            dimension: to,
            exit: border.clamp_to_block([x * scale, y, z * scale]),
        })
    }
}

/// A portal block the game's search may land at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Landing {
    /// The portal, by its index in the portals searched.
    pub portal: usize,
    /// The block of it the search picks.
    pub block: [i32; 3],
}

/// What the game's search finds from an exit block (`docs/specs/portals.md#the-search`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Link {
    /// It lands at this portal.
    Lands(Landing),
    /// It lands at one of these, equally near, which the game picks by an order spyglass can't
    /// know (`docs/specs/portals.md#ties`). By portal index.
    Tied(Vec<Landing>),
    /// No portal is in range, so the game builds one.
    NewPortal,
}

impl Link {
    /// The portals it may land at: one, the tied ones, or none.
    pub fn landings(&self) -> &[Landing] {
        match self {
            Link::Lands(landing) => std::slice::from_ref(landing),
            Link::Tied(landings) => landings,
            Link::NewPortal => &[],
        }
    }
}

/// How far the search reaches from the exit in x and z into `to`: 16 into the Nether, 128
/// elsewhere, chosen by which dimension it is, not by its scale.
pub fn portal_search_radius(to: Dimension) -> i32 {
    if to == Dimension::Nether {
        NETHER_SEARCH_RADIUS
    } else {
        OTHER_SEARCH_RADIUS
    }
}

/// `PortalForcer.findClosestPortalPosition`: the portal in `to` the game lands at from `exit`,
/// among `portals` (those in other dimensions are ignored), with `border` the dimension's.
///
/// The game takes every portal block within the search square and the border, nearest first
/// (`distSqr`), then lowest. Within one portal the nearest block is unique, the exit's
/// coordinates clamped to the portal's, so each portal has one candidate and a tie is always
/// between portals: settled by chunk order across chunks, and left tied within one.
pub fn portal_link(to: Dimension, exit: [i32; 3], portals: &[Portal], border: &Border) -> Link {
    let radius = i64::from(portal_search_radius(to));
    let [x_lo, x_hi, z_lo, z_hi] = border.block_ranges();
    let [ex, ey, ez] = exit.map(i64::from);
    // The square and the border, as ranges of x and z.
    let x_range = [x_lo.max(ex - radius), x_hi.min(ex + radius)];
    let z_range = [z_lo.max(ez - radius), z_hi.min(ez + radius)];

    let mut best: Option<(f64, i32)> = None;
    let mut candidates: Vec<(Landing, (f64, i32))> = Vec::new();
    for (index, portal) in portals.iter().enumerate() {
        if portal.dimension != to {
            continue;
        }
        let [px, py, pz] = portal.extent();
        let Some(x) = nearest(ex, intersect(px, x_range)) else {
            continue;
        };
        let Some(z) = nearest(ez, intersect(pz, z_range)) else {
            continue;
        };
        let Some(y) = nearest(ey, py) else {
            continue;
        };
        // The portal's range was checked to fit in i32 when it was made, and the square and
        // border only narrow it.
        let block = [x, y, z].map(|v| v as i32);
        let key = (dist_sqr(block, exit), block[1]);
        if best.is_none_or(|b| before(key, b)) {
            best = Some(key);
        }
        candidates.push((
            Landing {
                portal: index,
                block,
            },
            key,
        ));
    }
    let Some(best) = best else {
        return Link::NewPortal;
    };
    let tied: Vec<Landing> = candidates
        .into_iter()
        .filter(|&(_, key)| key == best)
        .map(|(landing, _)| landing)
        .collect();
    // ChunkPos.rangeClosed walks chunk z, then chunk x, both upwards; the first chunk's
    // candidates come first in the stream, and Stream.min keeps the first of equals.
    let chunk = |l: &Landing| (l.block[2] >> 4, l.block[0] >> 4);
    let first = tied
        .iter()
        .map(chunk)
        .min()
        .expect("the best candidate is among them");
    let mut first_chunk: Vec<Landing> = tied.into_iter().filter(|l| chunk(l) == first).collect();
    if first_chunk.len() == 1 {
        Link::Lands(first_chunk[0])
    } else {
        first_chunk.sort_by_key(|l| l.portal);
        Link::Tied(first_chunk)
    }
}

/// Whether search key `a` (distance, then y) comes strictly before `b`.
fn before(a: (f64, i32), b: (f64, i32)) -> bool {
    a.0 < b.0 || (a.0 == b.0 && a.1 < b.1)
}

/// `[lo, hi]` and `[lo2, hi2]` overlapped, possibly empty.
fn intersect([lo, hi]: [i64; 2], [lo2, hi2]: [i64; 2]) -> [i64; 2] {
    [lo.max(lo2), hi.min(hi2)]
}

/// The whole number in `[lo, hi]` nearest `v`, if the range isn't empty.
fn nearest(v: i64, [lo, hi]: [i64; 2]) -> Option<i64> {
    (lo <= hi).then(|| v.clamp(lo, hi))
}

/// `Vec3i.distSqr`: `record.distSqr(exit)`, each difference taken in `double` and the squares
/// summed x, y, z, as `distToLowCornerSqr` does.
fn dist_sqr(record: [i32; 3], exit: [i32; 3]) -> f64 {
    let [dx, dy, dz] = [0, 1, 2].map(|i| f64::from(record[i]) - f64::from(exit[i]));
    dx * dx + dy * dy + dz * dz
}

/// Half a player's width, `0.6f / 2` in `float`, widened (`EntityDimensions.makeBoundingBox`).
const PLAYER_HALF_WIDTH: f64 = 0.300_000_011_920_928_96;

/// A standing player's height, the `float` `1.8f` widened.
const PLAYER_HEIGHT: f64 = 1.799_999_952_316_284_2;

/// How much `Entity.checkInsideBlocks` shrinks the entity's box on every side before finding
/// the blocks it's in: the `float` `1.0E-5f` widened.
const INSIDE_DEFLATE: f64 = 9.999_999_747_378_752E-6;

impl Portal {
    /// Where a standing player's position (the middle of their feet) can be while they're in
    /// the portal, with a full frame around it (`docs/specs/portals.md#entering-a-portal`):
    /// `[[x_lo, x_hi], [y_lo, y_hi], [z_lo, z_hi]]`. Along the portal and up it's closed: the
    /// frame stops the player's box at its sides. Across it the high end is open: the box must
    /// overlap the portal's column by more than the inside check's deflation. `None` if the
    /// portal is too low for a standing player.
    pub fn standing_range(&self) -> Option<[[f64; 2]; 3]> {
        let [[along_lo, along_hi], [y_lo, y_hi], [across, _]] = self.extent_along();
        let h = PLAYER_HALF_WIDTH;
        let along = [along_lo as f64 + h, (along_hi + 1) as f64 - h];
        let up = [y_lo as f64, (y_hi + 1) as f64 - PLAYER_HEIGHT];
        let across = [
            across as f64 - h + INSIDE_DEFLATE,
            (across + 1) as f64 + h - INSIDE_DEFLATE,
        ];
        if up[0] > up[1] || along[0] > along[1] {
            return None;
        }
        Some(match self.axis {
            Axis::X => [along, up, across],
            Axis::Z => [across, up, along],
        })
    }

    /// [`extent`](Self::extent) as `[along, up, across]`.
    fn extent_along(&self) -> [[i64; 2]; 3] {
        let [x, y, z] = self.extent();
        match self.axis {
            Axis::X => [x, y, z],
            Axis::Z => [z, y, x],
        }
    }
}

/// Where a portal leads from every place a standing player can be in it.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct PortalLinks {
    /// The dimension it leads to.
    pub dimension: Dimension,
    /// The exit blocks the player can aim at, as inclusive ranges of x, y and z. Every block
    /// in them is reachable.
    pub exits: [[i32; 2]; 3],
    /// Each exit block's link, x fastest, then z, then y.
    pub links: Vec<ExitLink>,
}

/// One exit block's link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExitLink {
    /// The exit block.
    pub exit: [i32; 3],
    /// Where the search lands from it.
    pub link: Link,
}

/// Counts `exits` more exit blocks that lead to `portals` into `outcomes`.
pub(crate) fn add_outcome(
    outcomes: &mut Vec<LinkOutcome>,
    portals: Vec<usize>,
    tied: bool,
    exits: usize,
) {
    match outcomes
        .iter_mut()
        .find(|o| o.portals == portals && o.tied == tied)
    {
        Some(outcome) => outcome.exits += exits,
        None => outcomes.push(LinkOutcome {
            portals,
            tied,
            exits,
        }),
    }
}

/// Most exits first, then by portal index.
pub(crate) fn sort_outcomes(outcomes: &mut [LinkOutcome]) {
    outcomes.sort_by(|a, b| {
        b.exits
            .cmp(&a.exits)
            .then_with(|| a.portals.cmp(&b.portals))
    });
}

/// Exit blocks that lead to the same place, with how many there are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkOutcome {
    /// The portals it lands at, by index: one, the tied ones, or none (the game builds one).
    pub portals: Vec<usize>,
    /// Whether the portals are tied, rather than one.
    pub tied: bool,
    /// How many exit blocks lead there.
    pub exits: usize,
}

impl PortalLinks {
    /// Where its exit blocks lead, most exits first, then by portal index.
    pub fn outcomes(&self) -> Vec<LinkOutcome> {
        self.outcomes_by(|portal| portal)
    }

    /// [`outcomes`](Self::outcomes) with the portals searched gathered into groups: `group`
    /// gives each portal's, by index, such as the save portal a rectangle is part of. A tie
    /// within one group lands at that group.
    pub fn outcomes_by(&self, group: impl Fn(usize) -> usize) -> Vec<LinkOutcome> {
        let mut outcomes: Vec<LinkOutcome> = Vec::new();
        for exit in &self.links {
            let mut portals: Vec<usize> = exit
                .link
                .landings()
                .iter()
                .map(|l| group(l.portal))
                .collect();
            portals.sort_unstable();
            portals.dedup();
            let tied = portals.len() > 1;
            add_outcome(&mut outcomes, portals, tied, 1);
        }
        sort_outcomes(&mut outcomes);
        outcomes
    }

    /// The one portal every exit lands at, if there is one.
    pub fn only(&self) -> Option<usize> {
        match &self.outcomes()[..] {
            [only] if !only.tied => only.portals.first().copied(),
            _ => None,
        }
    }
}

impl Game {
    /// Where `portal` leads, from every position a standing player can be in it
    /// ([`Portal::standing_range`]): each exit block they can aim at, and the portal among
    /// `portals` the game lands at from it, with `border` the target dimension's. A player
    /// moving through the portal can aim up to a tick's movement beyond these exits. `None`
    /// if the portal is too low to stand in. Fails if the preset lacks either dimension.
    pub fn portal_links(
        &self,
        portal: &Portal,
        portals: &[Portal],
        border: &Border,
    ) -> Result<Option<PortalLinks>> {
        let Some(range) = portal.standing_range() else {
            return Ok(None);
        };
        let lowest = [range[0][0], range[1][0], range[2][0]];
        // The high end across the portal is open: the last position before it.
        let mut highest = [range[0][1], range[1][1], range[2][1]];
        let across = match portal.axis {
            Axis::X => 2,
            Axis::Z => 0,
        };
        highest[across] = highest[across].next_down();
        // The target is monotonic in each coordinate, so the range's corners give the
        // exits' ranges.
        let low = self.portal_target(portal.dimension, lowest, border)?;
        let high = self.portal_target(portal.dimension, highest, border)?;
        let exits = [0, 1, 2].map(|i| [low.exit[i], high.exit[i]]);
        // Only portals within the search square of some exit can be found: search those,
        // keeping their order so ties list them as the full slice would.
        let radius = i64::from(portal_search_radius(low.dimension));
        let reach = |[lo, hi]: [i32; 2]| [i64::from(lo) - radius, i64::from(hi) + radius];
        let (x_reach, z_reach) = (reach(exits[0]), reach(exits[2]));
        let (nearby, indices): (Vec<Portal>, Vec<usize>) = portals
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                let [px, _, pz] = p.extent();
                p.dimension == low.dimension
                    && px[0] <= x_reach[1]
                    && px[1] >= x_reach[0]
                    && pz[0] <= z_reach[1]
                    && pz[1] >= z_reach[0]
            })
            .map(|(i, p)| (*p, i))
            .unzip();
        let mut links = Vec::new();
        for y in exits[1][0]..=exits[1][1] {
            for z in exits[2][0]..=exits[2][1] {
                for x in exits[0][0]..=exits[0][1] {
                    let exit = [x, y, z];
                    let link = crate::link_map::reindex(
                        portal_link(low.dimension, exit, &nearby, border),
                        &indices,
                    );
                    links.push(ExitLink { exit, link });
                }
            }
        }
        Ok(Some(PortalLinks {
            dimension: low.dimension,
            exits,
            links,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const O: Dimension = Dimension::Overworld;
    const N: Dimension = Dimension::Nether;

    fn portal(dimension: Dimension, axis: Axis, corner: [i32; 3]) -> Portal {
        Portal::new(dimension, axis, corner, 2, 3).unwrap()
    }

    fn lands(link: &Link) -> Landing {
        match link {
            Link::Lands(landing) => *landing,
            other => panic!("expected one landing, got {other:?}"),
        }
    }

    #[test]
    fn the_end_and_the_overworld_lead_to_the_nether() {
        assert_eq!(portal_destination(Dimension::End), N);
        assert_eq!(portal_destination(O), N);
        assert_eq!(portal_destination(N), O);
    }

    #[test]
    fn the_default_border_reaches_the_world_edge() {
        let border = Border::default();
        assert_eq!(
            border.bounds(),
            [-29_999_984.0, 29_999_984.0, -29_999_984.0, 29_999_984.0]
        );
        assert!(border.contains_block([-29_999_984, 0, 29_999_983]));
        assert!(!border.contains_block([29_999_984, 0, 0]));
        assert_eq!(
            border.clamp_to_block([1e9, 70.5, -1e9]),
            [29_999_983, 70, -29_999_984]
        );
    }

    #[test]
    fn a_small_border_clamps_x_and_z_but_not_y() {
        let border = Border {
            center: [0.5, 0.0],
            size: 10.0,
            ..Border::default()
        };
        assert_eq!(border.bounds(), [-4.5, 5.5, -5.0, 5.0]);
        // Clamped to just inside the high edge, so the block is the edge's last.
        assert_eq!(border.clamp_to_block([100.0, 1000.0, 100.0]), [5, 1000, 4]);
        assert_eq!(
            border.clamp_to_block([-100.0, -999.5, -100.0]),
            [-5, -1000, -5]
        );
        // A block's low corner counts: -4.5 isn't a whole number, so -4 is the first in.
        assert!(!border.contains_block([-5, 0, 0]));
        assert!(border.contains_block([-4, 0, 0]));
        assert!(border.contains_block([5, 0, 4]));
        assert!(!border.contains_block([5, 0, 5]));
        assert_eq!(border.block_ranges(), [-4, 5, -5, 4]);
    }

    #[test]
    fn portals_have_sizes_the_game_allows() {
        assert!(Portal::new(O, Axis::X, [0, 0, 0], 0, 3).is_err());
        assert!(Portal::new(O, Axis::X, [0, 0, 0], 2, 22).is_err());
        assert!(Portal::new(O, Axis::Z, [0, 0, i32::MAX - 1], 3, 3).is_err());
        let p = portal(O, Axis::Z, [4, 64, -3]);
        assert_eq!(p.extent(), [[4, 4], [64, 66], [-3, -2]]);
        assert!(p.contains([4, 66, -2]));
        assert!(!p.contains([5, 64, -3]));
    }

    #[test]
    fn the_square_reaches_the_radius_and_no_further() {
        let border = Border::default();
        for (to, radius) in [(N, 16), (O, 128)] {
            assert_eq!(portal_search_radius(to), radius);
            // Portals along z, so x is a single column.
            let at = |x| [portal(to, Axis::Z, [x, 70, 0])];
            let inside = portal_link(to, [0, 70, 0], &at(radius), &border);
            assert_eq!(lands(&inside).block, [radius, 70, 0]);
            let inside = portal_link(to, [0, 70, 0], &at(-radius), &border);
            assert_eq!(lands(&inside).block, [-radius, 70, 0]);
            assert_eq!(
                portal_link(to, [0, 70, 0], &at(radius + 1), &border),
                Link::NewPortal
            );
            assert_eq!(
                portal_link(to, [0, 70, 0], &at(-radius - 1), &border),
                Link::NewPortal
            );
        }
    }

    #[test]
    fn height_doesnt_limit_the_search() {
        let border = Border::default();
        let far_up = [portal(N, Axis::X, [3, 120, 3])];
        let link = portal_link(N, [0, -2000, 0], &far_up, &border);
        assert_eq!(lands(&link).block, [3, 120, 3]);
    }

    #[test]
    fn portals_in_other_dimensions_and_outside_the_border_are_skipped() {
        let border = Border {
            size: 20.0,
            ..Border::default()
        };
        let portals = [
            portal(O, Axis::X, [1, 70, 1]),
            portal(N, Axis::Z, [10, 70, 0]),
            portal(N, Axis::Z, [-12, 70, 0]),
        ];
        assert_eq!(
            portal_link(N, [0, 70, 0], &portals, &border),
            Link::NewPortal
        );
        // A portal partly inside: its inside blocks are candidates.
        let straddling = [portal(N, Axis::X, [9, 70, 0])];
        let link = portal_link(N, [0, 70, 0], &straddling, &border);
        assert_eq!(lands(&link).block, [9, 70, 0]);
    }

    #[test]
    fn the_nearest_portal_wins_and_its_nearest_block() {
        let border = Border::default();
        let portals = [
            portal(O, Axis::X, [20, 64, 20]),
            portal(O, Axis::Z, [-5, 70, 3]),
        ];
        let link = portal_link(O, [0, 71, 0], &portals, &border);
        assert_eq!(
            lands(&link),
            Landing {
                portal: 1,
                block: [-5, 71, 3]
            }
        );
    }

    #[test]
    fn lower_wins_a_tie_of_distance() {
        let border = Border::default();
        // Both 2 away, one above the exit and one below.
        let portals = [
            portal(N, Axis::X, [0, 72, 4]),
            portal(N, Axis::X, [0, 66, 4]),
        ];
        let link = portal_link(N, [0, 70, 4], &portals, &border);
        assert_eq!(lands(&link).portal, 1);
    }

    #[test]
    fn chunk_order_settles_a_tie_across_chunks() {
        let border = Border::default();
        // Equally near, at the same height, either side of the exit in x: chunk -1 comes
        // before chunk 0 along x.
        let east_west = [
            portal(N, Axis::Z, [10, 70, 0]),
            portal(N, Axis::Z, [-10, 70, 0]),
        ];
        let link = portal_link(N, [0, 70, 0], &east_west, &border);
        assert_eq!(lands(&link).portal, 1);
        // Chunk z decides before chunk x: north (z -10) beats east and west.
        let mut around = east_west.to_vec();
        around.push(portal(N, Axis::X, [0, 70, -10]));
        let link = portal_link(N, [0, 70, 0], &around, &border);
        assert_eq!(lands(&link).portal, 2);
    }

    #[test]
    fn a_tie_within_one_section_stays_tied() {
        let border = Border::default();
        // Both in chunk 0, 0, 3 blocks from the exit at the same height.
        let portals = [
            portal(N, Axis::Z, [2, 70, 8]),
            portal(N, Axis::Z, [8, 70, 8]),
        ];
        let link = portal_link(N, [5, 70, 8], &portals, &border);
        assert_eq!(
            link,
            Link::Tied(vec![
                Landing {
                    portal: 0,
                    block: [2, 70, 8]
                },
                Landing {
                    portal: 1,
                    block: [8, 70, 8]
                },
            ])
        );
        assert_eq!(link.landings().len(), 2);
    }

    #[test]
    fn an_exit_inside_a_portal_lands_there() {
        let border = Border::default();
        let portals = [portal(O, Axis::X, [100, 64, -50])];
        let link = portal_link(O, [101, 65, -50], &portals, &border);
        assert_eq!(lands(&link).block, [101, 65, -50]);
    }

    #[test]
    fn links_from_a_portal_search_every_portal_in_range() {
        let bundle = spyglass_oracle::fixtures::overworld_and_nether();
        let game = Game::open(bundle.path()).unwrap();
        let border = Border::default();
        // Portals in both dimensions, near and far, a grid of them around the origin: the
        // nearby ones keep their indices, and ties between them list the same portals.
        let mut portals = Vec::new();
        for i in -6..=6 {
            for j in -6..=6 {
                let axis = if (i + j) % 2 == 0 { Axis::X } else { Axis::Z };
                portals.push(portal(O, axis, [i * 37, 60 + j, j * 41]));
                portals.push(portal(N, axis, [i * 5, 70 + i, j * 6]));
            }
        }
        for source in [
            portal(N, Axis::X, [3, 64, -2]),
            portal(O, Axis::Z, [-20, 71, 30]),
        ] {
            let links = game
                .portal_links(&source, &portals, &border)
                .unwrap()
                .unwrap();
            assert!(!links.links.is_empty());
            for exit in &links.links {
                assert_eq!(
                    exit.link,
                    portal_link(links.dimension, exit.exit, &portals, &border),
                    "{:?}",
                    exit.exit
                );
            }
        }
    }

    #[test]
    fn distance_is_the_games_double() {
        assert_eq!(dist_sqr([3, 4, 0], [0, 0, 0]), 25.0);
        assert_eq!(dist_sqr([-1, 0, 2], [1, 0, 0]), 8.0);
    }
}
