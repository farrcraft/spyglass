//! Woodland mansion layout (`WoodlandMansionStructure` and `WoodlandMansionPieces`): a floor
//! plan on an 11 × 11 grid of 8-block cells, then template pieces for its walls, roofs,
//! corridors and rooms. See `docs/specs/structure-placement.md#woodland-mansions`.
//!
//! The grid (`MansionGrid`) grows corridors from the entrance in random directions, marks the
//! cells beside them as rooms, fills gaps, and groups each floor's room cells into 1 × 1, 1 × 2
//! and 2 × 2 rooms with a door on a corridor. The placer (`MansionPiecePlacer`) walks the outer
//! walls, puts roofs over the top of each floor, then a template for each corridor cell, indoor
//! wall and room. Only the rooms' doors and templates draw; nothing reads the world, so the
//! start's height only moves the layout.

use std::sync::Arc;

use spyglass_rng::{LegacyRandomSource, RandomSource, worldgen};

use crate::assembly::Layout;
use crate::jigsaw::{BoundingBox, Rotation, Template};

/// The templates `WoodlandMansionPieces` names, `minecraft:woodland_mansion/<name>`, sorted.
pub(crate) const TEMPLATES: [&str; 73] = [
    "woodland_mansion/1x1_a1",
    "woodland_mansion/1x1_a2",
    "woodland_mansion/1x1_a3",
    "woodland_mansion/1x1_a4",
    "woodland_mansion/1x1_a5",
    "woodland_mansion/1x1_as1",
    "woodland_mansion/1x1_as2",
    "woodland_mansion/1x1_as3",
    "woodland_mansion/1x1_as4",
    "woodland_mansion/1x1_b1",
    "woodland_mansion/1x1_b2",
    "woodland_mansion/1x1_b3",
    "woodland_mansion/1x1_b4",
    "woodland_mansion/1x1_b5",
    "woodland_mansion/1x2_a1",
    "woodland_mansion/1x2_a2",
    "woodland_mansion/1x2_a3",
    "woodland_mansion/1x2_a4",
    "woodland_mansion/1x2_a5",
    "woodland_mansion/1x2_a6",
    "woodland_mansion/1x2_a7",
    "woodland_mansion/1x2_a8",
    "woodland_mansion/1x2_a9",
    "woodland_mansion/1x2_b1",
    "woodland_mansion/1x2_b2",
    "woodland_mansion/1x2_b3",
    "woodland_mansion/1x2_b4",
    "woodland_mansion/1x2_b5",
    "woodland_mansion/1x2_c1",
    "woodland_mansion/1x2_c2",
    "woodland_mansion/1x2_c3",
    "woodland_mansion/1x2_c4",
    "woodland_mansion/1x2_c_stairs",
    "woodland_mansion/1x2_d1",
    "woodland_mansion/1x2_d2",
    "woodland_mansion/1x2_d3",
    "woodland_mansion/1x2_d4",
    "woodland_mansion/1x2_d5",
    "woodland_mansion/1x2_d_stairs",
    "woodland_mansion/1x2_s1",
    "woodland_mansion/1x2_s2",
    "woodland_mansion/1x2_se1",
    "woodland_mansion/2x2_a1",
    "woodland_mansion/2x2_a2",
    "woodland_mansion/2x2_a3",
    "woodland_mansion/2x2_a4",
    "woodland_mansion/2x2_b1",
    "woodland_mansion/2x2_b2",
    "woodland_mansion/2x2_b3",
    "woodland_mansion/2x2_b4",
    "woodland_mansion/2x2_b5",
    "woodland_mansion/2x2_s1",
    "woodland_mansion/carpet_east",
    "woodland_mansion/carpet_north",
    "woodland_mansion/carpet_south_1",
    "woodland_mansion/carpet_south_2",
    "woodland_mansion/carpet_west_1",
    "woodland_mansion/carpet_west_2",
    "woodland_mansion/corridor_floor",
    "woodland_mansion/entrance",
    "woodland_mansion/indoors_door_1",
    "woodland_mansion/indoors_door_2",
    "woodland_mansion/indoors_wall_1",
    "woodland_mansion/indoors_wall_2",
    "woodland_mansion/roof",
    "woodland_mansion/roof_corner",
    "woodland_mansion/roof_front",
    "woodland_mansion/roof_inner_corner",
    "woodland_mansion/small_wall",
    "woodland_mansion/small_wall_corner",
    "woodland_mansion/wall_corner",
    "woodland_mansion/wall_flat",
    "woodland_mansion/wall_window",
];

// Grid cells (`MansionGrid`): what the base and third-floor grids hold.
const CLEAR: i32 = 0;
const CORRIDOR: i32 = 1;
const ROOM: i32 = 2;
const START_ROOM: i32 = 3;
const TEST_ROOM: i32 = 4;
const BLOCKED: i32 = 5;

// What a floor's room grid holds: a room's type and id, and flags on its cells.
const ROOM_1X1: i32 = 0x10000;
const ROOM_1X2: i32 = 0x20000;
const ROOM_2X2: i32 = 0x40000;
const ROOM_TYPE_MASK: i32 = 0xF0000;
const ROOM_ID_MASK: i32 = 0xFFFF;
/// The cell the room's template goes from.
const ROOM_ORIGIN_FLAG: i32 = 0x100000;
/// The room has a door onto a corridor.
const ROOM_DOOR_FLAG: i32 = 0x200000;
/// The second floor's 1 × 2 room with the stairs up.
const ROOM_STAIRS_FLAG: i32 = 0x400000;
/// Carpeted like a corridor: the entrance hall and the third floor's stairs.
const ROOM_CORRIDOR_FLAG: i32 = 0x800000;

/// The grids' side.
const SIZE: i32 = 11;

/// A horizontal `Direction`, in `Direction.Plane.HORIZONTAL`'s order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dir {
    North,
    East,
    South,
    West,
}

impl Dir {
    const HORIZONTAL: [Dir; 4] = [Dir::North, Dir::East, Dir::South, Dir::West];

