//! The user's own data: markers, done places, and recent and saved worlds, in one JSON file
//! beside the settings (`docs/specs/user-data.md`, ADR 0048). Read leniently, field by field,
//! so a bad entry costs only itself; written whole, through a temporary file, after merging
//! whatever another viewer wrote meanwhile.

use std::collections::BTreeMap;
use std::hash::{BuildHasher, Hasher};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value, json};
use spyglass::Dimension;

use crate::view::View;

/// The file's name, in the settings folder.
pub const FILE: &str = "spyglass-data.json";
/// The format this viewer reads and writes.
const FORMAT: i64 = 1;
/// The largest file read.
const MAX_FILE: u64 = 16 << 20;
/// Worlds in the file.
const MAX_WORLDS: usize = 1_000;
/// Markers, and done places, per world.
const MAX_ENTRIES: usize = 10_000;
/// Characters in a name, an id, a version or a save's name.
const MAX_TEXT: usize = 200;
/// Characters in a save's folder.
const MAX_PATH: usize = 4_096;
/// Characters in a marker's id.
const MAX_ID: usize = 64;
/// How far from 0 a coordinate may be: past the world border.
const MAX_COORD: i64 = 30_000_000;
/// The latest time: 2⁵³ milliseconds, which a double still holds exactly.
const MAX_TIME: i64 = 1 << 53;
/// Unsaved worlds kept with nothing in them.
const RECENT: usize = 20;
/// How often the file is written while things change.
const WRITE_EVERY: std::time::Duration = std::time::Duration::from_secs(1);

/// The colours the viewer offers for markers, the first the default.
pub const COLOURS: [(&str, [u8; 3]); 8] = [
    ("Amber", [0xf0, 0xb0, 0x30]),
    ("Red", [0xe0, 0x50, 0x50]),
    ("Green", [0x50, 0xc0, 0x60]),
    ("Blue", [0x4c, 0x8f, 0xf0]),
    ("Purple", [0xa0, 0x70, 0xe0]),
    ("Pink", [0xf0, 0x70, 0xb0]),
    ("White", [0xf0, 0xf0, 0xf0]),
    ("Black", [0x20, 0x20, 0x20]),
];

/// Milliseconds since the Unix epoch.
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(MAX_TIME))
        .min(MAX_TIME)
}

/// A time for a change to something last changed at `before`: now, but always later.
fn later(before: i64) -> i64 {
    now().max(before.saturating_add(1)).min(MAX_TIME)
}

/// A world: a seed, or a save by its level name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorldKey {
    /// The world's seed.
    pub seed: i64,
    /// The save's level name, for a world opened from a save; `None` for a seed.
    pub save: Option<String>,
}

/// What's kept for one world.
#[derive(Debug, Clone, PartialEq)]
pub struct WorldData {
    /// Which world.
    pub key: WorldKey,
    /// Where the save was last opened from.
    pub save_dir: Option<String>,
    /// The user's name for it; named worlds are saved.
    pub name: Option<String>,
    /// When `name` or `save_dir` last changed.
    pub modified: i64,
    /// When it was last shown, which orders the recent list.
    pub opened: i64,
    /// The game version it was last shown in.
    pub version: Option<String>,
    /// The dimension last shown, by id.
    pub dimension: String,
    /// Each dimension's last view, by id.
    pub views: BTreeMap<String, View>,
    /// The user's markers, deleted ones kept as tombstones.
    pub markers: Vec<Marker>,
    /// The places marked done, or once done and since not.
    pub done: Vec<DonePlace>,
    /// Fields this viewer doesn't know, kept to be written back.
    extra: Map<String, Value>,
}

impl WorldData {
    pub fn new(key: WorldKey) -> WorldData {
        WorldData {
            key,
            save_dir: None,
            name: None,
            modified: 0,
            opened: 0,
            version: None,
            dimension: Dimension::Overworld.id().to_string(),
            views: BTreeMap::new(),
            markers: Vec::new(),
            done: Vec::new(),
            extra: Map::new(),
        }
    }

    /// The markers not deleted.
    pub fn live_markers(&self) -> impl Iterator<Item = &Marker> {
        self.markers.iter().filter(|m| !m.deleted)
    }

    /// Whether it holds anything worth keeping beyond the recent list.
    fn holds_anything(&self) -> bool {
        self.name.is_some()
            || self.live_markers().next().is_some()
            || self.done.iter().any(|d| d.done)
    }

    /// Whether `place` in `dimension` is marked done.
    #[cfg(test)]
    pub fn is_done(&self, dimension: &str, place: &Place) -> bool {
        self.done
            .iter()
            .any(|d| d.done && d.dimension == dimension && d.place == *place)
    }

    /// Marks `place` in `dimension` done or not.
    pub fn set_done(&mut self, dimension: &str, place: &Place, done: bool) {
        match self
            .done
            .iter_mut()
            .find(|d| d.dimension == dimension && d.place == *place)
        {
            Some(d) => {
                if d.done != done {
                    d.done = done;
                    d.modified = later(d.modified);
                }
            }
            None if done => self.done.push(DonePlace {
                dimension: dimension.to_string(),
                place: place.clone(),
                done,
                modified: now(),
                extra: Map::new(),
            }),
            None => {}
        }
    }

    /// The marker with `id`, if it isn't deleted.
    pub fn marker_mut(&mut self, id: &str) -> Option<&mut Marker> {
        self.markers.iter_mut().find(|m| m.id == id && !m.deleted)
    }

    /// Adds a marker at block `(x, z)` of `dimension`, and returns its id.
    pub fn add_marker(&mut self, dimension: &str, [x, z]: [i32; 2], name: String) -> String {
        let id = new_id(|id| self.markers.iter().any(|m| m.id == id));
        let colour = COLOURS[0].1;
        self.markers.push(Marker {
            id: id.clone(),
            dimension: dimension.to_string(),
            x: x.clamp(-MAX_COORD as i32, MAX_COORD as i32),
            y: None,
            z: z.clamp(-MAX_COORD as i32, MAX_COORD as i32),
            name,
            colour,
            done: false,
            deleted: false,
            modified: now(),
            extra: Map::new(),
        });
        id
    }

    /// Records that the world was shown now, in `version`.
    pub fn touch(&mut self, version: Option<&str>) {
        self.opened = later(self.opened);
        if let Some(v) = version {
            self.version = Some(v.to_string());
        }
    }

