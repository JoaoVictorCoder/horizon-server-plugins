//! Edits around the player: filling holes, liquids, snow, grass, fire,
//! clearing above and below, removing mobs and items, and a calculator.

use crate::pattern::{Mask, Pattern, is_air, is_passable, kind_of};
use crate::{HorizonEdit, MAX_Y, MIN_Y, Pos, add, boxed, flags, here, msg, number};
use horizon_plugin_api::prelude::*;
use horizon_plugin_api::types::Region;
use std::collections::{HashMap, HashSet, VecDeque};

type Result = std::result::Result<(), String>;

fn radius(a: Option<&String>, default: i32, max: i32) -> std::result::Result<i32, String> {
    let r = number(a, default)?;
    if !(1..=max).contains(&r) {
        return Err(format!("The radius is 1 to {max}."));
    }
    Ok(r)
}

/// The cube of radius `r` around the player, clamped to the world, read.
fn around(e: &HorizonEdit, p: &Player, r: i32) -> std::result::Result<(Pos, Pos, Region), String> {
    let c = here(p);
    let lo = [c[0] - r, (c[1] - r).max(MIN_Y), c[2] - r];
    let hi = [c[0] + r, (c[1] + r).min(MAX_Y), c[2] + r];
    let region = e.read(p, lo, hi)?;
    Ok((lo, hi, region))
}

/// The air cells reachable from the player's feet without going up (or,
/// `under`, going up but never above their feet), within `r` across and
/// `depth` down.
fn holes(region: &Region, start: Pos, r: i32, depth: i32, under: bool) -> Vec<Pos> {
    let mut seen = HashSet::new();
    let mut queue = VecDeque::from([start]);
    let mut out = Vec::new();
    while let Some(q) = queue.pop_front() {
        if !seen.insert(q) {
            continue;
        }
        let horizontal = (q[0] - start[0]).pow(2) + (q[2] - start[2]).pow(2);
        if horizontal > r * r || q[1] > start[1] || q[1] <= start[1] - depth {
            continue;
        }
        if !region.get(q[0], q[1], q[2]).is_some_and(is_air) {
            continue;
        }
        out.push(q);
        let mut next = vec![[1, 0, 0], [-1, 0, 0], [0, 0, 1], [0, 0, -1], [0, -1, 0]];
        if under {
            next.push([0, 1, 0]);
        }
        for o in next {
            queue.push_back(add(q, o));
        }
    }
    out
}

fn fill_cmd(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String], under: bool) -> Result {
    let pattern = Pattern::parse(a.first().ok_or("Usage: //fill <pattern> <radius> [depth]")?)?;
    let r = radius(a.get(1), 0, 64).map_err(|_| "Usage: //fill <pattern> <radius> [depth] (radius 1 to 64)".to_owned())?;
    let depth = number(a.get(2), r)?.clamp(1, 128);
    let start = here(p);
    let region = e.read(p, [start[0] - r, start[1] - depth, start[2] - r], [start[0] + r, start[1], start[2] + r])?;
    let cells = holes(&region, start, r, depth, under);
    let changes = e.fill_with(cells, &pattern);
    let n = e.apply(ctx, p, changes)?;
    msg(p, &format!("{n} blocks have been created."));
    Ok(())
}

pub fn fill(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    fill_cmd(e, ctx, p, a, false)
}

pub fn fillr(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    fill_cmd(e, ctx, p, a, true)
}

/// Replaces, within `r` of the player, every block `change` maps.
fn change_around(
    e: &mut HorizonEdit,
    ctx: &mut Ctx<HorizonEdit>,
    p: &Player,
    r: i32,
    mut change: impl FnMut(&Region, Pos, &str) -> Option<String>,
) -> std::result::Result<usize, String> {
    let (lo, hi, region) = around(e, p, r)?;
    let changes: Vec<(Pos, String)> = boxed(lo, hi)
        .filter_map(|q| {
            let b = region.get(q[0], q[1], q[2])?;
            change(&region, q, b).map(|to| (q, to))
        })
        .collect();
    e.apply(ctx, p, changes)
}

/// The level property of a liquid's state (0 for a source, or none).
fn level(state: &str) -> i32 {
    state
        .split("level=")
        .nth(1)
        .and_then(|v| v.trim_end_matches(']').split(',').next())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

pub fn drain(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let r = radius(a.first(), 5, 32)?;
    let n = change_around(e, ctx, p, r, |_, _, b| {
        if matches!(kind_of(b), "minecraft:water" | "minecraft:lava" | "minecraft:bubble_column") {
            Some("minecraft:air".into())
        } else if b.contains("waterlogged=true") {
            Some(b.replace("waterlogged=true", "waterlogged=false"))
        } else {
            None
        }
    })?;
    msg(p, &format!("{n} blocks have been drained."));
    Ok(())
}

fn fix(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String], liquid: &str) -> Result {
    let r = radius(a.first(), 10, 64)?;
    let kind = format!("minecraft:{liquid}");
    let n = change_around(e, ctx, p, r, |_, _, b| {
        (kind_of(b) == kind && level(b) != 0).then(|| kind.clone())
    })?;
    msg(p, &format!("{n} blocks have been changed."));
    Ok(())
}

