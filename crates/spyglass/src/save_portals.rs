//! The Nether portals a save already has, from its points of interest, and where each one
//! leads (`docs/specs/portals.md#a-saves-portals`).
//!
//! A save keeps a record for every portal block, not the portals. Records that touch are one
//! portal, cut into the rectangles [`portal_link`](crate::portal_link) searches: a portal the
//! game lit is one rectangle, and blocks placed some other way may take several.

use std::collections::{HashSet, VecDeque};
use std::fmt;

use spyglass_world::poi::{self, PoiIndex};
use spyglass_world::{BorderRead, region};

use crate::portals::{add_outcome, sort_outcomes};
use crate::save::SaveReport;
use crate::{
    Axis, Border, DEFAULT_ABSOLUTE_MAX, Dimension, Game, LinkOutcome, MAX_PORTAL_SIDE, Portal,
    Result, portal_destination,
};

/// The schema of `ForcePoiRebuild` (1.16): chunks last saved before it have their points of
/// interest rebuilt from their blocks when the game loads them.
const FORCE_REBUILD: i32 = 2218;

/// A portal a save holds: portal records that touch, along x, y or z.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SavePortal {
    /// The dimension it's in.
    pub dimension: Dimension,
    /// Its blocks, as rectangles of portal blocks: one for a portal the game lit, more for
    /// blocks of another shape. The lowest first.
    pub pieces: Vec<Portal>,
    /// How many blocks it has.
    pub blocks: usize,
}

impl SavePortal {
    /// The portals among `records`, the positions of portal blocks in `dimension`: each set of
    /// records that touch is one, in order of its lowest block (y, then z, then x).
    pub fn group(dimension: Dimension, records: &[[i32; 3]]) -> Vec<SavePortal> {
        let mut remaining: HashSet<[i32; 3]> = records.iter().copied().collect();
        // Lowest first, so each group starts from its lowest block.
        let mut order: Vec<[i32; 3]> = remaining.iter().copied().collect();
        order.sort_by_key(|&[x, y, z]| (y, z, x));
        let mut portals = Vec::new();
        for start in order {
            if !remaining.remove(&start) {
                continue;
            }
            let mut group = vec![start];
            let mut queue = VecDeque::from([start]);
            while let Some(block) = queue.pop_front() {
                for next in neighbours(block) {
                    if remaining.remove(&next) {
                        group.push(next);
                        queue.push_back(next);
                    }
                }
            }
            portals.push(SavePortal {
                dimension,
                blocks: group.len(),
                pieces: rectangles(dimension, group),
            });
        }
        portals
    }

    /// The least and greatest x, y and z of its blocks: `[[x_lo, x_hi], [y_lo, y_hi],
    /// [z_lo, z_hi]]`.
    pub fn bounds(&self) -> [[i32; 2]; 3] {
        let mut bounds = [[i32::MAX, i32::MIN]; 3];
        for piece in &self.pieces {
            for (b, [lo, hi]) in bounds.iter_mut().zip(piece.extent()) {
                // A piece's extent fits in i32: Portal::new checks it.
                b[0] = b[0].min(lo as i32);
                b[1] = b[1].max(hi as i32);
            }
        }
        bounds
    }

    /// Its lowest block, by y, then z, then x: the first piece's corner.
    pub fn corner(&self) -> [i32; 3] {
        self.pieces.first().map_or([0; 3], |p| p.corner)
    }
}

/// The six blocks that share a face with `block`, those that don't overflow.
fn neighbours([x, y, z]: [i32; 3]) -> impl Iterator<Item = [i32; 3]> {
    [
        (x.checked_sub(1), y.into(), z.into()),
        (x.checked_add(1), y.into(), z.into()),
        (x.into(), y.checked_sub(1), z.into()),
        (x.into(), y.checked_add(1), z.into()),
        (x.into(), y.into(), z.checked_sub(1)),
        (x.into(), y.into(), z.checked_add(1)),
    ]
    .into_iter()
    .filter_map(|(x, y, z)| Some([x?, y?, z?]))
}