    /// Names the world, which saves it, or unnames it with `None`.
    pub fn set_name(&mut self, name: Option<String>) {
        let name = name
            .map(|n| clip(n.trim(), MAX_TEXT))
            .filter(|n| !n.is_empty());
        if self.name != name {
            self.name = name;
            self.modified = later(self.modified);
        }
    }

    /// Records where the save was opened from.
    pub fn set_save_dir(&mut self, dir: &Path) {
        let dir = Some(clip(&dir.display().to_string(), MAX_PATH));
        if self.save_dir != dir {
            self.save_dir = dir;
            self.modified = later(self.modified);
        }
    }
}

/// A random marker id that `taken` doesn't know.
fn new_id(taken: impl Fn(&str) -> bool) -> String {
    loop {
        // A new `RandomState` is seeded at random, so its hash of nothing is a random number.
        let random = std::collections::hash_map::RandomState::new()
            .build_hasher()
            .finish();
        let id = format!("m{random:016x}");
        if !taken(&id) {
            return id;
        }
    }
}

/// A marker the user placed.
#[derive(Debug, Clone, PartialEq)]
pub struct Marker {
    /// Unique in its world, so copies of it merge.
    pub id: String,
    /// The dimension it's in, by id.
    pub dimension: String,
    /// The block it's on, x.
    pub x: i32,
    /// The block's height, if the user gave one.
    pub y: Option<i32>,
    /// The block it's on, z.
    pub z: i32,
    /// The user's name for it; may be empty.
    pub name: String,
    /// Its colour, red, green and blue.
    pub colour: [u8; 3],
    /// Whether it's marked done.
    pub done: bool,
    /// A tombstone, so a merge doesn't bring a deleted marker back.
    pub deleted: bool,
    /// When it last changed: the later change wins a merge.
    pub modified: i64,
    /// Fields this viewer doesn't know, kept to be written back.
    extra: Map<String, Value>,
}

impl Marker {
    /// Records a change made just now.
    pub fn changed(&mut self) {
        self.modified = later(self.modified);
    }

    /// Deletes it, leaving a tombstone.
    pub fn delete(&mut self) {
        self.deleted = true;
        self.changed();
    }
}

/// What a done place is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// A structure start, by its set.
    Structure,
    /// A biome, by its id.
    Biome,
    /// A plain block.
    Place,
}

impl Kind {
    /// Its name in the file.
    fn name(self) -> &'static str {
        match self {
            Kind::Structure => "structure",
            Kind::Biome => "biome",
            Kind::Place => "place",
        }
    }
}

/// Something on the map that can be done, in its dimension: a structure start by its set and
/// position, a biome by its id and block, or a plain block.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Place {
    /// What it is.
    pub kind: Kind,
    /// The structure set or biome id; empty for a plain block.
    pub id: String,
    /// Its block, x and z.
    pub at: [i32; 2],
}

/// A place marked done, or once done and since not.
#[derive(Debug, Clone, PartialEq)]
pub struct DonePlace {
    /// The dimension it's in, by id.
    pub dimension: String,
    /// What and where it is.
    pub place: Place,
    /// Whether it's done now: kept when it isn't, so a merge doesn't mark it done again.
    pub done: bool,
    /// When it last changed: the later change wins a merge.
    pub modified: i64,
    /// Fields this viewer doesn't know, kept to be written back.
    extra: Map<String, Value>,
}

/// Everything in the file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct UserData {
    /// Every world kept, in no particular order until written.
    pub worlds: Vec<WorldData>,
    /// Top-level fields this viewer doesn't know, kept to be written back.
    extra: Map<String, Value>,
}

/// A file read: what could be, and what couldn't.
#[derive(Debug, Default)]
pub struct Read {
    /// What could be read.
    pub data: UserData,
    /// What was skipped or ignored, each with its place in the file.
    pub problems: Vec<String>,
    /// A newer format: read, but not to be written over.
    pub read_only: bool,
    /// Not read at all: too large, not JSON, or without a format.
    pub unreadable: bool,
}

impl UserData {
    /// The world with `key`, if it's kept.
    pub fn world(&self, key: &WorldKey) -> Option<&WorldData> {
        self.worlds.iter().find(|w| w.key == *key)
    }

    /// The world with `key`, if it's kept, to change.
    pub fn world_mut(&mut self, key: &WorldKey) -> Option<&mut WorldData> {
        self.worlds.iter_mut().find(|w| w.key == *key)
    }

    /// The world with `key`, made if it's new.
    pub fn world_entry(&mut self, key: &WorldKey) -> &mut WorldData {
        match self.worlds.iter().position(|w| w.key == *key) {
            Some(i) => &mut self.worlds[i],
            None => {
                self.worlds.push(WorldData::new(key.clone()));
                self.worlds.last_mut().expect("just pushed")
            }
        }
    }

    /// Drops the world with `key` and everything kept for it.
    pub fn forget(&mut self, key: &WorldKey) {
        self.worlds.retain(|w| w.key != *key);
    }

    /// Merges `other` in by the spec's rules: by key, and the later change wins.
    pub fn merge(&mut self, other: UserData) {
        for (field, value) in other.extra {
            self.extra.entry(field).or_insert(value);
        }
        for theirs in other.worlds {
            match self.worlds.iter().position(|w| w.key == theirs.key) {
                Some(i) => merge_world(&mut self.worlds[i], theirs),
                None if self.worlds.len() < MAX_WORLDS => self.worlds.push(theirs),
                None => {}
            }
        }
    }

    /// Drops unsaved worlds that hold nothing, past the most recently opened.
    fn prune(&mut self) {
        self.worlds
            .sort_by(|a, b| b.opened.cmp(&a.opened).then(a.key.cmp(&b.key)));
        let mut empty = 0;
        self.worlds.retain(|w| {
            if w.holds_anything() {
                return true;
            }
            empty += 1;
            empty <= RECENT
        });
    }

    /// The file's text: pruned and ordered as the spec says.
    pub fn file_text(&mut self) -> String {
        self.prune();
        let mut top = Map::new();
        top.insert("format".into(), json!(FORMAT));
        top.insert(
            "worlds".into(),
            Value::Array(self.worlds.iter().map(world_json).collect()),
        );
        for (field, value) in &self.extra {
            top.entry(field.clone()).or_insert_with(|| value.clone());
        }
        let mut text = serde_json::to_string_pretty(&Value::Object(top)).unwrap_or_default();
        text.push('\n');
        text
    }

