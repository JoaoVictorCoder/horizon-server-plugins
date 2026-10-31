//! Editing the selection: filling, replacing, covering, smoothing,
//! hollowing, lines, trees and plants, moving and stacking.

use crate::pattern::{Mask, Pattern, chance, is_air, is_passable, kind_of};
use crate::{HorizonEdit, Placed, Pos, add, boxed, dir, flags, float, in_world, msg, number};
use horizon_plugin_api::prelude::*;
use horizon_plugin_api::types::Region;
use std::collections::{HashMap, HashSet, VecDeque};

type Result = std::result::Result<(), String>;

pub fn set(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let pattern = Pattern::parse(a.first().ok_or("Usage: //set <pattern>")?)?;
    let (lo, hi) = e.selection(p)?;
    let changes = e.fill_with(boxed(lo, hi), &pattern);
    let n = e.apply(ctx, p, changes)?;
    msg(p, &format!("Setting {n} blocks..."));
    Ok(())
}

pub fn replace(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let (mask, to) = match a {
        [to] => (Mask::parse("#existing")?, to),
        [from, to] => (Mask::parse(from)?, to),
        _ => return Err("Usage: //replace [from] <to>".into()),
    };
    let pattern = Pattern::parse(to)?;
    let (lo, hi) = e.selection(p)?;
    let region = e.read(p, [lo[0], lo[1] - 1, lo[2]], [hi[0], hi[1] + 1, hi[2]])?;
    let hit: Vec<Pos> = boxed(lo, hi)
        .filter(|q| {
            region.get(q[0], q[1], q[2]).is_some_and(|b| {
                mask.test(b, region.get(q[0], q[1] - 1, q[2]), region.get(q[0], q[1] + 1, q[2]))
            })
        })
        .collect();
    let changes = e.fill_with(hit, &pattern);
    let n = e.apply(ctx, p, changes)?;
    msg(p, &format!("Replacing {n} blocks..."));
    Ok(())
}

/// The highest block of each column of the selection that isn't air.
fn tops(region: &Region, lo: Pos, hi: Pos) -> Vec<Pos> {
    let mut out = Vec::new();
    for z in lo[2]..=hi[2] {
        for x in lo[0]..=hi[0] {
            if let Some(y) = (lo[1]..=hi[1]).rev().find(|&y| region.get(x, y, z).is_some_and(|b| !is_air(b))) {
                out.push([x, y, z]);
            }
        }
    }
    out
}

pub fn overlay(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let pattern = Pattern::parse(a.first().ok_or("Usage: //overlay <pattern>")?)?;
    let (lo, hi) = e.selection(p)?;
    let region = e.read(p, lo, hi)?;
    let cells: Vec<Pos> = tops(&region, lo, hi)
        .into_iter()
        .map(|t| [t[0], t[1] + 1, t[2]])
        .filter(|q| q[1] <= hi[1])
        .collect();
    let changes = e.fill_with(cells, &pattern);
    let n = e.apply(ctx, p, changes)?;
    msg(p, &format!("{n} blocks have been overlaid."));
    Ok(())
}

pub fn center(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let pattern = Pattern::parse(a.first().ok_or("Usage: //center <pattern>")?)?;
    let (lo, hi) = e.selection(p)?;
    // One block across an odd side, two across an even one.
    let range = |i: usize| {
        let sum = lo[i] + hi[i];
        (sum.div_euclid(2), (sum + 1).div_euclid(2))
    };
    let (x, y, z) = (range(0), range(1), range(2));
    let changes = e.fill_with(boxed([x.0, y.0, z.0], [x.1, y.1, z.1]), &pattern);
    let n = e.apply(ctx, p, changes)?;
    msg(p, &format!("Center set ({n} blocks changed)."));
    Ok(())
}

pub fn naturalize(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, _: &[String]) -> Result {
    let (lo, hi) = e.selection(p)?;
    let region = e.read(p, lo, hi)?;
    let mut changes = Vec::new();
    for z in lo[2]..=hi[2] {
        for x in lo[0]..=hi[0] {
            let mut depth = 0;
            for y in (lo[1]..=hi[1]).rev() {
                let Some(b) = region.get(x, y, z) else {
                    continue;
                };
                if is_passable(b) {
                    depth = 0;
                    continue;
                }
                let block = match depth {
                    0 => "minecraft:grass_block",
                    1..=3 => "minecraft:dirt",
                    _ => "minecraft:stone",
                };
                changes.push(([x, y, z], block.to_owned()));
                depth += 1;
            }
        }
    }
    let n = e.apply(ctx, p, changes)?;
    msg(p, &format!("{n} blocks have been made to look more natural."));
    Ok(())
}