/// `blocks`, one group's, cut into flat rectangles. From the lowest block left, a rectangle
/// runs along whichever of x and z has the longer row (x if they're equal), then up while the
/// whole row continues, each side at most [`MAX_PORTAL_SIDE`]. A portal the game lit starts
/// at its lowest corner, so it's one rectangle.
fn rectangles(dimension: Dimension, mut blocks: Vec<[i32; 3]>) -> Vec<Portal> {
    blocks.sort_by_key(|&[x, y, z]| (y, z, x));
    let mut remaining: HashSet<[i32; 3]> = blocks.iter().copied().collect();
    let step = |[x, y, z]: [i32; 3], [dx, dy, dz]: [i32; 3], n: i32| {
        Some([
            x.checked_add(dx * n)?,
            y.checked_add(dy * n)?,
            z.checked_add(dz * n)?,
        ])
    };
    let run = |remaining: &HashSet<[i32; 3]>, from: [i32; 3], dir: [i32; 3]| {
        (0..MAX_PORTAL_SIDE)
            .take_while(|&n| step(from, dir, n).is_some_and(|b| remaining.contains(&b)))
            .count() as i32
    };
    let mut pieces = Vec::new();
    for corner in blocks {
        if !remaining.contains(&corner) {
            continue;
        }
        let (along_x, along_z) = (
            run(&remaining, corner, [1, 0, 0]),
            run(&remaining, corner, [0, 0, 1]),
        );
        let (axis, dir, width) = if along_z > along_x {
            (Axis::Z, [0, 0, 1], along_z)
        } else {
            (Axis::X, [1, 0, 0], along_x)
        };
        let row_at = |remaining: &HashSet<[i32; 3]>, up: i32| {
            step(corner, [0, 1, 0], up).is_some_and(|start| run(remaining, start, dir) >= width)
        };
        let height = (1..MAX_PORTAL_SIDE)
            .take_while(|&up| row_at(&remaining, up))
            .count() as i32
            + 1;
        for up in 0..height {
            for along in 0..width {
                if let Some(b) = step(corner, [0, 1, 0], up).and_then(|r| step(r, dir, along)) {
                    remaining.remove(&b);
                }
            }
        }
        // Only a rectangle reaching past i32 fails, and every block in it exists, so none
        // does.
        if let Ok(piece) = Portal::new(dimension, axis, corner, width, height) {
            pieces.push(piece);
        }
    }
    pieces
}

/// A save's portals in every dimension, and the borders their links are clamped to.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct SavePortals {
    /// Every portal, the Overworld's first, then the Nether's and the End's, each by its
    /// lowest block.
    pub portals: Vec<SavePortal>,
    /// Each dimension's border, by [`Dimension::index`].
    pub borders: [Border; 3],
    /// What may make the portals or their links differ from the game's.
    pub notes: Vec<PortalNote>,
}

/// Why a save's portals, or their links, may differ from the game's.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum PortalNote {
    /// Point-of-interest chunks or region files that couldn't be read: the game rebuilds them
    /// from the blocks, and spyglass has no portals there.
    UnreadablePoi {
        /// The dimension.
        dimension: Dimension,
        /// Chunks a region file lists that couldn't be read.
        chunks: usize,
        /// Region files that couldn't be read.
        files: usize,
    },
    /// Chunks last saved before 1.16, whose points of interest the game rebuilds from their
    /// blocks: portals in them may be missing.
    OldChunks {
        /// The dimension.
        dimension: Dimension,
        /// How many.
        chunks: usize,
    },
    /// Sections whose records hold portals but aren't marked valid: the game rebuilds them
    /// from the blocks, and spyglass uses the records as saved.
    InvalidSections {
        /// The dimension.
        dimension: Dimension,
        /// How many.
        sections: usize,
    },
    /// A border still moving: links are clamped to the size it's moving to.
    MovingBorder {
        /// The dimension.
        dimension: Dimension,
        /// The size it ends at.
        size: f64,
    },
    /// A border file that doesn't decode, which the game replaces with the default.
    UnreadableBorder {
        /// The dimension.
        dimension: Dimension,
        /// Why.
        reason: String,
    },
}

