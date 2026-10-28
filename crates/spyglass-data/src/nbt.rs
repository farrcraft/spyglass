//! Minecraft's NBT format: a reader for the binary tag trees in structure templates and world
//! saves (ADR 0013). It knows the format, not the meaning of any file.
//!
//! A file is one named tag, usually a compound, in big-endian binary, often gzip-compressed.
//! Strings are Java's "modified UTF-8" (`DataInput.readUTF`). See `docs/specs/bundle-format.md`.

use std::collections::BTreeMap;
use std::io::Read;

use flate2::read::GzDecoder;

/// The most lists and compounds nested in one another, as the game's `NbtAccounter` allows.
const MAX_DEPTH: usize = 512;

/// The most elements a list reserves room for before reading them. A list's length is only a
/// claim; past this, its `Vec` grows as elements actually arrive.
const LIST_RESERVE: usize = 1024;

/// A tag's value.
#[derive(Debug, Clone, PartialEq)]
pub enum Tag {
    /// `TAG_Byte` (1).
    Byte(i8),
    /// `TAG_Short` (2).
    Short(i16),
    /// `TAG_Int` (3).
    Int(i32),
    /// `TAG_Long` (4).
    Long(i64),
    /// `TAG_Float` (5).
    Float(f32),
    /// `TAG_Double` (6).
    Double(f64),
    /// `TAG_Byte_Array` (7).
    ByteArray(Vec<i8>),
    /// `TAG_String` (8).
    String(String),
    /// `TAG_List` (9): elements of one type.
    List(Vec<Tag>),
    /// `TAG_Compound` (10): named tags. A repeated name keeps the last value, as the game does.
    Compound(Compound),
    /// `TAG_Int_Array` (11).
    IntArray(Vec<i32>),
    /// `TAG_Long_Array` (12).
    LongArray(Vec<i64>),
}

/// A compound's tags, by name.
pub type Compound = BTreeMap<String, Tag>;

impl Tag {
    /// The compound, if this is one.
    pub fn as_compound(&self) -> Option<&Compound> {
        match self {
            Tag::Compound(c) => Some(c),
            _ => None,
        }
    }

    /// The list's elements, if this is a list.
    pub fn as_list(&self) -> Option<&[Tag]> {
        match self {
            Tag::List(l) => Some(l),
            _ => None,
        }
    }

    /// The string, if this is one.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Tag::String(s) => Some(s),
            _ => None,
        }
    }

    /// The value as an `int`, if this is a byte, short or int: `NumericTag.intValue()` without
    /// the narrowing of longs and floats.
    pub fn as_int(&self) -> Option<i32> {
        match *self {
            Tag::Byte(v) => Some(i32::from(v)),
            Tag::Short(v) => Some(i32::from(v)),
            Tag::Int(v) => Some(v),
            _ => None,
        }
    }

    /// The value as a `long`, if this is a byte, short, int or long.
    pub fn as_long(&self) -> Option<i64> {
        match *self {
            Tag::Long(v) => Some(v),
            _ => self.as_int().map(i64::from),
        }
    }

    /// The value as a `double`, if this is a float or double.
    pub fn as_double(&self) -> Option<f64> {
        match *self {
            Tag::Float(v) => Some(f64::from(v)),
            Tag::Double(v) => Some(v),
            _ => None,
        }
    }

    /// A child of a compound, by name.
    pub fn get(&self, name: &str) -> Option<&Tag> {
        self.as_compound()?.get(name)
    }
}

/// Why NBT couldn't be read. `path` names where, e.g. `blocks[3].pos`.
#[derive(Debug, thiserror::Error)]
pub enum NbtError {
    /// The gzip stream is malformed.
    #[error("not gzip-compressed NBT: {0}")]
    Gzip(std::io::Error),
    /// The data ends inside a tag.
    #[error("{path}: truncated")]
    Truncated {
        /// Where.
        path: String,
    },
    /// An unknown tag type, or `TAG_End` where a value belongs.
    #[error("{path}: unknown tag type {id}")]
    TagType {
        /// Where.
        path: String,
        /// The type byte.
        id: u8,
    },
    /// A string that isn't valid modified UTF-8.
    #[error("{path}: malformed string")]
    String {
        /// Where.
        path: String,
    },
    /// A negative length.
    #[error("{path}: negative length {len}")]
    Length {
        /// Where.
        path: String,
        /// The length read.
        len: i32,
    },
    /// Nesting deeper than 512.
    #[error("{path}: nested too deeply")]
    TooDeep {
        /// Where.
        path: String,
    },
    /// Bytes left after the root tag.
    #[error("{0} bytes after the root tag")]
    Trailing(usize),
    /// More data than the reader's limit, after decompression.
    #[error("more than {0} bytes uncompressed")]
    TooLarge(u64),
    /// A tree that would take more memory than the reader's limit.
    #[error("needs more than {0} bytes of memory")]
    Memory(u64),
}

