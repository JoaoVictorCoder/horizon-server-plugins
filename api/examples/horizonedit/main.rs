//! HorizonEdit: WorldEdit's commands for a Horizon server, written anew for
//! horizon-plugin-api (no WorldEdit code). `//help` lists them.
//!
//! Select with the wand (a wooden axe: left click the first corner, right
//! click the second) or `//pos1`, `//pos2`, `//hpos1`, `//chunk`... then
//! edit the region, generate shapes, copy and paste (and keep schematics),
//! use brushes and tools, and undo. Patterns are a block or a mix
//! (`50%stone,30%cobblestone,andesite`); masks pick blocks (`stone,dirt`,
//! `!air`, `#existing`, `#solid`, `>grass_block`). Directions default to
//! where the player looks (`me`). Every command also works with one slash.
//!
//! Edits are written with `/fill` rows, a few thousand commands a tick, so
//! a big edit doesn't stall the server; each can be undone.

mod clipboard;
mod expr;
mod generation;
mod navigation;
mod pattern;
mod region;
mod selection;
mod tools;
mod utility;

use horizon_plugin_api::prelude::*;
use horizon_plugin_api::types::Region;
use pattern::{Mask, Pattern, is_liquid};
use std::collections::{HashMap, VecDeque};

/// The wand.
pub const WAND: &str = "minecraft:wooden_axe";
pub const PERMISSION: &str = "horizonedit.use";
/// Undo steps kept per player.
const HISTORY: usize = 15;
/// The overworld's build height.
pub const MIN_Y: i32 = -64;
pub const MAX_Y: i32 = 319;

const CONFIG: &str = "\
# HorizonEdit's limits.
limits:
  # Blocks one edit may change (a player's //limit can lower it).
  max-blocks: 500000
  # Commands sent to the world per tick (20 a second) while an edit runs.
  commands-per-tick: 2000
";

pub type Pos = [i32; 3];
/// A block at a place, as `/setblock` writes it.
pub type Placed = (Pos, String);

/// What a player copied, relative to where they stood.
#[derive(Clone)]
pub struct Clipboard {
    pub blocks: Vec<Placed>,
    /// Where they stood.
    pub origin: Pos,
}

/// The shape of a `/brush set`.
#[derive(Clone, Copy, PartialEq)]
pub enum Shape {
    Sphere,
    Cylinder,
    Cuboid,
}

/// A brush bound to an item. `/size` changes its radius, `/material` its
/// pattern.
#[derive(Clone)]
pub enum Brush {
    Sphere { pattern: Pattern, radius: i32, hollow: bool },
    Cylinder { pattern: Pattern, radius: i32, height: i32, hollow: bool },
    Set { shape: Shape, pattern: Pattern, radius: i32 },
    Smooth { radius: i32, iterations: u32, mask: Option<Mask> },
    Clipboard { skip_air: bool },
    Gravity { radius: i32, full_height: bool },
    Forest { radius: i32, density: f64, tree: String },
    Extinguish { radius: i32 },
    Butcher { radius: i32 },
    Raise { radius: i32 },
    Lower { radius: i32 },
}

#[derive(Clone)]
pub struct BrushBinding {
    pub brush: Brush,
    pub mask: Option<Mask>,
    /// How far it reaches (`/range`).
    pub range: f64,
}

/// A tool bound to an item.
#[derive(Clone)]
pub enum Tool {
    Tree(String),
    /// Right click places the pattern; left click picks the block clicked.
    Replace(Pattern),
    Info,
    /// The block clicked and every one like it touching it, within range.
    FloodFill(Pattern, i32),
    /// Logs and leaves touching the clicked one.
    DelTree,
    /// The wand at any distance.
    FarWand,
    /// Left click places the first pattern where it looks, right click the
    /// second against the block it looks at.
    LrBuild(Pattern, Pattern),
    /// Left click `/jumpto`, right click `/thru`.
    NavWand,
    /// Repeats the clicked block away from the face clicked, within range.
    Stacker(i32),
    /// The selection wand on another item.
    SelWand,
}

/// What the super pickaxe breaks with a left click.
#[derive(Clone, Copy, PartialEq)]
pub enum SuperPickaxe {
    Single,
    Area(i32),
    Recursive(i32),
}

