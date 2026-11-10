//! Filling from hand-built tables, without game data: both versions' forms, and draws worked by
//! hand from `docs/specs/loot.md`.

use std::collections::HashMap;

use serde_json::{Value, json};
use spyglass_loot::{Loot, MapRequest, World};
use spyglass_rng::{LegacyRandomSource, RandomSource};

/// Resources keyed as `spyglass_density::Resources` reads a map: tables and item reports.
fn resources(tables: &[(&str, Value)]) -> HashMap<String, Value> {
    let mut map = HashMap::new();
    let item = |stack: i32, extra: Value| {
        let mut components = json!({
            "minecraft:max_stack_size": stack,
            "minecraft:enchantments": {},
        });
        for (k, v) in extra.as_object().unwrap() {
            components[k] = v.clone();
        }
        json!({ "components": components })
    };
    for (id, report) in [
        ("stick", item(64, json!({}))),
        ("ender_pearl", item(16, json!({}))),
        (
            "diamond_sword",
            item(
                1,
                json!({
                    "minecraft:enchantable": {"value": 10},
                    "minecraft:max_damage": 1561,
                    "minecraft:damage": 0,
                }),
            ),
        ),
        ("compass", item(64, json!({}))),
        ("map", item(64, json!({}))),
        ("filled_map", item(64, json!({}))),
    ] {
        map.insert(
            format!("reports/minecraft/components/item/{id}.json"),
            report,
        );
    }
    for (id, table) in tables {
        map.insert(format!("loot_table/minecraft:{id}"), table.clone());
    }
    map
}

/// Every block in plains; maps find their structure at (100, 200) unless told not to.
struct Fixed {
    maps_find: bool,
}

impl World for Fixed {
    fn biome(&self, _x: i32, _y: i32, _z: i32) -> Option<String> {
        Some("minecraft:plains".into())
    }

    fn map_target(&self, _request: &MapRequest) -> Option<[i32; 2]> {
        self.maps_find.then_some([100, 200])
    }
}

const WORLD: Fixed = Fixed { maps_find: true };

fn fill(resources: &HashMap<String, Value>, table: &str, seed: i64) -> Vec<Option<String>> {
    let mut loot = Loot::new();
    loot.load(resources, table).unwrap();
    loot.fill(table, seed, [0, 64, 0], 27, &WORLD)
        .unwrap()
        .into_iter()
        .map(|s| s.map(|s| format!("{s:?}")))
        .collect()
}

/// The same table in 26.2's form (`functions`, `conditions`, float numbers) and 26.3's
/// (`modifier`, `condition`, int numbers) fills the same, seed for seed.
#[test]
fn both_forms_fill_alike() {
    let lists = json!({
        "type": "minecraft:chest",
        "pools": [{
            "rolls": {"type": "minecraft:uniform", "min": 2.0, "max": 6.0},
            "bonus_rolls": 0.0,
            "entries": [
                {"type": "minecraft:item", "name": "minecraft:stick", "weight": 3,
                 "functions": [{"function": "minecraft:set_count",
                                "count": {"min": 2.0, "max": 40.0}}]},
                {"type": "minecraft:item", "name": "minecraft:ender_pearl", "weight": 2,
                 "functions": [{"function": "minecraft:set_count",
                                "count": {"type": "minecraft:uniform", "min": 10.0, "max": 30.0}}]},
                {"type": "minecraft:item", "name": "minecraft:diamond_sword",
                 "functions": [{"function": "minecraft:set_damage",
                                "damage": {"type": "minecraft:uniform", "min": 0.15, "max": 0.8}}]},
                {"type": "minecraft:empty", "weight": 2}
            ]
        }, {
            "rolls": 1.0,
            "conditions": [{"condition": "minecraft:random_chance", "chance": 0.5}],
            "entries": [{"type": "minecraft:item", "name": "minecraft:compass"}]
        }]
    });
    let single = json!({
        "type": "minecraft:chest",
        "pools": [{
            "rolls": {"type": "minecraft:uniform", "min": 2, "max": 6},
            "entries": [
                {"type": "minecraft:item", "name": "minecraft:stick", "weight": 3,
                 "modifier": {"type": "minecraft:set_count",
                              "count": {"type": "minecraft:uniform", "min": 2, "max": 40}}},
                {"type": "minecraft:item", "name": "minecraft:ender_pearl", "weight": 2,
                 "modifier": [{"type": "minecraft:set_count",
                               "count": {"type": "minecraft:uniform", "min": 10, "max": 30}}]},
                {"type": "minecraft:item", "name": "minecraft:diamond_sword",
                 "modifier": {"type": "minecraft:set_damage",
                              "damage": {"type": "minecraft:uniform", "min": 0.15, "max": 0.8}}},
                {"type": "minecraft:empty", "weight": 2}
            ]
        }, {
            "rolls": 1,
            "condition": {"type": "minecraft:random_chance", "chance": 0.5},
            "entries": [{"type": "minecraft:item", "name": "minecraft:compass"}]
        }]
    });
    let res = resources(&[("lists", lists), ("single", single)]);
    let mut filled = 0;
    for seed in 1..=500 {
        let a = fill(&res, "minecraft:lists", seed);
        assert_eq!(a, fill(&res, "minecraft:single", seed), "seed {seed}");
        filled += a.iter().flatten().count();
    }
    assert!(filled > 500);
}

/// The shuffled free slots, as the fill draws them first: `Util.shuffle` of `0..27`.
fn shuffled_slots(random: &mut LegacyRandomSource) -> Vec<usize> {
    let mut slots: Vec<usize> = (0..27).collect();
    for i in (2..=slots.len()).rev() {
        let j = random.next_int_bounded(i as i32) as usize;
        slots.swap(i - 1, j);
    }
    slots
}