/// Reads gzip-compressed NBT (`NbtIo.readCompressed`): the root tag's name and value.
pub fn read_compressed(bytes: &[u8]) -> Result<(String, Tag), NbtError> {
    read_compressed_limited(bytes, u64::MAX)
}

/// [`read_compressed`] for untrusted data: more than `max` bytes after decompression is an
/// error, and so is a tree that would take more than `max` bytes of memory, so a small file
/// can't expand to fill memory. (A tag takes several times the bytes it's stored in: a list of
/// a million bytes is a million [`Tag`]s.)
pub fn read_compressed_limited(bytes: &[u8], max: u64) -> Result<(String, Tag), NbtError> {
    let mut raw = Vec::new();
    GzDecoder::new(bytes)
        .take(max.saturating_add(1))
        .read_to_end(&mut raw)
        .map_err(NbtError::Gzip)?;
    if raw.len() as u64 > max {
        return Err(NbtError::TooLarge(max));
    }
    read_within(&raw, max)
}

/// Reads uncompressed NBT: one named root tag, and nothing after it.
pub fn read(bytes: &[u8]) -> Result<(String, Tag), NbtError> {
    read_within(bytes, u64::MAX)
}

/// [`read`], with an error if the tree would take more than `memory` bytes.
fn read_within(bytes: &[u8], memory: u64) -> Result<(String, Tag), NbtError> {
    let mut r = Reader::new(bytes, memory);
    let id = r.u8()?;
    let name = r.string()?;
    let tag = r.payload(id, 0)?;
    r.finish()?;
    Ok((name, tag))
}

/// The `TAG_Int` child `name` of uncompressed NBT's root compound, without building the tree:
/// `None` if the root isn't a compound or has no such int. The rest of the data is still
/// checked as [`read`] checks it, except that strings other than names aren't decoded.
pub fn read_root_int(bytes: &[u8], name: &str) -> Result<Option<i32>, NbtError> {
    // Skipping builds nothing, so needs no memory limit.
    let mut r = Reader::new(bytes, u64::MAX);
    let id = r.u8()?;
    r.skip_string()?;
    let mut found = None;
    if id == 10 {
        loop {
            let child = r.u8()?;
            if child == 0 {
                break;
            }
            let (child_name, segment) = r.name()?;
            r.path.push(segment);
            if child_name != name {
                r.skip(child, 1)?;
            } else if child == 3 {
                found = Some(r.i32()?);
            } else {
                // A repeated name keeps the last value, as in a compound.
                found = None;
                r.skip(child, 1)?;
            }
            r.path.pop();
        }
    } else {
        r.skip(id, 0)?;
    }
    r.finish()?;
    Ok(found)
}

/// One step of the path to the tag being read: a compound child's name, as the range of its
/// raw bytes in the input, or a list index. Paths are only rendered as text for errors.
#[derive(Clone, Copy)]
enum Segment {
    Name(usize, usize),
    Index(usize),
}

struct Reader<'b> {
    bytes: &'b [u8],
    pos: usize,
    path: Vec<Segment>,
    /// The memory the tree may take, and what's left of it.
    memory: u64,
    memory_left: u64,
}

impl<'b> Reader<'b> {
    fn new(bytes: &'b [u8], memory: u64) -> Self {
        Reader {
            bytes,
            pos: 0,
            path: Vec::new(),
            memory,
            memory_left: memory,
        }
    }

    /// Counts `bytes` of the tree's memory against the limit.
    fn charge(&mut self, bytes: usize) -> Result<(), NbtError> {
        self.memory_left = self
            .memory_left
            .checked_sub(bytes as u64)
            .ok_or(NbtError::Memory(self.memory))?;
        Ok(())
    }

    /// The path to the current tag, e.g. `blocks[3].pos`.
    fn path(&self) -> String {
        let mut out = String::new();
        for &segment in &self.path {
            if let Segment::Name(..) = segment
                && !out.is_empty()
            {
                out.push('.');
            }
            out.push_str(&self.segment_text(segment));
        }
        out
    }

    /// A segment as text. Names were checked when read, so they always decode.
    fn segment_text(&self, segment: Segment) -> String {
        match segment {
            Segment::Name(start, end) => name_text(&self.bytes[start..end]).unwrap_or_default(),
            Segment::Index(i) => format!("[{i}]"),
        }
    }

    fn finish(&self) -> Result<(), NbtError> {
        match self.bytes.len() - self.pos {
            0 => Ok(()),
            n => Err(NbtError::Trailing(n)),
        }
    }

