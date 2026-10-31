//! HorizonEdit: WorldEdit's commands for a Horizon server, written anew for
//! horizon-plugin-api (no WorldEdit code). Select with the wand (a wooden
//! axe: left click the first corner, right click the second) or `//pos1`
//! and `//pos2`, then edit:
//!
//! `//set`, `//replace`, `//walls`, `//outline`, `//sphere`, `//hsphere`,
//! `//cyl`, `//hcyl`, `//copy`, `//cut`, `//paste [-a]`, `//rotate`,
//! `//stack`, `//move`, `//expand`, `//contract`, `//shift`, `//size`,
//! `//count`, `//distr`, `//drain`, `//undo`, `//redo`, `//up`, `//wand`,
//! `//sel`, `//help`.
//!
//! Patterns are a block (`stone`, `oak_stairs[facing=east]`) or a mix
//! (`50%stone,30%cobblestone,andesite`). Directions default to where the
//! player looks (`me`). Edits are written with `/fill`
//! rows, a few thousand commands a tick, so a big edit doesn't stall the
//! server; each can be undone.

use horizon_plugin_api::prelude::*;
use horizon_plugin_api::types::namespaced;
use std::collections::{HashMap, VecDeque};

/// The wand.
const WAND: &str = "minecraft:wooden_axe";
const PERMISSION: &str = "horizonedit.use";
/// Undo steps kept per player.
const HISTORY: usize = 15;
/// The world's build height (the overworld's).
const MIN_Y: i32 = -64;
const MAX_Y: i32 = 319;

const CONFIG: &str = "\
# HorizonEdit's limits.
limits:
  # Blocks one edit may change.
  max-blocks: 500000
  # Commands sent to the world per tick (20 a second) while an edit runs.
  commands-per-tick: 2000
";

type Pos = [i32; 3];
/// A block at a place, as `/setblock` writes it.
type Placed = (Pos, String);

#[derive(Default)]
struct Session {
    world: String,
    pos1: Option<Pos>,
    pos2: Option<Pos>,
    /// Blocks relative to where the player stood when copying.
    clipboard: Option<Vec<Placed>>,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    /// The last wand click, so a click heard for both hands counts once.
    last_click: Option<(Pos, u64)>,
}

#[derive(Clone)]
struct Edit {
    world: String,
    before: Vec<Placed>,
    after: Vec<Placed>,
}

struct Job {
    owner: Uuid,
    commands: Vec<String>,
    next: usize,
    changed: usize,
}

#[derive(Default)]
struct HorizonEdit {
    sessions: HashMap<Uuid, Session>,
    jobs: VecDeque<Job>,
    rng: u64,
}

// What a command needs: the player, their arguments; an error is shown in red.
type Run = fn(&mut HorizonEdit, &mut Ctx<HorizonEdit>, &Player, &[String]) -> Result<(), String>;

fn msg(p: &Player, text: &str) {
    p.send_message(&format!("§d{text}"));
}

/// A direction word, or where the player looks (`me`, or none given).
fn dir(p: &Player, word: Option<&String>) -> Result<Pos, String> {
    Ok(match word.map(|w| w.to_ascii_lowercase()).as_deref() {
        None | Some("me" | "forward" | "f") => p.location.facing().offset(),
        Some("back" | "b") => p.location.facing().offset().map(|v| -v),
        Some("up" | "u") => [0, 1, 0],
        Some("down" | "d") => [0, -1, 0],
        Some("north" | "n") => [0, 0, -1],
        Some("south" | "s") => [0, 0, 1],
        Some("west" | "w") => [-1, 0, 0],
        Some("east" | "e") => [1, 0, 0],
        Some(other) => {
            return Err(format!(
                "Unknown direction: {other} (me, back, up, down, north, south, east, west)"
            ));
        }
    })
}

fn number(word: Option<&String>, default: i32) -> Result<i32, String> {
    match word {
        None => Ok(default),
        Some(w) => w.parse().map_err(|_| format!("Not a number: {w}")),
    }
}

/// A block id as commands take it, with its namespace.
fn block_id(text: &str) -> String {
    namespaced(text)
}

/// The id without the state: `minecraft:oak_stairs` of `minecraft:oak_stairs[facing=east]`.
fn kind_of(state: &str) -> &str {
    state.split('[').next().unwrap_or(state)
}

fn is_air(state: &str) -> bool {
    matches!(kind_of(state), "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air")
}

/// A pattern: blocks with weights.
struct Pattern(Vec<(String, u32)>);

impl Pattern {
    fn parse(text: &str) -> Result<Pattern, String> {
        let mut out = Vec::new();
        for part in text.split(',').filter(|p| !p.is_empty()) {
            let (weight, block) = match part.split_once('%') {
                Some((w, b)) => (w.parse::<u32>().map_err(|_| format!("Bad weight: {part}"))?, b),
                None => (1, part),
            };
            if block.is_empty() || block.contains(char::is_whitespace) {
                return Err(format!("Bad block: {part}"));
            }
            out.push((block_id(block), weight.max(1)));
        }
        if out.is_empty() {
            return Err("No block given.".into());
        }
        Ok(Pattern(out))
    }

