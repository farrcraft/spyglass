//! End city layout (`EndCityStructure` and `EndCityPieces`): a house tower, then towers, bridges,
//! fat towers and at most one ship, grown recursively from templates. See
//! `docs/specs/structure-placement.md#end-cities`.
//!
//! Each section generator adds its pieces to a list of its own and grows children into it; a
//! child's subtree joins the list only if none of its pieces' first collision in that list is
//! with a piece of another generation. Generations are random stamps (`genDepth`), so a subtree
//! may overlap its own parent's generation but not a sibling subtree's. Rejected subtrees keep
//! the draws they made. Nothing reads the world, so the start's height only moves the layout.

use std::sync::Arc;

use spyglass_rng::{LegacyRandomSource, RandomSource, worldgen};

use crate::assembly::Layout;
use crate::jigsaw::{BoundingBox, Rotation, Template};

/// The templates `EndCityPieces` names, `minecraft:end_city/<name>`, in the order
/// [`Part`] indexes them.
pub(crate) const TEMPLATES: [&str; 19] = [
    "end_city/base_floor",
    "end_city/base_roof",
    "end_city/second_floor_1",
    "end_city/second_floor_2",
    "end_city/second_roof",
    "end_city/third_floor_1",
    "end_city/third_floor_2",
    "end_city/third_roof",
    "end_city/tower_base",
    "end_city/tower_piece",
    "end_city/tower_top",
    "end_city/bridge_end",
    "end_city/bridge_piece",
    "end_city/bridge_steep_stairs",
    "end_city/bridge_gentle_stairs",
    "end_city/fat_tower_base",
    "end_city/fat_tower_middle",
    "end_city/fat_tower_top",
    "end_city/ship",
];

/// A template of [`TEMPLATES`], by its index there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    BaseFloor,
    BaseRoof,
    SecondFloor1,
    SecondFloor2,
    SecondRoof,
    ThirdFloor1,
    ThirdFloor2,
    ThirdRoof,
    TowerBase,
    TowerPiece,
    TowerTop,
    BridgeEnd,
    BridgePiece,
    BridgeSteepStairs,
    BridgeGentleStairs,
    FatTowerBase,
    FatTowerMiddle,
    FatTowerTop,
    Ship,
}

/// `EndCityPieces.MAX_GEN_DEPTH`: how deep sections nest.
const MAX_GEN_DEPTH: i32 = 8;

/// `TOWER_BRIDGES`: where a tower's bridges may leave from, each turned from the tower.
const TOWER_BRIDGES: [(Rotation, [i32; 3]); 4] = [
    (Rotation::None, [1, -1, 0]),
    (Rotation::Clockwise90, [6, -1, 1]),
    (Rotation::Counterclockwise90, [0, -1, 5]),
    (Rotation::Clockwise180, [5, -1, 6]),
];

/// `FAT_TOWER_BRIDGES`, likewise for a fat tower.
const FAT_TOWER_BRIDGES: [(Rotation, [i32; 3]); 4] = [
    (Rotation::None, [4, -1, 0]),
    (Rotation::Clockwise90, [12, -1, 4]),
    (Rotation::Counterclockwise90, [0, -1, 8]),
    (Rotation::Clockwise180, [8, -1, 12]),
];

/// `EndCityPieces.SectionGenerator`s.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    /// `HOUSE_TOWER_GENERATOR`: a house at a bridge's end, maybe with a tower on top.
    HouseTower,
    /// `TOWER_GENERATOR`: a tower, with bridges or a fat tower.
    Tower,
    /// `TOWER_BRIDGE_GENERATOR`: a bridge, ending in a house tower or the ship.
    TowerBridge,
    /// `FAT_TOWER_GENERATOR`: a wide tower, with bridges.
    FatTower,
}

/// One of an end city's pieces (`EndCityPiece`).
#[derive(Debug, Clone, PartialEq)]
pub struct CityPiece {
    /// Its template.
    pub template: Arc<Template>,
    /// Where its corner goes (`templatePosition`).
    pub pos: [i32; 3],
    /// Its rotation, about the corner; none is mirrored.
    pub rotation: Rotation,
}

impl CityPiece {
    /// Its box.
    pub fn bounds(&self) -> BoundingBox {
        self.template.bounding_box(self.pos, self.rotation)
    }
}

/// One start's generation: every piece made, accepted or not, and the generator state.
struct City<'a> {
    templates: &'a [Arc<Template>],
    random: LegacyRandomSource,
    pieces: Vec<CityPiece>,
    boxes: Vec<BoundingBox>,
    /// Each piece's `genDepth`, which collisions compare: 0, −1 on some bridge pieces, or the
    /// stamp of the last `recursiveChildren` that took it.
    gen_depth: Vec<i32>,
    /// `TOWER_BRIDGE_GENERATOR.shipCreated`: whether any bridge has tried the ship, kept or not.
    /// A static of the game's, reset for each start.
    ship_created: bool,
}

