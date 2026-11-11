//! `cargo xtask math-report`: how often each candidate implementation of Java's `Math` gives
//! another answer than the JDK's `Math` on the machines measured, from the files
//! `cargo xtask oracle math` writes (`oracle-out/math/<os>-<arch>/`, docs/specs/java-math.md).
//!
//! Each file holds the arguments of one function's set with three answers: `Math`'s,
//! `StrictMath`'s and the correctly rounded one. A candidate is compared with `Math`, which is
//! what the game computes on that machine, and with the correctly rounded answer.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use clap::Args;
use serde_json::Value;

use crate::oracle::math_dir;

#[derive(Args)]
pub struct MathReport {
    /// Platforms to report, as `oracle-out/math/` names them (`windows-x86_64`). Defaults to all.
    platforms: Vec<String>,
}

/// A candidate: a name and the function it computes, given the arguments.
struct Candidate {
    name: &'static str,
    unary: fn(&str, f64) -> f64,
    binary: fn(f64, f64) -> f64,
}

/// What spyglass computes today: the platform's libm, through Rust's `f64` methods.
#[expect(
    clippy::disallowed_methods,
    reason = "the platform's functions are the candidate being measured"
)]
fn platform(function: &str, x: f64) -> f64 {
    match function {
        "sin" => x.sin(),
        "cos" => x.cos(),
        _ => x.ln(),
    }
}

#[expect(
    clippy::disallowed_methods,
    reason = "the platform's functions are the candidate being measured"
)]
fn platform_pow(x: f64, y: f64) -> f64 {
    x.powf(y)
}

/// The `libm` crate: musl's functions in Rust, descended from fdlibm.
fn musl(function: &str, x: f64) -> f64 {
    match function {
        "sin" => libm::sin(x),
        "cos" => libm::cos(x),
        _ => libm::log(x),
    }
}

/// spyglass's own: correctly rounded, the same on every platform (`spyglass-math`).
fn ours(function: &str, x: f64) -> f64 {
    match function {
        "sin" => spyglass_math::sin(x),
        "cos" => spyglass_math::cos(x),
        _ => spyglass_math::log(x),
    }
}

/// spyglass's own, and whether its rounding test settled the result.
fn ours_settled(function: &str, x: &[f64]) -> (f64, bool) {
    use spyglass_math::settled;
    match (function, x) {
        ("sin", [x, ..]) => settled::sin(*x),
        ("cos", [x, ..]) => settled::cos(*x),
        ("pow", [x, y, ..]) => settled::pow(*x, *y),
        (_, [x, ..]) => settled::log(*x),
        _ => (f64::NAN, false),
    }
}

const CANDIDATES: &[Candidate] = &[
    Candidate {
        name: "platform",
        unary: platform,
        binary: platform_pow,
    },
    Candidate {
        name: "libm crate",
        unary: musl,
        binary: libm::pow,
    },
    Candidate {
        name: "spyglass",
        unary: ours,
        binary: spyglass_math::pow,
    },
];

pub fn run(args: &MathReport) -> Result<()> {
    let root = math_dir("");
    let platforms: Vec<String> = if args.platforms.is_empty() {
        let mut found: Vec<String> = fs::read_dir(&root)
            .with_context(|| format!("no {}; run cargo xtask oracle math first", root.display()))?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().join("math.json").exists())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        found.sort();
        found
    } else {
        args.platforms.clone()
    };
    if platforms.is_empty() {
        bail!("no platforms in {}", root.display());
    }
    for platform in &platforms {
        print!("{}", report(&math_dir(platform), platform)?);
    }
    Ok(())
}

/// Counts of answers that differ, per comparison.
#[derive(Default)]
struct Tally {
    cases: usize,
    /// By comparison name: how many differ, and the largest difference in ulps.
    differ: BTreeMap<String, (usize, u64)>,
}

impl Tally {
    fn count(&mut self, name: &str, a: f64, b: f64) {
        let entry = self.differ.entry(name.to_string()).or_default();
        let ulps = ulps(a, b);
        if ulps > 0 {
            entry.0 += 1;
            entry.1 = entry.1.max(ulps);
        }
    }
}

/// How many doubles apart `a` and `b` are; 0 when they're the same (or both NaN).
fn ulps(a: f64, b: f64) -> u64 {
    if a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan()) {
        return 0;
    }
    // Map the bits to a line that's monotonic across zero.
    let line = |x: f64| {
        let bits = x.to_bits() as i64;
        if bits < 0 { i64::MIN - bits } else { bits }
    };
    line(a).abs_diff(line(b)).max(1)
}