    /// Its step in x and z (the grids' y).
    fn step(self) -> (i32, i32) {
        match self {
            Dir::North => (0, -1),
            Dir::East => (1, 0),
            Dir::South => (0, 1),
            Dir::West => (-1, 0),
        }
    }

    fn clockwise(self) -> Dir {
        Dir::HORIZONTAL[(self as usize + 1) % 4]
    }

    fn counterclockwise(self) -> Dir {
        Dir::HORIZONTAL[(self as usize + 3) % 4]
    }

    fn opposite(self) -> Dir {
        Dir::HORIZONTAL[(self as usize + 2) % 4]
    }

    /// `Direction.from2DDataValue`: south, west, north, east.
    fn from_2d(i: i32) -> Dir {
        [Dir::South, Dir::West, Dir::North, Dir::East][(i % 4).unsigned_abs() as usize]
    }

    /// `Rotation.rotate(direction)`.
    fn turned(self, rotation: Rotation) -> Dir {
        let turns = match rotation {
            Rotation::None => 0,
            Rotation::Clockwise90 => 1,
            Rotation::Clockwise180 => 2,
            Rotation::Counterclockwise90 => 3,
        };
        Dir::HORIZONTAL[(self as usize + turns) % 4]
    }
}

/// A room's door: onto a corridor that way, or none, a secret room (`Direction.UP`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Door {
    To(Dir),
    Up,
}

/// `BlockPos.relative(direction, n)`.
fn rel(p: [i32; 3], d: Dir, n: i32) -> [i32; 3] {
    let (dx, dz) = d.step();
    [
        p[0].wrapping_add(dx.wrapping_mul(n)),
        p[1],
        p[2].wrapping_add(dz.wrapping_mul(n)),
    ]
}

fn above(p: [i32; 3], n: i32) -> [i32; 3] {
    [p[0], p[1].wrapping_add(n), p[2]]
}

/// A template's mirror (`Mirror`), applied before its rotation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mirror {
    /// `NONE`
    None,
    /// `LEFT_RIGHT`: z negated.
    LeftRight,
    /// `FRONT_BACK`: x negated.
    FrontBack,
}

/// One of a mansion's pieces (`WoodlandMansionPiece`).
#[derive(Debug, Clone, PartialEq)]
pub struct MansionPiece {
    /// Its template.
    pub template: Arc<Template>,
    /// Where its corner goes (`templatePosition`).
    pub pos: [i32; 3],
    /// Its rotation, about the corner.
    pub rotation: Rotation,
    /// Its mirror, before the rotation.
    pub mirror: Mirror,
}

impl MansionPiece {
    /// A template position `local` in the world (`StructureTemplate.transform` with pivot 0).
    pub fn world(&self, [x, y, z]: [i32; 3]) -> [i32; 3] {
        let (x, z) = match self.mirror {
            Mirror::None => (x, z),
            Mirror::LeftRight => (x, z.wrapping_neg()),
            Mirror::FrontBack => (x.wrapping_neg(), z),
        };
        let p = self.rotation.transform([x, y, z]);
        [0, 1, 2].map(|i| self.pos[i].wrapping_add(p[i]))
    }

    /// Its box.
    pub fn bounds(&self) -> BoundingBox {
        let far = self.template.size.map(|s| s.wrapping_sub(1));
        let (a, b) = (self.world([0; 3]), self.world(far));
        BoundingBox {
            min: [0, 1, 2].map(|i| a[i].min(b[i])),
            max: [0, 1, 2].map(|i| a[i].max(b[i])),
        }
    }
}

/// `SimpleGrid`: 11 × 11 cells, `outside` beyond them.
#[derive(Clone)]
struct Grid {
    cells: [[i32; SIZE as usize]; SIZE as usize],
    outside: i32,
}

impl Grid {
    fn new(outside: i32) -> Self {
        Grid {
            cells: [[0; SIZE as usize]; SIZE as usize],
            outside,
        }
    }

    fn get(&self, x: i32, y: i32) -> i32 {
        if (0..SIZE).contains(&x) && (0..SIZE).contains(&y) {
            self.cells[x as usize][y as usize]
        } else {
            self.outside
        }
    }

    fn set(&mut self, x: i32, y: i32, v: i32) {
        if (0..SIZE).contains(&x) && (0..SIZE).contains(&y) {
            self.cells[x as usize][y as usize] = v;
        }
    }

    /// Every cell from `(x0, y0)` to `(x1, y1)`, inclusive.
    fn fill(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, v: i32) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                self.set(x, y, v);
            }
        }
    }

    /// `setif`: `v` where the cell is `was`.
    fn set_if(&mut self, x: i32, y: i32, was: i32, v: i32) {
        if self.get(x, y) == was {
            self.set(x, y, v);
        }
    }

    /// `edgesTo`: whether a neighbour holds `v`.
    fn edges_to(&self, x: i32, y: i32, v: i32) -> bool {
        self.get(x - 1, y) == v
            || self.get(x + 1, y) == v
            || self.get(x, y + 1) == v
            || self.get(x, y - 1) == v
    }

    /// `isHouse`: a corridor or a room of any kind.
    fn is_house(&self, x: i32, y: i32) -> bool {
        matches!(self.get(x, y), CORRIDOR | ROOM | START_ROOM | TEST_ROOM)
    }
}

/// `MansionGrid`: the base grid, the third floor's, and each floor's rooms.
struct MansionGrid {
    base: Grid,
    third: Grid,
    rooms: [Grid; 3],
}

impl MansionGrid {
    /// `entranceX`, `entranceY`.
    const ENTRANCE: (i32, i32) = (7, 4);