fn shell(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String], top_bottom: bool) -> Result {
    let pattern = Pattern::parse(a.first().ok_or("Usage: //walls <pattern>")?)?;
    let (lo, hi) = e.selection(p)?;
    let cells: Vec<Pos> = boxed(lo, hi)
        .filter(|q| {
            q[0] == lo[0] || q[0] == hi[0] || q[2] == lo[2] || q[2] == hi[2] || (top_bottom && (q[1] == lo[1] || q[1] == hi[1]))
        })
        .collect();
    let changes = e.fill_with(cells, &pattern);
    let n = e.apply(ctx, p, changes)?;
    msg(p, &format!("{n} blocks have been changed."));
    Ok(())
}

pub fn walls(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    shell(e, ctx, p, a, false)
}

pub fn faces(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    shell(e, ctx, p, a, true)
}

/// The changes that smooth the surface of the columns `lo..hi` (`x`, `z`)
/// between heights `lo[1]..hi[1]`: each column's top moves towards the
/// average of its neighbours', `iterations` times.
pub fn smoothed(region: &Region, lo: Pos, hi: Pos, iterations: u32) -> Vec<Placed> {
    let (w, d) = ((hi[0] - lo[0] + 1) as usize, (hi[2] - lo[2] + 1) as usize);
    let index = |x: i32, z: i32| ((z - lo[2]) as usize) * w + (x - lo[0]) as usize;
    let mut height = vec![f64::from(lo[1] - 1); w * d];
    let mut top_block = vec![String::new(); w * d];
    for z in lo[2]..=hi[2] {
        for x in lo[0]..=hi[0] {
            if let Some(y) = (lo[1]..=hi[1]).rev().find(|&y| region.get(x, y, z).is_some_and(|b| !is_passable(b))) {
                height[index(x, z)] = f64::from(y);
                top_block[index(x, z)] = region.get(x, y, z).unwrap_or("minecraft:stone").to_owned();
            }
        }
    }
    let original = height.clone();
    for _ in 0..iterations {
        let prev = height.clone();
        for z in lo[2]..=hi[2] {
            for x in lo[0]..=hi[0] {
                // A 3 by 3 average, the column itself counting twice.
                let mut sum = prev[index(x, z)] * 2.0;
                let mut n = 2.0;
                for (dx, dz) in [(-1, -1), (-1, 0), (-1, 1), (0, -1), (0, 1), (1, -1), (1, 0), (1, 1)] {
                    let (nx, nz) = (x + dx, z + dz);
                    if (lo[0]..=hi[0]).contains(&nx) && (lo[2]..=hi[2]).contains(&nz) {
                        sum += prev[index(nx, nz)];
                        n += 1.0;
                    }
                }
                height[index(x, z)] = sum / n;
            }
        }
    }
    let mut changes = Vec::new();
    for z in lo[2]..=hi[2] {
        for x in lo[0]..=hi[0] {
            let i = index(x, z);
            let (old, new) = (original[i] as i32, (height[i].round() as i32).clamp(lo[1] - 1, hi[1]));
            if new < old {
                for y in (new + 1)..=old {
                    changes.push(([x, y, z], "minecraft:air".to_owned()));
                }
            } else if new > old {
                let block = if top_block[i].is_empty() { "minecraft:dirt".to_owned() } else { top_block[i].clone() };
                // The old top stays the surface block, the new ones under it are what lies below.
                let below = region.get(x, old - 1, z).filter(|b| !is_passable(b)).unwrap_or(&block).to_owned();
                for y in (old + 1)..=new {
                    changes.push(([x, y, z], if y == new { block.clone() } else { below.clone() }));
                }
                if old >= lo[1] {
                    changes.push(([x, old, z], below.clone()));
                }
            }
        }
    }
    changes
}

