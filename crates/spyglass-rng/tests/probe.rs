//! Every random-source value in the oracle's probe files, reproduced bit for bit.
//!
//! Needs `cargo xtask oracle probe` first (docs/specs/oracle.md, "Probe files"). Run with
//! `cargo test -p spyglass-rng -- --ignored`.

use spyglass_oracle::probe::{Case, ProbeFile, probe_file};
use spyglass_rng::support::{
    mix_stafford13, seed_from_hash_of, upgrade_seed_to_128bit, upgrade_seed_to_128bit_unmixed,
};
use spyglass_rng::{
    LegacyRandomSource, PositionalRandomFactory, RandomSource, XoroshiroRandomSource,
};

/// What spyglass computed for a case, in the case's value type.
#[derive(Debug, PartialEq)]
enum Values {
    I64(Vec<i64>),
    F32(Vec<u32>),
    F64(Vec<u64>),
    Text(String),
}

fn expected(case: &Case) -> Values {
    match case.kind_of_values.as_str() {
        "i64" => Values::I64(case.i64s()),
        "f32" => Values::F32(case.f32_bits()),
        "f64" => Values::F64(case.f64_bits()),
        "text" => Values::Text(case.text().to_string()),
        other => panic!("{}: unknown value type {other}", case.label()),
    }
}

fn repeat<T>(n: usize, mut f: impl FnMut() -> T) -> Vec<T> {
    (0..n).map(|_| f()).collect()
}

/// Replays a random-source case with sources from `make`.
fn replay<R: RandomSource>(
    case: &Case,
    make: impl Fn(i64) -> R,
    parity: impl Fn(&R::Positional) -> String,
) -> Values {
    let seed = case.seed.expect("random-source cases have a seed");
    let n = case.values.len();
    let mut r = make(seed);
    match case.test.as_str() {
        "next_long" => Values::I64(repeat(n, || r.next_long())),
        "next_int" => Values::I64(repeat(n, || i64::from(r.next_int()))),
        "next_int_bounded" => {
            let bound = case.bound.expect("a bound");
            Values::I64(repeat(n, || i64::from(r.next_int_bounded(bound))))
        }
        "next_float" => Values::F32(repeat(n, || r.next_float().to_bits())),
        "next_double" => Values::F64(repeat(n, || r.next_double().to_bits())),
        "next_gaussian" => Values::F64(repeat(n, || r.next_gaussian().to_bits())),
        "next_boolean" => Values::I64(repeat(n, || i64::from(r.next_boolean()))),
        "gaussian_interleaved" => Values::I64(vec![
            r.next_gaussian().to_bits() as i64,
            r.next_long(),
            r.next_gaussian().to_bits() as i64,
            r.next_gaussian().to_bits() as i64,
            i64::from(r.next_int_bounded(10)),
        ]),
        "consume_count_5" => {
            r.consume_count(5);
            Values::I64(vec![r.next_long(), r.next_long()])
        }
        "fork" => {
            let mut fork = r.fork();
            Values::I64(vec![
                fork.next_long(),
                fork.next_long(),
                r.next_long(),
                r.next_long(),
            ])
        }
        "positional_parity" => Values::Text(parity(&r.fork_positional())),
        "positional_at" => {
            let [x, y, z] = case.pos.expect("a position");
            let mut at = r.fork_positional().at(x, y, z);
            Values::I64(vec![at.next_long(), at.next_long()])
        }
        "positional_from_hash_of" => {
            let name = case.name.as_deref().expect("a name");
            let mut hashed = r.fork_positional().from_hash_of(name);
            Values::I64(vec![hashed.next_long(), hashed.next_long()])
        }
        other => panic!("{}: unknown random-source test {other}", case.label()),
    }
}

fn actual(case: &Case) -> Values {
    let seed = || case.seed.expect("a seed");
    match case.test.as_str() {
        "mix_stafford13" => Values::I64(vec![mix_stafford13(seed())]),
        "upgrade_seed_to_128bit" => {
            let s = upgrade_seed_to_128bit(seed());
            Values::I64(vec![s.lo, s.hi])
        }
        "upgrade_seed_to_128bit_unmixed" => {
            let s = upgrade_seed_to_128bit_unmixed(seed());
            Values::I64(vec![s.lo, s.hi])
        }
        "seed_from_hash_of" => {
            let s = seed_from_hash_of(case.name.as_deref().expect("a name"));
            Values::I64(vec![s.lo, s.hi])
        }
        _ => match case.kind.as_deref() {
            Some("xoroshiro") => replay(case, XoroshiroRandomSource::new, |f| {
                let s = f.seed();
                format!("seedLo: {}, seedHi: {}", s.lo, s.hi)
            }),
            Some("legacy") => replay(case, LegacyRandomSource::new, |f| {
                format!("LegacyPositionalRandomFactory{{{}}}", f.seed())
            }),
            other => panic!("{}: unknown kind {other:?}", case.label()),
        },
    }
}

#[test]
#[ignore = "needs the oracle's probe files: cargo xtask oracle probe"]
fn every_random_source_value_matches_the_game() {
    let path = probe_file("rng.json");
    let file = ProbeFile::read(&path).unwrap();
    let mut failures = Vec::new();
    for case in &file.cases {
        let (want, got) = (expected(case), actual(case));
        if want != got {
            failures.push(format!(
                "{}\n    game:     {want:?}\n    spyglass: {got:?}",
                case.label()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{}: {} of {} cases differ:\n{}",
        path.display(),
        failures.len(),
        file.cases.len(),
        failures[..failures.len().min(20)].join("\n")
    );
    println!("{}: all {} cases match", path.display(), file.cases.len());
}