pub fn fixwater(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    fix(e, ctx, p, a, "water")
}

pub fn fixlava(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    fix(e, ctx, p, a, "lava")
}

fn remove_column(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String], up: bool) -> Result {
    let size = number(a.first(), 1)?.clamp(1, 64) - 1;
    let c = here(p);
    let (from, to) = if up {
        (c[1] + 2, (c[1] + 1 + number(a.get(1), MAX_Y - c[1])?.max(1)).min(MAX_Y))
    } else {
        ((c[1] - number(a.get(1), c[1] - MIN_Y)?.max(1)).max(MIN_Y), c[1] - 1)
    };
    if from > to {
        return Err("Nothing to remove there.".into());
    }
    let changes = boxed([c[0] - size, from, c[2] - size], [c[0] + size, to, c[2] + size])
        .map(|q| (q, "minecraft:air".to_owned()))
        .collect();
    let n = e.apply(ctx, p, changes)?;
    msg(p, &format!("{n} blocks have been removed."));
    Ok(())
}

pub fn removeabove(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    remove_column(e, ctx, p, a, true)
}

pub fn removebelow(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    remove_column(e, ctx, p, a, false)
}

pub fn removenear(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let mask = Mask::parse(a.first().ok_or("Usage: //removenear <mask> [size]")?)?;
    let r = radius(a.get(1), 50, 64)?;
    let n = change_around(e, ctx, p, r, |reg, q, b| {
        mask.test(b, reg.get(q[0], q[1] - 1, q[2]), reg.get(q[0], q[1] + 1, q[2])).then(|| "minecraft:air".to_owned())
    })?;
    msg(p, &format!("{n} blocks have been removed."));
    Ok(())
}

pub fn replacenear(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let [size, from, to] = a else {
        return Err("Usage: //replacenear <size> <from> <to>".into());
    };
    let r = radius(Some(size), 1, 64)?;
    let mask = Mask::parse(from)?;
    let pattern = Pattern::parse(to)?;
    let mut rng = *e.rng();
    let n = change_around(e, ctx, p, r, |reg, q, b| {
        mask.test(b, reg.get(q[0], q[1] - 1, q[2]), reg.get(q[0], q[1] + 1, q[2]))
            .then(|| pattern.pick(&mut rng).to_owned())
    })?;
    msg(p, &format!("{n} blocks have been replaced."));
    Ok(())
}

/// Within `r` across, the highest block of each column (up to `r` above
/// the player) that isn't air.
fn column_tops(region: &Region, lo: Pos, hi: Pos, c: Pos, r: i32) -> Vec<Pos> {
    let mut out = Vec::new();
    for z in lo[2]..=hi[2] {
        for x in lo[0]..=hi[0] {
            if (x - c[0]).pow(2) + (z - c[2]).pow(2) > r * r {
                continue;
            }
            if let Some(y) = (lo[1]..=hi[1]).rev().find(|&y| region.get(x, y, z).is_some_and(|b| !is_air(b))) {
                out.push([x, y, z]);
            }
        }
    }
    out
}

pub fn snow(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let r = radius(a.first(), 10, 64)?;
    let (lo, hi, region) = around(e, p, r)?;
    let mut changes = Vec::new();
    for t in column_tops(&region, lo, hi, here(p), r) {
        let b = region.get(t[0], t[1], t[2]).unwrap_or_default();
        if kind_of(b) == "minecraft:water" && level(b) == 0 {
            changes.push((t, "minecraft:ice".to_owned()));
        } else if !is_passable(b) && !matches!(kind_of(b), "minecraft:ice" | "minecraft:packed_ice") && t[1] < MAX_Y {
            changes.push(([t[0], t[1] + 1, t[2]], "minecraft:snow".to_owned()));
        }
    }
    let n = e.apply(ctx, p, changes)?;
    msg(p, &format!("{n} surfaces covered. Let it snow~"));
    Ok(())
}

pub fn thaw(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let r = radius(a.first(), 10, 64)?;
    let n = change_around(e, ctx, p, r, |_, _, b| match kind_of(b) {
        "minecraft:snow" => Some("minecraft:air".into()),
        "minecraft:ice" => Some("minecraft:water".into()),
        _ => None,
    })?;
    msg(p, &format!("{n} surfaces thawed."));
    Ok(())
}