impl PortalNote {
    /// A short name for it, e.g. `old_chunks`.
    pub fn kind(&self) -> &'static str {
        match self {
            PortalNote::UnreadablePoi { .. } => "unreadable_poi",
            PortalNote::OldChunks { .. } => "old_chunks",
            PortalNote::InvalidSections { .. } => "invalid_sections",
            PortalNote::MovingBorder { .. } => "moving_border",
            PortalNote::UnreadableBorder { .. } => "unreadable_border",
        }
    }

    /// The dimension it's about.
    pub fn dimension(&self) -> Dimension {
        match self {
            PortalNote::UnreadablePoi { dimension, .. }
            | PortalNote::OldChunks { dimension, .. }
            | PortalNote::InvalidSections { dimension, .. }
            | PortalNote::MovingBorder { dimension, .. }
            | PortalNote::UnreadableBorder { dimension, .. } => *dimension,
        }
    }
}

impl fmt::Display for PortalNote {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PortalNote::UnreadablePoi {
                dimension,
                chunks,
                files,
            } => write!(
                f,
                "{chunks} point-of-interest chunks and {files} region files in {} couldn't be \
                 read, so portals there are missing",
                dimension.name()
            ),
            PortalNote::OldChunks { dimension, chunks } => write!(
                f,
                "{chunks} chunks in {} were last saved before 1.16; the game finds their \
                 portals from the blocks, so some may be missing",
                dimension.name()
            ),
            PortalNote::InvalidSections {
                dimension,
                sections,
            } => write!(
                f,
                "{sections} sections in {} hold portal records not marked valid; the game \
                 rebuilds them from the blocks, and spyglass uses the records",
                dimension.name()
            ),
            PortalNote::MovingBorder { dimension, size } => write!(
                f,
                "the border in {} is moving; links use the size it ends at, {size}",
                dimension.name()
            ),
            PortalNote::UnreadableBorder { dimension, reason } => write!(
                f,
                "the border in {} couldn't be read ({reason}), so the game uses the default",
                dimension.name()
            ),
        }
    }
}

impl SavePortals {
    /// `portals` grouped from each dimension's records, with `borders`, by
    /// [`Dimension::index`].
    pub fn from_records(records: [Vec<[i32; 3]>; 3], borders: [Border; 3]) -> SavePortals {
        let portals = Dimension::ALL
            .into_iter()
            .zip(records)
            .flat_map(|(dimension, records)| SavePortal::group(dimension, &records))
            .collect();
        SavePortals {
            portals,
            borders,
            notes: Vec::new(),
        }
    }

    /// Every portal's rectangles, as the search takes them ([`portal_link`](crate::portal_link)),
    /// and the portal each is of, by index in [`portals`](Self::portals).
    pub fn pieces(&self) -> (Vec<Portal>, Vec<usize>) {
        let mut pieces = Vec::new();
        let mut owners = Vec::new();
        for (index, portal) in self.portals.iter().enumerate() {
            pieces.extend(&portal.pieces);
            owners.extend(std::iter::repeat_n(index, portal.pieces.len()));
        }
        (pieces, owners)
    }
}

impl SaveReport {
    /// The save's portals, from each dimension's points of interest, and its borders
    /// (`docs/specs/portals.md#a-saves-portals`). `game`, the save's own
    /// ([`open_game`](Self::open_game)), gives each dimension's height. A dimension's chunks
    /// are read for their versions unless [`Scan::Chunks`](crate::Scan::Chunks) already did,
    /// to say whether any are old enough for the game to find portals the records don't list.
    pub fn portals(&self, game: &Game) -> Result<SavePortals> {
        let save = spyglass_world::Save::read(&self.save.dir)?;
        let mut records: [Vec<[i32; 3]>; 3] = Default::default();
        let mut borders = [Border::default(); 3];
        let mut notes = Vec::new();
        for dimension in Dimension::ALL {
            let i = dimension.index();
            let Ok([min_y, height]) = game.build_height(dimension) else {
                continue;
            };
            let sections = (min_y >> 4)..=((min_y + height - 1) >> 4);
            let folders = &self.dimensions[i];
            let index = poi::read(&folders.region.with_file_name("poi"), sections);
            records[i] = index.portals().map(|(pos, _)| pos).collect();
            notes.extend(poi_notes(dimension, &index));
            let old = match &folders.index {
                Some(scanned) => scanned.versions().older_than(FORCE_REBUILD),
                None => region::chunk_versions(&folders.region)
                    .map(|v| v.older_than(FORCE_REBUILD))
                    .unwrap_or(0),
            };
            if old > 0 {
                notes.push(PortalNote::OldChunks {
                    dimension,
                    chunks: old,
                });
            }
            let read = save.world_border(dimension.id())?;
            let settings = read.settings();
            borders[i] = Border {
                center: settings.center,
                size: settings.final_size(),
                absolute_max: DEFAULT_ABSOLUTE_MAX,
            };
            if settings.is_moving() {
                notes.push(PortalNote::MovingBorder {
                    dimension,
                    size: settings.final_size(),
                });
            }
            if let BorderRead::Unreadable(reason) = read {
                notes.push(PortalNote::UnreadableBorder { dimension, reason });
            }
        }
        let mut portals = SavePortals::from_records(records, borders);
        portals.notes = notes;
        Ok(portals)
    }
}