impl City<'_> {
    /// A new piece of template `part` at `pos`.
    fn piece(&mut self, part: Part, pos: [i32; 3], rotation: Rotation) -> usize {
        let template = self.templates[part as usize].clone();
        self.boxes.push(template.bounding_box(pos, rotation));
        self.pieces.push(CityPiece {
            template,
            pos,
            rotation,
        });
        self.gen_depth.push(0);
        self.pieces.len() - 1
    }

    /// `addPiece`: a piece at `offset` from `parent`'s corner, turned by the parent's rotation.
    fn add_piece(
        &mut self,
        parent: usize,
        offset: [i32; 3],
        part: Part,
        rotation: Rotation,
    ) -> usize {
        let from = &self.pieces[parent];
        let d = from.rotation.transform(offset);
        let pos = [0, 1, 2].map(|i| from.pos[i].wrapping_add(d[i]));
        self.piece(part, pos, rotation)
    }

    /// `addHelper(list, addPiece(...))`.
    fn add(
        &mut self,
        list: &mut Vec<usize>,
        parent: usize,
        offset: [i32; 3],
        part: Part,
        rotation: Rotation,
    ) -> usize {
        let p = self.add_piece(parent, offset, part, rotation);
        list.push(p);
        p
    }

    /// `recursiveChildren`: `section`'s pieces from `parent`, added to `list` unless one of them
    /// first collides with a piece there of another generation than `parent`'s.
    fn recursive_children(
        &mut self,
        section: Section,
        depth: i32,
        parent: usize,
        offset: [i32; 3],
        list: &mut Vec<usize>,
    ) -> bool {
        if depth > MAX_GEN_DEPTH {
            return false;
        }
        let mut children = Vec::new();
        if !self.generate(section, depth, parent, offset, &mut children) {
            return false;
        }
        let stamp = self.random.next_int();
        for &child in &children {
            self.gen_depth[child] = stamp;
            let hit = list
                .iter()
                .find(|&&p| self.boxes[p].intersects(&self.boxes[child]));
            if let Some(&hit) = hit
                && self.gen_depth[hit] != self.gen_depth[parent]
            {
                return false;
            }
        }
        list.extend(children);
        true
    }

    /// `SectionGenerator.generate`.
    fn generate(
        &mut self,
        section: Section,
        depth: i32,
        parent: usize,
        offset: [i32; 3],
        list: &mut Vec<usize>,
    ) -> bool {
        let rotation = self.pieces[parent].rotation;
        match section {
            Section::HouseTower => {
                if depth > MAX_GEN_DEPTH {
                    return false;
                }
                let mut p = self.add(list, parent, offset, Part::BaseFloor, rotation);
                match self.random.next_int_bounded(3) {
                    0 => {
                        self.add(list, p, [-1, 4, -1], Part::BaseRoof, rotation);
                    }
                    1 => {
                        p = self.add(list, p, [-1, 0, -1], Part::SecondFloor2, rotation);
                        p = self.add(list, p, [-1, 8, -1], Part::SecondRoof, rotation);
                        self.recursive_children(Section::Tower, depth + 1, p, [0; 3], list);
                    }
                    _ => {
                        p = self.add(list, p, [-1, 0, -1], Part::SecondFloor2, rotation);
                        p = self.add(list, p, [-1, 4, -1], Part::ThirdFloor2, rotation);
                        p = self.add(list, p, [-1, 8, -1], Part::ThirdRoof, rotation);
                        self.recursive_children(Section::Tower, depth + 1, p, [0; 3], list);
                    }
                }
                true
            }
            Section::Tower => {
                let ox = 3 + self.random.next_int_bounded(2);
                let oz = 3 + self.random.next_int_bounded(2);
                let mut p = self.add(list, parent, [ox, -3, oz], Part::TowerBase, rotation);
                p = self.add(list, p, [0, 7, 0], Part::TowerPiece, rotation);
                let mut bridge_at = (self.random.next_int_bounded(3) == 0).then_some(p);
                let floors = 1 + self.random.next_int_bounded(3);
                for i in 0..floors {
                    p = self.add(list, p, [0, 4, 0], Part::TowerPiece, rotation);
                    if i < floors - 1 && self.random.next_boolean() {
                        bridge_at = Some(p);
                    }
                }
                if let Some(from) = bridge_at {
                    for (turn, at) in TOWER_BRIDGES {
                        if self.random.next_boolean() {
                            let end =
                                self.add(list, from, at, Part::BridgeEnd, rotation.rotated(turn));
                            self.recursive_children(
                                Section::TowerBridge,
                                depth + 1,
                                end,
                                [0; 3],
                                list,
                            );
                        }
                    }
                } else if depth != 7 {
                    return self.recursive_children(Section::FatTower, depth + 1, p, [0; 3], list);
                }
                self.add(list, p, [-1, 4, -1], Part::TowerTop, rotation);
                true
            }
            Section::TowerBridge => {
                let segments = self.random.next_int_bounded(4) + 1;
                let mut p = self.add(list, parent, [0, 0, -4], Part::BridgePiece, rotation);
                self.gen_depth[p] = -1;
                let mut h = 0;
                for _ in 0..segments {
                    if self.random.next_boolean() {
                        p = self.add(list, p, [0, h, -4], Part::BridgePiece, rotation);
                        h = 0;
                    } else {
                        p = if self.random.next_boolean() {
                            self.add(list, p, [0, h, -4], Part::BridgeSteepStairs, rotation)
                        } else {
                            self.add(list, p, [0, h, -8], Part::BridgeGentleStairs, rotation)
                        };
                        h = 4;
                    }
                }
                if !self.ship_created && self.random.next_int_bounded(10 - depth) == 0 {
                    let sx = -8 + self.random.next_int_bounded(8);
                    let sz = -70 + self.random.next_int_bounded(10);
                    self.add(list, p, [sx, h, sz], Part::Ship, rotation);
                    self.ship_created = true;
                } else if !self.recursive_children(
                    Section::HouseTower,
                    depth + 1,
                    p,
                    [-3, h + 1, -11],
                    list,
                ) {
                    return false;
                }
                let turned = rotation.rotated(Rotation::Clockwise180);
                p = self.add(list, p, [4, h, 0], Part::BridgeEnd, turned);
                self.gen_depth[p] = -1;
                true
            }
            Section::FatTower => {
                let mut p = self.add(list, parent, [-3, 4, -3], Part::FatTowerBase, rotation);
                p = self.add(list, p, [0, 4, 0], Part::FatTowerMiddle, rotation);
                let mut i = 0;
                while i < 2 && self.random.next_int_bounded(3) != 0 {
                    p = self.add(list, p, [0, 8, 0], Part::FatTowerMiddle, rotation);
                    for (turn, at) in FAT_TOWER_BRIDGES {
                        if self.random.next_boolean() {
                            let end =
                                self.add(list, p, at, Part::BridgeEnd, rotation.rotated(turn));
                            self.recursive_children(
                                Section::TowerBridge,
                                depth + 1,
                                end,
                                [0; 3],
                                list,
                            );
                        }
                    }
                    i += 1;
                }
                self.add(list, p, [-2, 8, -2], Part::FatTowerTop, rotation);
                true
            }
        }
    }
}

