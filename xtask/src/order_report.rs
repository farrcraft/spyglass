//! `cargo xtask order-report`: how stable the game's decoration order around the spawn is, and
//! how many feature answers it changes, from the files `cargo xtask oracle order` and
//! `cargo xtask oracle replay` write (docs/specs/oracle.md#order-files).
//!
//! Prints Markdown tables, for the research notes.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::repo_root;

#[derive(clap::Args)]
pub struct OrderReport {
    /// Only these game versions (repeatable). Defaults to every version with recordings.
    #[arg(long = "game-version")]
    versions: Vec<String>,
}

type Chunk = (i32, i32);

/// One run's recording: the order it decorated chunks in, and where the player joined.
struct Recording {
    /// The file's name without `.json`: `run1`, `run2-cpus2`, `run1-startup`.
    name: String,
    /// The chunk the player joined in; none for startup alone.
    player: Option<Chunk>,
    /// Each decorated chunk's place in the order.
    index: HashMap<Chunk, usize>,
    /// How many were decorated before the server was ready.
    startup: usize,
}

impl Recording {
    /// The runs that differ only in their number compare with each other: `run1-cpus2` with
    /// `run2-cpus2`.
    fn kind(&self) -> &str {
        self.name.split_once('-').map_or("", |(_, kind)| kind)
    }
}

/// How two runs of one seed agree on the order.
#[derive(Default)]
struct Agreement {
    run_pairs: usize,
    same_player: usize,
    common_chunks: usize,
    /// Pairs of neighbouring chunks (8-connected) both runs decorated.
    neighbour_pairs: usize,
    /// Those the two runs decorated in opposite orders.
    opposite: usize,
    /// Chunks whose set of neighbours decorated before them differs between the runs.
    other_neighbours_first: usize,
}

/// How a replay's answers compare with the chunk decorated on its own, and with another run's.
#[derive(Default)]
struct Changes {
    chunks: usize,
    /// Chunks whose target calls differ from those decorated on their own.
    differ_from_alone: usize,
    /// Those whose differing calls include one that made a container.
    containers_differ: usize,
    /// Chunks two runs both decorated, and those whose replayed calls differ between them.
    common_with_other_run: usize,
    differ_between_runs: usize,
    /// Differing chunks by distance from the player's chunk (Chebyshev, in chunks).
    by_distance: BTreeMap<&'static str, usize>,
    /// Calls that differ from the chunk on its own, by placed feature.
    by_feature: BTreeMap<String, usize>,
}

pub fn run(args: &OrderReport) -> Result<()> {
    let out = repo_root().join("oracle-out");
    let mut versions = Vec::new();
    for entry in fs::read_dir(&out).with_context(|| format!("no {}", out.display()))? {
        let dir = entry?.path();
        let name = dir
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        if dir.join("order").is_dir() && (args.versions.is_empty() || args.versions.contains(&name))
        {
            versions.push((name, dir));
        }
    }
    if versions.is_empty() {
        bail!("no recordings; run cargo xtask oracle order first");
    }
    versions.sort();
    for (version, dir) in versions {
        report(&version, &dir)?;
    }
    Ok(())
}