pub fn smooth(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let iterations = number(a.first(), 1)?.clamp(1, 20) as u32;
    let (lo, hi) = e.selection(p)?;
    let region = e.read(p, lo, hi)?;
    let n = e.apply(ctx, p, smoothed(&region, lo, hi, iterations))?;
    msg(p, &format!("Terrain's height map smoothed: {n} blocks changed."));
    Ok(())
}

pub fn hollow(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let thickness = number(a.first(), 1)?.max(1);
    let pattern = match a.get(1) {
        Some(t) => Pattern::parse(t)?,
        None => Pattern::single("air"),
    };
    let (lo, hi) = e.selection(p)?;
    let region = e.read(p, lo, hi)?;
    // How far each block is from air or the selection's edge.
    let mut dist: HashMap<Pos, i32> = HashMap::new();
    let mut queue = VecDeque::new();
    for q in boxed(lo, hi) {
        let edge = (0..3).any(|i| q[i] == lo[i] || q[i] == hi[i]);
        let empty = region.get(q[0], q[1], q[2]).is_none_or(is_air);
        if empty {
            dist.insert(q, 0);
            queue.push_back(q);
        } else if edge {
            dist.insert(q, 1);
            queue.push_back(q);
        }
    }
    while let Some(q) = queue.pop_front() {
        let d = dist[&q];
        for off in [[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]] {
            let n = add(q, off);
            if (0..3).all(|i| (lo[i]..=hi[i]).contains(&n[i])) && !dist.contains_key(&n) {
                dist.insert(n, d + 1);
                queue.push_back(n);
            }
        }
    }
    let inside: Vec<Pos> = dist.into_iter().filter(|(_, d)| *d > thickness).map(|(q, _)| q).collect();
    let changes = e.fill_with(inside, &pattern);
    let n = e.apply(ctx, p, changes)?;
    msg(p, &format!("{n} blocks have been hollowed."));
    Ok(())
}

pub fn line(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let pattern = Pattern::parse(a.first().ok_or("Usage: //line <pattern> [thickness]")?)?;
    let thick = number(a.get(1), 0)?.clamp(0, 16);
    let s = e.session(p);
    let (Some(from), Some(to)) = (s.pos1, s.pos2) else {
        return Err("Set //pos1 and //pos2: the line runs between them.".into());
    };
    let steps = (0..3).map(|i| (to[i] - from[i]).abs()).max().unwrap_or(0).max(1);
    let mut cells: HashSet<Pos> = HashSet::new();
    for k in 0..=steps {
        let t = f64::from(k) / f64::from(steps);
        let c: Pos = std::array::from_fn(|i| (f64::from(from[i]) + f64::from(to[i] - from[i]) * t).round() as i32);
        for q in boxed(add(c, [-thick; 3]), add(c, [thick; 3])) {
            let d2: i32 = (0..3).map(|i| (q[i] - c[i]).pow(2)).sum();
            if d2 <= thick * thick {
                cells.insert(q);
            }
        }
    }
    let changes = e.fill_with(cells, &pattern);
    let n = e.apply(ctx, p, changes)?;
    msg(p, &format!("{n} blocks have been changed."));
    Ok(())
}

/// Tree types `//forest` and `/tool tree` take, and their features.
pub const TREES: &[(&str, &str)] = &[
    ("oak", "minecraft:oak"),
    ("big", "minecraft:fancy_oak"),
    ("birch", "minecraft:birch"),
    ("spruce", "minecraft:spruce"),
    ("pine", "minecraft:pine"),
    ("jungle", "minecraft:jungle_tree"),
    ("acacia", "minecraft:acacia"),
    ("dark_oak", "minecraft:dark_oak"),
    ("cherry", "minecraft:cherry"),
    ("mangrove", "minecraft:mangrove"),
];