    fn new(random: &mut LegacyRandomSource) -> Self {
        let (ex, ey) = Self::ENTRANCE;
        let mut base = Grid::new(BLOCKED);
        base.fill(ex, ey, ex + 1, ey + 1, START_ROOM);
        base.fill(ex - 1, ey, ex - 1, ey + 1, ROOM);
        base.fill(ex + 2, ey - 2, ex + 3, ey + 3, BLOCKED);
        base.fill(ex + 1, ey - 2, ex + 1, ey - 1, CORRIDOR);
        base.fill(ex + 1, ey + 2, ex + 1, ey + 3, CORRIDOR);
        base.set(ex - 1, ey - 1, CORRIDOR);
        base.set(ex - 1, ey + 2, CORRIDOR);
        base.fill(0, 0, 11, 1, BLOCKED);
        base.fill(0, 9, 11, 11, BLOCKED);
        recursive_corridor(&mut base, random, ex, ey - 2, Dir::West, 6);
        recursive_corridor(&mut base, random, ex, ey + 3, Dir::West, 6);
        recursive_corridor(&mut base, random, ex - 2, ey - 1, Dir::West, 3);
        recursive_corridor(&mut base, random, ex - 2, ey + 2, Dir::West, 3);
        while clean_edges(&mut base) {}
        let mut grid = MansionGrid {
            base,
            third: Grid::new(BLOCKED),
            rooms: [Grid::new(BLOCKED), Grid::new(BLOCKED), Grid::new(BLOCKED)],
        };
        let base = grid.base.clone();
        identify_rooms(&base, &mut grid.rooms[0], random);
        identify_rooms(&base, &mut grid.rooms[1], random);
        grid.rooms[0].fill(ex + 1, ey, ex + 1, ey + 1, ROOM_CORRIDOR_FLAG);
        grid.rooms[1].fill(ex + 1, ey, ex + 1, ey + 1, ROOM_CORRIDOR_FLAG);
        grid.setup_third_floor(random);
        let third = grid.third.clone();
        identify_rooms(&third, &mut grid.rooms[2], random);
        grid
    }

    /// `isRoomId`: whether `(x, y)` on `floor` belongs to room `id`.
    fn is_room_id(&self, x: i32, y: i32, floor: usize, id: i32) -> bool {
        (self.rooms[floor].get(x, y) & ROOM_ID_MASK) == id
    }

    /// `get1x2RoomDirection`: which way room `id`'s other cell is.
    fn room_direction(&self, x: i32, y: i32, floor: usize, id: i32) -> Option<Dir> {
        Dir::HORIZONTAL.into_iter().find(|d| {
            let (dx, dz) = d.step();
            self.is_room_id(x + dx, y + dz, floor, id)
        })
    }

    /// `setupThirdFloor`: a second floor 1 × 2 room with a door takes the stairs, and the third
    /// floor's corridor grows from its other cell.
    fn setup_third_floor(&mut self, random: &mut LegacyRandomSource) {
        let mut stairs = Vec::new();
        for y in 0..SIZE {
            for x in 0..SIZE {
                let cell = self.rooms[1].get(x, y);
                if (cell & ROOM_TYPE_MASK) == ROOM_1X2 && (cell & ROOM_DOOR_FLAG) == ROOM_DOOR_FLAG
                {
                    stairs.push((x, y));
                }
            }
        }
        if stairs.is_empty() {
            self.third.fill(0, 0, SIZE, SIZE, BLOCKED);
            return;
        }
        let (px, py) = stairs[random.next_int_bounded(stairs.len() as i32) as usize];
        let cell = self.rooms[1].get(px, py);
        self.rooms[1].set(px, py, cell | ROOM_STAIRS_FLAG);
        let Some(d) = self.room_direction(px, py, 1, cell & ROOM_ID_MASK) else {
            return;
        };
        let (dx, dz) = d.step();
        let (sx, sy) = (px + dx, py + dz);
        for y in 0..SIZE {
            for x in 0..SIZE {
                if !self.base.is_house(x, y) {
                    self.third.set(x, y, BLOCKED);
                } else if x == px && y == py {
                    self.third.set(x, y, START_ROOM);
                } else if x == sx && y == sy {
                    self.third.set(x, y, START_ROOM);
                    self.rooms[2].set(x, y, ROOM_CORRIDOR_FLAG);
                }
            }
        }
        let open: Vec<Dir> = Dir::HORIZONTAL
            .into_iter()
            .filter(|e| {
                let (ex, ez) = e.step();
                self.third.get(sx + ex, sy + ez) == CLEAR
            })
            .collect();
        if open.is_empty() {
            self.third.fill(0, 0, SIZE, SIZE, BLOCKED);
            self.rooms[1].set(px, py, cell);
            return;
        }
        let e = open[random.next_int_bounded(open.len() as i32) as usize];
        let (ex, ez) = e.step();
        recursive_corridor(&mut self.third, random, sx + ex, sy + ez, e, 4);
        while clean_edges(&mut self.third) {}
    }
}

/// `recursiveCorridor`: a corridor cell, one ahead, a branch in a random direction that's
/// clear two ahead, then rooms on both sides and ahead.
fn recursive_corridor(
    g: &mut Grid,
    random: &mut LegacyRandomSource,
    x: i32,
    y: i32,
    dir: Dir,
    depth: i32,
) {
    if depth <= 0 {
        return;
    }
    let (sx, sz) = dir.step();
    g.set(x, y, CORRIDOR);
    g.set_if(x + sx, y + sz, CLEAR, CORRIDOR);
    for _ in 0..8 {
        let r = Dir::from_2d(random.next_int_bounded(4));
        if r == dir.opposite() || (r == Dir::East && random.next_boolean()) {
            continue;
        }
        let (rx, rz) = r.step();
        let (nx, ny) = (x + sx, y + sz);
        if g.get(nx + rx, ny + rz) == CLEAR && g.get(nx + 2 * rx, ny + 2 * rz) == CLEAR {
            recursive_corridor(g, random, nx + rx, ny + rz, r, depth - 1);
            break;
        }
    }
    let (cx, cz) = dir.clockwise().step();
    let (ax, az) = dir.counterclockwise().step();
    g.set_if(x + cx, y + cz, CLEAR, ROOM);
    g.set_if(x + ax, y + az, CLEAR, ROOM);
    g.set_if(x + sx + cx, y + sz + cz, CLEAR, ROOM);
    g.set_if(x + sx + ax, y + sz + az, CLEAR, ROOM);
    g.set_if(x + 2 * sx, y + 2 * sz, CLEAR, ROOM);
    g.set_if(x + 2 * cx, y + 2 * cz, CLEAR, ROOM);
    g.set_if(x + 2 * ax, y + 2 * az, CLEAR, ROOM);
}