/// One version's tables.
fn report(version: &str, dir: &Path) -> Result<()> {
    println!("## {version}\n");
    let recordings = read_recordings(&dir.join("order"))?;

    println!(
        "| Seed | Runs | Startup chunks | Join chunks | Same spawn chunk | Neighbour pairs in opposite order | Chunks with other neighbours first |"
    );
    println!(
        "| ---- | ---- | -------------- | ----------- | ---------------- | --------------------------------- | ---------------------------------- |"
    );
    let mut total = Agreement::default();
    for (seed, runs) in &recordings {
        let joins: Vec<&Recording> = runs.iter().filter(|r| r.player.is_some()).collect();
        let agreement = agree(&joins);
        let startup = runs.iter().map(|r| r.startup).max().unwrap_or(0);
        let decorated = joins.iter().map(|r| r.index.len()).max().unwrap_or(0);
        println!(
            "| {seed} | {} | {startup} | {decorated} | {}/{} | {} | {} |",
            runs.len(),
            agreement.same_player,
            agreement.run_pairs,
            share(agreement.opposite, agreement.neighbour_pairs),
            share(agreement.other_neighbours_first, agreement.common_chunks),
        );
        total.add(&agreement);
    }
    println!(
        "| all | | | | {}/{} | {} | {} |\n",
        total.same_player,
        total.run_pairs,
        share(total.opposite, total.neighbour_pairs),
        share(total.other_neighbours_first, total.common_chunks),
    );

    let replays = dir.join("order-features");
    if !replays.is_dir() {
        println!("No replays; run cargo xtask oracle replay.\n");
        return Ok(());
    }
    let changes = compare_replays(&replays, &recordings)?;
    println!("| Seed | Chunks | Differ from on its own | With a container | Differ between runs |");
    println!("| ---- | ------ | ---------------------- | ---------------- | ------------------- |");
    let mut total = Changes::default();
    for (seed, c) in &changes {
        println!(
            "| {seed} | {} | {} | {} | {} |",
            c.chunks,
            share(c.differ_from_alone, c.chunks),
            c.containers_differ,
            share(c.differ_between_runs, c.common_with_other_run),
        );
        total.add(c);
    }
    println!(
        "| all | {} | {} | {} | {} |\n",
        total.chunks,
        share(total.differ_from_alone, total.chunks),
        total.containers_differ,
        share(total.differ_between_runs, total.common_with_other_run),
    );
    println!("Chunks that differ from on their own, by distance from the spawn chunk:\n");
    for (bucket, count) in &total.by_distance {
        println!("- {bucket}: {count}");
    }
    println!("\nCalls that differ from on their own, by placed feature:\n");
    let mut features: Vec<_> = total.by_feature.iter().collect();
    features.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (feature, count) in features {
        println!("- {feature}: {count}");
    }
    println!();
    Ok(())
}

/// Every seed's recordings, by seed folder name.
fn read_recordings(dir: &Path) -> Result<BTreeMap<String, Vec<Recording>>> {
    let mut seeds = BTreeMap::new();
    for entry in fs::read_dir(dir)? {
        let seed_dir = entry?.path();
        if !seed_dir.is_dir() {
            continue;
        }
        let seed = seed_dir
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let mut runs = Vec::new();
        for file in json_files(&seed_dir)? {
            runs.push(read_recording(&file)?);
        }
        runs.sort_by(|a, b| a.name.cmp(&b.name));
        seeds.insert(seed, runs);
    }
    Ok(seeds)
}