    fn take(&mut self, n: usize) -> Result<&'b [u8], NbtError> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.bytes.len());
        let end = end.ok_or_else(|| NbtError::Truncated { path: self.path() })?;
        let out = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(out)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], NbtError> {
        Ok(self.take(N)?.try_into().expect("take returns N bytes"))
    }

    fn u8(&mut self) -> Result<u8, NbtError> {
        Ok(self.array::<1>()?[0])
    }

    fn i32(&mut self) -> Result<i32, NbtError> {
        Ok(i32::from_be_bytes(self.array()?))
    }

    fn len(&mut self) -> Result<usize, NbtError> {
        let len = self.i32()?;
        usize::try_from(len).map_err(|_| NbtError::Length {
            path: self.path(),
            len,
        })
    }

    /// A string's raw bytes: a `u16` byte length, then that many bytes.
    fn raw_string(&mut self) -> Result<&'b [u8], NbtError> {
        let len = usize::from(u16::from_be_bytes(self.array()?));
        self.take(len)
    }

    /// `DataInput.readUTF`, with any unpaired surrogate as U+FFFD.
    fn string(&mut self) -> Result<String, NbtError> {
        let bytes = self.raw_string()?;
        let units =
            decode_modified_utf8(bytes).ok_or_else(|| NbtError::String { path: self.path() })?;
        Ok(String::from_utf16_lossy(&units))
    }

    fn skip_string(&mut self) -> Result<(), NbtError> {
        self.raw_string().map(drop)
    }

    /// A compound child's name, and its path segment.
    fn name(&mut self) -> Result<(String, Segment), NbtError> {
        let bytes = self.raw_string()?;
        let text = name_text(bytes).ok_or_else(|| NbtError::String { path: self.path() })?;
        Ok((text, Segment::Name(self.pos - bytes.len(), self.pos)))
    }

    /// `NbtAccounter.pushDepth`, which only lists and compounds call: a tag of type `id` inside
    /// `depth` of them may be one more only if `depth` is under the limit.
    /// [`name`](Self::name)'s segment, with the name checked but not kept.
    #[inline(never)]
    fn skip_name(&mut self) -> Result<Segment, NbtError> {
        self.name().map(|(_, segment)| segment)
    }

    fn check_depth(&self, id: u8, depth: usize) -> Result<(), NbtError> {
        if matches!(id, 9 | 10) && depth >= MAX_DEPTH {
            return Err(NbtError::TooDeep { path: self.path() });
        }
        Ok(())
    }

    fn unknown(&self, id: u8) -> NbtError {
        NbtError::TagType {
            path: self.path(),
            id,
        }
    }

    /// A tag of type `id`, inside `depth` lists and compounds. Lists and compounds recurse
    /// through their own functions, and everything else is read by [`leaf`](Self::leaf), so each
    /// level of nesting costs a few small stack frames even in a debug build: the game allows
    /// 512 levels, which must fit in a thread's default stack.
    fn payload(&mut self, id: u8, depth: usize) -> Result<Tag, NbtError> {
        self.check_depth(id, depth)?;
        self.charge(size_of::<Tag>())?;
        match id {
            9 => self.list(depth).map(Tag::List),
            10 => self.compound(depth).map(Tag::Compound),
            _ => self.leaf(id),
        }
    }

    fn list(&mut self, depth: usize) -> Result<Vec<Tag>, NbtError> {
        let element = self.u8()?;
        let len = self.len()?;
        let mut items = Vec::with_capacity(len.min(LIST_RESERVE));
        // An empty list may say TAG_End as its type.
        if element != 0 || len != 0 {
            for i in 0..len {
                self.path.push(Segment::Index(i));
                items.push(self.payload(element, depth + 1)?);
                self.path.pop();
            }
        }
        Ok(items)
    }

    fn compound(&mut self, depth: usize) -> Result<Compound, NbtError> {
        let mut compound = Compound::new();
        loop {
            let child = self.u8()?;
            if child == 0 {
                return Ok(compound);
            }
            let (name, segment) = self.name()?;
            self.charge(size_of::<String>() + name.len())?;
            self.path.push(segment);
            let value = self.payload(child, depth + 1)?;
            self.path.pop();
            compound.insert(name, value);
        }
    }

    /// A tag that holds no other tags.
    #[inline(never)]
    fn leaf(&mut self, id: u8) -> Result<Tag, NbtError> {
        Ok(match id {
            1 => Tag::Byte(i8::from_be_bytes(self.array()?)),
            2 => Tag::Short(i16::from_be_bytes(self.array()?)),
            3 => Tag::Int(self.i32()?),
            4 => Tag::Long(i64::from_be_bytes(self.array()?)),
            5 => Tag::Float(f32::from_be_bytes(self.array()?)),
            6 => Tag::Double(f64::from_be_bytes(self.array()?)),
            7 => {
                let len = self.len()?;
                let bytes = self.take(len)?;
                self.charge(len)?;
                Tag::ByteArray(bytes.iter().map(|&b| b as i8).collect())
            }
            8 => {
                let text = self.string()?;
                self.charge(text.len())?;
                Tag::String(text)
            }
            11 => {
                let len = self.len()?;
                let bytes = self.take(len.saturating_mul(4))?;
                self.charge(bytes.len())?;
                Tag::IntArray(
                    bytes
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|&c| i32::from_be_bytes(c))
                        .collect(),
                )
            }
            12 => {
                let len = self.len()?;
                let bytes = self.take(len.saturating_mul(8))?;
                self.charge(bytes.len())?;
                Tag::LongArray(
                    bytes
                        .as_chunks::<8>()
                        .0
                        .iter()
                        .map(|&c| i64::from_be_bytes(c))
                        .collect(),
                )
            }
            _ => return Err(self.unknown(id)),
        })
    }

    /// [`payload`](Self::payload) without building anything.
    fn skip(&mut self, id: u8, depth: usize) -> Result<(), NbtError> {
        self.check_depth(id, depth)?;
        match id {
            9 => {
                let element = self.u8()?;
                let len = self.len()?;
                if element != 0 || len != 0 {
                    for i in 0..len {
                        self.path.push(Segment::Index(i));
                        self.skip(element, depth + 1)?;
                        self.path.pop();
                    }
                }
                Ok(())
            }
            10 => loop {
                let child = self.u8()?;
                if child == 0 {
                    return Ok(());
                }
                let segment = self.skip_name()?;
                self.path.push(segment);
                self.skip(child, depth + 1)?;
                self.path.pop();
            },
            _ => self.skip_leaf(id),
        }
    }

    /// [`leaf`](Self::leaf) without building anything.
    #[inline(never)]
    fn skip_leaf(&mut self, id: u8) -> Result<(), NbtError> {
        match id {
            1 => self.take(1).map(drop),
            2 => self.take(2).map(drop),
            3 | 5 => self.take(4).map(drop),
            4 | 6 => self.take(8).map(drop),
            7 => {
                let len = self.len()?;
                self.take(len).map(drop)
            }
            8 => self.skip_string(),
            11 => {
                let len = self.len()?;
                self.take(len.saturating_mul(4)).map(drop)
            }
            12 => {
                let len = self.len()?;
                self.take(len.saturating_mul(8)).map(drop)
            }
            _ => Err(self.unknown(id)),
        }
    }
}

