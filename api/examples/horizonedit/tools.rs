//! Items that edit where they click: the wand, brushes (right click, at
//! range), tools (bound to an item), the navigation compass and the super
//! pickaxe (left click with a pickaxe).

use crate::clipboard::{clipboard, placed};
use crate::generation::{cylinder, ellipsoid};
use crate::pattern::{Mask, Pattern, chance, is_air, is_passable, kind_of};
use crate::region::{smoothed, tree_feature};
use crate::selection::set_pos;
use crate::{
    Brush, BrushBinding, HorizonEdit, MAX_Y, PERMISSION, Pos, Shape, SuperPickaxe, Tool, WAND, add, boxed, flags,
    float, msg, number, target,
};
use horizon_plugin_api::prelude::*;
use std::collections::{HashSet, VecDeque};

type Result = std::result::Result<(), String>;

/// How far brushes and far tools reach unless `/range` says otherwise.
const RANGE: f64 = 300.0;
const COMPASS: &str = "minecraft:compass";

fn held(p: &Player) -> std::result::Result<String, String> {
    p.item_in_hand()
        .map(|i| i.kind)
        .filter(|k| k != "minecraft:air")
        .ok_or_else(|| "Hold the item to bind first.".to_owned())
}

fn bind_brush(e: &mut HorizonEdit, p: &Player, brush: Brush, what: &str) -> Result {
    let item = held(p)?;
    let s = e.session(p);
    s.tools.remove(&item);
    s.brushes.insert(item.clone(), BrushBinding { brush, mask: None, range: RANGE });
    msg(p, &format!("{what} Right click with {} to use it.", item.trim_start_matches("minecraft:")));
    Ok(())
}

/// `/brush <type> ...`.
pub fn brush(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let (rest, f) = flags(a);
    let hollow = f.contains('h');
    let arg = |i: usize| rest.get(i);
    let (brush, what) = match rest.first().map(String::as_str) {
        Some("sphere" | "s") => {
            let radius = number(arg(2), 2)?.clamp(1, 16);
            let pattern = Pattern::parse(arg(1).ok_or("Usage: /brush sphere <pattern> [radius] [-h]")?)?;
            (Brush::Sphere { pattern, radius, hollow }, format!("Sphere brush shape equipped ({radius})."))
        }
        Some("cylinder" | "cyl" | "c") => {
            let radius = number(arg(2), 2)?.clamp(1, 16);
            let height = number(arg(3), 1)?.clamp(1, 64);
            let pattern = Pattern::parse(arg(1).ok_or("Usage: /brush cylinder <pattern> [radius] [height] [-h]")?)?;
            (Brush::Cylinder { pattern, radius, height, hollow }, format!("Cylinder brush shape equipped ({radius} by {height})."))
        }
        Some("set") => {
            let shape = match arg(1).map(String::as_str) {
                Some("sphere") => Shape::Sphere,
                Some("cylinder" | "cyl") => Shape::Cylinder,
                Some("cuboid" | "cube") => Shape::Cuboid,
                _ => return Err("Usage: /brush set <sphere|cylinder|cuboid> [radius] <pattern>".into()),
            };
            let (radius, pattern) = match (arg(2), arg(3)) {
                (Some(r), Some(t)) => (number(Some(r), 2)?, t),
                (Some(t), None) => (2, t),
                _ => return Err("Usage: /brush set <shape> [radius] <pattern>".into()),
            };
            let radius = radius.clamp(1, 16);
            (Brush::Set { shape, pattern: Pattern::parse(pattern)?, radius }, format!("Set brush equipped ({radius})."))
        }
        Some("smooth") => {
            let radius = number(arg(1), 2)?.clamp(1, 16);
            let iterations = number(arg(2), 4)?.clamp(1, 20) as u32;
            let mask = if rest.len() > 3 { Some(Mask::parse(&rest[3..].join(" "))?) } else { None };
            (Brush::Smooth { radius, iterations, mask }, format!("Smooth brush equipped ({radius} x {iterations}x)."))
        }
        Some("clipboard" | "copy") => {
            clipboard(e, p)?;
            (Brush::Clipboard { skip_air: f.contains('a') }, "Clipboard brush equipped.".to_owned())
        }
        Some("gravity" | "grav") => {
            let radius = number(arg(1), 5)?.clamp(1, 16);
            (Brush::Gravity { radius, full_height: hollow }, format!("Gravity brush equipped ({radius})."))
        }
        Some("forest") => {
            let radius = number(arg(1), 5)?.clamp(1, 32);
            let density = float(arg(2), 5.0)?.clamp(0.0, 100.0);
            let tree = arg(3).cloned().unwrap_or_else(|| "tree".to_owned());
            tree_feature(&tree)?;
            (Brush::Forest { radius, density, tree }, format!("Forest brush equipped ({radius}, {density}%)."))
        }
        Some("extinguish" | "ex") => {
            let radius = number(arg(1), 5)?.clamp(1, 32);
            (Brush::Extinguish { radius }, format!("Extinguisher equipped ({radius})."))
        }
        Some("butcher" | "kill") => {
            let radius = number(arg(1), 5)?.clamp(1, 32);
            (Brush::Butcher { radius }, format!("Butcher brush equipped ({radius})."))
        }
        Some("raise") => {
            let radius = number(arg(1), 5)?.clamp(1, 16);
            (Brush::Raise { radius }, format!("Raise brush equipped ({radius})."))
        }
        Some("lower") => {
            let radius = number(arg(1), 5)?.clamp(1, 16);
            (Brush::Lower { radius }, format!("Lower brush equipped ({radius})."))
        }
        _ => {
            return Err(
                "Usage: /brush <sphere|cylinder|set|smooth|clipboard|gravity|forest|extinguish|butcher|raise|lower> ..."
                    .into(),
            );
        }
    };
    bind_brush(e, p, brush, &what)
}

