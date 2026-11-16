//! Where a portal built in an area would link: for each pixel's block, the portal on the other
//! side the game's search lands at (`docs/specs/portals.md#where-a-portal-links`).
//!
//! The work is done on the target side. Into the Nether, the blocks of 8 × 8 columns share an
//! exit block, so each exit's link is found once and every pixel aiming at it reuses it. Out of
//! the Nether, each block aims at 8 × 8 exit blocks of its own, so they're searched per pixel,
//! in parallel.

use std::collections::HashMap;

use rayon::prelude::*;

use crate::{
    Border, Dimension, Game, Link, MapArea, Portal, Result, portal_destination, portal_link,
    portal_search_radius,
};

/// Which portal a search lands at, without the block of it: what a map shows.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Destination {
    /// This portal, by index.
    Portal(usize),
    /// One of these, equally near, by index: the game's pick can't be known.
    Tied(Vec<usize>),
    /// None in range: the game builds one.
    NewPortal,
}

impl From<&Link> for Destination {
    fn from(link: &Link) -> Self {
        match link {
            Link::Lands(landing) => Destination::Portal(landing.portal),
            Link::Tied(landings) => Destination::Tied(landings.iter().map(|l| l.portal).collect()),
            Link::NewPortal => Destination::NewPortal,
        }
    }
}

impl Destination {
    /// Whether it may be `portal`, by index.
    pub fn may_be(&self, portal: usize) -> bool {
        match self {
            Destination::Portal(p) => *p == portal,
            Destination::Tied(portals) => portals.contains(&portal),
            Destination::NewPortal => false,
        }
    }
}

/// What a portal built at one pixel's block would link to.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PixelLink {
    /// Wherever in the block a player stands, this.
    One(Destination),
    /// It depends where in the block the player stands: each destination some part of it
    /// gives, in order of exit block (x fastest, then z).
    Mixed(Vec<Destination>),
}

impl PixelLink {
    /// The portal every part of the block lands at, if there's one.
    pub fn portal(&self) -> Option<usize> {
        match self {
            PixelLink::One(Destination::Portal(portal)) => Some(*portal),
            _ => None,
        }
    }

    /// Whether some part of the block may land at `portal`, by index.
    pub fn may_reach(&self, portal: usize) -> bool {
        match self {
            PixelLink::One(destination) => destination.may_be(portal),
            PixelLink::Mixed(destinations) => destinations.iter().any(|d| d.may_be(portal)),
        }
    }
}

/// Where a portal built at each pixel of an area would link, from [`Game::portal_link_map`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkMap {
    /// The area, in the source dimension; its `y` is the height the portal's player stands at.
    area: MapArea,
    /// The dimension the links lead to.
    dimension: Dimension,
    /// The answers the pixels have, each once.
    answers: Vec<PixelLink>,
    /// Each pixel's answer, rows top to bottom.
    cells: Vec<u32>,
}

impl LinkMap {
    /// The area it's of.
    pub fn area(&self) -> &MapArea {
        &self.area
    }

    /// The dimension its links lead to.
    pub fn dimension(&self) -> Dimension {
        self.dimension
    }

    /// The answer at `pixel`.
    pub fn at(&self, [i, j]: [u32; 2]) -> &PixelLink {
        let index = j as usize * self.area.size[0] as usize + i as usize;
        &self.answers[self.cells[index] as usize]
    }

    /// The distinct answers, each with how many pixels have it, most first.
    pub fn counts(&self) -> Vec<(&PixelLink, u64)> {
        let mut uses = vec![0u64; self.answers.len()];
        for &c in &self.cells {
            uses[c as usize] += 1;
        }
        let mut counts: Vec<(&PixelLink, u64)> = self.answers.iter().zip(uses).collect();
        counts.sort_by_key(|c| std::cmp::Reverse(c.1));
        counts
    }
}

/// The exit blocks a position anywhere in `block` (at its own y) aims at, as inclusive ranges
/// `[[x_lo, x_hi], [z_lo, z_hi]]`, and their y.
fn block_exits(
    game: &Game,
    from: Dimension,
    [bx, y, bz]: [i32; 3],
    border: &Border,
) -> Result<([[i32; 2]; 2], i32)> {
    let (x, z) = (f64::from(bx), f64::from(bz));
    let low = game.portal_target(from, [x, f64::from(y), z], border)?;
    // The block's far edge is open: the last position before it.
    let far = |v: f64| (v + 1.0).next_down();
    let high = game.portal_target(from, [far(x), f64::from(y), far(z)], border)?;
    Ok((
        [[low.exit[0], high.exit[0]], [low.exit[2], high.exit[2]]],
        low.exit[1],
    ))
}