/// A compound child's name: its modified UTF-8, which must be well-formed UTF-16 too. Names
/// key a map, so an unpaired surrogate is an error rather than U+FFFD, which could make two
/// names one.
fn name_text(bytes: &[u8]) -> Option<String> {
    String::from_utf16(&decode_modified_utf8(bytes)?).ok()
}

/// Java's modified UTF-8 as `DataInput.readUTF` decodes it, into UTF-16 units: one byte up to
/// `0x7F` (writers send `U+0000` as `C0 80`, but a raw zero is accepted), then two- and
/// three-byte forms, surrogates included. `None` if malformed, as `readUTF` throws.
fn decode_modified_utf8(bytes: &[u8]) -> Option<Vec<u16>> {
    let mut units = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        let cont = |j: usize| match bytes.get(j) {
            Some(&c) if c & 0xC0 == 0x80 => Some(u16::from(c & 0x3F)),
            _ => None,
        };
        let (unit, width) = match b {
            0x00..=0x7F => (u16::from(b), 1),
            0xC0..=0xDF => (u16::from(b & 0x1F) << 6 | cont(i + 1)?, 2),
            0xE0..=0xEF => (
                u16::from(b & 0x0F) << 12 | cont(i + 1)? << 6 | cont(i + 2)?,
                3,
            ),
            _ => return None,
        };
        units.push(unit);
        i += width;
    }
    Some(units)
}

impl Tag {
    /// The tag as JSON, the way the game's codecs see NBT through `NbtOps`: numbers for every
    /// numeric tag (booleans are bytes), lists and arrays as arrays, compounds as objects.
    pub fn to_json(&self) -> serde_json::Value {
        use serde_json::{Number, Value};
        match self {
            Tag::Byte(v) => Value::from(*v),
            Tag::Short(v) => Value::from(*v),
            Tag::Int(v) => Value::from(*v),
            Tag::Long(v) => Value::from(*v),
            Tag::Float(v) => Number::from_f64(f64::from(*v)).map_or(Value::Null, Value::Number),
            Tag::Double(v) => Number::from_f64(*v).map_or(Value::Null, Value::Number),
            Tag::String(s) => Value::from(s.as_str()),
            Tag::ByteArray(a) => Value::from(a.clone()),
            Tag::IntArray(a) => Value::from(a.clone()),
            Tag::LongArray(a) => Value::from(a.clone()),
            Tag::List(l) => Value::Array(l.iter().map(Tag::to_json).collect()),
            Tag::Compound(c) => {
                Value::Object(c.iter().map(|(k, v)| (k.clone(), v.to_json())).collect())
            }
        }
    }