fn held_brush<'a>(e: &'a mut HorizonEdit, p: &Player) -> std::result::Result<&'a mut BrushBinding, String> {
    let item = held(p)?;
    e.session(p).brushes.get_mut(&item).ok_or_else(|| "The held item has no brush.".to_owned())
}

/// `/mask [mask]`: the held brush only changes what it lets through.
pub fn mask(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let mask = if a.is_empty() { None } else { Some(Mask::parse(&a.join(" "))?) };
    let set = mask.is_some();
    held_brush(e, p)?.mask = mask;
    msg(p, if set { "Brush mask set." } else { "Brush mask disabled." });
    Ok(())
}

/// `/material <pattern>`: what the held brush places.
pub fn material(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let pattern = Pattern::parse(a.first().ok_or("Usage: /material <pattern>")?)?;
    match &mut held_brush(e, p)?.brush {
        Brush::Sphere { pattern: m, .. } | Brush::Cylinder { pattern: m, .. } | Brush::Set { pattern: m, .. } => *m = pattern,
        _ => return Err("That brush places no pattern.".into()),
    }
    msg(p, "Brush material set.");
    Ok(())
}

/// `/size <radius>`: the held brush's size.
pub fn size(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let n = number(a.first(), 0)?;
    if !(1..=32).contains(&n) {
        return Err("Usage: /size <1 to 32>".into());
    }
    match &mut held_brush(e, p)?.brush {
        Brush::Sphere { radius, .. }
        | Brush::Cylinder { radius, .. }
        | Brush::Set { radius, .. }
        | Brush::Smooth { radius, .. }
        | Brush::Gravity { radius, .. }
        | Brush::Forest { radius, .. }
        | Brush::Extinguish { radius }
        | Brush::Butcher { radius }
        | Brush::Raise { radius }
        | Brush::Lower { radius } => *radius = n,
        Brush::Clipboard { .. } => return Err("A clipboard brush is the clipboard's size.".into()),
    }
    msg(p, &format!("Brush size set to {n}."));
    Ok(())
}

/// `/range <blocks>`: how far the held brush reaches.
pub fn range(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let n = number(a.first(), 0)?;
    if !(1..=1000).contains(&n) {
        return Err("Usage: /range <1 to 1000>".into());
    }
    held_brush(e, p)?.range = f64::from(n);
    msg(p, &format!("Brush range set to {n}."));
    Ok(())
}