    /// Only `key`'s world, as a file of its own.
    pub fn only(&self, key: &WorldKey) -> UserData {
        UserData {
            worlds: self.world(key).cloned().into_iter().collect(),
            extra: Map::new(),
        }
    }
}

/// Merges another copy of the same world in: each part from whichever copy changed it later.
fn merge_world(ours: &mut WorldData, theirs: WorldData) {
    if theirs.modified > ours.modified {
        ours.name = theirs.name;
        ours.save_dir = theirs.save_dir;
        ours.modified = theirs.modified;
    }
    if theirs.opened > ours.opened {
        ours.opened = theirs.opened;
        ours.version = theirs.version;
        ours.dimension = theirs.dimension;
        ours.views = theirs.views;
    }
    for (field, value) in theirs.extra {
        ours.extra.entry(field).or_insert(value);
    }
    for marker in theirs.markers {
        match ours.markers.iter().position(|m| m.id == marker.id) {
            Some(i) if marker.modified > ours.markers[i].modified => ours.markers[i] = marker,
            Some(_) => {}
            None if ours.markers.len() < MAX_ENTRIES => ours.markers.push(marker),
            None => {}
        }
    }
    for place in theirs.done {
        let same = |d: &DonePlace| d.dimension == place.dimension && d.place == place.place;
        match ours.done.iter().position(same) {
            Some(i) if place.modified > ours.done[i].modified => ours.done[i] = place,
            Some(_) => {}
            None if ours.done.len() < MAX_ENTRIES => ours.done.push(place),
            None => {}
        }
    }
}