fn json_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().is_some_and(|e| e == "json") {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

fn read_json(file: &Path) -> Result<Value> {
    let text =
        fs::read_to_string(file).with_context(|| format!("failed to read {}", file.display()))?;
    serde_json::from_str(&text).with_context(|| format!("{} isn't JSON", file.display()))
}

fn read_recording(file: &Path) -> Result<Recording> {
    let json = read_json(file)?;
    let name = stem(file);
    let player = json.pointer("/player/chunk").map(chunk).transpose()?;
    let features = json["features"]
        .as_array()
        .with_context(|| format!("{} has no features array", file.display()))?;
    let mut index = HashMap::new();
    let mut startup = 0;
    for (i, step) in features.iter().enumerate() {
        index.insert(chunk(&step["chunk"])?, i);
        if step["startup"].as_bool() == Some(true) {
            startup += 1;
        }
    }
    Ok(Recording {
        name,
        player,
        index,
        startup,
    })
}

fn stem(file: &Path) -> String {
    file.file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

fn chunk(value: &Value) -> Result<Chunk> {
    let coordinate = |i: usize| -> Result<i32> {
        value[i]
            .as_i64()
            .and_then(|v| i32::try_from(v).ok())
            .with_context(|| format!("{value} isn't a chunk position"))
    };
    Ok((coordinate(0)?, coordinate(1)?))
}

/// The agreement between every two runs of the same kind.
fn agree(runs: &[&Recording]) -> Agreement {
    let mut agreement = Agreement::default();
    for (i, a) in runs.iter().enumerate() {
        for b in &runs[i + 1..] {
            if a.kind() == b.kind() {
                agreement.add(&agree_pair(a, b));
            }
        }
    }
    agreement
}

fn agree_pair(a: &Recording, b: &Recording) -> Agreement {
    let mut agreement = Agreement {
        run_pairs: 1,
        same_player: usize::from(a.player == b.player),
        ..Agreement::default()
    };
    let common: BTreeSet<Chunk> = a
        .index
        .keys()
        .filter(|c| b.index.contains_key(c))
        .copied()
        .collect();
    agreement.common_chunks = common.len();
    // Each unordered neighbouring pair once: the neighbour east, south, south-east and
    // north-east of each chunk.
    for &(x, z) in &common {
        for (dx, dz) in [(1, 0), (0, 1), (1, 1), (1, -1)] {
            let other = (x + dx, z + dz);
            if common.contains(&other) {
                agreement.neighbour_pairs += 1;
                let a_first = a.index[&(x, z)] < a.index[&other];
                let b_first = b.index[&(x, z)] < b.index[&other];
                if a_first != b_first {
                    agreement.opposite += 1;
                }
            }
        }
    }
    for &c in &common {
        if neighbours_first(a, c) != neighbours_first(b, c) {
            agreement.other_neighbours_first += 1;
        }
    }
    agreement
}

/// The neighbours of `c` decorated before it, as offsets.
fn neighbours_first(run: &Recording, c: Chunk) -> BTreeSet<Chunk> {
    let own = run.index[&c];
    let mut first = BTreeSet::new();
    for dz in -1..=1 {
        for dx in -1..=1 {
            if (dx, dz) != (0, 0)
                && run
                    .index
                    .get(&(c.0 + dx, c.1 + dz))
                    .is_some_and(|&i| i < own)
            {
                first.insert((dx, dz));
            }
        }
    }
    first
}

/// Each seed's replays compared with the chunks on their own and with each other.
fn compare_replays(
    dir: &Path,
    recordings: &BTreeMap<String, Vec<Recording>>,
) -> Result<BTreeMap<String, Changes>> {
    let mut all = BTreeMap::new();
    for (seed, runs) in recordings {
        let seed_dir = dir.join(seed);
        if !seed_dir.is_dir() {
            continue;
        }
        let mut changes = Changes::default();
        // Each run's replayed calls, by chunk.
        let mut replayed: Vec<BTreeMap<Chunk, Value>> = Vec::new();
        for file in json_files(&seed_dir)? {
            let json = read_json(&file)?;
            let name = stem(&file);
            let player = runs.iter().find(|r| r.name == name).and_then(|r| r.player);
            let mut calls = BTreeMap::new();
            for entry in json["chunks"].as_array().into_iter().flatten() {
                let at = chunk(&entry["chunk"])?;
                let alone = &entry["tries"];
                let ordered = &entry["replayed"];
                changes.chunks += 1;
                if alone != ordered {
                    changes.differ_from_alone += 1;
                    let differing = differing_calls(alone, ordered);
                    if differing
                        .iter()
                        .any(|call| call.get("containers").is_some())
                    {
                        changes.containers_differ += 1;
                    }
                    for call in differing {
                        let feature = call["feature"].as_str().unwrap_or("?").to_owned();
                        *changes.by_feature.entry(feature).or_default() += 1;
                    }
                    if let Some(player) = player {
                        let distance = (at.0 - player.0).abs().max((at.1 - player.1).abs());
                        *changes.by_distance.entry(bucket(distance)).or_default() += 1;
                    }
                }
                calls.insert(at, ordered.clone());
            }
            replayed.push(calls);
        }
        for (i, a) in replayed.iter().enumerate() {
            for b in &replayed[i + 1..] {
                for (at, calls) in a {
                    if let Some(other) = b.get(at) {
                        changes.common_with_other_run += 1;
                        if calls != other {
                            changes.differ_between_runs += 1;
                        }
                    }
                }
            }
        }
        all.insert(seed.clone(), changes);
    }
    Ok(all)
}

/// The calls in one list and not the other, either way, counting repeats.
fn differing_calls<'a>(a: &'a Value, b: &'a Value) -> Vec<&'a Value> {
    let a = a.as_array().map_or(&[][..], Vec::as_slice);
    let b = b.as_array().map_or(&[][..], Vec::as_slice);
    let mut left: Vec<&Value> = a.iter().collect();
    let mut differing = Vec::new();
    for call in b {
        if let Some(i) = left.iter().position(|c| *c == call) {
            left.remove(i);
        } else {
            differing.push(call);
        }
    }
    differing.extend(left);
    differing
}

fn bucket(distance: i32) -> &'static str {
    match distance {
        ..=2 => "0 to 2",
        3..=5 => "3 to 5",
        6..=10 => "6 to 10",
        _ => "over 10",
    }
}

/// `part` of `whole`, with its percentage.
fn share(part: usize, whole: usize) -> String {
    if whole == 0 {
        return "-".to_owned();
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "counts far below 2^52, shown to one decimal"
    )]
    let percent = 100.0 * part as f64 / whole as f64;
    format!("{part}/{whole} ({percent:.1}%)")
}

