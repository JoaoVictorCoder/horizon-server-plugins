//! Getting around: floors above and below, ceilings, through walls, to
//! where the player looks, out of blocks.

use crate::pattern::is_passable;
use crate::{HorizonEdit, MAX_Y, MIN_Y, Pos, flags, here, msg, number, target};
use horizon_plugin_api::prelude::*;
use horizon_plugin_api::types::Region;

type Result = std::result::Result<(), String>;

/// The player's column from the bottom to the top of the world.
fn column(e: &HorizonEdit, p: &Player) -> std::result::Result<(Pos, Region), String> {
    let c = here(p);
    let region = e.read(p, [c[0], MIN_Y, c[2]], [c[0], MAX_Y, c[2]])?;
    Ok((c, region))
}

fn open(region: &Region, x: i32, y: i32, z: i32) -> bool {
    region.get(x, y, z).is_none_or(is_passable)
}

/// Whether a player can stand at `y`: something solid below, room for
/// their feet and head.
fn standable(region: &Region, x: i32, y: i32, z: i32) -> bool {
    !open(region, x, y - 1, z) && open(region, x, y, z) && open(region, x, y + 1, z)
}

fn go(p: &Player, at: Pos) {
    let l = &p.location;
    let to = Location::new(&l.world, f64::from(at[0]) + 0.5, f64::from(at[1]), f64::from(at[2]) + 0.5)
        .with_rotation(l.yaw, l.pitch);
    p.teleport(&to);
}

pub fn ascend(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let levels = number(a.first(), 1)?.max(1);
    let (c, region) = column(e, p)?;
    let mut y = c[1];
    let mut done = 0;
    while done < levels {
        // A floor above: a solid block, then room on it.
        let Some(next) = (y + 1..MAX_Y).find(|&ny| !open(&region, c[0], ny - 1, c[2]) && ny - 1 > y && standable(&region, c[0], ny, c[2])) else {
            break;
        };
        y = next;
        done += 1;
    }
    if done == 0 {
        return Err("No free spot above you found.".into());
    }
    go(p, [c[0], y, c[2]]);
    msg(p, &format!("Ascended {done} level(s)."));
    Ok(())
}

pub fn descend(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let levels = number(a.first(), 1)?.max(1);
    let (c, region) = column(e, p)?;
    let mut y = c[1];
    let mut done = 0;
    while done < levels {
        // Below the floor under the player's feet.
        let floor = (MIN_Y..y).rev().find(|&ny| !open(&region, c[0], ny, c[2]));
        let Some(next) = floor.and_then(|f| (MIN_Y + 1..f).rev().find(|&ny| standable(&region, c[0], ny, c[2]))) else {
            break;
        };
        y = next;
        done += 1;
    }
    if done == 0 {
        return Err("No free spot below you found.".into());
    }
    go(p, [c[0], y, c[2]]);
    msg(p, &format!("Descended {done} level(s)."));
    Ok(())
}

/// `/ceil [clearance] [-f] [-g]`: up under the ceiling, on a glass block
/// (`-f`: none, for a player who can fly; `-g` is the default here).
pub fn ceil(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let (a, f) = flags(a);
    let clearance = number(a.first(), 0)?.max(0);
    let (c, region) = column(e, p)?;
    let roof = (c[1] + 2..=MAX_Y).find(|&y| !open(&region, c[0], y, c[2])).ok_or("No ceiling above you.")?;
    let feet = roof - 2 - clearance;
    if feet <= c[1] {
        return Err("You are under the ceiling already.".into());
    }
    if !f.contains('f') {
        e.apply(ctx, p, vec![([c[0], feet - 1, c[2]], "minecraft:glass".to_owned())])?;
    }
    go(p, [c[0], feet, c[2]]);
    msg(p, "Whoosh!");
    Ok(())
}

pub fn thru(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, _: &[String]) -> Result {
    let d = p.location.facing().offset();
    let c = here(p);
    let far = [c[0] + d[0] * 64, c[1] + d[1] * 64, c[2] + d[2] * 64];
    let lo = [c[0].min(far[0]), (c[1].min(far[1]) - 1).max(MIN_Y), c[2].min(far[2])];
    let hi = [c[0].max(far[0]), (c[1].max(far[1]) + 1).min(MAX_Y), c[2].max(far[2])];
    let region = e.read(p, lo, hi)?;
    let free = |q: Pos| open(&region, q[0], q[1], q[2]) && open(&region, q[0], q[1] + 1, q[2]);
    let mut wall = false;
    for k in 1..=64 {
        let q = [c[0] + d[0] * k, c[1] + d[1] * k, c[2] + d[2] * k];
        if !free(q) {
            wall = true;
        } else if wall {
            go(p, q);
            msg(p, "Whoosh!");
            return Ok(());
        }
    }
    Err("No free spot ahead of you found.".into())
}

pub fn jumpto(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, _: &[String]) -> Result {
    let (hit, _) = target(p, 300.0).ok_or("No block in sight (or too far)!")?;
    let region = e.read(p, [hit[0], hit[1], hit[2]], [hit[0], (hit[1] + 40).min(MAX_Y), hit[2]])?;
    let y = (hit[1] + 1..=(hit[1] + 40).min(MAX_Y))
        .find(|&y| standable(&region, hit[0], y, hit[2]))
        .ok_or("No room on top of that block.")?;
    go(p, [hit[0], y, hit[2]]);
    msg(p, "Poof!");
    Ok(())
}

pub fn unstuck(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, _: &[String]) -> Result {
    let (c, region) = column(e, p)?;
    let y = (c[1]..=MAX_Y).find(|&y| open(&region, c[0], y, c[2]) && open(&region, c[0], y + 1, c[2])).ok_or("No free spot above you.")?;
    go(p, [c[0], y, c[2]]);
    msg(p, "There you go!");
    Ok(())
}

/// `//up <distance> [-f] [-g]`: straight up onto a glass block (`-f`:
/// none, for a player who can fly), stopping under a ceiling.
pub fn up(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let (a, f) = flags(a);
    let n = number(a.first(), 1)?;
    if !(1..=256).contains(&n) {
        return Err("Go up 1 to 256 blocks.".into());
    }
    let c = here(p);
    // Not through a ceiling: stop under the first solid block above.
    let (_, column) = column(e, p)?;
    let roof = (c[1] + 2..=c[1] + n + 1).find(|&y| !open(&column, c[0], y, c[2]));
    let n = roof.map_or(n, |r| (r - 2 - c[1]).min(n));
    if n < 1 {
        return Err("There is no room above you.".into());
    }
    let floor = [c[0], c[1] + n - 1, c[2]];
    if !f.contains('f') {
        e.apply(ctx, p, vec![(floor, "minecraft:glass".to_owned())])?;
    }
    let l = &p.location;
    p.teleport(&Location::new(&l.world, l.x, f64::from(floor[1] + 1), l.z).with_rotation(l.yaw, l.pitch));
    msg(p, "Whoosh!");
    Ok(())
}