/// `cleanEdges`: clear cells mostly surrounded by the house become rooms, in place.
fn clean_edges(g: &mut Grid) -> bool {
    let mut changed = false;
    for y in 0..SIZE {
        for x in 0..SIZE {
            if g.get(x, y) != CLEAR {
                continue;
            }
            let house = |x: i32, y: i32| i32::from(g.is_house(x, y));
            let sides = house(x + 1, y) + house(x - 1, y) + house(x, y + 1) + house(x, y - 1);
            let fill = if sides >= 3 {
                true
            } else if sides == 2 {
                house(x + 1, y + 1)
                    + house(x - 1, y + 1)
                    + house(x + 1, y - 1)
                    + house(x - 1, y - 1)
                    <= 1
            } else {
                false
            };
            if fill {
                g.set(x, y, ROOM);
                changed = true;
            }
        }
    }
    changed
}

/// `identifyRooms`: `g`'s room cells, shuffled, grouped into 2 × 2, 1 × 2 and 1 × 1 rooms in
/// `rooms`, each with its origin at a random corner, moved to one on a corridor for its door.
fn identify_rooms(g: &Grid, rooms: &mut Grid, random: &mut LegacyRandomSource) {
    let mut cells = Vec::new();
    for y in 0..SIZE {
        for x in 0..SIZE {
            if g.get(x, y) == ROOM {
                cells.push((x, y));
            }
        }
    }
    // Util.shuffle.
    for i in (2..=cells.len()).rev() {
        let j = random.next_int_bounded(i as i32) as usize;
        cells.swap(i - 1, j);
    }
    let mut id = 10;
    for (x, y) in cells {
        if rooms.get(x, y) != 0 {
            continue;
        }
        let (mut x0, mut x1, mut y0, mut y1) = (x, x, y, y);
        let free = |rooms: &Grid, x: i32, y: i32| rooms.get(x, y) == 0 && g.get(x, y) == ROOM;
        let kind = if free(rooms, x + 1, y) && free(rooms, x, y + 1) && free(rooms, x + 1, y + 1) {
            x1 += 1;
            y1 += 1;
            ROOM_2X2
        } else if free(rooms, x - 1, y) && free(rooms, x, y + 1) && free(rooms, x - 1, y + 1) {
            x0 -= 1;
            y1 += 1;
            ROOM_2X2
        } else if free(rooms, x - 1, y) && free(rooms, x, y - 1) && free(rooms, x - 1, y - 1) {
            x0 -= 1;
            y0 -= 1;
            ROOM_2X2
        } else if free(rooms, x + 1, y) {
            x1 += 1;
            ROOM_1X2
        } else if free(rooms, x, y + 1) {
            y1 += 1;
            ROOM_1X2
        } else if free(rooms, x - 1, y) {
            x0 -= 1;
            ROOM_1X2
        } else if free(rooms, x, y - 1) {
            y0 -= 1;
            ROOM_1X2
        } else {
            ROOM_1X1
        };
        let mut ox = if random.next_boolean() { x0 } else { x1 };
        let mut oy = if random.next_boolean() { y0 } else { y1 };
        let mut door = ROOM_DOOR_FLAG;
        let other_x = |ox: i32| if ox == x0 { x1 } else { x0 };
        let other_y = |oy: i32| if oy == y0 { y1 } else { y0 };
        if !g.edges_to(ox, oy, CORRIDOR) {
            ox = other_x(ox);
            oy = other_y(oy);
            if !g.edges_to(ox, oy, CORRIDOR) {
                oy = other_y(oy);
                if !g.edges_to(ox, oy, CORRIDOR) {
                    ox = other_x(ox);
                    oy = other_y(oy);
                    if !g.edges_to(ox, oy, CORRIDOR) {
                        door = 0;
                        ox = x0;
                        oy = y0;
                    }
                }
            }
        }
        for yy in y0..=y1 {
            for xx in x0..=x1 {
                let v = if xx == ox && yy == oy {
                    ROOM_ORIGIN_FLAG | door | kind | id
                } else {
                    kind | id
                };
                rooms.set(xx, yy, v);
            }
        }
        id += 1;
    }
}

/// `PlacementData`: where the outer-wall walk is, which way it faces, and its walls' template.
struct Walk {
    position: [i32; 3],
    rotation: Rotation,
    wall: &'static str,
}

/// `MansionPiecePlacer`.
struct Placer<'a> {
    templates: &'a [Arc<Template>],
    random: LegacyRandomSource,
    pieces: Vec<MansionPiece>,
}