pub fn tree_feature(name: &str) -> std::result::Result<&'static str, String> {
    TREES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, f)| *f)
        .ok_or_else(|| format!("Unknown tree: {name} ({})", TREES.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ")))
}

pub fn forest(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let feature = tree_feature(a.first().map_or("oak", String::as_str))?;
    let density = float(a.get(1), 5.0)?.clamp(0.0, 100.0);
    let (lo, hi) = e.selection(p)?;
    let region = e.read(p, lo, hi)?;
    let mut rng = *e.rng();
    let world = p.location.world.clone();
    let mut commands = Vec::new();
    let mut top = lo[1];
    for t in tops(&region, lo, hi) {
        let ground = region.get(t[0], t[1], t[2]).map(kind_of);
        if matches!(ground, Some("minecraft:grass_block" | "minecraft:dirt" | "minecraft:podzol")) && chance(&mut rng, density) {
            commands.push(format!("execute in {world} run place feature {feature} {} {} {}", t[0], t[1] + 1, t[2]));
            top = top.max(t[1]);
        }
    }
    e.rng = rng;
    let n = commands.len();
    if n == 0 {
        msg(p, "0 trees created (no grass or dirt on top of the selection?).");
        return Ok(());
    }
    // Trees reach out of the selection: keep what is around for an undo.
    e.apply_commands(p, [lo[0] - 6, lo[1], lo[2] - 6], [hi[0] + 6, (top + 32).min(crate::MAX_Y), hi[2] + 6], commands)?;
    msg(p, &format!("{n} trees created."));
    Ok(())
}

pub fn flora(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let density = float(a.first(), 10.0)?.clamp(0.0, 100.0);
    let (lo, hi) = e.selection(p)?;
    let region = e.read(p, lo, hi)?;
    const FLOWERS: [&str; 6] = ["dandelion", "poppy", "cornflower", "oxeye_daisy", "azure_bluet", "allium"];
    let mut rng = *e.rng();
    let mut changes = Vec::new();
    for t in tops(&region, lo, hi) {
        let on_grass = region.get(t[0], t[1], t[2]).map(kind_of) == Some("minecraft:grass_block");
        if on_grass && t[1] < hi[1] && chance(&mut rng, density) {
            let plant = if chance(&mut rng, 85.0) {
                "short_grass"
            } else {
                FLOWERS[(crate::pattern::next(&mut rng) % FLOWERS.len() as u64) as usize]
            };
            changes.push(([t[0], t[1] + 1, t[2]], format!("minecraft:{plant}")));
        }
    }
    e.rng = rng;
    let n = e.apply(ctx, p, changes)?;
    msg(p, &format!("{n} flora created."));
    Ok(())
}

pub fn move_cmd(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let (rest, f) = flags(a);
    let n = number(rest.first(), 1)?;
    let d = dir(p, rest.get(1))?;
    let (lo, hi) = e.selection(p)?;
    let region = e.read(p, lo, hi)?;
    let off = [d[0] * n, d[1] * n, d[2] * n];
    // The source becomes air, then the blocks land: a target wins over air.
    let mut target: HashMap<Pos, String> = boxed(lo, hi).map(|q| (q, "minecraft:air".to_owned())).collect();
    for q in boxed(lo, hi) {
        if let Some(b) = region.get(q[0], q[1], q[2])
            && !(f.contains('a') && is_air(b))
        {
            target.insert(add(q, off), b.to_owned());
        }
    }
    let count = e.apply(ctx, p, target.into_iter().collect())?;
    if f.contains('s') {
        e.set_selection(p, add(lo, off), add(hi, off));
    }
    msg(p, &format!("{count} blocks moved."));
    Ok(())
}

pub fn stack(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let (rest, f) = flags(a);
    let n = number(rest.first(), 1)?;
    let d = dir(p, rest.get(1))?;
    if !(1..=256).contains(&n) {
        return Err("Stack 1 to 256 times.".into());
    }
    let (lo, hi) = e.selection(p)?;
    let region = e.read(p, lo, hi)?;
    let step: Pos = std::array::from_fn(|i| d[i] * (hi[i] - lo[i] + 1));
    let mut changes = Vec::new();
    for k in 1..=n {
        let off = [step[0] * k, step[1] * k, step[2] * k];
        for q in boxed(lo, hi) {
            if let Some(b) = region.get(q[0], q[1], q[2])
                && !(f.contains('a') && is_air(b))
            {
                let to = add(q, off);
                if in_world(&to) {
                    changes.push((to, b.to_owned()));
                }
            }
        }
    }
    let count = e.apply(ctx, p, changes)?;
    if f.contains('s') {
        let off = [step[0] * n, step[1] * n, step[2] * n];
        e.set_selection(p, add(lo, off), add(hi, off));
    }
    msg(p, &format!("{count} blocks changed. Undo with //undo"));
    Ok(())
}