/// What one dimension's points of interest leave uncertain.
fn poi_notes(dimension: Dimension, index: &PoiIndex) -> Vec<PortalNote> {
    let mut notes = Vec::new();
    if index.unreadable_chunks > 0 || index.unreadable_files > 0 {
        notes.push(PortalNote::UnreadablePoi {
            dimension,
            chunks: index.unreadable_chunks,
            files: index.unreadable_files,
        });
    }
    // Chunks before 1.16 have every section invalid, and OldChunks counts those.
    let invalid = index
        .chunks
        .values()
        .filter(|c| c.data_version.is_some_and(|v| v >= FORCE_REBUILD))
        .flat_map(|c| &c.sections)
        .filter(|s| !s.valid && !s.portals.is_empty())
        .count();
    if invalid > 0 {
        notes.push(PortalNote::InvalidSections {
            dimension,
            sections: invalid,
        });
    }
    notes
}

/// Where one save portal leads, from every place a standing player can be in it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SavePortalLink {
    /// The portal, by index in [`SavePortals::portals`].
    pub portal: usize,
    /// The dimension it leads to.
    pub dimension: Dimension,
    /// Where its exits lead, most exits first, by save portal. Empty if no piece of it is tall
    /// enough to stand in.
    pub outcomes: Vec<LinkOutcome>,
    /// What's wrong with its link, if anything.
    pub flags: Vec<PortalFlag>,
}

/// Something about a save portal's link a player would want to know.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortalFlag {
    /// It leads to one portal, which doesn't lead back to it from anywhere.
    Mislinked {
        /// The portal it leads to.
        to: usize,
        /// Where that one leads instead: the portals it lands at, none if it would build one.
        back: Vec<usize>,
    },
    /// Different places in it lead to different portals, or some to none.
    Split,
    /// No portal is in range of any of its exits, so the game builds one.
    Unlinked,
    /// Some exits are tied between portals the game picks by an order spyglass can't know.
    Tied,
    /// No part of it is tall enough for a standing player.
    TooLow,
}

impl PortalFlag {
    /// A short name for it, e.g. `mislinked`.
    pub fn kind(&self) -> &'static str {
        match self {
            PortalFlag::Mislinked { .. } => "mislinked",
            PortalFlag::Split => "split",
            PortalFlag::Unlinked => "unlinked",
            PortalFlag::Tied => "tied",
            PortalFlag::TooLow => "too-low",
        }
    }
}

impl Game {
    /// Where each of a save's portals leads, both ways (`docs/specs/portals.md#a-saves-portals`),
    /// with its flags. One per portal, in order. A portal whose dimension or destination the
    /// game lacks is left out.
    pub fn save_portal_links(&self, save: &SavePortals) -> Result<Vec<SavePortalLink>> {
        let (pieces, owners) = save.pieces();
        let mut links = Vec::new();
        for (index, portal) in save.portals.iter().enumerate() {
            let to = portal_destination(portal.dimension);
            if self.coordinate_scale(portal.dimension).is_err()
                || self.coordinate_scale(to).is_err()
            {
                continue;
            }
            let border = &save.borders[to.index()];
            let mut outcomes: Vec<LinkOutcome> = Vec::new();
            for piece in &portal.pieces {
                let Some(piece_links) = self.portal_links(piece, &pieces, border)? else {
                    continue;
                };
                for outcome in piece_links.outcomes_by(|i| owners[i]) {
                    add_outcome(&mut outcomes, outcome.portals, outcome.tied, outcome.exits);
                }
            }
            sort_outcomes(&mut outcomes);
            links.push(SavePortalLink {
                portal: index,
                dimension: to,
                outcomes,
                flags: Vec::new(),
            });
        }
        flag(&mut links, save);
        Ok(links)
    }
}