    fn pick(&self, rng: &mut u64) -> &str {
        if self.0.len() == 1 {
            return &self.0[0].0;
        }
        // xorshift64
        *rng ^= *rng << 13;
        *rng ^= *rng >> 7;
        *rng ^= *rng << 17;
        let total: u32 = self.0.iter().map(|(_, w)| w).sum();
        let mut roll = (*rng % u64::from(total)) as u32;
        for (b, w) in &self.0 {
            if roll < *w {
                return b;
            }
            roll -= w;
        }
        &self.0[0].0
    }
}

/// A mask for `//replace`: block ids, matched with or without their state.
fn matches(mask: &[String], state: &str) -> bool {
    mask.iter().any(|m| {
        if m.contains('[') {
            m == state
        } else {
            m == kind_of(state)
        }
    })
}

/// A rotated state: `facing`, `axis`, `rotation` and the sides of fences and
/// walls turn with the blocks, `quarters` times clockwise (seen from above).
fn rotate_state(state: &str, quarters: u8) -> String {
    let Some((kind, rest)) = state.split_once('[') else {
        return state.to_owned();
    };
    let props = rest.trim_end_matches(']');
    let turn = |side: &str| -> String {
        let order = ["north", "east", "south", "west"];
        match order.iter().position(|s| *s == side) {
            Some(i) => order[(i + quarters as usize) % 4].to_owned(),
            None => side.to_owned(),
        }
    };
    let mut out: Vec<String> = Vec::new();
    for p in props.split(',') {
        let Some((k, v)) = p.split_once('=') else {
            continue;
        };
        let (k, v) = match k {
            "facing" | "horizontal_facing" => (k.to_owned(), turn(v)),
            "axis" if quarters % 2 == 1 => (
                k.to_owned(),
                match v {
                    "x" => "z".to_owned(),
                    "z" => "x".to_owned(),
                    o => o.to_owned(),
                },
            ),
            "rotation" => (
                k.to_owned(),
                v.parse::<u32>()
                    .map_or(v.to_owned(), |r| ((r + 4 * u32::from(quarters)) % 16).to_string()),
            ),
            "north" | "east" | "south" | "west" => (turn(k), v.to_owned()),
            _ => (k.to_owned(), v.to_owned()),
        };
        out.push(format!("{k}={v}"));
    }
    out.sort();
    format!("{kind}[{}]", out.join(","))
}

fn rotate_offset([x, y, z]: Pos, quarters: u8) -> Pos {
    match quarters % 4 {
        1 => [-z, y, x],
        2 => [-x, y, -z],
        3 => [z, y, -x],
        _ => [x, y, z],
    }
}

/// The commands that write `changes`: runs along x of one block become `/fill`.
fn commands_for(world: &str, changes: &[Placed]) -> Vec<String> {
    let mut sorted: Vec<&Placed> = changes.iter().collect();
    sorted.sort_by_key(|(p, _)| (p[1], p[2], p[0]));
    let mut out = Vec::new();
    let mut i = 0;
    while i < sorted.len() {
        let (start, block) = sorted[i];
        let mut end = *start;
        let mut j = i + 1;
        while j < sorted.len() {
            let (p, b) = sorted[j];
            if b == block && p[1] == end[1] && p[2] == end[2] && p[0] == end[0] + 1 && p[0] - start[0] < 32_000 {
                end = *p;
                j += 1;
            } else {
                break;
            }
        }
        out.push(if end == *start {
            format!("execute in {world} run setblock {} {} {} {block}", start[0], start[1], start[2])
        } else {
            format!(
                "execute in {world} run fill {} {} {} {} {} {} {block}",
                start[0], start[1], start[2], end[0], end[1], end[2]
            )
        });
        i = j;
    }
    out
}

impl HorizonEdit {
    fn session(&mut self, p: &Player) -> &mut Session {
        let s = self.sessions.entry(p.uuid).or_default();
        if s.world != p.location.world {
            // A selection belongs to one world.
            s.world = p.location.world.clone();
            s.pos1 = None;
            s.pos2 = None;
        }
        s
    }

    /// The selection's corners, lowest first.
    fn selection(&mut self, p: &Player) -> Result<(Pos, Pos), String> {
        let s = self.session(p);
        let (Some(a), Some(b)) = (s.pos1, s.pos2) else {
            return Err("Make a region selection first (the wand, or //pos1 and //pos2).".into());
        };
        Ok((
            [a[0].min(b[0]), a[1].min(b[1]), a[2].min(b[2])],
            [a[0].max(b[0]), a[1].max(b[1]), a[2].max(b[2])],
        ))
    }

    fn read(&self, p: &Player, min: Pos, max: Pos) -> Result<horizon_plugin_api::types::Region, String> {
        p.world().region(min, max)
    }

