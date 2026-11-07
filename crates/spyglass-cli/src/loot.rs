//! `spyglass loot`: loot containers and what they hold (`docs/specs/locate.md#loot`).

use anyhow::{Result, bail};
use spyglass::{FoundContainer, Item, ItemQuery, World};

use crate::args::{Enchantment, Loot, Origin};
use crate::{Out, SHOWN, json};

/// The most chunks `loot in` may ask for: each placed chunk takes a while.
const MAX_CHUNKS: i64 = 1024;

pub fn run(what: &Loot) -> Result<()> {
    let common = what.common();
    let player = matches!(what, Loot::Find { from, .. } if matches!(from.from, Origin::Player));
    // A region that's too large fails before anything loads.
    if let Loot::In { min, max, .. } = what {
        chunks_of(*min, *max)?;
    }
    let (world, seed, report) = crate::load(common, player)?;
    let session = world.session(seed)?;
    let out = Out {
        session: &session,
        json: common.json,
        report: report.as_ref(),
    };
    match what {
        Loot::At { position, .. } => at(&out, *position)?,
        Loot::In {
            min,
            max,
            item,
            enchantment,
            ..
        } => {
            let query = match item {
                Some(item) => Some(item_query(&world, item, enchantment)?),
                None => None,
            };
            within(&out, *min, *max, query.as_ref())?;
        }
        Loot::Find {
            item,
            enchantment,
            from,
            radius,
            ..
        } => {
            let query = item_query(&world, item, enchantment)?;
            find(&out, &query, out.origin(from.from)?, *radius)?;
        }
    }
    out.warn();
    Ok(())
}

/// The chunks of the region from `min` to `max` in blocks: the least chunk and the greatest.
fn chunks_of(min: [i32; 2], max: [i32; 2]) -> Result<([i32; 2], [i32; 2])> {
    let (low, high) = ([min[0] >> 4, min[1] >> 4], [max[0] >> 4, max[1] >> 4]);
    let size = [high[0] - low[0] + 1, high[1] - low[1] + 1].map(i64::from);
    if min[0] > max[0] || min[1] > max[1] {
        bail!("--max must be at least --min on both axes");
    }
    if size[0].saturating_mul(size[1]) > MAX_CHUNKS {
        bail!(
            "{} × {} chunks is more than {MAX_CHUNKS}; ask for a smaller region",
            size[0],
            size[1]
        );
    }
    Ok((low, high))
}

fn item_query(world: &World, item: &str, enchantment: &Enchantment) -> Result<ItemQuery> {
    let wanted = enchantment
        .enchantment
        .as_deref()
        .map(|e| (e, enchantment.level));
    Ok(world.item_query(item, wanted)?)
}

/// `loot at`: the containers at a block. A dungeon's or a ruined portal's chest can be beside
/// the chunk that places it, so the chunk's neighbours are placed too.
fn at(out: &Out<'_>, position: [i32; 3]) -> Result<()> {
    let [x, y, z] = position;
    let (cx, cz) = (x >> 4, z >> 4);
    let found = out
        .session
        .containers_in([cx - 1, cz - 1], [cx + 1, cz + 1])?;
    let here: Vec<&FoundContainer> = found.iter().filter(|c| c.position == position).collect();
    if out.json {
        return out.print(
            "loot at",
            json::LootAtAnswer {
                position,
                containers: here.iter().map(|c| json::Container::new(c, None)).collect(),
            },
        );
    }
    out!("loot at {x} {y} {z}{}", out.context(None));
    if here.is_empty() {
        out!("  no loot container spyglass places here");
    }
    for c in &here {
        container_line(c, None);
        match &c.contents {
            Some(slots) if slots.is_empty() => out!("      empty"),
            Some(slots) => {
                for s in slots {
                    out!("      slot {:>2}: {}", s.slot, describe(&s.item));
                }
            }
            None => {}
        }
    }
    notes(out, &here);
    Ok(())
}

/// `loot in`: the containers in a region, only those holding `query`'s item if given.
fn within(out: &Out<'_>, min: [i32; 2], max: [i32; 2], query: Option<&ItemQuery>) -> Result<()> {
    let (low, high) = chunks_of(min, max)?;
    let found = out.session.containers_in(low, high)?;
    let hits: Vec<&FoundContainer> = found
        .iter()
        .filter(|c| {
            let [x, _, z] = c.position;
            (min[0]..=max[0]).contains(&x) && (min[1]..=max[1]).contains(&z)
        })
        .filter(|c| query.is_none_or(|q| c.holds(q)))
        .collect();
    if out.json {
        return out.print(
            "loot in",
            json::LootInAnswer {
                min,
                max,
                item: query.map(ItemQuery::argument),
                containers: hits.iter().map(|c| json::Container::new(c, None)).collect(),
            },
        );
    }
    let what = query.map_or("loot containers".to_string(), |q| {
        format!("containers holding {}", q.argument())
    });
    out!(
        "{what} from {} {} to {} {}{}",
        min[0],
        min[1],
        max[0],
        max[1],
        out.context(None)
    );
    if hits.is_empty() {
        out!("  none");
    }
    for c in &hits {
        container_line(c, None);
        contents_line(c, query);
    }
    notes(out, &hits);
    Ok(())
}

