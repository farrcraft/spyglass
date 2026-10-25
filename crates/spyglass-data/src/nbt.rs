//! Minecraft's NBT format: a reader for the binary tag trees in structure templates and world
//! saves (ADR 0013). It knows the format, not the meaning of any file.
//!
//! A file is one named tag, usually a compound, in big-endian binary, often gzip-compressed.
//! Strings are Java's "modified UTF-8" (`DataInput.readUTF`). See `docs/specs/bundle-format.md`.

use std::collections::BTreeMap;
use std::io::Read;

use flate2::read::GzDecoder;

/// The deepest nesting accepted, as the game's `NbtAccounter` allows.
const MAX_DEPTH: usize = 512;

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
}

/// Reads gzip-compressed NBT (`NbtIo.readCompressed`): the root tag's name and value.
pub fn read_compressed(bytes: &[u8]) -> Result<(String, Tag), NbtError> {
    let mut raw = Vec::new();
    GzDecoder::new(bytes)
        .read_to_end(&mut raw)
        .map_err(NbtError::Gzip)?;
    read(&raw)
}

/// Reads uncompressed NBT: one named root tag, and nothing after it.
pub fn read(bytes: &[u8]) -> Result<(String, Tag), NbtError> {
    let mut r = Reader { bytes, pos: 0 };
    let id = r.u8("")?;
    let name = r.string("")?;
    let tag = r.payload(id, "", 0)?;
    match bytes.len() - r.pos {
        0 => Ok((name, tag)),
        n => Err(NbtError::Trailing(n)),
    }
}

struct Reader<'b> {
    bytes: &'b [u8],
    pos: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize, path: &str) -> Result<&[u8], NbtError> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.bytes.len());
        let end = end.ok_or_else(|| NbtError::Truncated {
            path: path.to_string(),
        })?;
        let out = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(out)
    }

    fn array<const N: usize>(&mut self, path: &str) -> Result<[u8; N], NbtError> {
        Ok(self
            .take(N, path)?
            .try_into()
            .expect("take returns N bytes"))
    }

    fn u8(&mut self, path: &str) -> Result<u8, NbtError> {
        Ok(self.array::<1>(path)?[0])
    }

    fn i32(&mut self, path: &str) -> Result<i32, NbtError> {
        Ok(i32::from_be_bytes(self.array(path)?))
    }

    fn len(&mut self, path: &str) -> Result<usize, NbtError> {
        let len = self.i32(path)?;
        usize::try_from(len).map_err(|_| NbtError::Length {
            path: path.to_string(),
            len,
        })
    }

    /// `DataInput.readUTF`: a `u16` byte length, then modified UTF-8: `U+0000` as two bytes, and
    /// characters outside the BMP as two three-byte surrogates.
    fn string(&mut self, path: &str) -> Result<String, NbtError> {
        let len = usize::from(u16::from_be_bytes(self.array(path)?));
        let bytes = self.take(len, path)?;
        let bad = || NbtError::String {
            path: path.to_string(),
        };
        let mut units = Vec::with_capacity(len);
        let mut i = 0;
        while i < bytes.len() {
            let b = bytes[i];
            let cont = |j: usize| match bytes.get(j) {
                Some(&c) if c & 0xC0 == 0x80 => Ok(u16::from(c & 0x3F)),
                _ => Err(bad()),
            };
            let (unit, width) = match b {
                0x01..=0x7F => (u16::from(b), 1),
                0xC0..=0xDF => (u16::from(b & 0x1F) << 6 | cont(i + 1)?, 2),
                0xE0..=0xEF => (
                    u16::from(b & 0x0F) << 12 | cont(i + 1)? << 6 | cont(i + 2)?,
                    3,
                ),
                _ => return Err(bad()),
            };
            units.push(unit);
            i += width;
        }
        String::from_utf16(&units).map_err(|_| bad())
    }

    fn payload(&mut self, id: u8, path: &str, depth: usize) -> Result<Tag, NbtError> {
        if depth > MAX_DEPTH {
            return Err(NbtError::TooDeep {
                path: path.to_string(),
            });
        }
        Ok(match id {
            1 => Tag::Byte(i8::from_be_bytes(self.array(path)?)),
            2 => Tag::Short(i16::from_be_bytes(self.array(path)?)),
            3 => Tag::Int(self.i32(path)?),
            4 => Tag::Long(i64::from_be_bytes(self.array(path)?)),
            5 => Tag::Float(f32::from_be_bytes(self.array(path)?)),
            6 => Tag::Double(f64::from_be_bytes(self.array(path)?)),
            7 => {
                let len = self.len(path)?;
                Tag::ByteArray(self.take(len, path)?.iter().map(|&b| b as i8).collect())
            }
            8 => Tag::String(self.string(path)?),
            9 => {
                let element = self.u8(path)?;
                let len = self.len(path)?;
                let mut items = Vec::with_capacity(len.min(self.bytes.len()));
                // An empty list may say TAG_End as its type.
                if element != 0 || len != 0 {
                    for i in 0..len {
                        items.push(self.payload(element, &format!("{path}[{i}]"), depth + 1)?);
                    }
                }
                Tag::List(items)
            }
            10 => {
                let mut compound = Compound::new();
                loop {
                    let child = self.u8(path)?;
                    if child == 0 {
                        break;
                    }
                    let name = self.string(path)?;
                    let child_path = if path.is_empty() {
                        name.clone()
                    } else {
                        format!("{path}.{name}")
                    };
                    let value = self.payload(child, &child_path, depth + 1)?;
                    compound.insert(name, value);
                }
                Tag::Compound(compound)
            }
            11 => {
                let len = self.len(path)?;
                let bytes = self.take(len.saturating_mul(4), path)?;
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
                let len = self.len(path)?;
                let bytes = self.take(len.saturating_mul(8), path)?;
                Tag::LongArray(
                    bytes
                        .as_chunks::<8>()
                        .0
                        .iter()
                        .map(|&c| i64::from_be_bytes(c))
                        .collect(),
                )
            }
            _ => {
                return Err(NbtError::TagType {
                    path: path.to_string(),
                    id,
                });
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // A raw zero byte and four-byte UTF-8 aren't modified UTF-8.
        for bad in [&[0x00][..], &[0xF0, 0x9F, 0x98, 0x80]] {
            let mut b = vec![8];
            b.extend(0_u16.to_be_bytes());
            b.extend((bad.len() as u16).to_be_bytes());
            b.extend(bad);
            assert!(matches!(read(&b), Err(NbtError::String { .. })));
        }
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
}