/// Each link's flags, from the links of the portals it leads to.
fn flag(links: &mut [SavePortalLink], save: &SavePortals) {
    // The single portal each one leads to, if that's all it does.
    let only: Vec<Option<usize>> = links
        .iter()
        .map(|l| match &l.outcomes[..] {
            [one] if !one.tied && one.portals.len() == 1 => Some(one.portals[0]),
            _ => None,
        })
        .collect();
    let by_portal: std::collections::HashMap<usize, usize> = links
        .iter()
        .enumerate()
        .map(|(i, l)| (l.portal, i))
        .collect();
    let flags: Vec<Vec<PortalFlag>> = links
        .iter()
        .zip(&only)
        .map(|(link, &only)| {
            let mut flags = Vec::new();
            match &link.outcomes[..] {
                [] => flags.push(PortalFlag::TooLow),
                [one] if one.portals.is_empty() => flags.push(PortalFlag::Unlinked),
                [_] => {}
                _ => flags.push(PortalFlag::Split),
            }
            if link.outcomes.iter().any(|o| o.tied) {
                flags.push(PortalFlag::Tied);
            }
            // Only a portal whose trips come back to this dimension can lead back: from the
            // End, the Nether's portals lead to the Overworld.
            let from = save.portals[link.portal].dimension;
            if let Some(to) = only
                && portal_destination(save.portals[to].dimension) == from
                && let Some(&back) = by_portal.get(&to)
                && !links[back]
                    .outcomes
                    .iter()
                    .any(|o| o.portals.contains(&link.portal))
            {
                let mut back: Vec<usize> = links[back]
                    .outcomes
                    .iter()
                    .flat_map(|o| o.portals.iter().copied())
                    .collect();
                back.sort_unstable();
                back.dedup();
                flags.push(PortalFlag::Mislinked { to, back });
            }
            flags
        })
        .collect();
    for (link, flags) in links.iter_mut().zip(flags) {
        link.flags = flags;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const O: Dimension = Dimension::Overworld;
    const N: Dimension = Dimension::Nether;

    /// The blocks of a `width` × `height` portal from `corner`.
    fn blocks(axis: Axis, [x, y, z]: [i32; 3], width: i32, height: i32) -> Vec<[i32; 3]> {
        let mut out = Vec::new();
        for up in 0..height {
            for along in 0..width {
                out.push(match axis {
                    Axis::X => [x + along, y + up, z],
                    Axis::Z => [x, y + up, z + along],
                });
            }
        }
        out
    }

    #[test]
    fn a_lit_portal_is_one_rectangle() {
        let mut records = blocks(Axis::Z, [5, 64, -3], 3, 4);
        records.extend(blocks(Axis::X, [100, 70, 100], 2, 3));
        records.reverse();
        let portals = SavePortal::group(O, &records);
        assert_eq!(portals.len(), 2);
        assert_eq!(
            portals[0].pieces,
            vec![Portal::new(O, Axis::Z, [5, 64, -3], 3, 4).unwrap()]
        );
        assert_eq!(portals[0].blocks, 12);
        assert_eq!(portals[0].bounds(), [[5, 5], [64, 67], [-3, -1]]);
        assert_eq!(
            portals[1].pieces,
            vec![Portal::new(O, Axis::X, [100, 70, 100], 2, 3).unwrap()]
        );
    }

    #[test]
    fn other_shapes_are_cut_into_rectangles() {
        // An L: a 3 × 1 row along x with a column of 2 above its west end.
        let mut records = blocks(Axis::X, [0, 0, 0], 3, 1);
        records.extend([[0, 1, 0], [0, 2, 0]]);
        // A corner joining the two axes.
        records.extend(blocks(Axis::Z, [0, 0, 1], 2, 1));
        let portals = SavePortal::group(N, &records);
        assert_eq!(portals.len(), 1);
        let portal = &portals[0];
        assert_eq!(portal.blocks, 7);
        let covered: usize = portal
            .pieces
            .iter()
            .map(|p| (p.width * p.height) as usize)
            .sum();
        assert_eq!(covered, 7);
        assert!(
            records
                .iter()
                .all(|&b| portal.pieces.iter().any(|p| p.contains(b)))
        );
        // A wall wider than the game allows is cut at 21.
        let wide = SavePortal::group(O, &blocks(Axis::X, [0, 0, 0], 30, 2));
        assert_eq!(wide[0].pieces.len(), 2);
        assert_eq!(wide[0].pieces[0].width, 21);
    }

    /// The fixture Overworld and Nether, with the Nether's scale of 8.
    fn game() -> Game {
        let bundle = spyglass_oracle::fixtures::overworld_and_nether();
        Game::open(bundle.path()).unwrap()
    }

    fn save(overworld: Vec<Vec<[i32; 3]>>, nether: Vec<Vec<[i32; 3]>>) -> SavePortals {
        SavePortals::from_records(
            [overworld.concat(), nether.concat(), Vec::new()],
            [Border::default(); 3],
        )
    }

    #[test]
    fn a_pair_links_and_a_third_portal_mislinks() {
        let game = game();
        // A at 800, 64, 800 leads to B at 100, 64, 100, and B back to A.
        let a = blocks(Axis::X, [800, 64, 800], 2, 3);
        let b = blocks(Axis::X, [100, 64, 100], 2, 3);
        let paired = save(vec![a.clone()], vec![b.clone()]);
        let links = game.save_portal_links(&paired).unwrap();
        assert_eq!(links.len(), 2);
        for link in &links {
            assert_eq!(link.flags, vec![], "{link:?}");
            assert_eq!(link.outcomes.len(), 1);
            assert_eq!(link.outcomes[0].portals, vec![1 - link.portal]);
        }
        // C, 7 blocks from B in the Nether: A's trips still land at B, the nearer, but C's
        // lead to A, the only Overworld portal in range. So C's link isn't returned.
        let c = blocks(Axis::X, [99, 64, 107], 2, 3);
        let mislinked = save(vec![a], vec![b, c]);
        let links = game.save_portal_links(&mislinked).unwrap();
        let by = |i: usize| links.iter().find(|l| l.portal == i).unwrap();
        // Portals: 0 is A, then the Nether's by lowest block: 1 is B (z 100), 2 is C (z 107).
        assert_eq!(by(0).outcomes[0].portals, vec![1]);
        assert!(by(2).flags.contains(&PortalFlag::Mislinked {
            to: 0,
            back: vec![1]
        }));
    }

    #[test]
    fn a_portal_with_nothing_in_range_is_unlinked() {
        let game = game();
        let lonely = save(vec![blocks(Axis::Z, [0, 64, 0], 2, 3)], vec![]);
        let links = game.save_portal_links(&lonely).unwrap();
        assert_eq!(links[0].flags, vec![PortalFlag::Unlinked]);
        assert_eq!(links[0].outcomes[0].portals, Vec::<usize>::new());
        assert_eq!(links[0].dimension, N);
        let low = save(vec![blocks(Axis::Z, [0, 64, 0], 2, 1)], vec![]);
        assert_eq!(
            game.save_portal_links(&low).unwrap()[0].flags,
            vec![PortalFlag::TooLow]
        );
    }

    #[test]
    fn a_portal_between_two_is_split() {
        let game = game();
        // Out of the Nether, a 2 × 3 portal at 0, 64, 0 aims at x 0 to 11 across 14 z. Two
        // Overworld portals far apart in x, each nearest one part of the span.
        let source = blocks(Axis::X, [0, 64, 0], 2, 3);
        let west = blocks(Axis::Z, [-40, 64, 4], 2, 3);
        let east = blocks(Axis::Z, [52, 64, 4], 2, 3);
        let split = save(vec![west, east], vec![source]);
        let links = game.save_portal_links(&split).unwrap();
        let nether = links.iter().find(|l| l.portal == 2).unwrap();
        assert!(nether.flags.contains(&PortalFlag::Split), "{nether:?}");
        let reached: std::collections::BTreeSet<usize> = nether
            .outcomes
            .iter()
            .flat_map(|o| o.portals.clone())
            .collect();
        assert_eq!(reached, std::collections::BTreeSet::from([0, 1]));
    }
}