/// `/tool <type> ...`.
pub fn tool(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let arg = |i: usize| a.get(i);
    let (tool, what) = match a.first().map(String::as_str) {
        Some("tree") => {
            let name = arg(1).map_or("tree", String::as_str);
            tree_feature(name)?;
            (Tool::Tree(name.to_owned()), "Tree tool: right click grass or dirt.")
        }
        Some("repl" | "replacer") => (
            Tool::Replace(Pattern::parse(arg(1).ok_or("Usage: /tool repl <pattern>")?)?),
            "Block replacer: right click to replace, left click to pick the pattern.",
        ),
        Some("info") => (Tool::Info, "Info tool: right click a block."),
        Some("floodfill" | "flood") => (
            Tool::FloodFill(
                Pattern::parse(arg(1).ok_or("Usage: /tool floodfill <pattern> [range]")?)?,
                number(arg(2), 5)?.clamp(1, 32),
            ),
            "Flood fill: right click a block.",
        ),
        Some("deltree") => (Tool::DelTree, "Tree remover: right click a log or leaves."),
        Some("farwand") => (Tool::FarWand, "Far wand: left and right click at any distance."),
        Some("lrbuild") => (
            Tool::LrBuild(
                Pattern::parse(arg(1).ok_or("Usage: /tool lrbuild <left pattern> <right pattern>")?)?,
                Pattern::parse(arg(2).ok_or("Usage: /tool lrbuild <left pattern> <right pattern>")?)?,
            ),
            "Long-range builder: left and right click.",
        ),
        Some("navwand") => (Tool::NavWand, "Navigation wand: left click jumps, right click goes through."),
        Some("stacker") => (Tool::Stacker(number(arg(1), 10)?.clamp(1, 64)), "Stacker: right click a block."),
        Some("selwand") => (Tool::SelWand, "Selection wand: left and right click."),
        Some("cycler") => {
            return Err("The block state cycler needs the server's block states, which plugins can't list yet.".into());
        }
        _ => {
            return Err(
                "Usage: /tool <tree|repl|info|floodfill|deltree|farwand|lrbuild|navwand|stacker|selwand> ...".into(),
            );
        }
    };
    let item = held(p)?;
    let s = e.session(p);
    s.brushes.remove(&item);
    s.tools.insert(item.clone(), tool);
    msg(p, &format!("{what} (bound to {})", item.trim_start_matches("minecraft:")));
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

/// The blocks of `kind`-matching blocks touching `at`, within `r`.
fn connected(e: &HorizonEdit, p: &Player, at: Pos, r: i32, same: impl Fn(&str) -> bool) -> std::result::Result<Vec<Pos>, String> {
    let (lo, hi) = (add(at, [-r; 3]), add(at, [r; 3]));
    let region = e.read(p, lo, hi)?;
    let mut seen = HashSet::new();
    let mut queue = VecDeque::from([at]);
    let mut out = Vec::new();
    while let Some(q) = queue.pop_front() {
        let fits = region.get(q[0], q[1], q[2]).is_some_and(&same);
        if !fits || !seen.insert(q) || (0..3).map(|i| (q[i] - at[i]).pow(2)).sum::<i32>() > r * r {
            continue;
        }
        out.push(q);
        for o in [[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]] {
            queue.push_back(add(q, o));
        }
    }
    Ok(out)
}

/// The top of each column within `r` of `c` across (searching `r` up and
/// down) that isn't air.
fn tops_around(e: &HorizonEdit, p: &Player, c: Pos, r: i32) -> std::result::Result<(Vec<Pos>, horizon_plugin_api::types::Region), String> {
    let (lo, hi) = (add(c, [-r; 3]), add(c, [r; 3]));
    let region = e.read(p, lo, hi)?;
    let mut out = Vec::new();
    for z in lo[2]..=hi[2] {
        for x in lo[0]..=hi[0] {
            if (x - c[0]).pow(2) + (z - c[2]).pow(2) > r * r {
                continue;
            }
            if let Some(y) = (lo[1]..=hi[1]).rev().find(|&y| region.get(x, y, z).is_some_and(|b| !is_passable(b))) {
                out.push([x, y, z]);
            }
        }
    }
    Ok((out, region))
}

fn use_brush(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, binding: BrushBinding) -> Result {
    let (hit, _) = target(p, binding.range).ok_or("No block in sight!")?;
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
        Brush::Set { shape, pattern, radius } => {
            let r = *radius;
            let cells = match shape {
                Shape::Sphere => ellipsoid(hit, [f64::from(r); 3], false),
                Shape::Cylinder => cylinder(add(hit, [0, -r, 0]), f64::from(r), f64::from(r), 2 * r + 1, false),
                Shape::Cuboid => boxed(add(hit, [-r; 3]), add(hit, [r; 3])).collect(),
            };
            e.fill_with(cells, pattern)
        }
        Brush::Smooth { radius, iterations, mask } => {
            let (lo, hi) = (add(hit, [-radius; 3]), add(hit, [*radius; 3]));
            let region = e.read(p, lo, hi)?;
            smoothed(&region, lo, hi, *iterations, mask.as_ref())
        }
        Brush::Clipboard { skip_air } => {
            let clip = clipboard(e, p)?;
            placed(&clip, add(hit, [0, 1, 0]), *skip_air)
        }
        Brush::Gravity { radius, full_height } => {
            // Each column's blocks fall to the bottom of the range, in order.
            let r = *radius;
            let top = if *full_height { MAX_Y } else { hit[1] + r };
            let (lo, hi) = ([hit[0] - r, hit[1] - r, hit[2] - r], [hit[0] + r, top, hit[2] + r]);
            let region = e.read(p, lo, hi)?;
            let mut changes = Vec::new();
            for z in lo[2]..=hi[2] {
                for x in lo[0]..=hi[0] {
                    let column: Vec<String> = (lo[1]..=hi[1])
                        .filter_map(|y| region.get(x, y, z))
                        .filter(|b| !is_air(b))
                        .map(str::to_owned)
                        .collect();
                    for (k, y) in (lo[1]..=hi[1]).enumerate() {
                        let b = column.get(k).cloned().unwrap_or_else(|| "minecraft:air".to_owned());
                        changes.push(([x, y, z], b));
                    }
                }
            }
            changes
        }
        Brush::Forest { radius, density, tree } => {
            let feature = tree_feature(tree)?;
            let (tops, region) = tops_around(e, p, hit, *radius)?;
            let mut rng = *e.rng();
            let mut commands = Vec::new();
            let world = p.location.world.clone();
            for t in tops {
                let ground = region.get(t[0], t[1], t[2]).map(kind_of);
                if matches!(ground, Some("minecraft:grass_block" | "minecraft:dirt" | "minecraft:podzol")) && chance(&mut rng, *density) {
                    let feature = if feature == "random" { "minecraft:oak" } else { feature };
                    commands.push(format!("execute in {world} run place feature {feature} {} {} {}", t[0], t[1] + 1, t[2]));
                }
            }
            e.rng = rng;
            let r = *radius + 6;
            e.apply_commands(p, add(hit, [-r, -radius, -r]), add(hit, [r, 32, r]), commands)?;
            return Ok(());
        }
        Brush::Extinguish { radius } => {
            let region = e.read(p, add(hit, [-radius; 3]), add(hit, [*radius; 3]))?;
            boxed(add(hit, [-radius; 3]), add(hit, [*radius; 3]))
                .filter(|q| (0..3).map(|i| (q[i] - hit[i]).pow(2)).sum::<i32>() <= radius * radius)
                .filter(|q| region.get(q[0], q[1], q[2]).is_some_and(|b| matches!(kind_of(b), "minecraft:fire" | "minecraft:soul_fire")))
                .map(|q| (q, "minecraft:air".to_owned()))
                .collect()
        }
        Brush::Butcher { radius } => {
            let at = Location::new(&p.location.world, f64::from(hit[0]) + 0.5, f64::from(hit[1]), f64::from(hit[2]) + 0.5);
            p.world().kill_entities(&format!("type=!minecraft:player,type=!minecraft:item,type=!minecraft:item_frame,type=!minecraft:painting,type=!minecraft:armor_stand,distance=..{radius}"), &at);
            return Ok(());
        }
        Brush::Raise { radius } | Brush::Lower { radius } => {
            let raise = matches!(binding.brush, Brush::Raise { .. });
            let (tops, region) = tops_around(e, p, hit, *radius)?;
            tops.into_iter()
                .map(|t| {
                    if raise {
                        ([t[0], t[1] + 1, t[2]], region.get(t[0], t[1], t[2]).unwrap_or("minecraft:dirt").to_owned())
                    } else {
                        (t, "minecraft:air".to_owned())
                    }
                })
                .collect()
        }
    };
    e.apply_masked(ctx, p, changes, binding.mask.as_ref())?;
    Ok(())
}