    /// Writes `changes` in the player's world, recording an undo step.
    fn apply(&mut self, ctx: &mut Ctx<Self>, p: &Player, changes: Vec<Placed>) -> Result<usize, String> {
        let limit = ctx.config().int_or("limits.max-blocks", 500_000).max(1) as usize;
        if changes.len() > limit {
            return Err(format!(
                "That edit changes {} blocks; the limit is {limit} (limits.max-blocks).",
                changes.len()
            ));
        }
        if changes.is_empty() {
            msg(p, "0 blocks changed.");
            return Ok(0);
        }
        let mut lo = changes[0].0;
        let mut hi = changes[0].0;
        for (q, _) in &changes {
            for i in 0..3 {
                lo[i] = lo[i].min(q[i]);
                hi[i] = hi[i].max(q[i]);
            }
        }
        let old = self.read(p, lo, hi)?;
        let mut before = Vec::new();
        let mut after = Vec::new();
        for (q, block) in changes {
            match old.get(q[0], q[1], q[2]) {
                Some(was) if was == block => {}
                Some(was) => {
                    before.push((q, was.to_owned()));
                    after.push((q, block));
                }
                // Unloaded: nothing to restore, and fill would fail there.
                None => {}
            }
        }
        let n = after.len();
        if n == 0 {
            // Nothing to write, and nothing to undo.
            return Ok(0);
        }
        let world = p.location.world.clone();
        self.queue(p.uuid, &world, &after);
        let s = self.session(p);
        s.undo.push(Edit { world, before, after });
        if s.undo.len() > HISTORY {
            s.undo.remove(0);
        }
        s.redo.clear();
        Ok(n)
    }

    fn queue(&mut self, owner: Uuid, world: &str, changes: &[Placed]) {
        let commands = commands_for(world, changes);
        if !commands.is_empty() {
            self.jobs.push_back(Job {
                owner,
                commands,
                next: 0,
                changed: changes.len(),
            });
        }
    }

    /// Runs queued edits within the tick's budget.
    fn work(&mut self, ctx: &mut Ctx<Self>) {
        let mut budget = ctx.config().int_or("limits.commands-per-tick", 2000).max(1) as usize;
        while budget > 0 {
            let Some(job) = self.jobs.front_mut() else {
                return;
            };
            let end = (job.next + budget).min(job.commands.len());
            for c in &job.commands[job.next..end] {
                ctx.server().dispatch_command(c);
            }
            budget -= end - job.next;
            job.next = end;
            if job.next == job.commands.len() {
                let job = self.jobs.pop_front().expect("front");
                if let Some(p) = ctx.server().player_by_uuid(job.owner) {
                    msg(&p, &format!("{} blocks changed.", job.changed));
                }
            }
        }
    }

    fn rng(&mut self) -> &mut u64 {
        if self.rng == 0 {
            self.rng = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0x9E37_79B9_7F4A_7C15, |d| d.as_nanos() as u64)
                | 1;
        }
        &mut self.rng
    }

    fn fill_with(&mut self, positions: impl Iterator<Item = Pos>, pattern: &Pattern) -> Vec<Placed> {
        let mut rng = *self.rng();
        let out = positions.map(|q| (q, pattern.pick(&mut rng).to_owned())).collect();
        self.rng = rng;
        out
    }
}

fn boxed(min: Pos, max: Pos) -> impl Iterator<Item = Pos> {
    (min[1]..=max[1]).flat_map(move |y| {
        (min[2]..=max[2]).flat_map(move |z| (min[0]..=max[0]).map(move |x| [x, y, z]))
    })
}

fn here(p: &Player) -> Pos {
    let b = p.location.block();
    [b.x, b.y, b.z]
}

// The commands.

fn wand(_: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, _: &[String]) -> Result<(), String> {
    p.give(WAND, 1);
    msg(p, "Left click: select pos #1; Right click: select pos #2");
    Ok(())
}

fn pos1(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, _: &[String]) -> Result<(), String> {
    set_pos(e, p, here(p), true);
    Ok(())
}

fn pos2(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, _: &[String]) -> Result<(), String> {
    set_pos(e, p, here(p), false);
    Ok(())
}

fn set_pos(e: &mut HorizonEdit, p: &Player, at: Pos, first: bool) {
    let s = e.session(p);
    if first {
        s.pos1 = Some(at);
    } else {
        s.pos2 = Some(at);
    }
    let volume = match (s.pos1, s.pos2) {
        (Some(a), Some(b)) => format!(
            " ({})",
            (0..3).map(|i| i64::from((a[i] - b[i]).abs() + 1)).product::<i64>()
        ),
        _ => String::new(),
    };
    msg(
        p,
        &format!(
            "{} position set to ({}, {}, {}){volume}.",
            if first { "First" } else { "Second" },
            at[0],
            at[1],
            at[2]
        ),
    );
}

fn desel(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, _: &[String]) -> Result<(), String> {
    let s = e.session(p);
    s.pos1 = None;
    s.pos2 = None;
    msg(p, "Selection cleared.");
    Ok(())
}

fn size(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, _: &[String]) -> Result<(), String> {
    let (lo, hi) = e.selection(p)?;
    let d: Vec<i64> = (0..3).map(|i| i64::from(hi[i] - lo[i] + 1)).collect();
    msg(p, &format!("Region: ({}, {}, {}) - ({}, {}, {})", lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]));
    msg(p, &format!("Size: {} x {} x {}, {} blocks", d[0], d[1], d[2], d[0] * d[1] * d[2]));
    Ok(())
}