impl Placer<'_> {
    fn add(&mut self, name: &str, pos: [i32; 3], rotation: Rotation, mirror: Mirror) {
        let id = format!("woodland_mansion/{name}");
        let i = TEMPLATES
            .binary_search(&id.as_str())
            .unwrap_or_else(|_| panic!("{id} is one of the mansion's templates"));
        self.pieces.push(MansionPiece {
            template: self.templates[i].clone(),
            pos,
            rotation,
            mirror,
        });
    }

    /// `traverseWallPiece`.
    fn wall_piece(&mut self, walk: &mut Walk) {
        let pos = rel(walk.position, Dir::East.turned(walk.rotation), 7);
        self.add(walk.wall, pos, walk.rotation, Mirror::None);
        walk.position = rel(walk.position, Dir::South.turned(walk.rotation), 8);
    }

    /// `traverseTurn`: an outer corner.
    fn turn(&mut self, walk: &mut Walk) {
        walk.position = rel(walk.position, Dir::South.turned(walk.rotation), -1);
        self.add("wall_corner", walk.position, walk.rotation, Mirror::None);
        walk.position = rel(walk.position, Dir::South.turned(walk.rotation), -7);
        walk.position = rel(walk.position, Dir::West.turned(walk.rotation), -6);
        walk.rotation = walk.rotation.rotated(Rotation::Clockwise90);
    }

    /// `traverseInnerTurn`: an inner corner, no piece.
    fn inner_turn(walk: &mut Walk) {
        walk.position = rel(walk.position, Dir::South.turned(walk.rotation), 6);
        walk.position = rel(walk.position, Dir::East.turned(walk.rotation), 8);
        walk.rotation = walk.rotation.rotated(Rotation::Counterclockwise90);
    }

    /// `traverseOuterWalls`: around `g`'s house clockwise from `start` facing `dir`, back to
    /// `end` facing `dir`.
    fn outer_walls(
        &mut self,
        walk: &mut Walk,
        g: &Grid,
        dir: Dir,
        start: (i32, i32),
        end: (i32, i32),
    ) {
        let (mut x, mut y) = start;
        let mut dir = dir;
        let first = dir;
        // The walk always comes back; past this many steps, something's wrong.
        for _ in 0..4 * SIZE * SIZE {
            let (sx, sz) = dir.step();
            let (ax, az) = dir.counterclockwise().step();
            if !g.is_house(x + sx, y + sz) {
                self.turn(walk);
                dir = dir.clockwise();
                if x != end.0 || y != end.1 || first != dir {
                    self.wall_piece(walk);
                }
            } else if g.is_house(x + sx + ax, y + sz + az) {
                Self::inner_turn(walk);
                x += sx;
                y += sz;
                dir = dir.counterclockwise();
            } else {
                x += sx;
                y += sz;
                if x != end.0 || y != end.1 || first != dir {
                    self.wall_piece(walk);
                }
            }
            if x == end.0 && y == end.1 && first == dir {
                return;
            }
        }
    }

    /// `createRoof`: over each cell of `g` with nothing of `upper` above, a roof and its edges;
    /// under `upper`, a low wall around it.
    fn roof(&mut self, base: [i32; 3], rot: Rotation, g: &Grid, upper: Option<&Grid>) {
        let r = |d: Dir| d.turned(rot);
        let turn = |by: Rotation| rot.rotated(by);
        let house = |x: i32, y: i32| g.is_house(x, y);
        let covered = |x: i32, y: i32| upper.is_some_and(|u| u.is_house(x, y));
        let none = Mirror::None;
        for y in 0..SIZE {
            for x in 0..SIZE {
                if !house(x, y) || covered(x, y) {
                    continue;
                }
                let p = cell(base, rot, x, y, 0);
                self.add("roof", above(p, 3), rot, none);
                if !house(x + 1, y) {
                    self.add("roof_front", rel(p, r(Dir::East), 6), rot, none);
                }
                if !house(x - 1, y) {
                    let q = rel(rel(p, r(Dir::East), 0), r(Dir::South), 7);
                    self.add("roof_front", q, turn(Rotation::Clockwise180), none);
                }
                if !house(x, y - 1) {
                    let q = rel(p, r(Dir::West), 1);
                    self.add("roof_front", q, turn(Rotation::Counterclockwise90), none);
                }
                if !house(x, y + 1) {
                    let q = rel(rel(p, r(Dir::East), 6), r(Dir::South), 6);
                    self.add("roof_front", q, turn(Rotation::Clockwise90), none);
                }
            }
        }
        if upper.is_some() {
            for y in 0..SIZE {
                for x in 0..SIZE {
                    if !house(x, y) || !covered(x, y) {
                        continue;
                    }
                    let p = cell(base, rot, x, y, 0);
                    if !house(x + 1, y) {
                        self.add("small_wall", rel(p, r(Dir::East), 7), rot, none);
                    }
                    if !house(x - 1, y) {
                        let q = rel(rel(p, r(Dir::West), 1), r(Dir::South), 6);
                        self.add("small_wall", q, turn(Rotation::Clockwise180), none);
                    }
                    if !house(x, y - 1) {
                        let q = rel(rel(p, r(Dir::West), 0), r(Dir::North), 1);
                        self.add("small_wall", q, turn(Rotation::Counterclockwise90), none);
                    }
                    if !house(x, y + 1) {
                        let q = rel(rel(p, r(Dir::East), 6), r(Dir::South), 7);
                        self.add("small_wall", q, turn(Rotation::Clockwise90), none);
                    }
                    if !house(x + 1, y) {
                        if !house(x, y - 1) {
                            let q = rel(rel(p, r(Dir::East), 7), r(Dir::North), 2);
                            self.add("small_wall_corner", q, rot, none);
                        }
                        if !house(x, y + 1) {
                            let q = rel(rel(p, r(Dir::East), 8), r(Dir::South), 7);
                            self.add("small_wall_corner", q, turn(Rotation::Clockwise90), none);
                        }
                    }
                    if !house(x - 1, y) {
                        if !house(x, y - 1) {
                            let q = rel(rel(p, r(Dir::West), 2), r(Dir::North), 1);
                            let by = turn(Rotation::Counterclockwise90);
                            self.add("small_wall_corner", q, by, none);
                        }
                        if !house(x, y + 1) {
                            let q = rel(rel(p, r(Dir::West), 1), r(Dir::South), 8);
                            self.add("small_wall_corner", q, turn(Rotation::Clockwise180), none);
                        }
                    }
                }
            }
        }
        for y in 0..SIZE {
            for x in 0..SIZE {
                if !house(x, y) || covered(x, y) {
                    continue;
                }
                let p = cell(base, rot, x, y, 0);
                if !house(x + 1, y) {
                    let q = rel(p, r(Dir::East), 6);
                    if !house(x, y + 1) {
                        self.add("roof_corner", rel(q, r(Dir::South), 6), rot, none);
                    } else if house(x + 1, y + 1) {
                        self.add("roof_inner_corner", rel(q, r(Dir::South), 5), rot, none);
                    }
                    if !house(x, y - 1) {
                        self.add("roof_corner", q, turn(Rotation::Counterclockwise90), none);
                    } else if house(x + 1, y - 1) {
                        let q = rel(rel(p, r(Dir::East), 9), r(Dir::North), 2);
                        self.add("roof_inner_corner", q, turn(Rotation::Clockwise90), none);
                    }
                }
                if !house(x - 1, y) {
                    let q = rel(rel(p, r(Dir::East), 0), r(Dir::South), 0);
                    if !house(x, y + 1) {
                        let by = turn(Rotation::Clockwise90);
                        self.add("roof_corner", rel(q, r(Dir::South), 6), by, none);
                    } else if house(x - 1, y + 1) {
                        let s = rel(rel(q, r(Dir::South), 8), r(Dir::West), 3);
                        self.add(
                            "roof_inner_corner",
                            s,
                            turn(Rotation::Counterclockwise90),
                            none,
                        );
                    }
                    if !house(x, y - 1) {
                        self.add("roof_corner", q, turn(Rotation::Clockwise180), none);
                    } else if house(x - 1, y - 1) {
                        let s = rel(q, r(Dir::South), 1);
                        self.add("roof_inner_corner", s, turn(Rotation::Clockwise180), none);
                    }
                }
            }
        }
    }

    /// The `FloorRoomCollection`'s template names, drawn as the game draws them.
    fn name_1x1(&mut self, floor: usize) -> String {
        let n = self.random.next_int_bounded(5) + 1;
        if floor == 0 {
            format!("1x1_a{n}")
        } else {
            format!("1x1_b{n}")
        }
    }

    fn name_1x1_secret(&mut self) -> String {
        format!("1x1_as{}", self.random.next_int_bounded(4) + 1)
    }

    fn name_1x2_side(&mut self, floor: usize, stairs: bool) -> String {
        if floor == 0 {
            format!("1x2_a{}", self.random.next_int_bounded(9) + 1)
        } else if stairs {
            "1x2_c_stairs".into()
        } else {
            format!("1x2_c{}", self.random.next_int_bounded(4) + 1)
        }
    }

    fn name_1x2_front(&mut self, floor: usize, stairs: bool) -> String {
        if floor == 0 {
            format!("1x2_b{}", self.random.next_int_bounded(5) + 1)
        } else if stairs {
            "1x2_d_stairs".into()
        } else {
            format!("1x2_d{}", self.random.next_int_bounded(5) + 1)
        }
    }

    fn name_1x2_secret(&mut self, floor: usize) -> String {
        if floor == 0 {
            format!("1x2_s{}", self.random.next_int_bounded(2) + 1)
        } else {
            // nextInt(1) still draws.
            format!("1x2_se{}", self.random.next_int_bounded(1) + 1)
        }
    }

    fn name_2x2(&mut self, floor: usize) -> String {
        if floor == 0 {
            format!("2x2_a{}", self.random.next_int_bounded(4) + 1)
        } else {
            format!("2x2_b{}", self.random.next_int_bounded(5) + 1)
        }
    }

    /// `addRoom1x1`: turned to its door; without one, a secret room.
    fn room_1x1(&mut self, p: [i32; 3], rot: Rotation, door: Door, floor: usize) {
        let mut name = self.name_1x1(floor);
        let turn = match door {
            Door::To(Dir::East) => Rotation::None,
            Door::To(Dir::North) => Rotation::Counterclockwise90,
            Door::To(Dir::West) => Rotation::Clockwise180,
            Door::To(Dir::South) => Rotation::Clockwise90,
            Door::Up => {
                name = self.name_1x1_secret();
                Rotation::None
            }
        };
        // getZeroPositionWithTransform((1, 0, 0), NONE, turn, 7, 7).
        let offset = match turn {
            Rotation::None => [1, 0, 0],
            Rotation::Clockwise90 => [7, 0, 0],
            Rotation::Clockwise180 => [7, 0, 6],
            Rotation::Counterclockwise90 => [1, 0, 6],
        };
        let offset = rot.transform(offset);
        let pos = [
            p[0].wrapping_add(offset[0]),
            p[1],
            p[2].wrapping_add(offset[2]),
        ];
        self.add(&name, pos, turn.rotated(rot), Mirror::None);
    }

    /// `addRoom1x2`: by its door and which way its other cell is.
    #[allow(clippy::too_many_arguments, reason = "the game's parameters")]
    fn room_1x2(
        &mut self,
        p: [i32; 3],
        rot: Rotation,
        towards: Option<Dir>,
        door: Door,
        stairs: bool,
        floor: usize,
    ) {
        use Dir::{East, North, South, West};
        let r = |d: Dir| d.turned(rot);
        let turn = |by: Rotation| rot.rotated(by);
        enum Name {
            Side,
            Front,
            Secret,
        }
        let Some(towards) = towards else {
            return;
        };
        let (pos, name, rotation, mirror) = match (door, towards) {
            (Door::To(East), South) => (rel(p, r(East), 1), Name::Side, rot, Mirror::None),
            (Door::To(East), North) => (
                rel(rel(p, r(East), 1), r(South), 6),
                Name::Side,
                rot,
                Mirror::LeftRight,
            ),
            (Door::To(West), North) => (
                rel(rel(p, r(East), 7), r(South), 6),
                Name::Side,
                turn(Rotation::Clockwise180),
                Mirror::None,
            ),
            (Door::To(West), South) => (rel(p, r(East), 7), Name::Side, rot, Mirror::FrontBack),
            (Door::To(South), East) => (
                rel(p, r(East), 1),
                Name::Side,
                turn(Rotation::Clockwise90),
                Mirror::LeftRight,
            ),
            (Door::To(South), West) => (
                rel(p, r(East), 7),
                Name::Side,
                turn(Rotation::Clockwise90),
                Mirror::None,
            ),
            (Door::To(North), West) => (
                rel(rel(p, r(East), 7), r(South), 6),
                Name::Side,
                turn(Rotation::Clockwise90),
                Mirror::FrontBack,
            ),
            (Door::To(North), East) => (
                rel(rel(p, r(East), 1), r(South), 6),
                Name::Side,
                turn(Rotation::Counterclockwise90),
                Mirror::None,
            ),
            (Door::To(South), North) => (
                rel(rel(p, r(East), 1), r(North), 8),
                Name::Front,
                rot,
                Mirror::None,
            ),
            (Door::To(North), South) => (
                rel(rel(p, r(East), 7), r(South), 14),
                Name::Front,
                turn(Rotation::Clockwise180),
                Mirror::None,
            ),
            (Door::To(West), East) => (
                rel(p, r(East), 15),
                Name::Front,
                turn(Rotation::Clockwise90),
                Mirror::None,
            ),
            (Door::To(East), West) => (
                rel(rel(p, r(West), 7), r(South), 6),
                Name::Front,
                turn(Rotation::Counterclockwise90),
                Mirror::None,
            ),
            (Door::Up, East) => (
                rel(p, r(East), 15),
                Name::Secret,
                turn(Rotation::Clockwise90),
                Mirror::None,
            ),
            (Door::Up, South) => (
                rel(rel(p, r(East), 1), r(North), 0),
                Name::Secret,
                rot,
                Mirror::None,
            ),
            _ => return,
        };
        let name = match name {
            Name::Side => self.name_1x2_side(floor, stairs),
            Name::Front => self.name_1x2_front(floor, stairs),
            Name::Secret => self.name_1x2_secret(floor),
        };
        self.add(&name, pos, rotation, mirror);
    }

    /// `addRoom2x2`: by its door and which way the room runs from it.
    fn room_2x2(&mut self, p: [i32; 3], rot: Rotation, towards: Dir, door: Dir, floor: usize) {
        use Dir::{East, North, South, West};
        let turn = |by: Rotation| rot.rotated(by);
        let (ox, oz, rotation, mirror) = match (door, towards) {
            (East, South) => (-7, 0, rot, Mirror::None),
            (East, North) => (-7, 6, rot, Mirror::LeftRight),
            (North, East) => (1, 14, turn(Rotation::Counterclockwise90), Mirror::None),
            (North, West) => (7, 14, turn(Rotation::Counterclockwise90), Mirror::LeftRight),
            (South, West) => (7, -8, turn(Rotation::Clockwise90), Mirror::None),
            (South, East) => (1, -8, turn(Rotation::Clockwise90), Mirror::LeftRight),
            (West, North) => (15, 6, turn(Rotation::Clockwise180), Mirror::None),
            (West, South) => (15, 0, rot, Mirror::FrontBack),
            _ => (0, 0, rot, Mirror::None),
        };
        let name = self.name_2x2(floor);
        let pos = rel(rel(p, East.turned(rot), ox), South.turned(rot), oz);
        self.add(&name, pos, rotation, mirror);
    }

    /// One floor's corridors, indoor walls and rooms.
    fn floor(&mut self, grid: &MansionGrid, pos: [i32; 3], rot: Rotation, floor: usize) {
        use Dir::{East, North, South, West};
        let r = |d: Dir| d.turned(rot);
        let base = above(pos, 8 * floor as i32 + if floor == 2 { 3 } else { 0 });
        let rooms = &grid.rooms[floor];
        let g = if floor == 2 { &grid.third } else { &grid.base };
        let (carpet_south, carpet_west, wall, door_name) = if floor == 0 {
            (
                "carpet_south_1",
                "carpet_west_1",
                "indoors_wall_1",
                "indoors_door_1",
            )
        } else {
            (
                "carpet_south_2",
                "carpet_west_2",
                "indoors_wall_2",
                "indoors_door_2",
            )
        };
        let carpeted = |x: i32, y: i32| {
            g.get(x, y) == CORRIDOR || (rooms.get(x, y) & ROOM_CORRIDOR_FLAG) == ROOM_CORRIDOR_FLAG
        };
        for y in 0..SIZE {
            for x in 0..SIZE {
                if g.get(x, y) != CORRIDOR {
                    continue;
                }
                let p = cell(base, rot, x, y, 0);
                self.add("corridor_floor", p, rot, Mirror::None);
                if carpeted(x, y - 1) {
                    self.add(
                        "carpet_north",
                        above(rel(p, r(East), 1), 1),
                        rot,
                        Mirror::None,
                    );
                }
                if carpeted(x + 1, y) {
                    let q = above(rel(rel(p, r(South), 1), r(East), 5), 1);
                    self.add("carpet_east", q, rot, Mirror::None);
                }
                if carpeted(x, y + 1) {
                    let q = rel(rel(p, r(South), 5), r(West), 1);
                    self.add(carpet_south, q, rot, Mirror::None);
                }
                if carpeted(x - 1, y) {
                    let q = rel(rel(p, r(West), 1), r(North), 1);
                    self.add(carpet_west, q, rot, Mirror::None);
                }
            }
        }
        for y in 0..SIZE {
            for x in 0..SIZE {
                let mut stairs = floor == 2 && g.get(x, y) == START_ROOM;
                if g.get(x, y) != ROOM && !stairs {
                    continue;
                }
                let value = rooms.get(x, y);
                let kind = value & ROOM_TYPE_MASK;
                let id = value & ROOM_ID_MASK;
                stairs = stairs && (value & ROOM_CORRIDOR_FLAG) == ROOM_CORRIDOR_FLAG;
                let mut door = None;
                if (value & ROOM_DOOR_FLAG) == ROOM_DOOR_FLAG {
                    let onto: Vec<Dir> = Dir::HORIZONTAL
                        .into_iter()
                        .filter(|d| {
                            let (dx, dz) = d.step();
                            g.get(x + dx, y + dz) == CORRIDOR
                        })
                        .collect();
                    if !onto.is_empty() {
                        let i = self.random.next_int_bounded(onto.len() as i32) as usize;
                        door = Some(Door::To(onto[i]));
                    }
                }
                if door.is_none() && (value & ROOM_ORIGIN_FLAG) == ROOM_ORIGIN_FLAG {
                    door = Some(Door::Up);
                }
                let p = cell(base, rot, x, y, -1);
                let wall_or_door = |d: Dir| {
                    if door == Some(Door::To(d)) {
                        door_name
                    } else {
                        wall
                    }
                };
                if g.is_house(x - 1, y) && !grid.is_room_id(x - 1, y, floor, id) {
                    self.add(wall_or_door(West), p, rot, Mirror::None);
                }
                if g.get(x + 1, y) == CORRIDOR && !stairs {
                    self.add(wall_or_door(East), rel(p, r(East), 8), rot, Mirror::None);
                }
                if g.is_house(x, y + 1) && !grid.is_room_id(x, y + 1, floor, id) {
                    let q = rel(rel(p, r(South), 7), r(East), 7);
                    let by = rot.rotated(Rotation::Clockwise90);
                    self.add(wall_or_door(South), q, by, Mirror::None);
                }
                if g.get(x, y - 1) == CORRIDOR && !stairs {
                    let q = rel(rel(p, r(North), 1), r(East), 7);
                    let by = rot.rotated(Rotation::Clockwise90);
                    self.add(wall_or_door(North), q, by, Mirror::None);
                }
                match (kind, door) {
                    (ROOM_1X1, Some(door)) => self.room_1x1(p, rot, door, floor),
                    (ROOM_1X2, Some(door)) => {
                        let towards = grid.room_direction(x, y, floor, id);
                        let has_stairs = (value & ROOM_STAIRS_FLAG) == ROOM_STAIRS_FLAG;
                        self.room_1x2(p, rot, towards, door, has_stairs, floor);
                    }
                    (ROOM_2X2, Some(Door::To(door))) => {
                        let mut towards = door.clockwise();
                        let (dx, dz) = towards.step();
                        if !grid.is_room_id(x + dx, y + dz, floor, id) {
                            towards = towards.opposite();
                        }
                        self.room_2x2(p, rot, towards, door, floor);
                    }
                    (ROOM_2X2, Some(Door::Up)) => {
                        // get2x2Secret: the one template, no draw.
                        self.add("2x2_s1", rel(p, r(East), 1), rot, Mirror::None);
                    }
                    _ => {}
                }
            }
        }
    }
}