/// One stack of one item, no other draws: it lands in the last of the shuffled slots (the
/// split has nothing to split, and shuffling one stack draws nothing).
#[test]
fn a_single_stack_takes_the_last_shuffled_slot() {
    let table = json!({"pools": [{"rolls": 1, "entries": [
        {"type": "minecraft:item", "name": "minecraft:stick"}
    ]}]});
    let res = resources(&[("one", table)]);
    for seed in [1, -1, 42, i64::MIN, i64::MAX] {
        let slots = shuffled_slots(&mut LegacyRandomSource::new(seed));
        let contents = fill(&res, "minecraft:one", seed);
        let filled: Vec<usize> = (0..27).filter(|&i| contents[i].is_some()).collect();
        assert_eq!(filled, [*slots.last().unwrap()], "seed {seed}");
    }
}

/// `set_damage` 0.5 on a diamond sword: damage `floor((1 - 0.5) × 1561)` = 780, with no draw.
/// On a compass, which can't be damaged, nothing, and the uniform isn't drawn.
#[test]
fn set_damage_by_hand() {
    let table = |item: &str| {
        json!({"pools": [{"rolls": 1, "entries": [
            {"type": "minecraft:item", "name": item, "modifier": [
                {"type": "minecraft:set_damage", "damage": 0.5}
            ]}
        ]}]})
    };
    let drawn = json!({"pools": [{"rolls": 1, "entries": [
        {"type": "minecraft:item", "name": "minecraft:compass", "modifier": [
            {"type": "minecraft:set_damage",
             "damage": {"type": "minecraft:uniform", "min": 0.1, "max": 0.9}}
        ]}
    ]}]});
    let res = resources(&[
        ("sword", table("minecraft:diamond_sword")),
        ("compass", drawn),
        ("plain_compass", table("minecraft:compass")),
    ]);
    let sword = fill(&res, "minecraft:sword", 7);
    let sword = sword.iter().flatten().next().unwrap();
    assert!(sword.contains("damage: 780"), "{sword}");
    // The compass fills as if there were no modifier: the uniform drew nothing.
    assert_eq!(
        fill(&res, "minecraft:compass", 7),
        fill(&res, "minecraft:plain_compass", 7)
    );
}

/// A stack over its item's maximum is cut by the splitter: 40 ender pearls (16 a stack) are
/// 16, 16 and 8 before the split, and 40 pearls in all after it.
#[test]
fn the_splitter_cuts_and_the_split_keeps_the_count() {
    let table = json!({"pools": [{"rolls": 1, "entries": [
        {"type": "minecraft:item", "name": "minecraft:ender_pearl",
         "modifier": {"type": "minecraft:set_count", "count": 40}}
    ]}]});
    let res = resources(&[("pearls", table)]);
    let mut loot = Loot::new();
    loot.load(&res, "minecraft:pearls").unwrap();
    for seed in 1..=50 {
        let contents = loot
            .fill("minecraft:pearls", seed, [0, 0, 0], 27, &WORLD)
            .unwrap();
        let counts: Vec<i32> = contents.iter().flatten().map(|s| s.count).collect();
        assert_eq!(counts.iter().sum::<i32>(), 40, "seed {seed}");
        assert!(counts.iter().all(|&c| (1..=16).contains(&c)), "{counts:?}");
        assert!(counts.len() >= 3);
    }
}

/// From 26.3, a map whose search finds nothing is discarded by `filtered`, and takes no slot;
/// one that finds its structure keeps its mark.
#[test]
fn a_map_that_finds_nothing_takes_no_slot() {
    let table = json!({"pools": [{"rolls": 1, "entries": [
        {"type": "minecraft:item", "name": "minecraft:map", "modifier": [
            {"type": "minecraft:exploration_map", "destination": "#minecraft:on_treasure_maps",
             "decoration": "minecraft:red_x", "zoom": 1, "skip_existing_chunks": false},
            {"type": "minecraft:filtered", "item_filter": {"predicates": {"minecraft:map_id": {}}},
             "on_fail": {"type": "minecraft:discard"}}
        ]}
    ]}]});
    let res = resources(&[("map", table)]);
    let mut loot = Loot::new();
    loot.load(&res, "minecraft:map").unwrap();
    let found = loot
        .fill("minecraft:map", 3, [0, 0, 0], 27, &WORLD)
        .unwrap();
    let map = found.iter().flatten().next().unwrap();
    assert_eq!(map.map.as_ref().unwrap().target, [100, 200]);
    let none = loot
        .fill(
            "minecraft:map",
            3,
            [0, 0, 0],
            27,
            &Fixed { maps_find: false },
        )
        .unwrap();
    assert!(none.iter().all(Option::is_none));
}

/// A table that names itself rolls nothing the second time, but its choice was drawn.
#[test]
fn a_table_reaching_itself_rolls_nothing_again() {
    let table = json!({"pools": [{"rolls": 1, "entries": [
        {"type": "minecraft:item", "name": "minecraft:stick"},
        {"type": "minecraft:loot_table", "value": "minecraft:loop"}
    ]}]});
    let res = resources(&[("loop", table)]);
    for seed in 1..=20 {
        let contents = fill(&res, "minecraft:loop", seed);
        assert!(contents.iter().flatten().count() <= 1);
    }
}

#[test]
fn a_seed_of_zero_is_refused() {
    let res = resources(&[("one", json!({"pools": []}))]);
    let mut loot = Loot::new();
    loot.load(&res, "minecraft:one").unwrap();
    assert_eq!(
        loot.fill("minecraft:one", 0, [0, 0, 0], 27, &WORLD).err(),
        Some(spyglass_loot::FillError::NoSeed)
    );
}