#[derive(Default)]
pub struct Session {
    pub world: String,
    pub pos1: Option<Pos>,
    pub pos2: Option<Pos>,
    pub clipboard: Option<Clipboard>,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    /// The last click handled, so one heard for both hands counts once.
    pub last_click: Option<(Pos, u64)>,
    /// `//gmask`: every edit only changes blocks it lets through.
    pub mask: Option<Mask>,
    /// `//limit`.
    pub limit: Option<usize>,
    /// By item id.
    pub brushes: HashMap<String, BrushBinding>,
    pub tools: HashMap<String, Tool>,
    pub super_pickaxe: Option<SuperPickaxe>,
}

#[derive(Clone)]
struct Edit {
    world: String,
    before: Vec<Placed>,
    after: Vec<Placed>,
    /// Commands that made it (trees), when not plain blocks.
    commands: Vec<String>,
}

struct Job {
    owner: Uuid,
    commands: Vec<String>,
    next: usize,
    changed: usize,
    failed: usize,
    quiet: bool,
}

#[derive(Default)]
pub struct HorizonEdit {
    pub sessions: HashMap<Uuid, Session>,
    jobs: VecDeque<Job>,
    rng: u64,
}

/// What runs a command: the plugin, its ctx, the player, the arguments.
pub type Run = fn(&mut HorizonEdit, &mut Ctx<HorizonEdit>, &Player, &[String]) -> Result<(), String>;

pub fn msg(p: &Player, text: &str) {
    p.send_message(&format!("§d{text}"));
}

/// The arguments without their flags (`-a`, `-hr`), and the flags' letters.
pub fn flags(args: &[String]) -> (Vec<String>, String) {
    let mut rest = Vec::new();
    let mut letters = String::new();
    for a in args {
        match a.strip_prefix('-') {
            Some(f) if !f.is_empty() && f.chars().all(|c| c.is_ascii_alphabetic()) => letters.push_str(f),
            _ => rest.push(a.clone()),
        }
    }
    (rest, letters)
}

pub fn number(word: Option<&String>, default: i32) -> Result<i32, String> {
    match word {
        None => Ok(default),
        Some(w) => w.parse().map_err(|_| format!("Not a number: {w}")),
    }
}

pub fn float(word: Option<&String>, default: f64) -> Result<f64, String> {
    match word {
        None => Ok(default),
        Some(w) => w.trim_end_matches('%').parse().map_err(|_| format!("Not a number: {w}")),
    }
}

/// A direction word, or where the player looks (`me`, or none given).
pub fn dir(p: &Player, word: Option<&String>) -> Result<Pos, String> {
    Ok(match word.map(|w| w.to_ascii_lowercase()).as_deref() {
        None | Some("me" | "forward" | "f") => p.location.facing().offset(),
        Some("back" | "b") => p.location.facing().offset().map(|v| -v),
        Some("up" | "u") => [0, 1, 0],
        Some("down" | "d") => [0, -1, 0],
        Some("north" | "n") => [0, 0, -1],
        Some("south" | "s") => [0, 0, 1],
        Some("west" | "w") => [-1, 0, 0],
        Some("east" | "e") => [1, 0, 0],
        Some("left" | "l") => {
            let [x, _, z] = p.location.facing().offset();
            [z, 0, -x]
        }
        Some("right" | "r") => {
            let [x, _, z] = p.location.facing().offset();
            [-z, 0, x]
        }
        Some(other) => {
            return Err(format!(
                "Unknown direction: {other} (me, back, left, right, up, down, north, south, east, west)"
            ));
        }
    })
}

pub fn boxed(min: Pos, max: Pos) -> impl Iterator<Item = Pos> {
    (min[1]..=max[1]).flat_map(move |y| (min[2]..=max[2]).flat_map(move |z| (min[0]..=max[0]).map(move |x| [x, y, z])))
}

pub fn here(p: &Player) -> Pos {
    let b = p.location.block();
    [b.x, b.y, b.z]
}