/// An end city's pieces for its start at `at` (the stub: the chunk's (7, 7) and the lowest
/// corner's height) in `chunk`, for world `seed`, in the game's order (`startHouseTower`).
/// `templates` are [`TEMPLATES`]' in order.
pub(crate) fn pieces(
    templates: &[Arc<Template>],
    seed: i64,
    chunk: [i32; 2],
    at: [i32; 3],
) -> Vec<CityPiece> {
    let mut random = worldgen::large_feature(seed, chunk[0], chunk[1]);
    // findGenerationPoint's draw; its corners gave `at`'s height.
    let rotation = Rotation::random(&mut random);
    let mut city = City {
        templates,
        random,
        pieces: Vec::new(),
        boxes: Vec::new(),
        gen_depth: Vec::new(),
        ship_created: false,
    };
    let mut list = Vec::new();
    let mut p = city.piece(Part::BaseFloor, at, rotation);
    list.push(p);
    p = city.add(&mut list, p, [-1, 0, -1], Part::SecondFloor1, rotation);
    p = city.add(&mut list, p, [-1, 4, -1], Part::ThirdFloor1, rotation);
    p = city.add(&mut list, p, [-1, 8, -1], Part::ThirdRoof, rotation);
    city.recursive_children(Section::Tower, 1, p, [0; 3], &mut list);
    list.into_iter().map(|i| city.pieces[i].clone()).collect()
}

/// [`pieces`] as a [`Layout`]: end cities don't adapt terrain, so the bounds are the pieces'.
pub(crate) fn layout(pieces: &[CityPiece]) -> Option<Layout> {
    let boxes: Vec<BoundingBox> = pieces.iter().map(CityPiece::bounds).collect();
    let bounds = boxes.iter().copied().reduce(BoundingBox::encapsulate)?;
    Some(Layout {
        pieces: boxes,
        placed: Vec::new(),
        bounds,
    })
}