/// A tool's right click on the block at `at` (`state`), its face `face`.
fn use_tool(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, tool: Tool, at: Pos, state: &str, face: Pos) -> Result {
    match tool {
        Tool::Tree(name) => {
            let feature = tree_feature(&name)?;
            let feature = if feature == "random" { "minecraft:oak" } else { feature };
            let command = format!("execute in {} run place feature {feature} {} {} {}", p.location.world, at[0], at[1] + 1, at[2]);
            e.apply_commands(p, add(at, [-6, 1, -6]), add(at, [6, 32, 6]), vec![command])?;
        }
        Tool::Replace(pattern) => {
            let changes = e.fill_with([at], &pattern);
            e.apply(ctx, p, changes)?;
        }
        Tool::Info => {
            msg(p, &format!("@({}, {}, {}): {}", at[0], at[1], at[2], state));
        }
        Tool::FloodFill(pattern, r) => {
            let kind = kind_of(state).to_owned();
            let cells = connected(e, p, at, r, |b| kind_of(b) == kind)?;
            let changes = e.fill_with(cells, &pattern);
            e.apply(ctx, p, changes)?;
        }
        Tool::DelTree => {
            let tree = |b: &str| {
                let k = kind_of(b);
                k.ends_with("_log") || k.ends_with("_leaves") || k.ends_with("_wood") || k.ends_with("_stem")
                    || k.ends_with("_wart_block") || k == "minecraft:shroomlight" || k == "minecraft:mangrove_roots"
            };
            if !tree(state) {
                return Err("That's not a tree.".into());
            }
            let cells = connected(e, p, at, 32, tree)?;
            let changes = cells.into_iter().map(|q| (q, "minecraft:air".to_owned())).collect();
            e.apply(ctx, p, changes)?;
        }
        Tool::FarWand | Tool::SelWand => set_pos(e, p, at, false),
        Tool::LrBuild(_, right) => {
            let changes = e.fill_with([add(at, face)], &right);
            e.apply(ctx, p, changes)?;
        }
        Tool::NavWand => crate::navigation::thru(e, ctx, p, &[])?,
        Tool::Stacker(r) => {
            let cells: Vec<Pos> = (1..=r).map(|k| [at[0] + face[0] * k, at[1] + face[1] * k, at[2] + face[2] * k]).collect();
            let far = cells.last().copied().unwrap_or(at);
            let region = e.read(p, [at[0].min(far[0]), at[1].min(far[1]), at[2].min(far[2])], [at[0].max(far[0]), at[1].max(far[1]), at[2].max(far[2])])?;
            // Up to the first block that isn't air.
            let free: Vec<Pos> = cells.into_iter().take_while(|q| region.get(q[0], q[1], q[2]).is_some_and(is_air)).collect();
            let changes = free.into_iter().map(|q| (q, state.to_owned())).collect();
            e.apply(ctx, p, changes)?;
        }
    }
    Ok(())
}

