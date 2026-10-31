//! Items that edit where they click: the wand, brushes (right click, at
//! range), tools (right click on a block) and the super pickaxe (left
//! click with a pickaxe).

use crate::clipboard::{clipboard, placed};
use crate::generation::{cylinder, ellipsoid};
use crate::pattern::{Mask, Pattern, kind_of};
use crate::region::{smoothed, tree_feature};
use crate::selection::set_pos;
use crate::{
    Brush, BrushBinding, HorizonEdit, PERMISSION, Pos, SuperPickaxe, Tool, WAND, add, boxed, flags, msg, number,
    target,
};
use horizon_plugin_api::prelude::*;
use std::collections::{HashSet, VecDeque};

type Result = std::result::Result<(), String>;

fn held(p: &Player) -> std::result::Result<String, String> {
    p.item_in_hand()
        .map(|i| i.kind)
        .filter(|k| k != "minecraft:air")
        .ok_or_else(|| "Hold the item to bind first.".to_owned())
}

pub fn brush(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let (rest, f) = flags(a);
    let hollow = f.contains('h');
    let brush = match rest.first().map(String::as_str) {
        Some("sphere" | "s") => Brush::Sphere {
            pattern: Pattern::parse(rest.get(1).ok_or("Usage: /brush sphere <pattern> [radius] [-h]")?)?,
            radius: number(rest.get(2), 2)?.clamp(1, 16),
            hollow,
        },
        Some("cylinder" | "cyl" | "c") => Brush::Cylinder {
            pattern: Pattern::parse(rest.get(1).ok_or("Usage: /brush cylinder <pattern> [radius] [height] [-h]")?)?,
            radius: number(rest.get(2), 2)?.clamp(1, 16),
            height: number(rest.get(3), 1)?.clamp(1, 64),
            hollow,
        },
        Some("smooth") => Brush::Smooth {
            radius: number(rest.get(1), 2)?.clamp(1, 16),
            iterations: number(rest.get(2), 4)?.clamp(1, 20) as u32,
        },
        Some("clipboard" | "copy") => {
            clipboard(e, p)?;
            Brush::Clipboard { skip_air: f.contains('a') }
        }
        _ => return Err("Usage: /brush <sphere|cylinder|smooth|clipboard> ...".into()),
    };
    let item = held(p)?;
    let what = match &brush {
        Brush::Sphere { radius, .. } => format!("Sphere brush shape equipped ({radius})."),
        Brush::Cylinder { radius, height, .. } => format!("Cylinder brush shape equipped ({radius} by {height})."),
        Brush::Smooth { radius, iterations } => format!("Smooth brush equipped ({radius} x {iterations}x)."),
        Brush::Clipboard { .. } => "Clipboard brush equipped.".to_owned(),
    };
    let s = e.session(p);
    s.tools.remove(&item);
    s.brushes.insert(item.clone(), BrushBinding { brush, mask: None });
    msg(p, &format!("{what} Right click with {} to use it.", item.trim_start_matches("minecraft:")));
    Ok(())
}

pub fn mask(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let item = held(p)?;
    let mask = if a.is_empty() { None } else { Some(Mask::parse(&a.join(" "))?) };
    let set = mask.is_some();
    let b = e.session(p).brushes.get_mut(&item).ok_or("The held item has no brush.")?;
    b.mask = mask;
    msg(p, if set { "Brush mask set." } else { "Brush mask disabled." });
    Ok(())
}

pub fn tool(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let tool = match a.first().map(String::as_str) {
        Some("tree") => {
            let name = a.get(1).map_or("oak", String::as_str);
            tree_feature(name)?;
            Tool::Tree(name.to_owned())
        }
        Some("repl" | "replacer") => Tool::Replace(Pattern::parse(a.get(1).ok_or("Usage: /tool repl <pattern>")?)?),
        Some("info") => Tool::Info,
        _ => return Err("Usage: /tool <tree [type]|repl <pattern>|info>".into()),
    };
    let item = held(p)?;
    let s = e.session(p);
    s.brushes.remove(&item);
    s.tools.insert(item.clone(), tool);
    msg(p, &format!("Tool bound to {}: right click a block.", item.trim_start_matches("minecraft:")));
    Ok(())
}