/// A grid cell's corner on a level whose base is `base`: corridors' and roofs' at `x_shift` 0,
/// rooms' and indoor walls' a block west, at −1.
fn cell(base: [i32; 3], rot: Rotation, x: i32, y: i32, x_shift: i32) -> [i32; 3] {
    let p = rel(base, Dir::South.turned(rot), 8 + (y - 5) * 8);
    rel(p, Dir::East.turned(rot), x_shift + (x - 8) * 8)
}

/// A mansion's pieces for its start at `at` (the stub: the chunk's (7, 7) and the lowest
/// corner's height) in `chunk`, for world `seed`, in the game's order (`generateMansion`).
/// `templates` are [`TEMPLATES`]' in order.
pub(crate) fn pieces(
    templates: &[Arc<Template>],
    seed: i64,
    chunk: [i32; 2],
    at: [i32; 3],
) -> Vec<MansionPiece> {
    let mut random = worldgen::large_feature(seed, chunk[0], chunk[1]);
    // findGenerationPoint's draw; its corners gave `at`'s height.
    let rot = Rotation::random(&mut random);
    let grid = MansionGrid::new(&mut random);
    let mut placer = Placer {
        templates,
        random,
        pieces: Vec::new(),
    };
    let r = |d: Dir| d.turned(rot);
    // createMansion.
    placer.add("entrance", rel(at, r(Dir::West), 9), rot, Mirror::None);
    let mut ground = Walk {
        position: rel(at, r(Dir::South), 16),
        rotation: rot,
        wall: "wall_flat",
    };
    let mut first = Walk {
        position: above(ground.position, 8),
        rotation: rot,
        wall: "wall_window",
    };
    placer.outer_walls(&mut ground, &grid.base, Dir::South, (8, 5), (8, 4));
    placer.outer_walls(&mut first, &grid.base, Dir::South, (8, 5), (8, 4));
    let mut top = Walk {
        position: above(ground.position, 19),
        rotation: ground.rotation,
        wall: "wall_window",
    };
    'third: for y in 0..SIZE {
        for x in (0..SIZE).rev() {
            if grid.third.is_house(x, y) {
                top.position = rel(top.position, r(Dir::South), 8 + (y - 5) * 8);
                top.position = rel(top.position, r(Dir::East), (x - 8) * 8);
                placer.wall_piece(&mut top);
                placer.outer_walls(&mut top, &grid.third, Dir::South, (x, y), (x, y));
                break 'third;
            }
        }
    }
    placer.roof(above(at, 16), rot, &grid.base, Some(&grid.third));
    placer.roof(above(at, 27), rot, &grid.third, None);
    for floor in 0..3 {
        placer.floor(&grid, at, rot, floor);
    }
    placer.pieces
}

