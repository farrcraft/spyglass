//! Reading JSON numbers as the game's codecs do (ADR 0011).
//!
//! The game parses JSON with Gson, which keeps each number's text, and a codec then calls
//! `floatValue()`, `doubleValue()` or `intValue()` on it. These helpers do the same from the text
//! `serde_json` keeps (its `arbitrary_precision` feature), so nothing is rounded twice.

use serde_json::{Number, Value};

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
    fn ints_follow_int_value() {
        assert_eq!(int_of(&num("-64")), Some(-64));
        assert_eq!(int_of(&num("4294967296")), Some(0));
        assert_eq!(int_of(&num("4294967297")), Some(1));
        assert_eq!(int_of(&num("1.9")), Some(1));
        assert_eq!(int_of(&num("-1.9")), Some(-1));
        assert_eq!(int_of(&Value::from("1")), None);
    }
}