/// A colour as `#rrggbb`.
fn colour_text([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// `#rrggbb`, in either case.
pub fn parse_colour(text: &str) -> Option<[u8; 3]> {
    let digits = text.strip_prefix('#')?;
    if digits.len() != 6 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

/// A world as the file's object, its fields in the spec's order and then the unknown ones.
fn world_json(world: &WorldData) -> Value {
    let mut object = Map::new();
    object.insert("seed".into(), json!(world.key.seed.to_string()));
    object.insert("save".into(), json!(world.key.save));
    object.insert("save_dir".into(), json!(world.save_dir));
    object.insert("name".into(), json!(world.name));
    object.insert("modified".into(), json!(world.modified));
    object.insert("opened".into(), json!(world.opened));
    object.insert("version".into(), json!(world.version));
    object.insert("dimension".into(), json!(world.dimension));
    let views: Map<String, Value> = world
        .views
        .iter()
        .map(|(dimension, view)| {
            (
                dimension.clone(),
                json!({"centre": view.centre, "scale": view.scale, "y": view.y}),
            )
        })
        .collect();
    object.insert("views".into(), Value::Object(views));
    object.insert(
        "markers".into(),
        Value::Array(world.markers.iter().map(marker_json).collect()),
    );
    object.insert(
        "done".into(),
        Value::Array(world.done.iter().map(done_json).collect()),
    );
    extend(&mut object, &world.extra);
    Value::Object(object)
}

/// A marker as the file's object.
fn marker_json(marker: &Marker) -> Value {
    let mut object = Map::new();
    object.insert("id".into(), json!(marker.id));
    object.insert("dimension".into(), json!(marker.dimension));
    object.insert("x".into(), json!(marker.x));
    object.insert("y".into(), json!(marker.y));
    object.insert("z".into(), json!(marker.z));
    object.insert("name".into(), json!(marker.name));
    object.insert("colour".into(), json!(colour_text(marker.colour)));
    object.insert("done".into(), json!(marker.done));
    object.insert("deleted".into(), json!(marker.deleted));
    object.insert("modified".into(), json!(marker.modified));
    extend(&mut object, &marker.extra);
    Value::Object(object)
}

/// A done place as the file's object.
fn done_json(done: &DonePlace) -> Value {
    let mut object = Map::new();
    object.insert("dimension".into(), json!(done.dimension));
    object.insert("kind".into(), json!(done.place.kind.name()));
    object.insert("id".into(), json!(done.place.id));
    object.insert("x".into(), json!(done.place.at[0]));
    object.insert("z".into(), json!(done.place.at[1]));
    object.insert("done".into(), json!(done.done));
    object.insert("modified".into(), json!(done.modified));
    extend(&mut object, &done.extra);
    Value::Object(object)
}

/// Adds the fields of `extra` that `object` doesn't have.
fn extend(object: &mut Map<String, Value>, extra: &Map<String, Value>) {
    for (field, value) in extra {
        object.entry(field.clone()).or_insert_with(|| value.clone());
    }
}

/// `text`, cut to `max` characters.
fn clip(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// Reading one object's fields, noting what's wrong at `path`.
struct Fields<'a> {
    /// Where the object is in the file, such as `worlds[2].markers[0]`, for problems.
    path: String,
    /// The fields not taken yet.
    map: Map<String, Value>,
    /// Where problems with optional fields are noted.
    problems: &'a mut Vec<String>,
}

/// Why a required field couldn't be read.
struct Skip(String);

impl Fields<'_> {
    /// A field, taken out of what's left; `None` if it's missing or null.
    fn take(&mut self, key: &str) -> Option<Value> {
        self.map.remove(key).filter(|v| !v.is_null())
    }

    /// A required field: missing or wrong, and the object is skipped.
    fn required<T>(
        &mut self,
        key: &str,
        read: impl FnOnce(&Value) -> Result<T, String>,
    ) -> Result<T, Skip> {
        match self.take(key) {
            None => Err(Skip(format!("{key} is missing"))),
            Some(value) => read(&value).map_err(|e| Skip(format!("{key}: {e}"))),
        }
    }

    /// An optional field: `None` if missing, and noted and `None` if it's wrong.
    fn optional<T>(
        &mut self,
        key: &str,
        read: impl FnOnce(&Value) -> Result<T, String>,
    ) -> Option<T> {
        let value = self.take(key)?;
        match read(&value) {
            Ok(read) => Some(read),
            Err(e) => {
                self.problems
                    .push(format!("{}: {key}: {e}; ignored", self.path));
                None
            }
        }
    }

    /// What's left: the fields the spec doesn't name.
    fn rest(self) -> Map<String, Value> {
        self.map
    }
}

/// An integer in `range`.
fn integer(v: &Value, range: std::ops::RangeInclusive<i64>) -> Result<i64, String> {
    v.as_i64().filter(|n| range.contains(n)).ok_or_else(|| {
        format!(
            "{v} is not an integer from {} to {}",
            range.start(),
            range.end()
        )
    })
}

/// A block coordinate, within `MAX_COORD` of 0.
fn coordinate(v: &Value) -> Result<i32, String> {
    integer(v, -MAX_COORD..=MAX_COORD).map(|n| n as i32)
}

/// A time in milliseconds since the Unix epoch.
fn time(v: &Value) -> Result<i64, String> {
    integer(v, 0..=MAX_TIME)
}

/// A reader of strings of at most `max` characters.
fn text(max: usize) -> impl Fn(&Value) -> Result<String, String> {
    move |v| match v.as_str() {
        Some(text) if text.chars().count() <= max => Ok(text.to_string()),
        Some(_) => Err(format!("longer than {max} characters")),
        None => Err(format!("{v} is not a string")),
    }
}

/// `true` or `false`.
fn boolean(v: &Value) -> Result<bool, String> {
    v.as_bool()
        .ok_or_else(|| format!("{v} is not true or false"))
}

/// A marker's id: letters, digits, `-` and `_`.
fn marker_id(v: &Value) -> Result<String, String> {
    let id = text(MAX_ID)(v)?;
    let ok = !id.is_empty()
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if ok {
        Ok(id)
    } else {
        Err(format!("{v} is not 1 to {MAX_ID} of A-Z a-z 0-9 - _"))
    }
}

/// A dimension's view, fixed as settings' views are.
fn view(v: &Value) -> Result<View, String> {
    let object = v.as_object().ok_or("not an object")?;
    let centre = object
        .get("centre")
        .and_then(Value::as_array)
        .filter(|a| a.len() == 2)
        .and_then(|a| Some([a[0].as_f64()?, a[1].as_f64()?]))
        .ok_or("centre is not two numbers")?;
    let scale = object
        .get("scale")
        .and_then(Value::as_u64)
        .and_then(|scale| u32::try_from(scale).ok())
        .ok_or("scale is not an integer")?;
    let y = object
        .get("y")
        .and_then(Value::as_i64)
        .and_then(|y| i32::try_from(y).ok())
        .ok_or("y is not an integer")?;
    Ok(View { centre, scale, y }.sanitized())
}

/// The objects of an array field, each with its path; anything else is a problem.
fn objects(
    value: Option<Value>,
    path: &str,
    max: usize,
    problems: &mut Vec<String>,
) -> Vec<(String, Map<String, Value>)> {
    let items = match value {
        None | Some(Value::Null) => return Vec::new(),
        Some(Value::Array(items)) => items,
        Some(v) => {
            problems.push(format!("{path}: {} is not an array; ignored", kind_of(&v)));
            return Vec::new();
        }
    };
    if items.len() > max {
        problems.push(format!(
            "{path}: {} entries, more than {max}; the rest skipped",
            items.len()
        ));
    }
    let mut out = Vec::new();
    for (i, item) in items.into_iter().take(max).enumerate() {
        match item {
            Value::Object(map) => out.push((format!("{path}[{i}]"), map)),
            v => problems.push(format!(
                "{path}[{i}]: {} is not an object; skipped",
                kind_of(&v)
            )),
        }
    }
    out
}

/// What a JSON value is, for problems.
fn kind_of(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

/// Reads a file's bytes leniently (`docs/specs/user-data.md#reading`).
pub fn parse(bytes: &[u8]) -> Read {
    let mut read = Read::default();
    let fail = |mut read: Read, why: String| {
        read.problems.push(why);
        read.unreadable = true;
        read
    };
    if bytes.len() as u64 > MAX_FILE {
        return fail(
            read,
            format!("the file is larger than {} MiB", MAX_FILE >> 20),
        );
    }
    let top = match serde_json::from_slice::<Value>(bytes) {
        Ok(Value::Object(top)) => top,
        Ok(v) => return fail(read, format!("the file is {}, not an object", kind_of(&v))),
        Err(e) => return fail(read, format!("the file isn't JSON: {e}")),
    };
    let problems = &mut read.problems;
    let mut top = Fields {
        path: String::new(),
        map: top,
        problems,
    };
    let format = match top.required("format", |v| integer(v, 1..=i64::MAX)) {
        Ok(f) => f,
        Err(Skip(why)) => return fail(read, why),
    };
    let worlds = top.take("worlds");
    let extra = top.rest();
    if format > FORMAT {
        read.read_only = true;
        read.problems.push(format!(
            "the file is format {format}, from a newer spyglass: it's read, but not written"
        ));
    }
    let mut data = UserData {
        worlds: Vec::new(),
        extra,
    };
    for (path, map) in objects(worlds, "worlds", MAX_WORLDS, &mut read.problems) {
        match parse_world(path.clone(), map, &mut read.problems) {
            Ok(world) => data.merge(UserData {
                worlds: vec![world],
                extra: Map::new(),
            }),
            Err(Skip(why)) => read.problems.push(format!("{path}: {why}; skipped")),
        }
    }
    read.data = data;
    read
}

/// A world's object, its markers and done places read one by one; duplicates merge as copies
/// from two files would.
fn parse_world(
    path: String,
    map: Map<String, Value>,
    problems: &mut Vec<String>,
) -> Result<WorldData, Skip> {
    let mut fields = Fields {
        path: path.clone(),
        map,
        problems,
    };
    let seed = fields.required("seed", |v| {
        v.as_str()
            .and_then(|seed| seed.parse::<i64>().ok())
            .ok_or_else(|| format!("{v} is not a 64-bit integer in a string"))
    })?;
    let save = fields.optional("save", text(MAX_TEXT));
    let save_dir = fields.optional("save_dir", text(MAX_PATH));
    let name = fields.optional("name", text(MAX_TEXT));
    let modified = fields.optional("modified", time).unwrap_or(0);
    let opened = fields.optional("opened", time).unwrap_or(0);
    let version = fields.optional("version", text(MAX_TEXT));
    let dimension = fields
        .optional("dimension", text(MAX_TEXT))
        .unwrap_or_else(|| Dimension::Overworld.id().to_string());
    let mut views = BTreeMap::new();
    match fields.take("views") {
        None => {}
        Some(Value::Object(map)) => {
            for (dimension, value) in map.into_iter().take(16) {
                match view(&value) {
                    Ok(view) => {
                        views.insert(clip(&dimension, MAX_TEXT), view);
                    }
                    Err(e) => fields
                        .problems
                        .push(format!("{path}.views.{dimension}: {e}; ignored")),
                }
            }
        }
        Some(v) => fields.problems.push(format!(
            "{path}.views: {} is not an object; ignored",
            kind_of(&v)
        )),
    }
    let markers_value = fields.take("markers");
    let done_value = fields.take("done");
    let problems = fields.problems;
    let extra = std::mem::take(&mut fields.map);
    let mut world = WorldData {
        key: WorldKey { seed, save },
        save_dir,
        name,
        modified,
        opened,
        version,
        dimension,
        views,
        markers: Vec::new(),
        done: Vec::new(),
        extra,
    };
    for (entry_path, map) in objects(
        markers_value,
        &format!("{path}.markers"),
        MAX_ENTRIES,
        problems,
    ) {
        match parse_marker(entry_path.clone(), map, problems) {
            Ok(marker) => match world.markers.iter_mut().find(|m| m.id == marker.id) {
                Some(earlier) if marker.modified > earlier.modified => *earlier = marker,
                Some(_) => {}
                None => world.markers.push(marker),
            },
            Err(Skip(why)) => problems.push(format!("{entry_path}: {why}; skipped")),
        }
    }
    for (entry_path, map) in objects(done_value, &format!("{path}.done"), MAX_ENTRIES, problems) {
        match parse_done(entry_path.clone(), map, problems) {
            Ok(done) => {
                let same = |earlier: &&mut DonePlace| {
                    earlier.dimension == done.dimension && earlier.place == done.place
                };
                match world.done.iter_mut().find(same) {
                    Some(earlier) if done.modified > earlier.modified => *earlier = done,
                    Some(_) => {}
                    None => world.done.push(done),
                }
            }
            Err(Skip(why)) => problems.push(format!("{entry_path}: {why}; skipped")),
        }
    }
    Ok(world)
}

/// A marker's object.
fn parse_marker(
    path: String,
    map: Map<String, Value>,
    problems: &mut Vec<String>,
) -> Result<Marker, Skip> {
    let mut fields = Fields {
        path,
        map,
        problems,
    };
    let id = fields.required("id", marker_id)?;
    let dimension = fields.required("dimension", text(MAX_TEXT))?;
    let x = fields.required("x", coordinate)?;
    let z = fields.required("z", coordinate)?;
    let y = fields.optional("y", coordinate);
    let name = fields.optional("name", text(MAX_TEXT)).unwrap_or_default();
    let colour = fields
        .optional("colour", |v| {
            v.as_str()
                .and_then(parse_colour)
                .ok_or_else(|| format!("{v} is not #rrggbb"))
        })
        .unwrap_or(COLOURS[0].1);
    let done = fields.optional("done", boolean).unwrap_or(false);
    let deleted = fields.optional("deleted", boolean).unwrap_or(false);
    let modified = fields.optional("modified", time).unwrap_or(0);
    Ok(Marker {
        id,
        dimension,
        x,
        y,
        z,
        name,
        colour,
        done,
        deleted,
        modified,
        extra: fields.rest(),
    })
}

/// A done place's object.
fn parse_done(
    path: String,
    map: Map<String, Value>,
    problems: &mut Vec<String>,
) -> Result<DonePlace, Skip> {
    let mut fields = Fields {
        path,
        map,
        problems,
    };
    let dimension = fields.required("dimension", text(MAX_TEXT))?;
    let kind = fields.required("kind", |v| match v.as_str() {
        Some("structure") => Ok(Kind::Structure),
        Some("biome") => Ok(Kind::Biome),
        Some("place") => Ok(Kind::Place),
        _ => Err(format!("{v} is not structure, biome or place")),
    })?;
    let x = fields.required("x", coordinate)?;
    let z = fields.required("z", coordinate)?;
    let id = fields.optional("id", text(MAX_TEXT)).unwrap_or_default();
    let done = fields.optional("done", boolean).unwrap_or(true);
    let modified = fields.optional("modified", time).unwrap_or(0);
    Ok(DonePlace {
        dimension,
        place: Place {
            kind,
            id,
            at: [x, z],
        },
        done,
        modified,
        extra: fields.rest(),
    })
}

/// The data file on disk and the data in memory: loaded once, written after changes.
pub struct Store {
    /// The file.
    path: PathBuf,
    /// What's in memory, the file's and this session's changes.
    pub data: UserData,
    /// What reading found wrong, for the user.
    pub problems: Vec<String>,
    /// Never written: a newer format, a file that couldn't be read for now, or no folder.
    pub read_only: bool,
    /// The file couldn't be read at all; it's moved aside before the first write.
    unreadable: bool,
    /// A hash of the file as last read or written, to see whether another viewer wrote it.
    seen: Option<u64>,
    /// Whether the file was copied to `.bak` this session, as it is before the first write.
    backed_up: bool,
    /// Changes not written yet.
    dirty: bool,
    /// When the file was last written, so writes come at most every `WRITE_EVERY`.
    last_write: Option<Instant>,
    /// The last write's failure.
    pub error: Option<String>,
}

/// The bytes' hash, to compare with the file's next time.
fn hash(bytes: &[u8]) -> u64 {
    // A fixed hasher, so the same bytes hash the same within a run.
    let mut hasher = std::hash::DefaultHasher::new();
    hasher.write(bytes);
    hasher.finish()
}

/// The file's bytes, `None` if there's no file, or an error.
fn read_bytes(path: &Path) -> Result<Option<Vec<u8>>, String> {
    let meta = match std::fs::metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("reading {}: {e}", path.display())),
    };
    if meta.len() > MAX_FILE {
        // Not read into memory: `parse` reports it from its length.
        return Ok(Some(vec![b' '; MAX_FILE as usize + 1]));
    }
    std::fs::read(path)
        .map(Some)
        .map_err(|e| format!("reading {}: {e}", path.display()))
}

impl Store {
    /// The data in `dir`'s file.
    pub fn open(dir: &Path) -> Store {
        let path = dir.join(FILE);
        let mut store = Store {
            path,
            data: UserData::default(),
            problems: Vec::new(),
            read_only: false,
            unreadable: false,
            seen: None,
            backed_up: false,
            dirty: false,
            last_write: None,
            error: None,
        };
        match read_bytes(&store.path) {
            Ok(None) => {}
            Ok(Some(bytes)) => {
                let read = parse(&bytes);
                store.seen = Some(hash(&bytes));
                store.data = read.data;
                store.problems = read.problems;
                store.read_only = read.read_only;
                store.unreadable = read.unreadable;
            }
            Err(e) => {
                // Unreadable for now (locked, or no permission): don't write over it.
                store.problems.push(e);
                store.read_only = true;
            }
        }
        store
    }

    /// A store with no folder to keep data in: nothing is written.
    pub fn detached() -> Store {
        let mut store = Store::open(Path::new(""));
        store.data = UserData::default();
        store.read_only = true;
        store.problems = vec!["there's no folder for user data, so markers aren't kept".into()];
        store
    }

    /// The file's path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Notes a change, to be written soon.
    pub fn changed(&mut self) {
        self.dirty = true;
    }

    /// Writes the file if there are changes and a second has passed since the last write, or
    /// at once with `now`.
    pub fn flush(&mut self, now: bool) {
        if !self.dirty || self.read_only {
            return;
        }
        if !now && self.last_write.is_some_and(|t| t.elapsed() < WRITE_EVERY) {
            return;
        }
        self.dirty = false;
        self.last_write = Some(Instant::now());
        self.error = self.write().err();
    }

    /// Writes the file: merges what another viewer wrote since, backs the old file up the
    /// first time (or moves it aside if it couldn't be read), and replaces it through a
    /// temporary file, so a crash never leaves half a file.
    fn write(&mut self) -> Result<(), String> {
        let on_disk = read_bytes(&self.path)?;
        if let Some(bytes) = &on_disk
            && Some(hash(bytes)) != self.seen
        {
            // Another viewer wrote it since: take its changes too.
            let read = parse(bytes);
            if read.read_only {
                self.read_only = true;
                self.problems.extend(read.problems);
                return Err("a newer spyglass wrote the data file; not writing over it".into());
            }
            self.unreadable = read.unreadable;
            self.data.merge(read.data);
        }
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
        }
        if on_disk.is_some() && (self.unreadable || !self.backed_up) {
            let (aside, how) = if self.unreadable {
                (with_suffix(&self.path, ".bad"), "moving")
            } else {
                (with_suffix(&self.path, ".bak"), "copying")
            };
            let done = if self.unreadable {
                std::fs::rename(&self.path, &aside)
            } else {
                std::fs::copy(&self.path, &aside).map(|_| ())
            };
            done.map_err(|e| format!("{how} the data file to {}: {e}", aside.display()))?;
            self.backed_up = true;
            self.unreadable = false;
        }
        let text = self.data.file_text();
        let temp = with_suffix(&self.path, ".tmp");
        std::fs::write(&temp, &text).map_err(|e| format!("writing {}: {e}", temp.display()))?;
        std::fs::rename(&temp, &self.path)
            .map_err(|e| format!("writing {}: {e}", self.path.display()))?;
        self.seen = Some(hash(text.as_bytes()));
        Ok(())
    }

    /// Merges the file at `path` in, and says what was skipped.
    pub fn import(&mut self, path: &Path) -> Result<(usize, Vec<String>), String> {
        let bytes = read_bytes(path)?.ok_or_else(|| format!("{} doesn't exist", path.display()))?;
        let read = parse(&bytes);
        if read.unreadable {
            return Err(read.problems.join("; "));
        }
        let worlds = read.data.worlds.len();
        self.data.merge(read.data);
        self.changed();
        Ok((worlds, read.problems))
    }

    /// Writes everything, or only `world`, to `path`.
    pub fn export(&self, path: &Path, world: Option<&WorldKey>) -> Result<(), String> {
        let mut data = match world {
            Some(key) => self.data.only(key),
            None => self.data.clone(),
        };
        std::fs::write(path, data.file_text())
            .map_err(|e| format!("writing {}: {e}", path.display()))
    }
}

