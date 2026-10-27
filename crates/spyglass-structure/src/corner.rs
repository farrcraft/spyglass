//! Corner constraints on pool elements (`CornerConstraint`, 26.4-snapshot-6 on): density
//! functions sampled at a piece's corners, which must land in a range at enough of them for the
//! piece to be placed. See `docs/specs/structure-placement.md`.

use std::cmp::Ordering;
use std::collections::HashMap;

use serde_json::Value;
use spyglass_data::anchor::AnchorHeights;
use spyglass_data::bundle::Bundle;
use spyglass_data::codec::{self, At, LoadErrorKind, LoadResult};
use spyglass_density::{Binder, DensityFunction, Df, Loader, Sampler};

use crate::jigsaw::BoundingBox;

/// `CornerConstraint`: every predicate must hold.
#[derive(Debug, Clone, PartialEq)]
pub struct CornerConstraint {
    predicates: Vec<DensityPredicate>,
}

/// `CornerConstraint.DensityPredicate`.
#[derive(Debug, Clone, PartialEq)]
struct DensityPredicate {
    /// `density_function`: a reference or an inline function.
    function: Df,
    /// `acceptable_density`, inclusive at both ends.
    acceptable: [f64; 2],
    /// `padding`, 0 to 8: how far the box is grown on every side first.
    padding: i32,
    /// `corners`.
    corners: Corners,
    /// `required_corners`, inclusive, each 1 to 8.
    required: [i32; 2],
}

/// `CornersToCheck`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Corners {
    /// `top`: the four at the box's top.
    Top,
    /// `bottom`: the four at its bottom.
    Bottom,
    /// `all`: all eight.
    All,
}

impl CornerConstraint {
    /// `CornerConstraint.CODEC`: a list of 1 to 10 predicates. Their functions are loaded by
    /// `loader`.
    pub(crate) fn parse(
        json: &Value,
        at: &At<'_>,
        loader: &mut Loader<'_, Bundle>,
    ) -> LoadResult<Self> {
        let list = json
            .as_array()
            .filter(|l| (1..=10).contains(&l.len()))
            .ok_or_else(|| {
                at.error(LoadErrorKind::Shape("a list of 1 to 10 density predicates"))
            })?;
        let predicates = list
            .iter()
            .enumerate()
            .map(|(i, p)| DensityPredicate::parse(p, &at.child(&format!("[{i}]")), loader))
            .collect::<LoadResult<_>>()?;
        Ok(CornerConstraint { predicates })
    }

    /// The density functions it samples.
    pub(crate) fn functions(&self) -> impl Iterator<Item = &Df> {
        self.predicates.iter().map(|p| &p.function)
    }

    /// `test(box, randomState)`: whether every predicate holds for `bbox`, its functions bound
    /// by `samplers`.
    pub(crate) fn test(&self, bbox: &BoundingBox, samplers: &mut CornerSamplers<'_>) -> bool {
        self.predicates.iter().all(|p| p.test(bbox, samplers))
    }
}

impl DensityPredicate {
    /// `DensityPredicate.CODEC`, with its validation: a box's top or bottom has only 4 corners.
    fn parse(json: &Value, at: &At<'_>, loader: &mut Loader<'_, Bundle>) -> LoadResult<Self> {
        let obj = codec::object(json, at)?;
        let function_at = at.child("density_function");
        let function = loader.inline(
            function_at.resource(),
            function_at.path(),
            codec::field(obj, "density_function", at)?,
        )?;
        let acceptable_at = at.child("acceptable_density");
        let acceptable = interval(
            codec::field(obj, "acceptable_density", at)?,
            &acceptable_at,
            &|v, at| codec::double(v, at),
        )?;
        if java_compare(acceptable[0], acceptable[1]).is_gt() {
            return Err(acceptable_at.error(MIN_ABOVE_MAX));
        }
        let padding = match obj.get("padding") {
            None => 0,
            Some(v) => codec::int_in(v, 0, 8, &at.child("padding"))?,
        };
        let corners = match obj.get("corners").map(Value::as_str) {
            None | Some(Some("all")) => Corners::All,
            Some(Some("top")) => Corners::Top,
            Some(Some("bottom")) => Corners::Bottom,
            Some(_) => {
                return Err(at
                    .child("corners")
                    .error(LoadErrorKind::Shape("\"top\", \"bottom\" or \"all\"")));
            }
        };
        let required_at = at.child("required_corners");
        let required = interval(
            codec::field(obj, "required_corners", at)?,
            &required_at,
            &|v, at| codec::int_in(v, 1, 8, at),
        )?;
        if required[0] > required[1] {
            return Err(required_at.error(MIN_ABOVE_MAX));
        }
        if corners != Corners::All && required[1] > 4 {
            return Err(at.error(LoadErrorKind::Invalid(
                "required_corners can't be above 4 for the top or bottom corners",
            )));
        }
        Ok(DensityPredicate {
            function,
            acceptable,
            padding,
            corners,
            required,
        })
    }

    /// `DensityPredicate.test`: the function at each corner of the box grown by `padding`, in
    /// the game's order, stopping as soon as too many corners match or too few can.
    fn test(&self, bbox: &BoundingBox, samplers: &mut CornerSamplers<'_>) -> bool {
        let sampler = samplers.get(&self.function);
        let b = bbox.inflated(self.padding);
        let corners = corners(&b, self.corners);
        let [min_required, max_required] = self.required;
        let mut matched = 0;
        for (checked, [x, y, z]) in corners.iter().copied().enumerate() {
            let v = f64::from(sampler.sample(x, y, z));
            // InclusiveRange.isValueInRange, with Double.compareTo's order.
            if java_compare(v, self.acceptable[0]).is_ge()
                && java_compare(v, self.acceptable[1]).is_le()
            {
                matched += 1;
                if matched > max_required {
                    return false;
                }
            }
            let remaining = (corners.len() - checked - 1) as i32;
            if matched + remaining < min_required {
                return false;
            }
        }
        true
    }
}