/// A tool's left click on `at` (`state`).
fn left_tool(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, item: &str, tool: Tool, at: Pos, state: &str) -> Result {
    match tool {
        Tool::Replace(_) => {
            // Picks the block clicked as the pattern.
            e.session(p).tools.insert(item.to_owned(), Tool::Replace(Pattern::single(state)));
            msg(p, &format!("Replacer pattern set to {state}."));
        }
        Tool::FarWand => {
            let (hit, _) = target(p, RANGE).ok_or("No block in sight!")?;
            set_pos(e, p, hit, true);
        }
        Tool::SelWand => set_pos(e, p, at, true),
        Tool::LrBuild(left, _) => {
            let changes = e.fill_with([at], &left);
            e.apply(ctx, p, changes)?;
        }
        Tool::NavWand => crate::navigation::jumpto(e, ctx, p, &[])?,
        _ => {}
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
        SuperPickaxe::Recursive(r) => connected(e, p, at, r, |b| kind_of(b) == kind)?,
    };
    let changes = cells.into_iter().map(|q| (q, "minecraft:air".to_owned())).collect();
    e.apply(ctx, p, changes)?;
    Ok(())
}

/// The wand, brushes, tools, the compass and the super pickaxe hear clicks.
pub fn listen(ctx: &mut Ctx<HorizonEdit>) {
    ctx.listen(EventPriority::Lowest, false, |plugin: &mut HorizonEdit, ctx, e: &mut PlayerInteractEvent| {
        let item = e.item.as_ref().map(|i| i.kind.clone()).unwrap_or_default();
        if item.is_empty() || !e.player.has_permission(PERMISSION) {
            return;
        }
        let block = e.block.as_ref().map(|b| ([b.pos.x, b.pos.y, b.pos.z], b.state_string()));
        let face = e.face.map_or([0, 1, 0], |f| f.offset());
        let tick = ctx.tick();
        let p = e.player.clone();
        let s = plugin.session(&p);
        let tool = s.tools.get(&item).cloned().or_else(|| (item == COMPASS).then_some(Tool::NavWand));
        let brush = s.brushes.get(&item).cloned();
        let pickaxe = s.super_pickaxe.filter(|_| item.ends_with("_pickaxe"));
        let wand = item == WAND && tool.is_none() && brush.is_none();
        if !wand && tool.is_none() && brush.is_none() && pickaxe.is_none() {
            return;
        }
        let result = match (e.action.as_str(), block) {
            ("left_click_block", Some((at, state))) => {
                if wand {
                    e.set_cancelled(true);
                    set_pos(plugin, &p, at, true);
                    Ok(())
                } else if let Some(tool) = tool {
                    e.set_cancelled(true);
                    left_tool(plugin, ctx, &p, &item, tool, at, &state)
                } else if let Some(mode) = pickaxe {
                    e.set_cancelled(true);
                    use_pickaxe(plugin, ctx, &p, mode, at, &state)
                } else {
                    Ok(())
                }
            }
            ("right_click_block", Some((at, state))) => {
                if !wand && tool.is_none() && brush.is_none() {
                    return;
                }
                e.set_cancelled(true);
                // Heard once for each hand: act on the first.
                let s = plugin.session(&p);
                if s.last_click == Some((at, tick)) {
                    return;
                }
                s.last_click = Some((at, tick));
                if wand {
                    set_pos(plugin, &p, at, false);
                    Ok(())
                } else if let Some(Tool::FarWand) = tool {
                    match target(&p, RANGE) {
                        Some((hit, _)) => {
                            set_pos(plugin, &p, hit, false);
                            Ok(())
                        }
                        None => Err("No block in sight!".into()),
                    }
                } else if let Some(tool) = tool {
                    use_tool(plugin, ctx, &p, tool, at, &state, face)
                } else if let Some(brush) = brush {
                    use_brush(plugin, ctx, &p, brush)
                } else {
                    Ok(())
                }
            }
            ("right_click_air", _) => {
                let marker = [i32::MIN, 0, 0];
                let s = plugin.session(&p);
                if s.last_click == Some((marker, tick)) {
                    return;
                }
                let run: Option<Result> = if let Some(brush) = brush {
                    Some(use_brush(plugin, ctx, &p, brush))
                } else {
                    match tool {
                        Some(Tool::NavWand) => Some(crate::navigation::thru(plugin, ctx, &p, &[])),
                        Some(Tool::FarWand) => Some(match target(&p, RANGE) {
                            Some((hit, _)) => {
                                set_pos(plugin, &p, hit, false);
                                Ok(())
                            }
                            None => Err("No block in sight!".into()),
                        }),
                        Some(Tool::LrBuild(_, right)) => Some(match target(&p, RANGE) {
                            Some((_, before)) => {
                                let changes = plugin.fill_with([before], &right);
                                plugin.apply(ctx, &p, changes).map(|_| ())
                            }
                            None => Err("No block in sight!".into()),
                        }),
                        _ => None,
                    }
                };
                let Some(run) = run else {
                    return;
                };
                e.set_cancelled(true);
                plugin.session(&p).last_click = Some((marker, tick));
                run
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