/// One platform's report: a table per function, a row per set.
fn report(dir: &Path, platform: &str) -> Result<String> {
    let meta: Value = serde_json::from_str(
        &fs::read_to_string(dir.join("math.json"))
            .with_context(|| format!("failed to read {}", dir.join("math.json").display()))?,
    )?;
    let mut out = format!(
        "{platform}: {} on {} {}\n",
        meta["vm"].as_str().unwrap_or("?"),
        meta["os"].as_str().unwrap_or("?"),
        meta["arch"].as_str().unwrap_or("?")
    );
    let _ = writeln!(
        out,
        "cases where an answer differs from Math (\"correctly rounded\": Math from it; the last two: spyglass's own against it, and left open by its rounding test), as a percentage, with the largest gap in ulps\n"
    );
    let mut columns = vec!["StrictMath".to_string()];
    columns.extend(CANDIDATES.iter().map(|c| c.name.to_string()));
    columns.push("correctly rounded".to_string());
    // Checks of spyglass's own against the correctly rounded answer: how often it differs, and
    // how often its rounding test left the result open.
    columns.push("spyglass vs CR".to_string());
    columns.push("spyglass unsettled".to_string());
    let _ = writeln!(
        out,
        "{:<28}{:>9}{}",
        "set",
        "cases",
        columns
            .iter()
            .map(|c| format!("{c:>20}"))
            .collect::<String>()
    );
    for set in meta["sets"].as_array().context("math.json has no sets")? {
        let function = set["function"].as_str().context("a set has no function")?;
        let name = set["set"].as_str().context("a set has no name")?;
        let arity = set["args"].as_u64().context("a set has no args")? as usize;
        let file = dir.join(format!("{function}-{}.bin", name.replace('/', "_")));
        let bytes =
            fs::read(&file).with_context(|| format!("failed to read {}", file.display()))?;
        let width = arity + 3;
        let values: Vec<f64> = bytes
            .as_chunks::<8>()
            .0
            .iter()
            .map(|b| f64::from_le_bytes(*b))
            .collect();
        // The carvers' table and density functions' `pow` narrow the result to a float, so
        // there only the floats' differences matter: a second row counts those.
        let narrows = matches!(name, "table" | "float");
        let mut tallies = vec![(String::new(), Tally::default())];
        if narrows {
            tallies.push((" (as float)".to_string(), Tally::default()));
        }
        for record in values.chunks_exact(width) {
            for (i, (_, tally)) in tallies.iter_mut().enumerate() {
                let cut = |v: f64| if i == 1 { f64::from(v as f32) } else { v };
                let math = cut(record[arity]);
                tally.cases += 1;
                tally.count("StrictMath", cut(record[arity + 1]), math);
                for candidate in CANDIDATES {
                    let ours = if arity == 1 {
                        (candidate.unary)(function, record[0])
                    } else {
                        (candidate.binary)(record[0], record[1])
                    };
                    tally.count(candidate.name, cut(ours), math);
                }
                tally.count("correctly rounded", math, cut(record[arity + 2]));
                let (value, settled) = ours_settled(function, &record[..arity]);
                tally.count("spyglass vs CR", cut(value), cut(record[arity + 2]));
                if !settled {
                    tally
                        .differ
                        .entry("spyglass unsettled".into())
                        .or_default()
                        .0 += 1;
                }
            }
        }
        for (suffix, tally) in &tallies {
            let cells: String = columns
                .iter()
                .map(|c| {
                    let (n, ulps) = tally.differ.get(c).copied().unwrap_or_default();
                    let cell = if n == 0 {
                        "0".to_string()
                    } else {
                        format!("{:.3}% ({ulps})", 100.0 * n as f64 / tally.cases as f64)
                    };
                    format!("{cell:>20}")
                })
                .collect();
            let _ = writeln!(
                out,
                "{:<28}{:>9}{cells}",
                format!("{function} {name}{suffix}"),
                tally.cases
            );
        }
    }
    out.push('\n');
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_ulps_across_zero() {
        assert_eq!(ulps(1.0, 1.0), 0);
        assert_eq!(ulps(1.0, f64::from_bits(1.0f64.to_bits() + 1)), 1);
        assert_eq!(ulps(f64::from_bits(1), -f64::from_bits(1)), 2);
        assert_eq!(ulps(0.0, -0.0), 1);
        assert_eq!(ulps(f64::NAN, f64::NAN), 0);
    }
}