/// [`pieces`] as a [`Layout`]: mansions don't adapt terrain, so the bounds are the pieces'.
pub(crate) fn layout(pieces: &[MansionPiece]) -> Option<Layout> {
    let boxes: Vec<BoundingBox> = pieces.iter().map(MansionPiece::bounds).collect();
    let bounds = boxes.iter().copied().reduce(BoundingBox::encapsulate)?;
    Some(Layout {
        pieces: boxes,
        placed: Vec::new(),
        bounds,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_are_sorted_for_lookup() {
        assert!(TEMPLATES.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn mirrors_negate_before_turning() {
        let t = Arc::new(Template::for_test([3, 1, 2], Vec::new()));
        let piece = |rotation, mirror| MansionPiece {
            template: t.clone(),
            pos: [10, 0, 20],
            rotation,
            mirror,
        };
        assert_eq!(
            piece(Rotation::None, Mirror::LeftRight).world([1, 0, 1]),
            [11, 0, 19]
        );
        assert_eq!(
            piece(Rotation::None, Mirror::FrontBack).world([1, 0, 1]),
            [9, 0, 21]
        );
        // Left-right, then clockwise: (x, -z) turned to (z, x).
        assert_eq!(
            piece(Rotation::Clockwise90, Mirror::LeftRight).world([1, 0, 1]),
            [11, 0, 21]
        );
    }
}