fn set(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, args: &[String]) -> Result<(), String> {
    let pattern = Pattern::parse(args.first().ok_or("Usage: //set <pattern>")?)?;
    let (lo, hi) = e.selection(p)?;
    let changes = e.fill_with(boxed(lo, hi), &pattern);
    let n = e.apply(ctx, p, changes)?;
    msg(p, &format!("Setting {n} blocks..."));
    Ok(())
}

fn replace(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, args: &[String]) -> Result<(), String> {
    let (mask, to) = match args {
        [to] => (None, to),
        [from, to] => (Some(from.split(',').map(block_id).collect::<Vec<_>>()), to),
        _ => return Err("Usage: //replace [from] <to>".into()),
    };
    let pattern = Pattern::parse(to)?;
    let (lo, hi) = e.selection(p)?;
    let region = e.read(p, lo, hi)?;
    let hit: Vec<Pos> = boxed(lo, hi)
        .filter(|q| {
            region.get(q[0], q[1], q[2]).is_some_and(|b| match &mask {
                Some(m) => matches(m, b),
                None => !is_air(b),
            })
        })
        .collect();
    let changes = e.fill_with(hit.into_iter(), &pattern);
    let n = e.apply(ctx, p, changes)?;
    msg(p, &format!("Replacing {n} blocks..."));
    Ok(())
}

fn walls(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, args: &[String]) -> Result<(), String> {
    faces(e, ctx, p, args, false)
}

fn outline(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, args: &[String]) -> Result<(), String> {
    faces(e, ctx, p, args, true)
}

fn faces(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, args: &[String], top_bottom: bool) -> Result<(), String> {
    let pattern = Pattern::parse(args.first().ok_or("Usage: //walls <pattern>")?)?;
    let (lo, hi) = e.selection(p)?;
    let on_face = |q: &Pos| {
        q[0] == lo[0] || q[0] == hi[0] || q[2] == lo[2] || q[2] == hi[2]
            || (top_bottom && (q[1] == lo[1] || q[1] == hi[1]))
    };
    let cells: Vec<Pos> = boxed(lo, hi).filter(on_face).collect();
    let changes = e.fill_with(cells.into_iter(), &pattern);
    let n = e.apply(ctx, p, changes)?;
    msg(p, &format!("{n} blocks changed."));
    Ok(())
}

fn sphere_cmd(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, args: &[String], hollow: bool) -> Result<(), String> {
    let pattern = Pattern::parse(args.first().ok_or("Usage: //sphere <pattern> <radius>")?)?;
    let r = number(args.get(1), 0)?;
    if !(1..=64).contains(&r) {
        return Err("The radius is 1 to 64.".into());
    }
    let c = here(p);
    let rf = f64::from(r) + 0.5;
    let cells: Vec<Pos> = boxed([c[0] - r, c[1] - r, c[2] - r], [c[0] + r, c[1] + r, c[2] + r])
        .filter(|q| {
            let d = |a: i32, b: i32| f64::from(a - b);
            let dist = (d(q[0], c[0]).powi(2) + d(q[1], c[1]).powi(2) + d(q[2], c[2]).powi(2)).sqrt();
            dist <= rf && (!hollow || dist > rf - 1.0) && (MIN_Y..=MAX_Y).contains(&q[1])
        })
        .collect();
    let changes = e.fill_with(cells.into_iter(), &pattern);
    let n = e.apply(ctx, p, changes)?;
    msg(p, &format!("{n} blocks have been created."));
    Ok(())
}