/// `BoundingBox.forAllCorners`, `forAllTopCorners` and `forAllBottomCorners`: max x before min,
/// then max y before min, then max z before min.
fn corners(b: &BoundingBox, which: Corners) -> Vec<[i32; 3]> {
    let (min, max) = (b.min, b.max);
    let ys: &[i32] = match which {
        Corners::Top => &[max[1]],
        Corners::Bottom => &[min[1]],
        Corners::All => &[max[1], min[1]],
    };
    let mut out = Vec::with_capacity(8);
    for z in [max[2], min[2]] {
        for &y in ys {
            for x in [max[0], min[0]] {
                out.push([x, y, z]);
            }
        }
    }
    out
}

/// `Double.compare`: `-0.0` below `+0.0`, and NaN above everything, equal to itself.
fn java_compare(a: f64, b: f64) -> Ordering {
    if a < b {
        return Ordering::Less;
    }
    if a > b {
        return Ordering::Greater;
    }
    // doubleToLongBits: every NaN as the canonical one.
    let bits = |v: f64| {
        if v.is_nan() {
            0x7ff8_0000_0000_0000_i64
        } else {
            v.to_bits() as i64
        }
    };
    bits(a).cmp(&bits(b))
}

/// `InclusiveRange`'s validation, by the type's `compareTo`.
const MIN_ABOVE_MAX: LoadErrorKind = LoadErrorKind::Invalid("min_inclusive is above max_inclusive");

/// `InclusiveRange.codec`'s forms: one value for both ends, `[min, max]`, or
/// `{"min_inclusive", "max_inclusive"}`. The caller checks the order.
fn interval<T: Copy>(
    json: &Value,
    at: &At<'_>,
    value: &dyn Fn(&Value, &At<'_>) -> LoadResult<T>,
) -> LoadResult<[T; 2]> {
    let [min, max] = match json {
        Value::Array(list) if list.len() == 2 => [
            value(&list[0], &at.child("[0]"))?,
            value(&list[1], &at.child("[1]"))?,
        ],
        Value::Object(obj) => [
            value(
                codec::field(obj, "min_inclusive", at)?,
                &at.child("min_inclusive"),
            )?,
            value(
                codec::field(obj, "max_inclusive", at)?,
                &at.child("max_inclusive"),
            )?,
        ],
        _ => {
            let v = value(json, at)?;
            [v, v]
        }
    };
    Ok([min, max])
}

/// The corner constraints' functions bound for one seed (`RandomState.getSampler`), each once.
pub(crate) struct CornerSamplers<'n> {
    binder: Binder<'n>,
    bound: HashMap<*const DensityFunction, (Df, Sampler)>,
}

impl<'n> CornerSamplers<'n> {
    /// A binder for `seed`, as the dimension's `RandomState` binds: its random source, and
    /// anchors resolved against `anchors`.
    pub(crate) fn new(
        noises: &'n HashMap<String, spyglass_noise::NormalNoiseParameters>,
        seed: i64,
        legacy_random: bool,
        anchors: AnchorHeights,
    ) -> Self {
        CornerSamplers {
            binder: Binder::with_legacy_random(noises, seed, legacy_random).with_anchors(anchors),
            bound: HashMap::new(),
        }
    }

    /// Binds `function`, as loading already did for this dimension.
    pub(crate) fn bind(&mut self, function: &Df) -> LoadResult<()> {
        let key = std::sync::Arc::as_ptr(function);
        if !self.bound.contains_key(&key) {
            let sampler = self.binder.bind(function)?;
            self.bound.insert(key, (function.clone(), sampler));
        }
        Ok(())
    }

    fn get(&mut self, function: &Df) -> &Sampler {
        let key = std::sync::Arc::as_ptr(function);
        if !self.bound.contains_key(&key) {
            // Loading bound every constraint function for this dimension's heights and random
            // source; binding depends on nothing else, so it can't fail for another seed.
            self.bind(function)
                .expect("corner constraint functions bind, as checked at load");
        }
        &self.bound[&key].1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_in_the_games_order() {
        let b = BoundingBox {
            min: [0, 10, 20],
            max: [1, 11, 21],
        };
        assert_eq!(
            corners(&b, Corners::All),
            vec![
                [1, 11, 21],
                [0, 11, 21],
                [1, 10, 21],
                [0, 10, 21],
                [1, 11, 20],
                [0, 11, 20],
                [1, 10, 20],
                [0, 10, 20],
            ]
        );
        assert_eq!(
            corners(&b, Corners::Bottom),
            vec![[1, 10, 21], [0, 10, 21], [1, 10, 20], [0, 10, 20]]
        );
    }

    #[test]
    fn double_compare_orders_zeros_and_nan() {
        assert!(java_compare(-0.0, 0.0).is_lt());
        assert!(java_compare(f64::NAN, f64::INFINITY).is_gt());
        assert!(java_compare(-f64::NAN, f64::NAN).is_eq());
        assert!(java_compare(1.0, 1.0).is_eq());
    }

    #[test]
    fn intervals_in_three_forms() {
        let at = At::root("t");
        let f = |v: Value| interval(&v, &at, &|v, at| codec::double(v, at));
        assert_eq!(f(serde_json::json!(0.5)).unwrap(), [0.5, 0.5]);
        assert_eq!(f(serde_json::json!([-1, 2])).unwrap(), [-1.0, 2.0]);
        assert_eq!(
            f(serde_json::json!({"min_inclusive": 0, "max_inclusive": 1})).unwrap(),
            [0.0, 1.0]
        );
    }
}