    /// The tag's type byte.
    pub fn id(&self) -> u8 {
        match self {
            Tag::Byte(_) => 1,
            Tag::Short(_) => 2,
            Tag::Int(_) => 3,
            Tag::Long(_) => 4,
            Tag::Float(_) => 5,
            Tag::Double(_) => 6,
            Tag::ByteArray(_) => 7,
            Tag::String(_) => 8,
            Tag::List(_) => 9,
            Tag::Compound(_) => 10,
            Tag::IntArray(_) => 11,
            Tag::LongArray(_) => 12,
        }
    }
}

/// Writes uncompressed NBT (`NbtIo.write`): one named root tag. A list's element type is its
/// first element's (`TAG_End` when empty); strings are modified UTF-8 (`DataOutput.writeUTF`)
/// and must fit in 65,535 bytes. The inverse of [`read`].
pub fn write(name: &str, root: &Tag) -> Vec<u8> {
    let mut out = vec![root.id()];
    write_string(name, &mut out);
    write_payload(root, &mut out);
    out
}

/// Writes gzip-compressed NBT (`NbtIo.writeCompressed`), the inverse of [`read_compressed`].
pub fn write_compressed(name: &str, root: &Tag) -> Vec<u8> {
    use std::io::Write;
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder
        .write_all(&write(name, root))
        .expect("writing to a Vec doesn't fail");
    encoder.finish().expect("writing to a Vec doesn't fail")
}

fn write_string(s: &str, out: &mut Vec<u8>) {
    let mut bytes = Vec::with_capacity(s.len());
    for unit in s.encode_utf16() {
        match unit {
            0x0001..=0x007F => bytes.push(unit as u8),
            0x0000 | 0x0080..=0x07FF => {
                bytes.extend([0xC0 | (unit >> 6) as u8, 0x80 | (unit & 0x3F) as u8]);
            }
            _ => bytes.extend([
                0xE0 | (unit >> 12) as u8,
                0x80 | (unit >> 6 & 0x3F) as u8,
                0x80 | (unit & 0x3F) as u8,
            ]),
        }
    }
    let len = u16::try_from(bytes.len()).expect("NBT strings are at most 65,535 bytes");
    out.extend(len.to_be_bytes());
    out.extend(bytes);
}

