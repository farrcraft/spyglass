//! Vertical anchors (`VerticalAnchor`): a y given absolutely, or from the bottom or top of the
//! heights it's resolved against.

use serde_json::Value;

use crate::codec::{self, At, LoadErrorKind, LoadResult};

/// What anchors resolve against (`VerticalAnchor.Context`'s heights).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnchorHeights {
    /// The lowest y.
    pub min_y: i32,
    /// How many blocks tall.
    pub height: i32,
}

impl AnchorHeights {
    /// `VerticalAnchor.Context.from(generator, level)`: the larger of the noise settings' and the
    /// level's minimum y, and the smaller of their heights.
    pub fn within_level(noise: AnchorHeights, level: AnchorHeights) -> Self {
        AnchorHeights {
            min_y: noise.min_y.max(level.min_y),
            height: noise.height.min(level.height),
        }
    }

    /// Noise settings' own heights, `noise.min_y` and `noise.height`, if they have `noise`.
    pub fn of_noise_settings(json: &Value, resource: &str) -> LoadResult<Option<AnchorHeights>> {
        let Some(noise) = json.get("noise") else {
            return Ok(None);
        };
        let at = At::new(resource, "noise");
        let noise = codec::object(noise, &at)?;
        Ok(Some(AnchorHeights {
            min_y: codec::int(codec::field(noise, "min_y", &at)?, &at.child("min_y"))?,
            height: codec::int(codec::field(noise, "height", &at)?, &at.child("height"))?,
        }))
    }

    /// A level's heights: a dimension type's `min_y` and `height`.
    pub fn of_dimension_type(json: &Value, at: &At<'_>) -> LoadResult<AnchorHeights> {
        let dimension = codec::object(json, at)?;
        Ok(AnchorHeights {
            min_y: codec::int(codec::field(dimension, "min_y", at)?, &at.child("min_y"))?,
            height: codec::int(codec::field(dimension, "height", at)?, &at.child("height"))?,
        })
    }
}

/// A vertical anchor (`VerticalAnchor`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VerticalAnchor {
    /// `absolute`.
    Absolute(i32),
    /// `above_bottom`: offset up from the minimum y.
    AboveBottom(i32),
    /// `below_top`: offset down from the top block.
    BelowTop(i32),
}

impl VerticalAnchor {
    /// `resolveY(context)`, in wrapping `int` arithmetic.
    pub fn resolve(self, heights: AnchorHeights) -> i32 {
        match self {
            VerticalAnchor::Absolute(y) => y,
            VerticalAnchor::AboveBottom(o) => heights.min_y.wrapping_add(o),
            VerticalAnchor::BelowTop(o) => heights
                .height
                .wrapping_sub(1)
                .wrapping_add(heights.min_y)
                .wrapping_sub(o),
        }
    }

    /// `VerticalAnchor.CODEC`: exactly one of `absolute`, `above_bottom` or `below_top`.
    pub fn parse(json: &Value, at: &At<'_>) -> LoadResult<VerticalAnchor> {
        let obj = json
            .as_object()
            .filter(|o| o.len() == 1)
            .ok_or_else(|| at.error(LoadErrorKind::Shape("a vertical anchor")))?;
        let (key, value) = obj.iter().next().expect("one entry");
        let n = codec::int_of(value)
            .ok_or_else(|| at.child(key).error(LoadErrorKind::Shape("an integer")))?;
        match key.as_str() {
            "absolute" => Ok(VerticalAnchor::Absolute(n)),
            "above_bottom" => Ok(VerticalAnchor::AboveBottom(n)),
            "below_top" => Ok(VerticalAnchor::BelowTop(n)),
            _ => Err(at.error(LoadErrorKind::Shape("a vertical anchor"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchors_resolve_from_either_end() {
        let heights = AnchorHeights {
            min_y: -64,
            height: 384,
        };
        assert_eq!(VerticalAnchor::Absolute(5).resolve(heights), 5);
        assert_eq!(VerticalAnchor::AboveBottom(8).resolve(heights), -56);
        assert_eq!(VerticalAnchor::BelowTop(2).resolve(heights), 317);
    }

    #[test]
    fn a_level_narrows_the_noise_heights() {
        let noise = AnchorHeights {
            min_y: 0,
            height: 128,
        };
        // The Nether's dimension type is 0..256: the noise settings decide.
        let nether = AnchorHeights {
            min_y: 0,
            height: 256,
        };
        assert_eq!(AnchorHeights::within_level(noise, nether), noise);
        // The smaller height, not the overlap: a level from 64 keeps 128 blocks.
        let high = AnchorHeights {
            min_y: 64,
            height: 256,
        };
        assert_eq!(
            AnchorHeights::within_level(noise, high),
            AnchorHeights {
                min_y: 64,
                height: 128
            }
        );
    }

    #[test]
    fn exactly_one_key() {
        let at = At::root("t");
        let parse = |v: Value| VerticalAnchor::parse(&v, &at);
        assert_eq!(
            parse(serde_json::json!({"below_top": 3})).unwrap(),
            VerticalAnchor::BelowTop(3)
        );
        assert!(parse(serde_json::json!({"absolute": 1, "below_top": 3})).is_err());
        assert!(parse(serde_json::json!({"top": 3})).is_err());
        assert!(parse(serde_json::json!(3)).is_err());
    }
}
