//! What every loader of worldgen JSON shares (ADR 0010): load errors that name the resource and
//! the JSON path ([`LoadError`], [`At`]), field readers, ids, and reading numbers as the game's
//! codecs do (ADR 0011).
//!
//! The game parses JSON with Gson, which keeps each number's text, and a codec then calls
//! `floatValue()`, `doubleValue()` or `intValue()` on it. The number helpers do the same from the
//! text `serde_json` keeps (its `arbitrary_precision` feature), so nothing is rounded twice.

use std::fmt;

use serde_json::{Map, Number, Value};

use crate::bundle::BundleError;

/// Why a worldgen resource couldn't be loaded.
#[derive(Debug, thiserror::Error)]
pub struct LoadError {
    /// The resource being loaded, e.g. `minecraft:overworld/offset`.
    pub resource: String,
    /// Where in its JSON, e.g. `input.second.right.spline.points[5].value`. Empty for the root.
    pub path: String,
    /// What's wrong. Boxed, so results carrying a load error stay small.
    pub kind: Box<LoadErrorKind>,
}

impl LoadError {
    /// An error in `resource` at `path` (empty for the root).
    pub fn new(resource: impl Into<String>, path: impl Into<String>, kind: LoadErrorKind) -> Self {
        LoadError {
            resource: resource.into(),
            path: path.into(),
            kind: Box::new(kind),
        }
    }
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            write!(f, "{}: {}", self.resource, self.kind)
        } else {
            write!(f, "{}: {}: {}", self.resource, self.path, self.kind)
        }
    }
}

/// The kinds of [`LoadError`].
#[derive(Debug, thiserror::Error)]
pub enum LoadErrorKind {
    /// The resource couldn't be read from the bundle.
    #[error("{0}")]
    Bundle(#[from] BundleError),
    /// A `type` (or other variant) spyglass doesn't support yet.
    #[error("unsupported {what} {name:?}")]
    Unsupported {
        /// What kind of thing, e.g. `density function type`.
        what: &'static str,
        /// Its name, e.g. `minecraft:old_blended_noise`.
        name: String,
    },
    /// A required field is missing.
    #[error("missing field {0:?}")]
    Missing(&'static str),
    /// A value has the wrong JSON shape.
    #[error("expected {0}")]
    Shape(&'static str),
    /// A number is outside the range the game's codec accepts.
    #[error("{value} is outside {min}..={max}")]
    OutOfRange {
        /// The value, as written.
        value: String,
        /// The smallest value accepted.
        min: String,
        /// The largest value accepted.
        max: String,
    },
    /// A value the game's codec rejects for another reason.
    #[error("{0}")]
    Invalid(&'static str),
    /// Resources refer to each other in a cycle.
    #[error("reference cycle: {}", .0.join(" -> "))]
    Cycle(Vec<String>),
}

/// A load result.
pub type LoadResult<T> = Result<T, LoadError>;

/// An id with the default namespace made explicit, as `Identifier.parse` reads it.
pub fn full_id(id: &str) -> String {
    if id.contains(':') {
        id.to_string()
    } else {
        format!("minecraft:{id}")
    }
}

/// A position in the JSON being loaded: the resource and the path into it, for errors.
#[derive(Debug, Clone)]
pub struct At<'a> {
    resource: &'a str,
    path: String,
}

impl<'a> At<'a> {
    /// The root of `resource`.
    pub fn root(resource: &'a str) -> Self {
        At::new(resource, "")
    }

    /// `path` in `resource`, for JSON reached some other way, such as a router entry.
    pub fn new(resource: &'a str, path: &str) -> Self {
        At {
            resource,
            path: path.to_string(),
        }
    }

    /// A field (`name`) or list index (`[i]`) below this position.
    pub fn child(&self, segment: &str) -> At<'a> {
        At {
            resource: self.resource,
            path: if self.path.is_empty() {
                segment.to_string()
            } else if segment.starts_with('[') {
                format!("{}{segment}", self.path)
            } else {
                format!("{}.{segment}", self.path)
            },
        }
    }

    /// An error here.
    pub fn error(&self, kind: LoadErrorKind) -> LoadError {
        LoadError::new(self.resource, self.path.clone(), kind)
    }
}

/// A required field of an object.
pub fn field<'v>(
    obj: &'v Map<String, Value>,
    name: &'static str,
    at: &At<'_>,
) -> LoadResult<&'v Value> {
    obj.get(name)
        .ok_or_else(|| at.error(LoadErrorKind::Missing(name)))
}

/// An object.
pub fn object<'v>(json: &'v Value, at: &At<'_>) -> LoadResult<&'v Map<String, Value>> {
    json.as_object()
        .ok_or_else(|| at.error(LoadErrorKind::Shape("an object")))
}

/// An id, as a string.
pub fn id<'v>(json: &'v Value, at: &At<'_>) -> LoadResult<&'v str> {
    json.as_str()
        .ok_or_else(|| at.error(LoadErrorKind::Shape("an id")))
}