pub fn none(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, _: &[String]) -> Result {
    let item = held(p)?;
    let s = e.session(p);
    let had = s.brushes.remove(&item).is_some() | s.tools.remove(&item).is_some();
    msg(p, if had { "Tool unbound from the held item." } else { "The held item had no tool." });
    Ok(())
}

pub fn superpickaxe(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let s = e.session(p);
    let mode = match a.first().map(String::as_str) {
        None => match s.super_pickaxe {
            Some(_) => None,
            None => Some(SuperPickaxe::Single),
        },
        Some("off") => None,
        Some("single") => Some(SuperPickaxe::Single),
        Some("area") => Some(SuperPickaxe::Area(number(a.get(1), 2)?.clamp(1, 8))),
        Some("recur" | "recursive") => Some(SuperPickaxe::Recursive(number(a.get(1), 8)?.clamp(1, 32))),
        Some(_) => return Err("Usage: /sp [single|area <r>|recur <r>|off]".into()),
    };
    s.super_pickaxe = mode;
    msg(
        p,
        match mode {
            None => "Super pickaxe disabled.",
            Some(SuperPickaxe::Single) => "Super pickaxe enabled: left click with a pickaxe.",
            Some(SuperPickaxe::Area(_)) => "Area super pickaxe enabled: left click with a pickaxe.",
            Some(SuperPickaxe::Recursive(_)) => "Recursive super pickaxe enabled: left click with a pickaxe.",
        },
    );
    Ok(())
}

fn use_brush(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, binding: BrushBinding) -> Result {
    let (hit, _) = target(p, 300.0).ok_or("No block in sight!")?;
    let changes = match &binding.brush {
        Brush::Sphere { pattern, radius, hollow } => {
            let cells = ellipsoid(hit, [f64::from(*radius); 3], *hollow);
            e.fill_with(cells, pattern)
        }
        Brush::Cylinder { pattern, radius, height, hollow } => {
            let r = f64::from(*radius);
            let cells = cylinder(hit, r, r, *height, *hollow);
            e.fill_with(cells, pattern)
        }
        Brush::Smooth { radius, iterations } => {
            let (lo, hi) = (add(hit, [-radius; 3]), add(hit, [*radius; 3]));
            let region = e.read(p, lo, hi)?;
            smoothed(&region, lo, hi, *iterations)
        }
        Brush::Clipboard { skip_air } => {
            let clip = clipboard(e, p)?;
            placed(&clip, add(hit, [0, 1, 0]), *skip_air)
        }
    };
    e.apply_masked(ctx, p, changes, binding.mask.as_ref())?;
    Ok(())
}

fn use_tool(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, tool: Tool, at: Pos, state: &str) -> Result {
    match tool {
        Tool::Tree(name) => {
            let feature = tree_feature(&name)?;
            let command = format!(
                "execute in {} run place feature {feature} {} {} {}",
                p.location.world,
                at[0],
                at[1] + 1,
                at[2]
            );
            e.apply_commands(p, add(at, [-6, 1, -6]), add(at, [6, 32, 6]), vec![command])?;
        }
        Tool::Replace(pattern) => {
            let changes = e.fill_with([at], &pattern);
            e.apply(ctx, p, changes)?;
        }
        Tool::Info => {
            msg(p, &format!("@({}, {}, {}): {}", at[0], at[1], at[2], state));
        }
    }
    Ok(())
}