fn sphere(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result<(), String> {
    sphere_cmd(e, ctx, p, a, false)
}

fn hsphere(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result<(), String> {
    sphere_cmd(e, ctx, p, a, true)
}

fn cyl_cmd(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, args: &[String], hollow: bool) -> Result<(), String> {
    let pattern = Pattern::parse(args.first().ok_or("Usage: //cyl <pattern> <radius> [height]")?)?;
    let r = number(args.get(1), 0)?;
    let h = number(args.get(2), 1)?;
    if !(1..=64).contains(&r) || !(1..=256).contains(&h) {
        return Err("The radius is 1 to 64, the height 1 to 256.".into());
    }
    let c = here(p);
    let rf = f64::from(r) + 0.5;
    let cells: Vec<Pos> = boxed([c[0] - r, c[1], c[2] - r], [c[0] + r, c[1] + h - 1, c[2] + r])
        .filter(|q| {
            let dist = (f64::from(q[0] - c[0]).powi(2) + f64::from(q[2] - c[2]).powi(2)).sqrt();
            dist <= rf && (!hollow || dist > rf - 1.0) && (MIN_Y..=MAX_Y).contains(&q[1])
        })
        .collect();
    let changes = e.fill_with(cells.into_iter(), &pattern);
    let n = e.apply(ctx, p, changes)?;
    msg(p, &format!("{n} blocks have been created."));
    Ok(())
}

fn cyl(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result<(), String> {
    cyl_cmd(e, ctx, p, a, false)
}

fn hcyl(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result<(), String> {
    cyl_cmd(e, ctx, p, a, true)
}

fn copy_region(e: &mut HorizonEdit, p: &Player) -> Result<(Pos, Pos, usize), String> {
    let (lo, hi) = e.selection(p)?;
    let region = e.read(p, lo, hi)?;
    let origin = here(p);
    let blocks: Vec<Placed> = boxed(lo, hi)
        .filter_map(|q| {
            let b = region.get(q[0], q[1], q[2])?;
            Some(([q[0] - origin[0], q[1] - origin[1], q[2] - origin[2]], b.to_owned()))
        })
        .collect();
    let n = blocks.len();
    e.session(p).clipboard = Some(blocks);
    Ok((lo, hi, n))
}

fn copy(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, _: &[String]) -> Result<(), String> {
    let (_, _, n) = copy_region(e, p)?;
    msg(p, &format!("{n} blocks copied."));
    Ok(())
}

fn cut(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, _: &[String]) -> Result<(), String> {
    let (lo, hi, n) = copy_region(e, p)?;
    let changes = boxed(lo, hi).map(|q| (q, "minecraft:air".to_owned())).collect();
    e.apply(ctx, p, changes)?;
    msg(p, &format!("{n} blocks cut."));
    Ok(())
}

fn paste(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, args: &[String]) -> Result<(), String> {
    let skip_air = args.iter().any(|a| a == "-a");
    let clip = e
        .session(p)
        .clipboard
        .clone()
        .ok_or("Your clipboard is empty. Use //copy first.")?;
    let o = here(p);
    let changes: Vec<Placed> = clip
        .into_iter()
        .filter(|(_, b)| !(skip_air && is_air(b)))
        .map(|(d, b)| ([o[0] + d[0], o[1] + d[1], o[2] + d[2]], b))
        .filter(|(q, _)| (MIN_Y..=MAX_Y).contains(&q[1]))
        .collect();
    let n = e.apply(ctx, p, changes)?;
    msg(p, &format!("The clipboard has been pasted at ({}, {}, {}): {n} blocks.", o[0], o[1], o[2]));
    Ok(())
}

fn rotate(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, args: &[String]) -> Result<(), String> {
    let deg = number(args.first(), 90)?;
    if deg % 90 != 0 {
        return Err("Rotate by a multiple of 90 degrees.".into());
    }
    let quarters = (deg.rem_euclid(360) / 90) as u8;
    let s = e.session(p);
    let clip = s.clipboard.as_mut().ok_or("Your clipboard is empty. Use //copy first.")?;
    for (d, b) in clip.iter_mut() {
        *d = rotate_offset(*d, quarters);
        *b = rotate_state(b, quarters);
    }
    msg(p, &format!("The clipboard copy has been rotated by {deg} degrees."));
    Ok(())
}

fn resize(e: &mut HorizonEdit, p: &Player, args: &[String], how: &str) -> Result<(), String> {
    let (mut lo, mut hi) = e.selection(p)?;
    if how == "expand" && args.first().is_some_and(|a| a == "vert") {
        lo[1] = MIN_Y;
        hi[1] = MAX_Y;
    } else {
        let n = number(args.first(), 1)?;
        let d = dir(p, args.get(1))?;
        for i in 0..3 {
            match (how, d[i]) {
                ("expand", 1) => hi[i] += n,
                ("expand", -1) => lo[i] -= n,
                ("contract", 1) => lo[i] = (lo[i] + n).min(hi[i]),
                ("contract", -1) => hi[i] = (hi[i] - n).max(lo[i]),
                ("shift", s) => {
                    lo[i] += s * n;
                    hi[i] += s * n;
                }
                _ => {}
            }
        }
    }
    let s = e.session(p);
    s.pos1 = Some(lo);
    s.pos2 = Some(hi);
    let volume: i64 = (0..3).map(|i| i64::from(hi[i] - lo[i] + 1)).product();
    msg(p, &format!("Region {how}ed: {volume} blocks."));
    Ok(())
}

fn expand(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result<(), String> {
    resize(e, p, a, "expand")
}

fn contract(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result<(), String> {
    resize(e, p, a, "contract")
}

fn shift(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result<(), String> {
    resize(e, p, a, "shift")
}

fn stack(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, args: &[String]) -> Result<(), String> {
    let n = number(args.first(), 1)?;
    let d = dir(p, args.get(1))?;
    if !(1..=64).contains(&n) {
        return Err("Stack 1 to 64 times.".into());
    }
    let (lo, hi) = e.selection(p)?;
    let region = e.read(p, lo, hi)?;
    let step: Vec<i32> = (0..3).map(|i| d[i] * (hi[i] - lo[i] + 1)).collect();
    let mut changes = Vec::new();
    for k in 1..=n {
        for q in boxed(lo, hi) {
            if let Some(b) = region.get(q[0], q[1], q[2]) {
                let to = [q[0] + step[0] * k, q[1] + step[1] * k, q[2] + step[2] * k];
                if (MIN_Y..=MAX_Y).contains(&to[1]) {
                    changes.push((to, b.to_owned()));
                }
            }
        }
    }
    let count = e.apply(ctx, p, changes)?;
    msg(p, &format!("{count} blocks changed. Undo with //undo"));
    Ok(())
}

fn move_cmd(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, args: &[String]) -> Result<(), String> {
    let n = number(args.first(), 1)?;
    let d = dir(p, args.get(1))?;
    let (lo, hi) = e.selection(p)?;
    let region = e.read(p, lo, hi)?;
    let off = [d[0] * n, d[1] * n, d[2] * n];
    // The source becomes air, then the blocks land: a target wins over air.
    let mut target: HashMap<Pos, String> = boxed(lo, hi).map(|q| (q, "minecraft:air".to_owned())).collect();
    for q in boxed(lo, hi) {
        if let Some(b) = region.get(q[0], q[1], q[2]) {
            target.insert([q[0] + off[0], q[1] + off[1], q[2] + off[2]], b.to_owned());
        }
    }
    let count = e.apply(ctx, p, target.into_iter().collect())?;
    let s = e.session(p);
    s.pos1 = Some([lo[0] + off[0], lo[1] + off[1], lo[2] + off[2]]);
    s.pos2 = Some([hi[0] + off[0], hi[1] + off[1], hi[2] + off[2]]);
    msg(p, &format!("{count} blocks moved."));
    Ok(())
}

fn count(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, args: &[String]) -> Result<(), String> {
    let mask: Vec<String> = args
        .first()
        .ok_or("Usage: //count <block>")?
        .split(',')
        .map(block_id)
        .collect();
    let (lo, hi) = e.selection(p)?;
    let region = e.read(p, lo, hi)?;
    let n = boxed(lo, hi)
        .filter(|q| region.get(q[0], q[1], q[2]).is_some_and(|b| matches(&mask, b)))
        .count();
    msg(p, &format!("Counted: {n}"));
    Ok(())
}

fn distr(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, _: &[String]) -> Result<(), String> {
    let (lo, hi) = e.selection(p)?;
    let region = e.read(p, lo, hi)?;
    let mut by: HashMap<&str, usize> = HashMap::new();
    let mut total = 0;
    for q in boxed(lo, hi) {
        if let Some(b) = region.get(q[0], q[1], q[2]) {
            *by.entry(kind_of(b)).or_default() += 1;
            total += 1;
        }
    }
    let mut list: Vec<(&str, usize)> = by.into_iter().collect();
    list.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    msg(p, &format!("# total blocks: {total}"));
    for (kind, n) in list.into_iter().take(10) {
        let pct = n as f64 * 100.0 / total.max(1) as f64;
        p.send_message(&format!("§7{n:>8}  §f{pct:5.1}%  §e{}", kind.trim_start_matches("minecraft:")));
    }
    Ok(())
}

fn drain(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, args: &[String]) -> Result<(), String> {
    let r = number(args.first(), 5)?;
    if !(1..=32).contains(&r) {
        return Err("The radius is 1 to 32.".into());
    }
    let c = here(p);
    let (lo, hi) = (
        [c[0] - r, (c[1] - r).max(MIN_Y), c[2] - r],
        [c[0] + r, (c[1] + r).min(MAX_Y), c[2] + r],
    );
    let region = e.read(p, lo, hi)?;
    let changes: Vec<Placed> = boxed(lo, hi)
        .filter(|q| {
            region
                .get(q[0], q[1], q[2])
                .is_some_and(|b| matches!(kind_of(b), "minecraft:water" | "minecraft:lava"))
        })
        .map(|q| (q, "minecraft:air".to_owned()))
        .collect();
    let n = e.apply(ctx, p, changes)?;
    msg(p, &format!("{n} blocks have been drained."));
    Ok(())
}

fn history(e: &mut HorizonEdit, p: &Player, args: &[String], undo: bool) -> Result<(), String> {
    let times = number(args.first(), 1)?.clamp(1, HISTORY as i32);
    let mut done = 0;
    for _ in 0..times {
        let s = e.session(p);
        let Some(edit) = (if undo { s.undo.pop() } else { s.redo.pop() }) else {
            break;
        };
        let blocks = if undo { &edit.before } else { &edit.after };
        let (world, blocks) = (edit.world.clone(), blocks.clone());
        if undo {
            s.redo.push(edit);
        } else {
            s.undo.push(edit);
        }
        e.queue(p.uuid, &world, &blocks);
        done += 1;
    }
    if done == 0 {
        return Err(format!("Nothing left to {}.", if undo { "undo" } else { "redo" }));
    }
    msg(p, &format!("{} {done} edit(s).", if undo { "Undid" } else { "Redid" }));
    Ok(())
}

fn undo(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result<(), String> {
    history(e, p, a, true)
}

fn redo(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result<(), String> {
    history(e, p, a, false)
}

fn up(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, args: &[String]) -> Result<(), String> {
    let n = number(args.first(), 1)?;
    if !(1..=256).contains(&n) {
        return Err("Go up 1 to 256 blocks.".into());
    }
    let c = here(p);
    let floor = [c[0], c[1] + n - 1, c[2]];
    e.apply(ctx, p, vec![(floor, "minecraft:glass".to_owned())])?;
    let l = &p.location;
    p.teleport(&Location::new(&l.world, l.x, f64::from(floor[1] + 1), l.z));
    msg(p, "Whoosh!");
    Ok(())
}

fn help(_: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, _: &[String]) -> Result<(), String> {
    msg(p, "HorizonEdit commands:");
    for (usage, what) in COMMANDS.iter().map(|c| (c.1, c.2)) {
        p.send_message(&format!("§e{usage} §7{what}"));
    }
    Ok(())
}

/// Name (with WorldEdit's leading `/`), usage, description, what runs it.
const COMMANDS: &[(&str, &str, &str, Run)] = &[
    ("/wand", "//wand", "a wooden axe to select with", wand),
    ("/pos1", "//pos1", "the first corner where you stand", pos1),
    ("/pos2", "//pos2", "the second corner where you stand", pos2),
    ("/sel", "//sel", "clear the selection", desel),
    ("/size", "//size", "the selection's size", size),
    ("/set", "//set <pattern>", "fill the selection", set),
    ("/replace", "//replace [from] <to>", "replace blocks (all but air without from)", replace),
    ("/walls", "//walls <pattern>", "the selection's four walls", walls),
    ("/outline", "//outline <pattern>", "walls, floor and ceiling", outline),
    ("/sphere", "//sphere <pattern> <radius>", "a sphere where you stand", sphere),
    ("/hsphere", "//hsphere <pattern> <radius>", "a hollow sphere", hsphere),
    ("/cyl", "//cyl <pattern> <radius> [height]", "a cylinder", cyl),
    ("/hcyl", "//hcyl <pattern> <radius> [height]", "a hollow cylinder", hcyl),
    ("/copy", "//copy", "copy the selection, from where you stand", copy),
    ("/cut", "//cut", "copy, then clear the selection", cut),
    ("/paste", "//paste [-a]", "paste where you stand (-a: skip air)", paste),
    ("/rotate", "//rotate <90|180|270>", "turn the clipboard", rotate),
    ("/stack", "//stack [times] [dir]", "repeat the selection", stack),
    ("/move", "//move [blocks] [dir]", "move the selection's blocks", move_cmd),
    ("/expand", "//expand <blocks> [dir] | vert", "grow the selection", expand),
    ("/contract", "//contract <blocks> [dir]", "shrink the selection", contract),
    ("/shift", "//shift <blocks> [dir]", "move the selection itself", shift),
    ("/count", "//count <block>", "count a block in the selection", count),
    ("/distr", "//distr", "the selection's blocks, most first", distr),
    ("/drain", "//drain [radius]", "remove water and lava around you", drain),
    ("/undo", "//undo [times]", "undo your edits", undo),
    ("/redo", "//redo [times]", "redo what you undid", redo),
    ("/up", "//up <blocks>", "go up onto a glass block", up),
    ("/help", "//help", "this list", help),
];

/// `//expand vert`'s first word.
fn name_is_expand(args: &[String]) -> bool {
    args.len() == 1
}

/// Blocks offered as a pattern is typed.
const COMMON_BLOCKS: &[&str] = &[
    "air", "stone", "cobblestone", "dirt", "grass_block", "sand", "gravel", "glass", "oak_planks",
    "spruce_planks", "oak_log", "stone_bricks", "bricks", "smooth_stone", "deepslate", "andesite",
    "diorite", "granite", "water", "lava", "white_concrete", "black_concrete", "red_concrete",
    "quartz_block", "glowstone", "sea_lantern", "obsidian", "bedrock",
];

impl Plugin for HorizonEdit {
    fn on_enable(&mut self, ctx: &mut Ctx<Self>) {
        ctx.save_default_config(CONFIG);
        for &(name, usage, description, run) in COMMANDS {
            ctx.command(
                CommandSpec::new(name)
                    .description(description)
                    .usage(usage)
                    .permission(PERMISSION),
                move |plugin, ctx, sender, _, args| {
                    let Some(p) = sender.player() else {
                        sender.send_message("HorizonEdit's commands are for players.");
                        return true;
                    };
                    if let Err(e) = run(plugin, ctx, p, args) {
                        p.send_message(&format!("§c{e}"));
                    }
                    true
                },
            );
        }

        // The wand: a left click is the first corner, digging nothing.
        ctx.listen(EventPriority::Lowest, false, |plugin: &mut HorizonEdit, _, e: &mut PlayerInteractEvent| {
            let holds = e.item.as_ref().is_some_and(|i| i.kind == WAND);
            let Some(at) = e.block.as_ref().map(|b| [b.pos.x, b.pos.y, b.pos.z]) else {
                return;
            };
            if e.action == "left_click_block" && holds && e.player.has_permission(PERMISSION) {
                e.set_cancelled(true);
                set_pos(plugin, &e.player, at, true);
            }
        });
        // In creative a break may come without a click: keep the block.
        ctx.listen(EventPriority::Lowest, false, |_, _, e: &mut BlockBreakEvent| {
            let holds = e.player.item_in_hand().is_some_and(|i| i.kind == WAND);
            if holds && e.player.has_permission(PERMISSION) {
                e.set_cancelled(true);
            }
        });
        // Right click on a block is the second.
        ctx.listen(EventPriority::Lowest, false, |plugin: &mut HorizonEdit, ctx, e: &mut PlayerInteractEvent| {
            let holds = e.item.as_ref().is_some_and(|i| i.kind == WAND);
            let Some(at) = e.block.as_ref().map(|b| [b.pos.x, b.pos.y, b.pos.z]) else {
                return;
            };
            if e.action != "right_click_block" || !holds || !e.player.has_permission(PERMISSION) {
                return;
            }
            e.set_cancelled(true);
            let tick = ctx.tick();
            let s = plugin.session(&e.player);
            if s.last_click == Some((at, tick)) {
                return;
            }
            s.last_click = Some((at, tick));
            set_pos(plugin, &e.player, at, false);
        });
        ctx.on(|plugin: &mut HorizonEdit, _, e: &mut PlayerQuitEvent| {
            plugin.sessions.remove(&e.player.uuid);
        });

        // What arguments complete to.
        const DIRS: [&str; 8] = ["me", "back", "up", "down", "north", "south", "east", "west"];
        for name in ["/expand", "/contract", "/shift", "/stack", "/move"] {
            ctx.set_tab_completer(name, |_, _, _, _, args| {
                if args.len() == 2 {
                    DIRS.iter().map(|d| (*d).to_owned()).collect()
                } else if name_is_expand(args) {
                    vec!["vert".to_owned()]
                } else {
                    Vec::new()
                }
            });
        }
        ctx.set_tab_completer("/rotate", |_, _, _, _, _| vec!["90".into(), "180".into(), "270".into()]);
        ctx.set_tab_completer("/paste", |_, _, _, _, _| vec!["-a".into()]);
        for name in ["/set", "/walls", "/outline", "/sphere", "/hsphere", "/cyl", "/hcyl", "/replace", "/count"] {
            ctx.set_tab_completer(name, |_, _, _, _, args| {
                if args.len() <= 2 {
                    COMMON_BLOCKS.iter().map(|b| (*b).to_owned()).collect()
                } else {
                    Vec::new()
                }
            });
        }

        ctx.run_timer(1, 1, |plugin, ctx| plugin.work(ctx));
        ctx.logger().info(&format!("{} commands ready; //help lists them", COMMANDS.len()));
    }
}

horizon_plugin!(
    HorizonEdit::default(),
    PluginDescription::new("HorizonEdit", "1.0.0")
        .description("WorldEdit's commands, for Horizon")
        .author("Horizon")
        .permission(
            Permission::new(PERMISSION)
                .description("Use HorizonEdit's wand and commands")
                .default(PermissionDefault::Op)
        )
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_become_fills() {
        let changes: Vec<Placed> = (0..5)
            .map(|x| ([x, 64, 0], "minecraft:stone".to_owned()))
            .chain([([7, 64, 0], "minecraft:stone".to_owned()), ([8, 64, 0], "minecraft:dirt".to_owned())])
            .collect();
        let c = commands_for("minecraft:overworld", &changes);
        assert_eq!(
            c,
            vec![
                "execute in minecraft:overworld run fill 0 64 0 4 64 0 minecraft:stone",
                "execute in minecraft:overworld run setblock 7 64 0 minecraft:stone",
                "execute in minecraft:overworld run setblock 8 64 0 minecraft:dirt",
            ]
        );
    }

    #[test]
    fn rotation_turns_blocks_and_offsets() {
        assert_eq!(rotate_offset([0, 0, -1], 1), [1, 0, 0], "north becomes east");
        assert_eq!(
            rotate_state("minecraft:oak_stairs[facing=north,half=bottom]", 1),
            "minecraft:oak_stairs[facing=east,half=bottom]"
        );
        assert_eq!(rotate_state("minecraft:oak_log[axis=x]", 1), "minecraft:oak_log[axis=z]");
        assert_eq!(rotate_state("minecraft:oak_sign[rotation=14]", 1), "minecraft:oak_sign[rotation=2]");
        assert_eq!(
            rotate_state("minecraft:oak_fence[east=false,north=true,south=false,waterlogged=false,west=false]", 1),
            "minecraft:oak_fence[east=true,north=false,south=false,waterlogged=false,west=false]"
        );
        assert_eq!(rotate_state("minecraft:stone", 3), "minecraft:stone");
    }

    #[test]
    fn patterns_and_masks() {
        let p = Pattern::parse("50%stone,50%dirt").unwrap();
        let mut rng = 12345;
        let picks: Vec<&str> = (0..200).map(|_| p.pick(&mut rng)).collect();
        assert!(picks.contains(&"minecraft:stone") && picks.contains(&"minecraft:dirt"));
        assert!(Pattern::parse("").is_err());
        assert!(Pattern::parse("x%stone").is_err());
        let mask = vec!["minecraft:oak_stairs".to_owned()];
        assert!(matches(&mask, "minecraft:oak_stairs[facing=east]"));
        assert!(!matches(&mask, "minecraft:stone"));
        assert!(is_air("minecraft:cave_air"));
    }
}