pub fn add(a: Pos, b: Pos) -> Pos {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub fn in_world(q: &Pos) -> bool {
    (MIN_Y..=MAX_Y).contains(&q[1])
}

/// The block a player looks at within `range`, and the air before it.
pub fn target(p: &Player, range: f64) -> Option<(Pos, Pos)> {
    let l = &p.location;
    let (yaw, pitch) = (f64::from(l.yaw).to_radians(), f64::from(l.pitch).to_radians());
    let d = [-yaw.sin() * pitch.cos(), -pitch.sin(), yaw.cos() * pitch.cos()];
    let eye = [l.x, l.y + 1.62, l.z];
    let w = p.world();
    let mut last = here(p);
    let mut prev = None;
    let mut t = 0.0;
    while t < range {
        let q = [
            (eye[0] + d[0] * t).floor() as i32,
            (eye[1] + d[1] * t).floor() as i32,
            (eye[2] + d[2] * t).floor() as i32,
        ];
        if prev != Some(q) {
            prev = Some(q);
            if !in_world(&q) {
                return None;
            }
            let b = w.block_at(q[0], q[1], q[2])?;
            if !b.is_air() && !is_liquid(&b.kind) {
                return Some((q, last));
            }
            last = q;
        }
        t += 0.1;
    }
    None
}

/// The commands that write `changes`: runs along x of one block become `/fill`.
pub fn commands_for(world: &str, changes: &[Placed]) -> Vec<String> {
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
    pub fn session(&mut self, p: &Player) -> &mut Session {
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
    pub fn selection(&mut self, p: &Player) -> Result<(Pos, Pos), String> {
        let s = self.session(p);
        let (Some(a), Some(b)) = (s.pos1, s.pos2) else {
            return Err("Make a region selection first (the wand, or //pos1 and //pos2).".into());
        };
        Ok((
            [a[0].min(b[0]), a[1].min(b[1]), a[2].min(b[2])],
            [a[0].max(b[0]), a[1].max(b[1]), a[2].max(b[2])],
        ))
    }

    pub fn set_selection(&mut self, p: &Player, lo: Pos, hi: Pos) {
        let s = self.session(p);
        s.pos1 = Some(lo);
        s.pos2 = Some(hi);
    }

    pub fn read(&self, p: &Player, min: Pos, max: Pos) -> Result<Region, String> {
        let lo = [min[0], min[1].max(MIN_Y), min[2]];
        let hi = [max[0], max[1].min(MAX_Y), max[2]];
        if lo[1] > hi[1] {
            return Err("That is outside the world's height.".into());
        }
        p.world().region(lo, hi)
    }

    pub fn rng(&mut self) -> &mut u64 {
        if self.rng == 0 {
            self.rng = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0x9E37_79B9_7F4A_7C15, |d| d.as_nanos() as u64)
                | 1;
        }
        &mut self.rng
    }

    pub fn fill_with(&mut self, positions: impl IntoIterator<Item = Pos>, pattern: &Pattern) -> Vec<Placed> {
        let mut rng = *self.rng();
        let out = positions.into_iter().map(|q| (q, pattern.pick(&mut rng).to_owned())).collect();
        self.rng = rng;
        out
    }

    fn limit(&mut self, ctx: &mut Ctx<Self>, p: &Player) -> usize {
        let max = ctx.config().int_or("limits.max-blocks", 500_000).max(1) as usize;
        self.session(p).limit.map_or(max, |l| l.min(max))
    }

    /// Writes `changes` in the player's world (the last of two at one place
    /// wins), through the session's mask, recording an undo step.
    pub fn apply(&mut self, ctx: &mut Ctx<Self>, p: &Player, changes: Vec<Placed>) -> Result<usize, String> {
        self.apply_masked(ctx, p, changes, None)
    }

    /// [`apply`](Self::apply) with one more mask (a brush's).
    pub fn apply_masked(
        &mut self,
        ctx: &mut Ctx<Self>,
        p: &Player,
        changes: Vec<Placed>,
        extra: Option<&Mask>,
    ) -> Result<usize, String> {
        let limit = self.limit(ctx, p);
        let mut unique: HashMap<Pos, String> = HashMap::with_capacity(changes.len());
        for (q, b) in changes {
            if in_world(&q) {
                unique.insert(q, b);
            }
        }
        if unique.len() > limit {
            return Err(format!(
                "That edit changes {} blocks; the limit is {limit} (//limit, limits.max-blocks).",
                unique.len()
            ));
        }
        if unique.is_empty() {
            msg(p, "0 blocks changed.");
            return Ok(0);
        }
        let mask = self.session(p).mask.clone();
        let masks: Vec<&Mask> = mask.iter().chain(extra).collect();
        let around = i32::from(masks.iter().any(|m| m.needs_neighbours()));
        let mut lo = [i32::MAX; 3];
        let mut hi = [i32::MIN; 3];
        for q in unique.keys() {
            for i in 0..3 {
                lo[i] = lo[i].min(q[i]);
                hi[i] = hi[i].max(q[i]);
            }
        }
        let old = self.read(p, [lo[0], lo[1] - around, lo[2]], [hi[0], hi[1] + around, hi[2]])?;
        let mut before = Vec::new();
        let mut after = Vec::new();
        for (q, block) in unique {
            let Some(was) = old.get(q[0], q[1], q[2]) else {
                // Unloaded: nothing to restore, and the write would fail.
                continue;
            };
            if was == block {
                continue;
            }
            let below = old.get(q[0], q[1] - 1, q[2]);
            let above = old.get(q[0], q[1] + 1, q[2]);
            if !masks.iter().all(|m| m.test(was, below, above)) {
                continue;
            }
            before.push((q, was.to_owned()));
            after.push((q, block));
        }
        let n = after.len();
        if n == 0 {
            msg(p, "0 blocks changed.");
            return Ok(0);
        }
        let world = p.location.world.clone();
        self.queue(p.uuid, &world, &after, false);
        self.remember(p, Edit {
            world,
            before,
            after,
            commands: Vec::new(),
        });
        Ok(n)
    }

    /// Runs `commands` (trees...) after a snapshot of `lo..hi`, so an undo
    /// puts those blocks back.
    pub fn apply_commands(&mut self, p: &Player, lo: Pos, hi: Pos, commands: Vec<String>) -> Result<(), String> {
        let old = self.read(p, lo, hi)?;
        let before: Vec<Placed> = boxed(lo, hi)
            .filter_map(|q| old.get(q[0], q[1], q[2]).map(|b| (q, b.to_owned())))
            .collect();
        let world = p.location.world.clone();
        self.jobs.push_back(Job {
            owner: p.uuid,
            changed: commands.len(),
            commands: commands.clone(),
            next: 0,
            failed: 0,
            quiet: true,
        });
        self.remember(p, Edit {
            world,
            before,
            after: Vec::new(),
            commands,
        });
        Ok(())
    }

    fn remember(&mut self, p: &Player, edit: Edit) {
        let s = self.session(p);
        s.undo.push(edit);
        if s.undo.len() > HISTORY {
            s.undo.remove(0);
        }
        s.redo.clear();
    }

    fn queue(&mut self, owner: Uuid, world: &str, changes: &[Placed], quiet: bool) {
        let commands = commands_for(world, changes);
        if !commands.is_empty() {
            self.jobs.push_back(Job {
                owner,
                commands,
                next: 0,
                changed: changes.len(),
                failed: 0,
                quiet,
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
                if !ctx.server().dispatch_command(c) {
                    job.failed += 1;
                }
            }
            budget -= end - job.next;
            job.next = end;
            if job.next == job.commands.len() {
                let job = self.jobs.pop_front().expect("front");
                let Some(p) = ctx.server().player_by_uuid(job.owner) else {
                    continue;
                };
                if job.failed > 0 {
                    p.send_message(&format!(
                        "§c{} of {} writes failed (is that area loaded?). See the server log.",
                        job.failed,
                        job.commands.len()
                    ));
                } else if !job.quiet {
                    msg(&p, &format!("{} blocks changed.", job.changed));
                }
            }
        }
    }
}

// History and limits.

fn history(e: &mut HorizonEdit, p: &Player, args: &[String], undo: bool) -> Result<(), String> {
    let times = number(args.first(), 1)?.clamp(1, HISTORY as i32);
    let mut done = 0;
    for _ in 0..times {
        let s = e.session(p);
        let Some(edit) = (if undo { s.undo.pop() } else { s.redo.pop() }) else {
            break;
        };
        let world = edit.world.clone();
        if undo {
            let blocks = edit.before.clone();
            s.redo.push(edit);
            e.queue(p.uuid, &world, &blocks, false);
        } else {
            let (blocks, commands) = (edit.after.clone(), edit.commands.clone());
            s.undo.push(edit);
            e.queue(p.uuid, &world, &blocks, false);
            if !commands.is_empty() {
                e.jobs.push_back(Job {
                    owner: p.uuid,
                    changed: commands.len(),
                    commands,
                    next: 0,
                    failed: 0,
                    quiet: true,
                });
            }
        }
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

fn clearhistory(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, _: &[String]) -> Result<(), String> {
    let s = e.session(p);
    s.undo.clear();
    s.redo.clear();
    msg(p, "History cleared.");
    Ok(())
}

fn limit(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, args: &[String]) -> Result<(), String> {
    let n = number(args.first(), -1)?;
    e.session(p).limit = (n >= 0).then_some(n as usize);
    msg(p, &if n >= 0 { format!("Block change limit set to {n}.") } else { "Block change limit cleared.".into() });
    Ok(())
}

fn gmask(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, args: &[String]) -> Result<(), String> {
    if args.is_empty() {
        e.session(p).mask = None;
        msg(p, "Global mask disabled.");
    } else {
        let mask = Mask::parse(&args.join(" "))?;
        e.session(p).mask = Some(mask);
        msg(p, "Global mask set.");
    }
    Ok(())
}

fn help(_: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, args: &[String]) -> Result<(), String> {
    const PER_PAGE: usize = 10;
    let pages = COMMANDS.len().div_ceil(PER_PAGE);
    let page = (number(args.first(), 1)?.max(1) as usize).min(pages);
    msg(p, &format!("HorizonEdit commands ({page}/{pages}, //help <page>):"));
    for c in COMMANDS.iter().skip((page - 1) * PER_PAGE).take(PER_PAGE) {
        p.send_message(&format!("§e{} §7{}", c.2, c.3));
    }
    Ok(())
}

/// Name (a leading `/` makes WorldEdit's `//name`), aliases, usage,
/// description, what runs it. Names with a `/` also answer with one slash.
#[rustfmt::skip]
pub const COMMANDS: &[(&str, &[&str], &str, &str, Run)] = &[
    // Selection.
    ("/wand", &[], "//wand", "a wooden axe to select with", selection::wand),
    ("/pos1", &[], "//pos1 [x,y,z]", "the first corner (where you stand)", selection::pos1),
    ("/pos2", &[], "//pos2 [x,y,z]", "the second corner (where you stand)", selection::pos2),
    ("/hpos1", &[], "//hpos1", "the first corner where you look", selection::hpos1),
    ("/hpos2", &[], "//hpos2", "the second corner where you look", selection::hpos2),
    ("/chunk", &[], "//chunk", "select the chunk you stand in", selection::chunk),
    ("/sel", &["/desel"], "//sel", "clear the selection", selection::desel),
    ("/size", &[], "//size", "the selection's size", selection::size),
    ("/expand", &[], "//expand <n> [dir] | vert", "grow the selection", selection::expand),
    ("/contract", &[], "//contract <n> [dir]", "shrink the selection", selection::contract),
    ("/shift", &[], "//shift <n> [dir]", "move the selection itself", selection::shift),
    ("/outset", &[], "//outset <n> [-h|-v]", "grow the selection on every side", selection::outset),
    ("/inset", &[], "//inset <n> [-h|-v]", "shrink the selection on every side", selection::inset),
    ("/count", &[], "//count <mask>", "count blocks in the selection", selection::count),
    ("/distr", &[], "//distr [-c]", "the selection's (or clipboard's) blocks", selection::distr),
    // Region.
    ("/set", &[], "//set <pattern>", "fill the selection", region::set),
    ("/replace", &["/re", "/rep"], "//replace [from] <to>", "replace blocks (all but air without from)", region::replace),
    ("/overlay", &[], "//overlay <pattern>", "cover the top of the selection", region::overlay),
    ("/center", &["/middle"], "//center <pattern>", "the selection's centre", region::center),
    ("/naturalize", &[], "//naturalize", "grass, then dirt, then stone", region::naturalize),
    ("/walls", &[], "//walls <pattern>", "the selection's four walls", region::walls),
    ("/faces", &["/outline"], "//faces <pattern>", "walls, floor and ceiling", region::faces),
    ("/smooth", &[], "//smooth [iterations]", "smooth the terrain's surface", region::smooth),
    ("/hollow", &[], "//hollow [thickness] [pattern]", "empty the inside of shapes", region::hollow),
    ("/line", &[], "//line <pattern> [thickness]", "a line from pos1 to pos2", region::line),
    ("/forest", &[], "//forest [type] [density%]", "trees on grass and dirt", region::forest),
    ("/flora", &[], "//flora [density%]", "grass and flowers on grass", region::flora),
    ("/move", &[], "//move [n] [dir] [-a] [-s]", "move the selection's blocks", region::move_cmd),
    ("/stack", &[], "//stack [n] [dir] [-a] [-s]", "repeat the selection", region::stack),
    // Generation.
    ("/sphere", &[], "//sphere <pattern> <r[,r,r]> [-r]", "a sphere (or ellipsoid) where you stand", generation::sphere),
    ("/hsphere", &[], "//hsphere <pattern> <r[,r,r]> [-r]", "a hollow sphere", generation::hsphere),
    ("/cyl", &[], "//cyl <pattern> <r[,r]> [height]", "a cylinder", generation::cyl),
    ("/hcyl", &[], "//hcyl <pattern> <r[,r]> [height]", "a hollow cylinder", generation::hcyl),
    ("/pyramid", &[], "//pyramid <pattern> <size>", "a pyramid", generation::pyramid),
    ("/hpyramid", &[], "//hpyramid <pattern> <size>", "a hollow pyramid", generation::hpyramid),
    ("/generate", &["/gen", "/g"], "//generate <pattern> <expression> [-h] [-r]", "a shape where the expression holds", generation::generate),
    // Clipboard.
    ("/copy", &[], "//copy", "copy the selection, from where you stand", clipboard::copy),
    ("/cut", &[], "//cut", "copy, then clear the selection", clipboard::cut),
    ("/paste", &[], "//paste [-a] [-o] [-s]", "paste (-a skip air, -o where copied, -s select)", clipboard::paste),
    ("/rotate", &[], "//rotate <90|180|270>", "turn the clipboard", clipboard::rotate),
    ("/flip", &[], "//flip [dir]", "mirror the clipboard", clipboard::flip),
    ("/clearclipboard", &[], "//clearclipboard", "empty the clipboard", clipboard::clear),
    ("/schematic", &["/schem"], "//schem <save|load|list|delete> [name]", "keep clipboards in files", clipboard::schematic),
    // History.
    ("/undo", &[], "//undo [times]", "undo your edits", undo),
    ("/redo", &[], "//redo [times]", "redo what you undid", redo),
    ("/clearhistory", &[], "//clearhistory", "forget your edits", clearhistory),
    // Utilities.
    ("/fill", &[], "//fill <pattern> <radius> [depth]", "fill holes below you", utility::fill),
    ("/fillr", &[], "//fillr <pattern> <radius> [depth]", "fill holes, reaching under overhangs", utility::fillr),
    ("/drain", &[], "//drain [radius]", "remove water and lava around you", utility::drain),
    ("/fixwater", &[], "//fixwater [radius]", "flowing water becomes still", utility::fixwater),
    ("/fixlava", &[], "//fixlava [radius]", "flowing lava becomes still", utility::fixlava),
    ("/removeabove", &["/rma"], "//removeabove [size] [height]", "clear what is above you", utility::removeabove),
    ("/removebelow", &["/rmb"], "//removebelow [size] [depth]", "clear what is below you", utility::removebelow),
    ("/removenear", &[], "//removenear <mask> [size]", "remove blocks around you", utility::removenear),
    ("/replacenear", &[], "//replacenear <size> <from> <to>", "replace blocks around you", utility::replacenear),
    ("/snow", &[], "//snow [radius]", "snow on the ground, ice on water", utility::snow),
    ("/thaw", &[], "//thaw [radius]", "melt snow and ice", utility::thaw),
    ("/green", &[], "//green [radius]", "dirt in the open becomes grass", utility::green),
    ("/extinguish", &["/ex"], "//extinguish [radius]", "put out fire", utility::extinguish),
    ("/butcher", &[], "//butcher [radius] [-a]", "kill hostile mobs (-a: animals too)", utility::butcher),
    ("/remove", &[], "//remove <type> <radius>", "remove items, arrows, xp... (an entity type)", utility::remove),
    ("/calc", &["/eval"], "//calc <expression>", "a calculator", utility::calc),
    ("/limit", &[], "//limit [blocks]", "your edits' block limit", limit),
    ("/gmask", &[], "//gmask [mask]", "the mask every edit goes through", gmask),
    // Navigation.
    ("ascend", &["asc"], "/ascend [levels]", "up to the floor above", navigation::ascend),
    ("descend", &["desc"], "/descend [levels]", "down to the floor below", navigation::descend),
    ("ceil", &[], "/ceil [clearance]", "up to the ceiling, on glass", navigation::ceil),
    ("thru", &[], "/thru", "through the wall in front of you", navigation::thru),
    ("jumpto", &["j"], "/jumpto", "onto the block you look at", navigation::jumpto),
    ("unstuck", &[], "/unstuck", "out of the blocks you are in", navigation::unstuck),
    ("/up", &[], "//up <blocks>", "up onto a glass block", navigation::up),
    // Brushes and tools.
    ("brush", &["br"], "/brush <sphere|cylinder|set|smooth|clipboard|gravity|forest|extinguish|butcher|raise|lower> ...", "bind a brush to the held item", tools::brush),
    ("mask", &[], "/mask [mask]", "the held brush's mask", tools::mask),
    ("material", &["mat"], "/material <pattern>", "the held brush's pattern", tools::material),
    ("size", &[], "/size <radius>", "the held brush's size", tools::size),
    ("range", &[], "/range <blocks>", "how far the held brush reaches", tools::range),
    ("tool", &[], "/tool <tree|repl|info|floodfill|deltree|farwand|lrbuild|navwand|stacker|selwand> ...", "bind a tool to the held item", tools::tool),
    ("none", &[], "/none", "unbind the held item", tools::none),
    ("sp", &["superpickaxe"], "/sp [single|area <r>|recur <r>|off]", "the super pickaxe", tools::superpickaxe),
    ("/help", &[], "//help [page]", "this list", help),
];

/// Words offered as arguments are typed.
pub const COMMON_BLOCKS: &[&str] = &[
    "air", "stone", "cobblestone", "dirt", "grass_block", "short_grass", "sand", "gravel", "glass",
    "oak_planks", "spruce_planks", "oak_log", "stone_bricks", "bricks", "smooth_stone", "deepslate",
    "andesite", "diorite", "granite", "water", "lava", "white_concrete", "black_concrete",
    "red_concrete", "quartz_block", "glowstone", "sea_lantern", "obsidian", "bedrock", "snow_block",
];
const DIRS: [&str; 10] = ["me", "back", "left", "right", "up", "down", "north", "south", "east", "west"];

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|w| (*w).to_owned()).collect()
}

/// What a command's arguments complete to.
fn complete(name: &str, a: &[String]) -> Vec<String> {
    let bare = name.trim_start_matches('/');
    match (bare, a.len()) {
        ("set" | "replace" | "overlay" | "center" | "walls" | "faces" | "line" | "sphere" | "hsphere"
            | "cyl" | "hcyl" | "pyramid" | "hpyramid" | "generate" | "count" | "removenear" | "fill"
            | "fillr" | "gmask" | "mask", 1 | 2) => words(COMMON_BLOCKS),
        ("expand" | "contract" | "shift" | "stack" | "move", 2) => words(&DIRS),
        ("expand", 1) => vec!["vert".into()],
        ("rotate", 1) => words(&["90", "180", "270"]),
        ("flip", 1) => words(&DIRS),
        ("paste", _) => words(&["-a", "-o", "-s"]),
        ("schematic", 1) => words(&["save", "load", "list", "delete"]),
        ("schematic", 2) => clipboard::schematic_names(),
        ("forest", 1) => region::TREES.iter().map(|(n, _)| (*n).to_owned()).collect(),
        ("brush", 1) => words(&["sphere", "cylinder", "set", "smooth", "clipboard", "gravity", "forest", "extinguish", "butcher", "raise", "lower"]),
        ("brush", 2) if a[0] == "set" => words(&["sphere", "cylinder", "cuboid"]),
        ("brush", 2) if matches!(a[0].as_str(), "sphere" | "s" | "cylinder" | "cyl" | "c") => words(COMMON_BLOCKS),
        ("brush", 4) if a[0] == "forest" => region::TREES.iter().map(|(n, _)| (*n).to_owned()).collect(),
        ("material" | "mat", 1) => words(COMMON_BLOCKS),
        ("tool", 1) => words(&["tree", "repl", "info", "floodfill", "deltree", "farwand", "lrbuild", "navwand", "stacker", "selwand"]),
        ("tool", 2) if a[0] == "tree" => region::TREES.iter().map(|(n, _)| (*n).to_owned()).collect(),
        ("tool", 2 | 3) if matches!(a[0].as_str(), "repl" | "floodfill" | "lrbuild") => words(COMMON_BLOCKS),
        ("drain", _) => words(&["-w"]),
        ("snow", _) => words(&["-s"]),
        ("green", _) => words(&["-f"]),
        ("butcher", _) => words(&["-a", "-p", "-n", "-g", "-b", "-w", "-f", "-r"]),
        ("generate", 3) => words(&["-h", "-r", "-o", "-c"]),
        ("sp", 1) => words(&["single", "area", "recur", "off"]),
        ("remove", 1) => words(utility::REMOVABLE),
        _ => Vec::new(),
    }
}

impl Plugin for HorizonEdit {
    fn on_enable(&mut self, ctx: &mut Ctx<Self>) {
        ctx.save_default_config(CONFIG);
        clipboard::set_folder(ctx.data_folder().join("schematics"));
        for &(name, aliases, usage, description, run) in COMMANDS {
            let mut spec = CommandSpec::new(name)
                .description(description)
                .usage(usage)
                .permission(PERMISSION);
            // `//wand` and `/wand` both work (a name vanilla has, such as
            // /help or /fill, stays vanilla's with one slash, and one of
            // ours, such as the brush's /size, stays that one's).
            for a in std::iter::once(&name).chain(aliases) {
                if *a != name {
                    spec = spec.alias(a);
                }
                if let Some(bare) = a.strip_prefix('/')
                    && !COMMANDS.iter().any(|c| c.0 == bare || c.1.contains(&bare))
                {
                    spec = spec.alias(bare);
                }
            }
            ctx.command(spec, move |plugin, ctx, sender, _, args| {
                let Some(p) = sender.player() else {
                    sender.send_message("HorizonEdit's commands are for players.");
                    return true;
                };
                if let Err(e) = run(plugin, ctx, p, args) {
                    p.send_message(&format!("§c{e}"));
                }
                true
            });
            ctx.set_tab_completer(name, move |_, _, _, _, args| complete(name, args));
        }
        tools::listen(ctx);
        ctx.on(|plugin: &mut HorizonEdit, _, e: &mut PlayerQuitEvent| {
            plugin.sessions.remove(&e.player.uuid);
        });
        ctx.run_timer(1, 1, |plugin, ctx| plugin.work(ctx));
        ctx.logger().info(&format!("{} commands ready; //help lists them", COMMANDS.len()));
    }
}

horizon_plugin!(
    HorizonEdit::default(),
    PluginDescription::new("HorizonEdit", "2.0.0")
        .description("WorldEdit's commands, for Horizon")
        .author("Horizon")
        .permission(
            Permission::new(PERMISSION)
                .description("Use HorizonEdit's wand, commands, brushes and tools")
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
        assert_eq!(
            commands_for("minecraft:overworld", &changes),
            vec![
                "execute in minecraft:overworld run fill 0 64 0 4 64 0 minecraft:stone",
                "execute in minecraft:overworld run setblock 7 64 0 minecraft:stone",
                "execute in minecraft:overworld run setblock 8 64 0 minecraft:dirt",
            ]
        );
    }

    #[test]
    fn flags_split_off() {
        let args: Vec<String> = ["stone", "-ha", "5", "-1"].iter().map(|s| (*s).to_owned()).collect();
        let (rest, f) = flags(&args);
        assert_eq!(rest, vec!["stone", "5", "-1"]);
        assert_eq!(f, "ha");
    }

    #[test]
    fn every_command_has_a_distinct_name() {
        let mut names: Vec<&str> = COMMANDS.iter().flat_map(|c| std::iter::once(c.0).chain(c.1.iter().copied())).collect();
        names.sort();
        let n = names.len();
        names.dedup();
        assert_eq!(n, names.len());
    }
}