impl Game {
    /// For each pixel of `area` in `source`, where a Nether portal built at its corner block
    /// `(min.x + i·scale, y, min.z + j·scale)` would link: the portal among `portals` the
    /// game's search lands at from wherever in that block a player stands at height `y`, with
    /// `border` the target dimension's (`docs/specs/portals.md#where-a-portal-links`). Equals
    /// [`portal_link`] from each of the block's exit blocks. Fails if the area isn't valid or
    /// the preset lacks either dimension.
    pub fn portal_link_map(
        &self,
        source: Dimension,
        area: &MapArea,
        portals: &[Portal],
        border: &Border,
    ) -> Result<LinkMap> {
        area.check()?;
        let to = portal_destination(source);
        let [w, h] = area.size;
        let scale = i64::from(area.scale);
        let block = |i: u32, j: u32| {
            // The area's check keeps every corner within i32.
            [
                (i64::from(area.min[0]) + i64::from(i) * scale) as i32,
                area.y,
                (i64::from(area.min[1]) + i64::from(j) * scale) as i32,
            ]
        };
        // Only portals within the search square of the area's exits can be found.
        let (corner, _) = block_exits(self, source, block(0, 0), border)?;
        let (far, y) = block_exits(self, source, block(w - 1, h - 1), border)?;
        let radius = i64::from(portal_search_radius(to));
        let reach = |lo: i32, hi: i32| [i64::from(lo) - radius, i64::from(hi) + radius];
        let (x_reach, z_reach) = (
            reach(corner[0][0], far[0][1]),
            reach(corner[1][0], far[1][1]),
        );
        let (nearby, indices): (Vec<Portal>, Vec<usize>) = portals
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                let [px, _, pz] = p.extent();
                p.dimension == to
                    && px[0] <= x_reach[1]
                    && px[1] >= x_reach[0]
                    && pz[0] <= z_reach[1]
                    && pz[1] >= z_reach[0]
            })
            .map(|(i, p)| (*p, i))
            .unzip();
        let link_at = |x: i32, z: i32| {
            Destination::from(&reindex(
                portal_link(to, [x, y, z], &nearby, border),
                &indices,
            ))
        };
        let pixel = |exits: [[i32; 2]; 2], cache: &mut HashMap<[i32; 2], Destination>| {
            let mut links: Vec<Destination> = Vec::new();
            for z in exits[1][0]..=exits[1][1] {
                for x in exits[0][0]..=exits[0][1] {
                    let link = cache.entry([x, z]).or_insert_with(|| link_at(x, z)).clone();
                    if !links.contains(&link) {
                        links.push(link);
                    }
                }
            }
            if links.len() == 1 {
                PixelLink::One(links.pop().expect("one"))
            } else {
                PixelLink::Mixed(links)
            }
        };
        // Rows in parallel, each with its own cache of exit blocks' links: into the Nether a
        // row's pixels share them; out of it, nothing is shared, and the cache stays small.
        let rows: Vec<Vec<PixelLink>> = (0..h)
            .into_par_iter()
            .map(|j| {
                let mut cache = HashMap::new();
                (0..w)
                    .map(|i| {
                        let (exits, _) = block_exits(self, source, block(i, j), border)?;
                        if cache.len() > 4096 {
                            cache.clear();
                        }
                        Ok(pixel(exits, &mut cache))
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .collect::<Result<_>>()?;
        let mut answers: Vec<PixelLink> = Vec::new();
        let mut index: HashMap<PixelLink, u32> = HashMap::new();
        let mut cells = Vec::with_capacity(w as usize * h as usize);
        for answer in rows.into_iter().flatten() {
            let next = answers.len() as u32;
            let at = *index.entry(answer.clone()).or_insert_with(|| {
                answers.push(answer);
                next
            });
            cells.push(at);
        }
        Ok(LinkMap {
            area: *area,
            dimension: to,
            answers,
            cells,
        })
    }
}

/// `link`'s portals, from indices in a filtered slice back to the full one's.
pub(crate) fn reindex(link: Link, indices: &[usize]) -> Link {
    let back = |mut landing: crate::Landing| {
        landing.portal = indices[landing.portal];
        landing
    };
    match link {
        Link::Lands(landing) => Link::Lands(back(landing)),
        Link::Tied(landings) => Link::Tied(landings.into_iter().map(back).collect()),
        Link::NewPortal => Link::NewPortal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Axis;

    const O: Dimension = Dimension::Overworld;
    const N: Dimension = Dimension::Nether;

    fn game() -> Game {
        let bundle = spyglass_oracle::fixtures::overworld_and_nether();
        Game::open(bundle.path()).unwrap()
    }

    /// Portals in both dimensions, some tied, some far.
    fn portals() -> Vec<Portal> {
        let mut portals = Vec::new();
        for i in -4..=4 {
            for j in -4..=4 {
                let axis = if (i + j) % 2 == 0 { Axis::X } else { Axis::Z };
                portals.push(Portal::new(O, axis, [i * 61, 64 + j, j * 53], 2, 3).unwrap());
                portals.push(Portal::new(N, axis, [i * 9, 64 - i, j * 7], 2, 3).unwrap());
            }
        }
        // Two equally near a column in one chunk, for ties.
        portals.push(Portal::new(N, Axis::Z, [2, 70, 8], 2, 3).unwrap());
        portals.push(Portal::new(N, Axis::Z, [8, 70, 8], 2, 3).unwrap());
        portals
    }

    #[test]
    fn every_pixel_is_portal_link_from_its_block_s_exits() {
        let game = game();
        let portals = portals();
        let border = Border {
            size: 300.0,
            ..Border::default()
        };
        for (source, scale, centre) in [(O, 1, [40, 64]), (O, 4, [-100, 30]), (N, 1, [3, -2])] {
            let area = MapArea::centred_on(centre, [48, 40], scale, 70).unwrap();
            let map = game
                .portal_link_map(source, &area, &portals, &border)
                .unwrap();
            assert_eq!(map.dimension(), portal_destination(source));
            let mut kinds = [0; 3];
            for j in 0..area.size[1] {
                for i in 0..area.size[0] {
                    let s = i64::from(scale);
                    let block = [
                        (i64::from(area.min[0]) + i64::from(i) * s) as i32,
                        70,
                        (i64::from(area.min[1]) + i64::from(j) * s) as i32,
                    ];
                    let (exits, y) = block_exits(&game, source, block, &border).unwrap();
                    let mut expected = Vec::new();
                    for z in exits[1][0]..=exits[1][1] {
                        for x in exits[0][0]..=exits[0][1] {
                            let link = Destination::from(&portal_link(
                                map.dimension(),
                                [x, y, z],
                                &portals,
                                &border,
                            ));
                            if !expected.contains(&link) {
                                expected.push(link);
                            }
                        }
                    }
                    let expected = if expected.len() == 1 {
                        PixelLink::One(expected.pop().unwrap())
                    } else {
                        PixelLink::Mixed(expected)
                    };
                    let got = map.at([i, j]);
                    assert_eq!(got, &expected, "{source} {block:?}");
                    kinds[match got {
                        PixelLink::One(Destination::Portal(_)) => 0,
                        PixelLink::Mixed(_) => 1,
                        _ => 2,
                    }] += 1;
                }
            }
            assert!(kinds[0] > 0, "{source} scale {scale}: {kinds:?}");
            if source == N {
                assert!(kinds[1] > 0, "no mixed pixels: {kinds:?}");
            }
        }
    }

    /// A full view's time, by eye: `cargo test --release -p spyglass --lib link_map_time --
    /// --ignored --nocapture`.
    #[test]
    #[ignore = "a timing, not a check"]
    fn link_map_time() {
        let game = game();
        let portals = portals();
        let border = Border::default();
        for (source, scale) in [(O, 1), (O, 16), (N, 1), (N, 4)] {
            let area = MapArea::centred_on([0, 0], [1024, 768], scale, 70).unwrap();
            let start = std::time::Instant::now();
            let map = game
                .portal_link_map(source, &area, &portals, &border)
                .unwrap();
            println!(
                "{source} scale {scale}: {:.2?}, {} answers",
                start.elapsed(),
                map.counts().len()
            );
        }
    }

    #[test]
    fn out_of_the_nether_a_block_spans_eight_by_eight_exits() {
        let game = game();
        let border = Border::default();
        let (exits, y) = block_exits(&game, N, [-3, 70, 5], &border).unwrap();
        assert_eq!(exits, [[-24, -17], [40, 47]]);
        assert_eq!(y, 70);
        let (exits, _) = block_exits(&game, O, [-3, 70, 15], &border).unwrap();
        assert_eq!(exits, [[-1, -1], [1, 1]]);
    }

    #[test]
    fn a_block_between_two_portals_is_mixed() {
        let game = game();
        let border = Border::default();
        // Out of the Nether, block 0, 0 aims at x 0 to 7. Overworld portals at x -10 and 17,
        // in one column of z: x 0 to 3 are nearer the west one, 4 to 7 the east.
        let portals = [
            Portal::new(O, Axis::Z, [-10, 70, 0], 2, 3).unwrap(),
            Portal::new(O, Axis::Z, [17, 70, 0], 2, 3).unwrap(),
        ];
        let area = MapArea {
            min: [0, 0],
            size: [1, 1],
            scale: 1,
            y: 70,
        };
        let map = game.portal_link_map(N, &area, &portals, &border).unwrap();
        let pixel = map.at([0, 0]);
        assert!(
            matches!(pixel, PixelLink::Mixed(links) if links.len() == 2),
            "{pixel:?}"
        );
        assert!(pixel.may_reach(0) && pixel.may_reach(1));
        assert_eq!(pixel.portal(), None);
        assert_eq!(map.counts().len(), 1);
    }
}