/// `loot find`: the nearest container holding `query`'s item.
fn find(out: &Out<'_>, query: &ItemQuery, from: [i32; 3], radius: i32) -> Result<()> {
    let found = out.session.nearest_container(query, from, radius)?;
    if out.json {
        return out.print(
            "loot find",
            json::LootFindAnswer {
                item: query.argument(),
                from,
                radius,
                complete: found.complete,
                hits: found
                    .hits
                    .iter()
                    .map(|c| json::Container::new(c, Some(from)))
                    .collect(),
            },
        );
    }
    out!("nearest {}{}", query.argument(), out.context(Some(from)));
    if found.hits.is_empty() {
        out!("  none within {radius} chunks");
        return Ok(());
    }
    for c in found.hits.iter().take(SHOWN) {
        container_line(c, Some(from));
        contents_line(c, Some(query));
    }
    if found.hits.len() > SHOWN {
        out!(
            "  ... and {} more (--json lists all)",
            found.hits.len() - SHOWN
        );
    }
    let hits: Vec<&FoundContainer> = found.hits.iter().collect();
    notes(out, &hits);
    Ok(())
}

/// One container, as text, and why its contents might not be the game's.
fn container_line(c: &FoundContainer, from: Option<[i32; 3]>) {
    let [x, y, z] = c.position;
    let away = from.map_or(String::new(), |origin| {
        let (dx, dz) = (f64::from(x - origin[0]), f64::from(z - origin[2]));
        format!(" {:>7} blocks", (dx * dx + dz * dz).sqrt() as i32)
    });
    let kind = if c.certain { "certain" } else { "candidate" };
    let by = match &c.source {
        spyglass::ContainerSource::Structure(id) | spyglass::ContainerSource::Feature(id) => id,
        _ => "",
    };
    let why = c
        .reason
        .as_deref()
        .map_or(String::new(), |r| format!(" ({r})"));
    out!(
        "  {x:>9} {y:>4} {z:<9}{away}  {kind:<10} {} in {by}, {}{why}",
        c.holder,
        c.table
    );
    if let Some(doubt) = &c.contents_doubt {
        out!("      contents: {doubt}");
    }
}

/// A container's contents on one line, identical stacks added up, what `query` names first.
fn contents_line(c: &FoundContainer, query: Option<&ItemQuery>) {
    let Some(slots) = &c.contents else {
        return;
    };
    if slots.is_empty() {
        out!("      empty");
        return;
    }
    // Each kind of stack (the item and its components) with its total, in first-slot order.
    let mut totals: Vec<(&Item, i32)> = Vec::new();
    for s in slots {
        let same = |i: &&Item| with_count(i, 0) == with_count(&s.item, 0);
        match totals.iter_mut().find(|(i, _)| same(i)) {
            Some((_, total)) => *total += s.item.count,
            None => totals.push((&s.item, s.item.count)),
        }
    }
    totals.sort_by_key(|(i, _)| !query.is_some_and(|q| q.matches(i)));
    let text: Vec<String> = totals
        .iter()
        .map(|(i, total)| describe(&with_count(i, *total)))
        .collect();
    out!("      {}", text.join(", "));
}

/// `item` with `count` of it.
fn with_count(item: &Item, count: i32) -> Item {
    let mut item = item.clone();
    item.count = count;
    item
}

/// A stack, as text: `3 × minecraft:bread`, with the components its table set.
fn describe(item: &Item) -> String {
    let levels = |list: &[(String, i32)]| {
        list.iter()
            .map(|(e, l)| format!("{e} {l}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut extra = Vec::new();
    if !item.enchantments.is_empty() {
        extra.push(levels(&item.enchantments));
    }
    if !item.stored_enchantments.is_empty() {
        extra.push(levels(&item.stored_enchantments));
    }
    if let Some(damage) = item.damage {
        extra.push(format!("damage {damage}"));
    }
    if let Some(potion) = &item.potion {
        extra.push(potion.clone());
    }
    for (effect, ticks) in &item.stew_effects {
        extra.push(format!("{effect} for {ticks} ticks"));
    }
    if let Some(instrument) = &item.instrument {
        extra.push(instrument.clone());
    }
    if let Some(name) = &item.name {
        extra.push(format!("named {name}"));
    }
    if let Some(amplifier) = item.ominous_bottle_amplifier {
        extra.push(format!("amplifier {amplifier}"));
    }
    if let Some(map) = &item.map {
        extra.push(format!("to {} {}", map.target[0], map.target[1]));
    }
    let extra = if extra.is_empty() {
        String::new()
    } else {
        format!(" ({})", extra.join("; "))
    };
    format!("{} × {}{extra}", item.count, item.id)
}

/// What the answers mean, after them.
fn notes(out: &Out<'_>, hits: &[&FoundContainer]) {
    if hits.is_empty() {
        return;
    }
    out!("Certain is for the chunk decorated before its neighbours.");
    if hits.iter().any(|c| !c.certain) {
        out!(
            "Candidates are where something spyglass can't know could change the container or its seed."
        );
    }
    if out.report.is_some() {
        out!(
            "Contents are as the seed generates them: a container already opened, or in a chunk generated by another version, may hold something else."
        );
    }
}