/// `path` with `suffix` added to its file name.
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(seed: i64) -> WorldKey {
        WorldKey { seed, save: None }
    }

    /// A world with a named marker and a done village, in 26.3's Nether view.
    fn sample() -> UserData {
        let mut data = UserData::default();
        let w = data.world_entry(&key(-5));
        w.set_name(Some("Speedrun".into()));
        w.touch(Some("26.3"));
        w.dimension = Dimension::Nether.id().into();
        w.views.insert(
            Dimension::Nether.id().into(),
            View {
                centre: [100.5, -200.5],
                scale: 8,
                y: 64,
            },
        );
        let id = w.add_marker(Dimension::Overworld.id(), [120, -310], "Base".into());
        w.marker_mut(&id).unwrap().colour = COLOURS[3].1;
        let village = Place {
            kind: Kind::Structure,
            id: "minecraft:villages".into(),
            at: [128, -320],
        };
        w.set_done(Dimension::Overworld.id(), &village, true);
        let save = data.world_entry(&WorldKey {
            seed: i64::MIN,
            save: Some("My World".into()),
        });
        save.touch(Some("26.4-snapshot-6"));
        data
    }

    fn reparse(data: &mut UserData) -> Read {
        parse(data.file_text().as_bytes())
    }

    #[test]
    fn a_file_round_trips() {
        let mut data = sample();
        let read = reparse(&mut data);
        assert!(read.problems.is_empty(), "{:?}", read.problems);
        assert!(!read.read_only && !read.unreadable);
        assert_eq!(read.data, data);
        // Seeds are strings, so every i64 survives any JSON reader.
        assert!(data.file_text().contains(&format!("\"{}\"", i64::MIN)));
    }

    #[test]
    fn markers_and_done_places_survive_a_version_switch() {
        let mut data = sample();
        let w = data.world_mut(&key(-5)).unwrap();
        w.touch(Some("26.4-snapshot-6"));
        let read = reparse(&mut data).data;
        let w = read.world(&key(-5)).unwrap();
        assert_eq!(w.version.as_deref(), Some("26.4-snapshot-6"));
        assert_eq!(w.live_markers().count(), 1);
        let village = Place {
            kind: Kind::Structure,
            id: "minecraft:villages".into(),
            at: [128, -320],
        };
        assert!(w.is_done(Dimension::Overworld.id(), &village));
        assert!(!w.is_done(Dimension::Nether.id(), &village));
    }

    #[test]
    fn unknown_fields_are_kept() {
        let text = r##"{"format": 1, "theirs": [1, 2],
            "worlds": [{"seed": "7", "note": "hi", "markers": [
                {"id": "a", "dimension": "minecraft:overworld", "x": 1, "z": 2, "tag": {"k": 1.5}}],
              "done": [{"dimension": "minecraft:overworld", "kind": "place", "x": 0, "z": 0,
                "by": "script"}]}]}"##;
        let mut data = parse(text.as_bytes()).data;
        let again: Value = serde_json::from_str(&data.file_text()).unwrap();
        assert_eq!(again["theirs"], json!([1, 2]));
        assert_eq!(again["worlds"][0]["note"], "hi");
        assert_eq!(again["worlds"][0]["markers"][0]["tag"], json!({"k": 1.5}));
        assert_eq!(again["worlds"][0]["done"][0]["by"], "script");
    }

    #[test]
    fn a_newer_format_is_read_but_not_written() {
        let text = r#"{"format": 2, "worlds": [{"seed": "1", "name": "Kept"}]}"#;
        let read = parse(text.as_bytes());
        assert!(read.read_only);
        assert_eq!(read.data.worlds[0].name.as_deref(), Some("Kept"));

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE), text).unwrap();
        let mut store = Store::open(dir.path());
        assert!(store.read_only);
        store.data.world_entry(&key(2)).touch(None);
        store.changed();
        store.flush(true);
        assert_eq!(
            std::fs::read_to_string(dir.path().join(FILE)).unwrap(),
            text
        );
    }

    #[test]
    fn merging_twice_changes_nothing() {
        let mut ours = sample();
        let mut theirs = UserData::default();
        let w = theirs.world_entry(&key(-5));
        w.add_marker(Dimension::End.id(), [0, 0], "Gateway".into());
        theirs.world_entry(&key(9)).touch(Some("26.2"));
        ours.merge(theirs.clone());
        let once = ours.clone();
        ours.merge(theirs);
        assert_eq!(ours, once);
        assert_eq!(ours.world(&key(-5)).unwrap().live_markers().count(), 2);
        // Merging a file with itself, too.
        let copy = ours.clone();
        ours.merge(copy);
        assert_eq!(ours, once);
    }

    #[test]
    fn the_later_change_wins_and_deletions_stick() {
        let mut ours = sample();
        let mut theirs = ours.clone();
        // We delete the marker; they rename it, earlier.
        let id = ours.worlds[0].markers[0].id.clone();
        theirs
            .world_mut(&key(-5))
            .unwrap()
            .marker_mut(&id)
            .unwrap()
            .name = "Old".into();
        std::thread::sleep(std::time::Duration::from_millis(2));
        ours.world_mut(&key(-5))
            .unwrap()
            .marker_mut(&id)
            .unwrap()
            .delete();
        ours.merge(theirs.clone());
        assert_eq!(ours.world(&key(-5)).unwrap().live_markers().count(), 0);
        // Their side, merging ours, deletes it too.
        theirs.merge(ours.clone());
        assert_eq!(theirs.world(&key(-5)).unwrap().live_markers().count(), 0);
        // Not done any more, later, wins over done.
        let village = ours.worlds[0].done[0].place.clone();
        let mut undone = ours.clone();
        std::thread::sleep(std::time::Duration::from_millis(2));
        undone.worlds[0].set_done(Dimension::Overworld.id(), &village, false);
        ours.merge(undone);
        assert!(!ours.worlds[0].is_done(Dimension::Overworld.id(), &village));
    }

    #[test]
    fn two_writers_keep_each_others_markers() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = Store::open(dir.path());
        let mut b = Store::open(dir.path());
        a.data
            .world_entry(&key(1))
            .add_marker("minecraft:overworld", [1, 1], "A".into());
        a.changed();
        a.flush(true);
        b.data
            .world_entry(&key(1))
            .add_marker("minecraft:overworld", [2, 2], "B".into());
        b.changed();
        b.flush(true);
        assert_eq!(b.error, None);
        let read = Store::open(dir.path());
        let mut names: Vec<&str> = read.data.worlds[0]
            .live_markers()
            .map(|m| m.name.as_str())
            .collect();
        names.sort_unstable();
        assert_eq!(names, ["A", "B"]);
        // The first write of a run keeps the old file.
        assert!(dir.path().join(format!("{FILE}.bak")).exists());
    }

    #[test]
    fn import_and_export_carry_a_world_to_another_machine() {
        let here = tempfile::tempdir().unwrap();
        let there = tempfile::tempdir().unwrap();
        let mut a = Store::open(here.path());
        a.data = sample();
        let file = here.path().join("export.json");
        a.export(&file, None).unwrap();
        let mut b = Store::open(there.path());
        let (worlds, problems) = b.import(&file).unwrap();
        assert_eq!((worlds, problems.len()), (2, 0));
        assert_eq!(b.data.world(&key(-5)), a.data.world(&key(-5)));
        let once = b.data.clone();
        b.import(&file).unwrap();
        assert_eq!(b.data, once);
        // One world alone.
        a.export(&file, Some(&key(-5))).unwrap();
        let only = parse(&std::fs::read(&file).unwrap()).data;
        assert_eq!(only.worlds.len(), 1);
    }

    #[test]
    fn recent_worlds_are_pruned_but_not_kept_ones() {
        let mut data = UserData::default();
        for seed in 0..30 {
            data.world_entry(&key(seed)).opened = seed;
        }
        data.world_entry(&key(0))
            .set_name(Some("Oldest, but named".into()));
        data.world_entry(&key(1))
            .add_marker("minecraft:overworld", [0, 0], String::new());
        let read = reparse(&mut data).data;
        assert_eq!(read.worlds.len(), RECENT + 2);
        assert!(read.world(&key(0)).is_some() && read.world(&key(1)).is_some());
        assert!(read.world(&key(2)).is_none());
        assert!(read.world(&key(29)).is_some());
    }

    /// Each hand-made bad file loads what it can and says what it couldn't.
    #[test]
    fn bad_files_load_what_they_can() {
        let good_marker = r#"{"id": "ok", "dimension": "minecraft:overworld", "x": 1, "z": 2}"#;
        let cases: &[(&str, usize, &str)] = &[
            // Not a file spyglass can read at all.
            ("", 0, "isn't JSON"),
            ("{\"format\": 1, \"worlds\": [", 0, "isn't JSON"),
            ("[1, 2]", 0, "not an object"),
            ("{\"worlds\": []}", 0, "format is missing"),
            ("{\"format\": \"1\"}", 0, "format"),
            (&"[".repeat(100_000), 0, "isn't JSON"),
            // Worlds and entries skipped, the rest kept.
            (r#"{"format": 1, "worlds": 3}"#, 0, "not an array"),
            (
                r#"{"format": 1, "worlds": [1, {"seed": "1"}]}"#,
                1,
                "not an object",
            ),
            (
                r#"{"format": 1, "worlds": [{"seed": 1}, {"seed": "2"}]}"#,
                1,
                "seed",
            ),
            (
                r#"{"format": 1, "worlds": [{"seed": "99999999999999999999"}]}"#,
                0,
                "seed",
            ),
            (
                r#"{"format": 1, "worlds": [{"seed": "1", "name": 5, "opened": -1}]}"#,
                1,
                "name",
            ),
            (
                &format!(
                    r##"{{"format": 1, "worlds": [{{"seed": "1", "markers": [{good_marker},
                    {{"id": "x", "dimension": "minecraft:overworld", "x": 1e300, "z": 0}},
                    {{"id": "y", "dimension": "minecraft:overworld", "x": 40000000, "z": 0}},
                    {{"id": "", "dimension": "minecraft:overworld", "x": 0, "z": 0}},
                    {{"id": "z", "dimension": "minecraft:overworld", "x": 0, "z": 0,
                      "colour": "#zz0000"}},
                    {{"id": "ok", "dimension": "minecraft:overworld", "x": 5, "z": 5}},
                    {{"id": "n", "dimension": "minecraft:overworld", "x": "NaN", "z": 0}}]}}]}}"##
                ),
                1,
                "colour",
            ),
            (
                r#"{"format": 1, "worlds": [{"seed": "1", "done": [
                    {"dimension": "minecraft:overworld", "kind": "castle", "x": 0, "z": 0},
                    {"dimension": "minecraft:overworld", "kind": "biome", "x": 0}]}]}"#,
                1,
                "castle",
            ),
            (
                r#"{"format": 1, "worlds": [{"seed": "1", "views": {"minecraft:overworld":
                    {"centre": [1e308, -1e308], "scale": 3, "y": 0}, "x": 7}}]}"#,
                1,
                "views.x",
            ),
        ];
        for (text, worlds, says) in cases {
            let read = parse(text.as_bytes());
            assert_eq!(read.data.worlds.len(), *worlds, "{text:.80}");
            let all = read.problems.join("\n");
            assert!(all.contains(says), "{text:.80}: {all}");
        }
        // The markers that were fine, with the duplicate id kept once.
        let (text, _, _) = &cases[11];
        let read = parse(text.as_bytes());
        let world = &read.data.worlds[0];
        let ids: Vec<&str> = world.markers.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["ok", "z"]);
        assert_eq!(world.markers[1].colour, COLOURS[0].1);
        // A view out of range is fixed, not dropped.
        let (text, _, _) = &cases[13];
        let view = parse(text.as_bytes()).data.worlds[0].views["minecraft:overworld"];
        assert_eq!(view.scale, View::default().scale);
        assert!(view.centre.iter().all(|c| c.abs() <= 1e9));
    }

    #[test]
    fn huge_and_missing_files_are_handled() {
        let read = parse(&vec![b' '; MAX_FILE as usize + 1]);
        assert!(read.unreadable);
        // Too many markers: the first ones kept.
        let marker = r#"{"id": "IDX", "dimension": "minecraft:overworld", "x": 0, "z": 0}"#;
        let markers: Vec<String> = (0..MAX_ENTRIES + 5)
            .map(|i| marker.replace("IDX", &format!("m{i}")))
            .collect();
        let text = format!(
            r#"{{"format": 1, "worlds": [{{"seed": "1", "markers": [{}]}}]}}"#,
            markers.join(",")
        );
        let read = parse(text.as_bytes());
        assert_eq!(read.data.worlds[0].markers.len(), MAX_ENTRIES);
        assert!(read.problems[0].contains("more than"));

        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path());
        assert!(store.problems.is_empty() && store.data.worlds.is_empty());
    }

    #[test]
    fn an_unreadable_file_is_moved_aside_not_lost() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE), "not json").unwrap();
        let mut store = Store::open(dir.path());
        assert!(!store.problems.is_empty());
        store.data.world_entry(&key(3)).set_name(Some("New".into()));
        store.changed();
        store.flush(true);
        assert_eq!(store.error, None);
        assert_eq!(
            std::fs::read_to_string(dir.path().join(format!("{FILE}.bad"))).unwrap(),
            "not json"
        );
        let again = Store::open(dir.path());
        assert!(again.problems.is_empty());
        assert_eq!(again.data.worlds[0].name.as_deref(), Some("New"));
    }

    /// Bytes changed at random in a good file: never a panic, whatever loads.
    #[test]
    fn mutated_files_never_panic() {
        let good = sample().file_text().into_bytes();
        let mut state: u64 = 0x2545_f491_4f6c_dd1d;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let alphabet = b"{}[]\",:0123456789-.eE#truefalsn \\u";
        for _ in 0..3_000 {
            let mut bytes = good.clone();
            for _ in 0..1 + next() % 4 {
                let at = (next() % bytes.len() as u64) as usize;
                match next() % 3 {
                    0 => bytes[at] = alphabet[(next() % alphabet.len() as u64) as usize],
                    1 => {
                        bytes.remove(at);
                    }
                    _ => bytes.insert(at, (next() % 256) as u8),
                }
            }
            let mut read = parse(&bytes);
            // Whatever was read writes and reads back to itself.
            let text = read.data.file_text();
            let again = parse(text.as_bytes());
            assert!(again.problems.is_empty(), "{:?}", again.problems);
        }
    }
}