impl Agreement {
    fn add(&mut self, other: &Agreement) {
        self.run_pairs += other.run_pairs;
        self.same_player += other.same_player;
        self.common_chunks += other.common_chunks;
        self.neighbour_pairs += other.neighbour_pairs;
        self.opposite += other.opposite;
        self.other_neighbours_first += other.other_neighbours_first;
    }
}

impl Changes {
    fn add(&mut self, other: &Changes) {
        self.chunks += other.chunks;
        self.differ_from_alone += other.differ_from_alone;
        self.containers_differ += other.containers_differ;
        self.common_with_other_run += other.common_with_other_run;
        self.differ_between_runs += other.differ_between_runs;
        for (bucket, count) in &other.by_distance {
            *self.by_distance.entry(bucket).or_default() += count;
        }
        for (feature, count) in &other.by_feature {
            *self.by_feature.entry(feature.clone()).or_default() += count;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recording(name: &str, order: &[Chunk]) -> Recording {
        Recording {
            name: name.to_owned(),
            player: Some((0, 0)),
            index: order.iter().enumerate().map(|(i, &c)| (c, i)).collect(),
            startup: 0,
        }
    }

    #[test]
    fn opposite_pairs_and_neighbour_sets() {
        // Two chunks side by side, decorated in opposite orders, and a third diagonal to both
        // that's last in each.
        let a = recording("run1", &[(0, 0), (1, 0), (1, 1)]);
        let b = recording("run2", &[(1, 0), (0, 0), (1, 1)]);
        let agreement = agree_pair(&a, &b);
        assert_eq!(agreement.common_chunks, 3);
        // (0,0)-(1,0), (0,0)-(1,1), (1,0)-(1,1).
        assert_eq!(agreement.neighbour_pairs, 3);
        assert_eq!(agreement.opposite, 1);
        // (0,0) and (1,0) swap; (1,1) has both first in each.
        assert_eq!(agreement.other_neighbours_first, 2);
        assert_eq!(agreement.same_player, 1);
    }

    #[test]
    fn kinds_pair_runs_of_the_same_setup() {
        assert_eq!(recording("run1", &[]).kind(), "");
        assert_eq!(recording("run2-cpus2", &[]).kind(), "cpus2");
    }

    #[test]
    fn differing_calls_count_repeats_both_ways() {
        let a: Value = serde_json::json!([{"feature": "x"}, {"feature": "x"}, {"feature": "y"}]);
        let b: Value = serde_json::json!([{"feature": "x"}, {"feature": "z"}]);
        let differing: Vec<_> = differing_calls(&a, &b)
            .into_iter()
            .map(|c| c["feature"].as_str().unwrap_or_default())
            .collect();
        assert_eq!(differing, ["z", "x", "y"]);
    }

    #[test]
    fn shares_show_a_percentage() {
        assert_eq!(share(1, 4), "1/4 (25.0%)");
        assert_eq!(share(0, 0), "-");
    }
}