fn use_pickaxe(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, mode: SuperPickaxe, at: Pos, state: &str) -> Result {
    let kind = kind_of(state).to_owned();
    let cells: Vec<Pos> = match mode {
        SuperPickaxe::Single => vec![at],
        SuperPickaxe::Area(r) => {
            let (lo, hi) = (add(at, [-r; 3]), add(at, [r; 3]));
            let region = e.read(p, lo, hi)?;
            boxed(lo, hi).filter(|q| region.get(q[0], q[1], q[2]).is_some_and(|b| kind_of(b) == kind)).collect()
        }
        SuperPickaxe::Recursive(r) => {
            let (lo, hi) = (add(at, [-r; 3]), add(at, [r; 3]));
            let region = e.read(p, lo, hi)?;
            let mut seen = HashSet::new();
            let mut queue = VecDeque::from([at]);
            let mut out = Vec::new();
            while let Some(q) = queue.pop_front() {
                let same = region.get(q[0], q[1], q[2]).is_some_and(|b| kind_of(b) == kind);
                if !same || !seen.insert(q) || (0..3).map(|i| (q[i] - at[i]).pow(2)).sum::<i32>() > r * r {
                    continue;
                }
                out.push(q);
                for o in [[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]] {
                    queue.push_back(add(q, o));
                }
            }
            out
        }
    };
    let changes = cells.into_iter().map(|q| (q, "minecraft:air".to_owned())).collect();
    e.apply(ctx, p, changes)?;
    Ok(())
}

/// The wand, brushes, tools and the super pickaxe hear clicks.
pub fn listen(ctx: &mut Ctx<HorizonEdit>) {
    ctx.listen(EventPriority::Lowest, false, |plugin: &mut HorizonEdit, ctx, e: &mut PlayerInteractEvent| {
        let item = e.item.as_ref().map(|i| i.kind.clone()).unwrap_or_default();
        if item.is_empty() || !e.player.has_permission(PERMISSION) {
            return;
        }
        let block = e.block.as_ref().map(|b| ([b.pos.x, b.pos.y, b.pos.z], b.state_string()));
        let tick = ctx.tick();
        let p = e.player.clone();
        let result = match (e.action.as_str(), block) {
            ("left_click_block", Some((at, _))) if item == WAND => {
                e.set_cancelled(true);
                set_pos(plugin, &p, at, true);
                Ok(())
            }
            ("left_click_block", Some((at, state))) if item.ends_with("_pickaxe") => {
                match plugin.session(&p).super_pickaxe {
                    Some(mode) => {
                        e.set_cancelled(true);
                        use_pickaxe(plugin, ctx, &p, mode, at, &state)
                    }
                    None => Ok(()),
                }
            }
            ("right_click_block", Some((at, state))) => {
                // Heard once for each hand: act on the first.
                let s = plugin.session(&p);
                let tool = s.tools.get(&item).cloned();
                let brush = s.brushes.get(&item).cloned();
                if item != WAND && tool.is_none() && brush.is_none() {
                    return;
                }
                e.set_cancelled(true);
                if s.last_click == Some((at, tick)) {
                    return;
                }
                s.last_click = Some((at, tick));
                if item == WAND {
                    set_pos(plugin, &p, at, false);
                    Ok(())
                } else if let Some(tool) = tool {
                    use_tool(plugin, ctx, &p, tool, at, &state)
                } else if let Some(brush) = brush {
                    use_brush(plugin, ctx, &p, brush)
                } else {
                    Ok(())
                }
            }
            ("right_click_air", _) => {
                let s = plugin.session(&p);
                let Some(brush) = s.brushes.get(&item).cloned() else {
                    return;
                };
                e.set_cancelled(true);
                let marker = [i32::MIN, 0, 0];
                if s.last_click == Some((marker, tick)) {
                    return;
                }
                s.last_click = Some((marker, tick));
                use_brush(plugin, ctx, &p, brush)
            }
            _ => Ok(()),
        };
        if let Err(err) = result {
            p.send_message(&format!("§c{err}"));
        }
    });
    // In creative a break may come without a click: the wand keeps the block.
    ctx.listen(EventPriority::Lowest, false, |_, _, e: &mut BlockBreakEvent| {
        let holds = e.player.item_in_hand().is_some_and(|i| i.kind == WAND);
        if holds && e.player.has_permission(PERMISSION) {
            e.set_cancelled(true);
        }
    });
}