/// `Codec.FLOAT`.
pub fn float(json: &Value, at: &At<'_>) -> LoadResult<f32> {
    f32_of(json).ok_or_else(|| at.error(LoadErrorKind::Shape("a number")))
}

/// `Codec.DOUBLE`.
pub fn double(json: &Value, at: &At<'_>) -> LoadResult<f64> {
    f64_of(json).ok_or_else(|| at.error(LoadErrorKind::Shape("a number")))
}

/// `Codec.INT`.
pub fn int(json: &Value, at: &At<'_>) -> LoadResult<i32> {
    int_of(json).ok_or_else(|| at.error(LoadErrorKind::Shape("an integer")))
}

/// `Codec.intRange(min, max)`.
pub fn int_in(json: &Value, min: i32, max: i32, at: &At<'_>) -> LoadResult<i32> {
    let v = int(json, at)?;
    if (min..=max).contains(&v) {
        Ok(v)
    } else {
        Err(at.error(out_of_range(json, min, max)))
    }
}

/// An optional `intRange` field with a default; required when `default` is `None`.
pub fn int_field(
    obj: &Map<String, Value>,
    name: &'static str,
    (min, max): (i32, i32),
    default: Option<i32>,
    at: &At<'_>,
) -> LoadResult<i32> {
    match (obj.get(name), default) {
        (Some(v), _) => int_in(v, min, max, &at.child(name)),
        (None, Some(d)) => Ok(d),
        (None, None) => Err(at.error(LoadErrorKind::Missing(name))),
    }
}

/// [`LoadErrorKind::OutOfRange`] for `json`.
pub fn out_of_range(json: &Value, min: impl ToString, max: impl ToString) -> LoadErrorKind {
    LoadErrorKind::OutOfRange {
        value: json.to_string(),
        min: min.to_string(),
        max: max.to_string(),
    }
}

/// `LazilyParsedNumber.floatValue()`: `Float.parseFloat` of the text, the nearest `f32`.
pub fn f32_of(value: &Value) -> Option<f32> {
    text(value)?.parse().ok()
}

/// `LazilyParsedNumber.doubleValue()`: `Double.parseDouble` of the text.
pub fn f64_of(value: &Value) -> Option<f64> {
    text(value)?.parse().ok()
}

/// `LazilyParsedNumber.intValue()`: `Integer.parseInt`, else `(int) Long.parseLong`, else
/// `new BigDecimal(text).intValue()` (truncated toward zero, then the low 32 bits).
pub fn int_of(value: &Value) -> Option<i32> {
    let text = text(value)?;
    if let Ok(i) = text.parse::<i32>() {
        return Some(i);
    }
    if let Ok(l) = text.parse::<i64>() {
        return Some(l as i32);
    }
    // BigDecimal.intValue: the integer part's low 32 bits. An f64 holds the integer part exactly
    // up to 2^53, which covers anything a codec range would accept.
    let d: f64 = text.parse().ok()?;
    d.is_finite().then(|| (d.trunc() as i64) as i32)
}

fn text(value: &Value) -> Option<String> {
    value.as_number().map(Number::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn num(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn keeps_the_text() {
        // With arbitrary_precision the text survives, digits and all.
        assert_eq!(
            text(&num("0.10000000000000000555")).unwrap(),
            "0.10000000000000000555"
        );
    }

    #[test]
    fn floats_round_once() {
        // 1.00000005960464477539062500000001 is just above the midpoint between the f32s 1.0 and
        // 1.0000001: rounding it straight to f32 goes up. Through f64 it first becomes exactly
        // the midpoint, which then ties to even, down to 1.0.
        let v = num("1.00000005960464477539062500000001");
        assert_eq!(f32_of(&v), Some(1.000_000_1));
        assert_eq!(f64_of(&v).map(|d| d as f32), Some(1.0));
    }

    #[test]
    fn errors_name_the_resource_and_path() {
        let root = At::root("minecraft:x");
        let at = root.child("points").child("[2]").child("value");
        let e = at.error(LoadErrorKind::Missing("value"));
        assert_eq!(
            e.to_string(),
            r#"minecraft:x: points[2].value: missing field "value""#
        );
        assert_eq!(
            root.error(LoadErrorKind::Shape("an object")).to_string(),
            "minecraft:x: expected an object"
        );
        let e = int_in(&num("40"), 0, 32, &root.child("n")).unwrap_err();
        assert_eq!(e.to_string(), "minecraft:x: n: 40 is outside 0..=32");
        assert_eq!(full_id("plains"), "minecraft:plains");
        assert_eq!(full_id("mod:plains"), "mod:plains");
    }

    #[test]
    fn ints_follow_int_value() {
        assert_eq!(int_of(&num("-64")), Some(-64));
        assert_eq!(int_of(&num("4294967296")), Some(0));
        assert_eq!(int_of(&num("4294967297")), Some(1));
        assert_eq!(int_of(&num("1.9")), Some(1));
        assert_eq!(int_of(&num("-1.9")), Some(-1));
        assert_eq!(int_of(&Value::from("1")), None);
    }
}