pub fn green(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let r = radius(a.first(), 10, 64)?;
    let n = change_around(e, ctx, p, r, |reg, q, b| {
        let open = reg.get(q[0], q[1] + 1, q[2]).is_none_or(is_passable);
        (kind_of(b) == "minecraft:dirt" && open).then(|| "minecraft:grass_block".to_owned())
    })?;
    msg(p, &format!("{n} surfaces greened."));
    Ok(())
}

pub fn extinguish(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let r = radius(a.first(), 40, 64)?;
    let n = change_around(e, ctx, p, r, |_, _, b| {
        matches!(kind_of(b), "minecraft:fire" | "minecraft:soul_fire").then(|| "minecraft:air".to_owned())
    })?;
    msg(p, &format!("{n} fires put out."));
    Ok(())
}

/// Mobs `//butcher` kills, and with `-a` the animals too.
const HOSTILE: &[&str] = &[
    "zombie", "zombie_villager", "husk", "drowned", "skeleton", "stray", "bogged", "wither_skeleton",
    "creeper", "spider", "cave_spider", "enderman", "endermite", "silverfish", "witch", "slime",
    "magma_cube", "blaze", "ghast", "phantom", "pillager", "vindicator", "evoker", "ravager", "vex",
    "guardian", "elder_guardian", "shulker", "hoglin", "zoglin", "piglin_brute", "zombified_piglin",
    "breeze", "creaking",
];
const ANIMALS: &[&str] = &[
    "cow", "pig", "sheep", "chicken", "horse", "donkey", "mule", "rabbit", "goat", "llama", "fox",
    "wolf", "cat", "ocelot", "parrot", "bee", "turtle", "frog", "armadillo", "camel", "sniffer",
    "mooshroom", "panda", "polar_bear", "squid", "glow_squid", "cod", "salmon", "tropical_fish",
    "pufferfish", "axolotl", "dolphin", "bat", "strider",
];

pub fn butcher(_: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let (rest, f) = flags(a);
    let r = number(rest.first(), 32)?.clamp(1, 512);
    let at = &p.location;
    let mut kinds: Vec<&str> = HOSTILE.to_vec();
    if f.contains('a') {
        kinds.extend(ANIMALS);
    }
    let w = p.world();
    for k in &kinds {
        w.kill_entities(&format!("type=minecraft:{k},distance=..{r}"), at);
    }
    msg(p, &format!("Killed the {} mobs within {r} blocks.", if f.contains('a') { "hostile and passive" } else { "hostile" }));
    Ok(())
}

/// What `//remove` takes besides any entity type.
pub const REMOVABLE: &[&str] = &["items", "arrows", "xp", "boats", "minecarts", "tnt", "falling_blocks", "paintings", "itemframes"];

pub fn remove(_: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let (Some(kind), Some(r)) = (a.first(), a.get(1)) else {
        return Err(format!("Usage: //remove <type> <radius> (type: {}, or an entity id)", REMOVABLE.join(", ")));
    };
    let r = number(Some(r), 0)?.clamp(1, 512);
    let types: Vec<String> = match kind.as_str() {
        "items" => vec!["item".into()],
        "arrows" => vec!["arrow".into(), "spectral_arrow".into()],
        "xp" => vec!["experience_orb".into()],
        "boats" => vec!["oak_boat".into(), "spruce_boat".into(), "birch_boat".into(), "jungle_boat".into(), "acacia_boat".into(), "dark_oak_boat".into(), "mangrove_boat".into(), "cherry_boat".into(), "bamboo_raft".into()],
        "minecarts" => vec!["minecart".into(), "chest_minecart".into(), "hopper_minecart".into(), "tnt_minecart".into(), "furnace_minecart".into()],
        "tnt" => vec!["tnt".into()],
        "falling_blocks" => vec!["falling_block".into()],
        "paintings" => vec!["painting".into()],
        "itemframes" => vec!["item_frame".into(), "glow_item_frame".into()],
        "player" | "minecraft:player" => return Err("Players can't be removed.".into()),
        other => vec![other.trim_start_matches("minecraft:").to_owned()],
    };
    let w = p.world();
    for t in &types {
        w.kill_entities(&format!("type=minecraft:{t},distance=..{r}"), &p.location);
    }
    msg(p, &format!("Removed the {kind} within {r} blocks."));
    Ok(())
}

pub fn calc(_: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let text = a.join(" ");
    let value = crate::expr::parse(&text)?.eval(&HashMap::new())?;
    msg(p, &format!("{text} = {}", if value.fract() == 0.0 && value.abs() < 1e15 { format!("{}", value as i64) } else { format!("{value}") }));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn liquid_levels() {
        assert_eq!(level("minecraft:water[level=0]"), 0);
        assert_eq!(level("minecraft:water[level=7]"), 7);
        assert_eq!(level("minecraft:lava"), 0);
    }
}