fn write_payload(tag: &Tag, out: &mut Vec<u8>) {
    let len = |n: usize| {
        i32::try_from(n)
            .expect("NBT lengths fit in an int")
            .to_be_bytes()
    };
    match tag {
        Tag::Byte(v) => out.extend(v.to_be_bytes()),
        Tag::Short(v) => out.extend(v.to_be_bytes()),
        Tag::Int(v) => out.extend(v.to_be_bytes()),
        Tag::Long(v) => out.extend(v.to_be_bytes()),
        Tag::Float(v) => out.extend(v.to_be_bytes()),
        Tag::Double(v) => out.extend(v.to_be_bytes()),
        Tag::ByteArray(a) => {
            out.extend(len(a.len()));
            out.extend(a.iter().map(|&b| b as u8));
        }
        Tag::String(s) => write_string(s, out),
        Tag::List(items) => {
            out.push(items.first().map_or(0, Tag::id));
            out.extend(len(items.len()));
            items.iter().for_each(|t| write_payload(t, out));
        }
        Tag::Compound(c) => {
            for (name, t) in c {
                out.push(t.id());
                write_string(name, out);
                write_payload(t, out);
            }
            out.push(0);
        }
        Tag::IntArray(a) => {
            out.extend(len(a.len()));
            a.iter().for_each(|v| out.extend(v.to_be_bytes()));
        }
        Tag::LongArray(a) => {
            out.extend(len(a.len()));
            a.iter().for_each(|v| out.extend(v.to_be_bytes()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn written_nbt_reads_back() {
        let root = Tag::Compound(Compound::from([
            ("b".to_string(), Tag::Byte(-1)),
            ("s".to_string(), Tag::Short(300)),
            ("l".to_string(), Tag::Long(i64::MIN)),
            ("f".to_string(), Tag::Float(0.5)),
            ("d".to_string(), Tag::Double(-2.25)),
            ("bytes".to_string(), Tag::ByteArray(vec![1, -2])),
            ("text".to_string(), Tag::String("nul\0, é, 😀".into())),
            ("empty".to_string(), Tag::List(vec![])),
            ("ints".to_string(), Tag::IntArray(vec![1, -1])),
            ("longs".to_string(), Tag::LongArray(vec![7])),
            (
                "list".to_string(),
                Tag::List(vec![
                    Tag::Compound(Compound::new()),
                    Tag::Compound(Compound::new()),
                ]),
            ),
        ]));
        assert_eq!(
            read(&write("root", &root)).unwrap(),
            ("root".to_string(), root.clone())
        );
        assert_eq!(
            read_compressed(&write_compressed("", &root)).unwrap().1,
            root
        );
    }

    /// A named tag header: type, then a modified-UTF-8 name.
    fn named(id: u8, name: &str) -> Vec<u8> {
        let mut out = vec![id];
        out.extend((name.len() as u16).to_be_bytes());
        out.extend(name.as_bytes());
        out
    }

    #[test]
    fn reads_a_template_shaped_compound() {
        // {size: [3, 4, 5], blocks: [{pos: [0, 1, 2], state: 0}], name: "a"}
        let mut b = named(10, "");
        b.extend(named(9, "size"));
        b.push(3);
        b.extend(3_i32.to_be_bytes());
        for v in [3_i32, 4, 5] {
            b.extend(v.to_be_bytes());
        }
        b.extend(named(9, "blocks"));
        b.push(10);
        b.extend(1_i32.to_be_bytes());
        b.extend(named(9, "pos"));
        b.push(3);
        b.extend(3_i32.to_be_bytes());
        for v in [0_i32, 1, 2] {
            b.extend(v.to_be_bytes());
        }
        b.extend(named(3, "state"));
        b.extend(0_i32.to_be_bytes());
        b.push(0);
        b.extend(named(8, "name"));
        b.extend(1_u16.to_be_bytes());
        b.push(b'a');
        b.push(0);

        let (name, root) = read(&b).unwrap();
        assert_eq!(name, "");
        let size: Vec<i32> = root
            .get("size")
            .unwrap()
            .as_list()
            .unwrap()
            .iter()
            .map(|t| t.as_int().unwrap())
            .collect();
        assert_eq!(size, [3, 4, 5]);
        let block = &root.get("blocks").unwrap().as_list().unwrap()[0];
        assert_eq!(block.get("state"), Some(&Tag::Int(0)));
        assert_eq!(root.get("name").and_then(Tag::as_str), Some("a"));
    }

    #[test]
    fn decodes_modified_utf8() {
        // "\0" as C0 80, "é" as two bytes, and U+1F600 as a surrogate pair of three-byte units.
        let text: &[u8] = &[0xC0, 0x80, 0xC3, 0xA9, 0xED, 0xA0, 0xBD, 0xED, 0xB8, 0x80];
        let mut b = vec![8];
        b.extend(0_u16.to_be_bytes());
        b.extend((text.len() as u16).to_be_bytes());
        b.extend(text);
        assert_eq!(read(&b).unwrap().1, Tag::String("\0é\u{1F600}".into()));
        // Four-byte UTF-8, a stray continuation byte and a cut-off character aren't modified
        // UTF-8; `readUTF` throws on each.
        for bad in [&[0xF0, 0x9F, 0x98, 0x80][..], &[0x80], &[0xE2, 0x82]] {
            assert!(
                matches!(read(&string_tag(bad)), Err(NbtError::String { .. })),
                "{bad:?}"
            );
        }
    }

    /// A root string tag, unnamed, holding `bytes`.
    fn string_tag(bytes: &[u8]) -> Vec<u8> {
        let mut b = named(8, "");
        b.extend((bytes.len() as u16).to_be_bytes());
        b.extend(bytes);
        b
    }

    #[test]
    fn strings_read_as_java_reads_them() {
        // `readUTF` takes a raw zero byte as U+0000, as it does `C0 80`.
        assert_eq!(
            read(&string_tag(&[b'a', 0x00, b'b'])).unwrap().1,
            Tag::String("a\0b".into())
        );
        // A Java string may hold an unpaired surrogate, which a Rust one can't: a value reads
        // with U+FFFD in its place, rather than failing the whole file.
        let lone = [b'x', 0xED, 0xA0, 0xBD];
        assert_eq!(
            read(&string_tag(&lone)).unwrap().1,
            Tag::String("x\u{FFFD}".into())
        );
        // A name with one is an error: two names could otherwise become one.
        let mut b = vec![10, 0, 0, 8];
        b.extend((lone.len() as u16).to_be_bytes());
        b.extend(lone);
        b.extend([0, 0, 0]);
        assert!(matches!(read(&b), Err(NbtError::String { .. })));
    }

    /// `n` lists nested in one another, the innermost empty.
    fn nested_lists(n: usize) -> Vec<u8> {
        let mut b = named(9, "");
        for _ in 1..n {
            b.push(9);
            b.extend(1_i32.to_be_bytes());
        }
        b.push(0);
        b.extend(0_i32.to_be_bytes());
        b
    }

    /// `n` compounds nested in one another, each the only child, named `c`, of the one outside.
    fn nested_compounds(n: usize) -> Vec<u8> {
        let mut b = named(10, "");
        for _ in 1..n {
            b.extend(named(10, "c"));
        }
        b.resize(b.len() + n, 0);
        b
    }

    #[test]
    fn nesting_stops_where_the_games_does() {
        // `NbtAccounter` counts lists and compounds: 512 of them read, 513 don't. The deepest
        // must fit in 1 MiB of stack, a Windows main thread's, even in a debug build.
        std::thread::Builder::new()
            .stack_size(1 << 20)
            .spawn(|| {
                for nested in [nested_lists, nested_compounds] {
                    assert!(read(&nested(512)).is_ok());
                    assert!(read_root_int(&nested(512), "x").is_ok());
                    assert!(matches!(read(&nested(513)), Err(NbtError::TooDeep { .. })));
                    assert!(matches!(
                        read_root_int(&nested(513), "x"),
                        Err(NbtError::TooDeep { .. })
                    ));
                }
            })
            .unwrap()
            .join()
            .unwrap();
        // A number in the 512th is fine: only containers count.
        let mut b = named(9, "");
        for _ in 1..512 {
            b.push(9);
            b.extend(1_i32.to_be_bytes());
        }
        b.push(3);
        b.extend(1_i32.to_be_bytes());
        b.extend(7_i32.to_be_bytes());
        assert!(read(&b).is_ok());
    }

    #[test]
    fn list_lengths_are_claims_not_reservations() {
        // 500 nested lists that each claim 2^31 - 1 elements, around 4 MB of bytes: reserving
        // even 4 MB of elements per list would ask for 64 GB and abort.
        let mut b = named(9, "");
        for _ in 0..500 {
            b.push(9);
            b.extend(i32::MAX.to_be_bytes());
        }
        b.push(1);
        b.extend(i32::MAX.to_be_bytes());
        b.resize(b.len() + (4 << 20), 0);
        assert!(matches!(read(&b), Err(NbtError::Truncated { .. })));
        // Limited, the innermost list's four million tags (over 100 MB) exceed the limit
        // before the data runs out.
        let gz = write_compressed_raw(&b);
        assert!(matches!(
            read_compressed_limited(&gz, 64 << 20),
            Err(NbtError::Memory(_))
        ));
    }

    #[test]
    fn limited_reads_count_the_trees_memory() {
        // A million-byte list is 1 MB stored, but a million tags in memory.
        let mut b = named(9, "");
        b.push(1);
        b.extend(1_000_000_i32.to_be_bytes());
        b.resize(b.len() + 1_000_000, 0);
        let gz = write_compressed_raw(&b);
        assert!(matches!(
            read_compressed_limited(&gz, 2_000_000),
            Err(NbtError::Memory(2_000_000))
        ));
        assert!(read_compressed_limited(&gz, 64 << 20).is_ok());
        assert!(read(&b).is_ok());
    }

    #[test]
    fn json_as_the_codecs_see_it() {
        let c = Tag::Compound(Compound::from([
            ("features".to_string(), Tag::Byte(0)),
            ("type".to_string(), Tag::String("minecraft:flat".into())),
            (
                "ys".to_string(),
                Tag::List(vec![Tag::Int(-64), Tag::Int(320)]),
            ),
        ]));
        assert_eq!(
            c.to_json(),
            serde_json::json!({"features": 0, "type": "minecraft:flat", "ys": [-64, 320]})
        );
    }

    #[test]
    fn damaged_data_is_an_error_not_a_panic() {
        let root = Tag::Compound(Compound::from([
            ("DataVersion".to_string(), Tag::Int(5023)),
            (
                "list".to_string(),
                Tag::List(vec![
                    Tag::Compound(Compound::from([(
                        "name".to_string(),
                        Tag::String("é\u{0}😀".into()),
                    )])),
                    Tag::Compound(Compound::new()),
                ]),
            ),
            ("bytes".to_string(), Tag::ByteArray(vec![1, -2, 3])),
            ("ints".to_string(), Tag::IntArray(vec![7, -8])),
            ("longs".to_string(), Tag::LongArray(vec![i64::MIN])),
            ("d".to_string(), Tag::Double(0.5)),
        ]));
        let bytes = write("", &root);
        assert_eq!(read(&bytes).unwrap().1, root);
        // Every truncation fails cleanly.
        for len in 0..bytes.len() {
            assert!(read(&bytes[..len]).is_err(), "{len}");
            let _ = read_root_int(&bytes[..len], "DataVersion");
        }
        // Every byte, replaced by values that break lengths, types and strings, reads or fails;
        // nothing panics or allocates wildly.
        for i in 0..bytes.len() {
            for v in [0x00, 0x01, 0x0c, 0x0d, 0x7f, 0x80, 0xc0, 0xed, 0xff] {
                let mut damaged = bytes.clone();
                damaged[i] = v;
                let _ = read(&damaged);
                let _ = read_root_int(&damaged, "DataVersion");
            }
        }
    }

    #[test]
    fn list_paths_name_the_index() {
        // {a: [{}, {b: <truncated int>}]}
        let mut b = named(10, "");
        b.extend(named(9, "a"));
        b.push(10);
        b.extend(2_i32.to_be_bytes());
        b.push(0);
        b.extend(named(3, "b"));
        b.extend([0, 0]);
        match read(&b) {
            Err(NbtError::Truncated { path }) => assert_eq!(path, "a[1].b"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn root_ints_without_the_tree() {
        let root = Tag::Compound(Compound::from([
            (
                "Level".to_string(),
                Tag::List(vec![Tag::Int(1), Tag::Int(2)]),
            ),
            ("Name".to_string(), Tag::String("x".into())),
            ("DataVersion".to_string(), Tag::Int(5023)),
            ("Short".to_string(), Tag::Short(4)),
        ]));
        let bytes = write("", &root);
        assert_eq!(read_root_int(&bytes, "DataVersion").unwrap(), Some(5023));
        // Only an int counts, and only a child of the root.
        assert_eq!(read_root_int(&bytes, "Short").unwrap(), None);
        assert_eq!(read_root_int(&bytes, "Missing").unwrap(), None);
        assert_eq!(read_root_int(&write("", &Tag::Int(3)), "x").unwrap(), None);
        // A repeated name keeps the last value.
        let mut b = named(10, "");
        b.extend(named(3, "v"));
        b.extend(1_i32.to_be_bytes());
        b.extend(named(3, "v"));
        b.extend(2_i32.to_be_bytes());
        b.push(0);
        assert_eq!(read_root_int(&b, "v").unwrap(), Some(2));
        assert_eq!(read(&b).unwrap().1.get("v"), Some(&Tag::Int(2)));
        b.pop();
        b.extend(named(8, "v"));
        b.extend([0, 0, 0]);
        assert_eq!(read_root_int(&b, "v").unwrap(), None);
        // The rest is still checked.
        assert!(matches!(
            read_root_int(&bytes[..bytes.len() - 1], "DataVersion"),
            Err(NbtError::Truncated { .. })
        ));
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(matches!(
            read_root_int(&trailing, "DataVersion"),
            Err(NbtError::Trailing(1))
        ));
    }

    #[test]
    fn errors_name_the_path() {
        let mut b = named(10, "");
        b.extend(named(10, "outer"));
        b.extend(named(3, "x"));
        b.extend([0, 0]);
        match read(&b) {
            Err(NbtError::Truncated { path }) => assert_eq!(path, "outer.x"),
            other => panic!("{other:?}"),
        }
        let mut b = named(10, "");
        b.extend(named(42, "x"));
        assert!(matches!(read(&b), Err(NbtError::TagType { id: 42, .. })));
        let mut b = named(1, "");
        b.extend([7, 0]);
        assert!(matches!(read(&b), Err(NbtError::Trailing(1))));
    }

    #[test]
    fn empty_list_may_have_end_type() {
        let mut b = named(9, "");
        b.push(0);
        b.extend(0_i32.to_be_bytes());
        assert_eq!(read(&b).unwrap().1, Tag::List(vec![]));
    }

    #[test]
    fn reads_gzip() {
        use flate2::{Compression, write::GzEncoder};
        use std::io::Write;
        let mut b = named(3, "n");
        b.extend(7_i32.to_be_bytes());
        let mut gz = GzEncoder::new(Vec::new(), Compression::default());
        gz.write_all(&b).unwrap();
        let (name, tag) = read_compressed(&gz.finish().unwrap()).unwrap();
        assert_eq!((name.as_str(), tag), ("n", Tag::Int(7)));
        assert!(matches!(
            read_compressed(b"not gzip"),
            Err(NbtError::Gzip(_))
        ));
    }

    #[test]
    fn limited_reads_stop_at_the_limit() {
        // A megabyte of zeros inside a byte array compresses to about a kilobyte.
        let mut b = named(7, "");
        b.extend(1_000_000_i32.to_be_bytes());
        b.resize(b.len() + 1_000_000, 0);
        let gz = write_compressed_raw(&b);
        assert!(gz.len() < 10_000);
        assert!(matches!(
            read_compressed_limited(&gz, 100_000),
            Err(NbtError::TooLarge(100_000))
        ));
        // The limit covers the decompressed bytes and the array they become, with room for the
        // tag around it.
        assert!(read_compressed_limited(&gz, b.len() as u64 + 1000).is_ok());
    }

    fn write_compressed_raw(bytes: &[u8]) -> Vec<u8> {
        use flate2::{Compression, write::GzEncoder};
        use std::io::Write;
        let mut gz = GzEncoder::new(Vec::new(), Compression::default());
        gz.write_all(bytes).unwrap();
        gz.finish().unwrap()
    }
}
