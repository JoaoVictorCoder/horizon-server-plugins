//! Making and changing the selection, and looking at what is in it.

use crate::pattern::{Mask, kind_of};
use crate::{HorizonEdit, MAX_Y, MIN_Y, Pos, WAND, boxed, dir, flags, here, msg, number, target};
use horizon_plugin_api::prelude::*;
use std::collections::HashMap;

type Result = std::result::Result<(), String>;

pub fn set_pos(e: &mut HorizonEdit, p: &Player, at: Pos, first: bool) {
    let s = e.session(p);
    if first {
        s.pos1 = Some(at);
    } else {
        s.pos2 = Some(at);
    }
    let volume = match (s.pos1, s.pos2) {
        (Some(a), Some(b)) => format!(" ({})", (0..3).map(|i| i64::from((a[i] - b[i]).abs() + 1)).product::<i64>()),
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

/// `x,y,z`, or where the player stands.
fn position(p: &Player, args: &[String]) -> std::result::Result<Pos, String> {
    let Some(text) = args.first() else {
        return Ok(here(p));
    };
    let parts: Vec<&str> = text.split(',').collect();
    match parts.as_slice() {
        [x, y, z] => {
            let n = |s: &str| s.trim().parse::<i32>().map_err(|_| format!("Not a position: {text}"));
            Ok([n(x)?, n(y)?, n(z)?])
        }
        _ => Err(format!("Not a position: {text} (x,y,z)")),
    }
}

pub fn wand(_: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, _: &[String]) -> Result {
    p.give(WAND, 1);
    msg(p, "Left click: select pos #1; Right click: select pos #2");
    Ok(())
}

pub fn pos1(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    set_pos(e, p, position(p, a)?, true);
    Ok(())
}

pub fn pos2(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    set_pos(e, p, position(p, a)?, false);
    Ok(())
}

fn hpos(e: &mut HorizonEdit, p: &Player, first: bool) -> Result {
    let (at, _) = target(p, 300.0).ok_or("No block in sight!")?;
    set_pos(e, p, at, first);
    Ok(())
}

pub fn hpos1(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, _: &[String]) -> Result {
    hpos(e, p, true)
}

pub fn hpos2(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, _: &[String]) -> Result {
    hpos(e, p, false)
}

/// `//chunk`: the chunk the player stands in; `-s` every chunk the
/// selection touches; `-c x,z` a chunk by its chunk coordinates.
pub fn chunk(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let (rest, f) = flags(a);
    let (lo_c, hi_c) = if f.contains('s') {
        let (lo, hi) = e.selection(p)?;
        ((lo[0].div_euclid(16), lo[2].div_euclid(16)), (hi[0].div_euclid(16), hi[2].div_euclid(16)))
    } else if f.contains('c') {
        let text = rest.first().ok_or("Usage: //chunk -c <x,z>")?;
        let (x, z) = text.split_once(',').ok_or("Chunk coordinates are x,z")?;
        let n = |s: &str| s.trim().parse::<i32>().map_err(|_| format!("Not a chunk: {text}"));
        ((n(x)?, n(z)?), (n(x)?, n(z)?))
    } else {
        let [x, _, z] = here(p);
        ((x.div_euclid(16), z.div_euclid(16)), (x.div_euclid(16), z.div_euclid(16)))
    };
    e.set_selection(p, [lo_c.0 * 16, MIN_Y, lo_c.1 * 16], [hi_c.0 * 16 + 15, MAX_Y, hi_c.1 * 16 + 15]);
    if lo_c == hi_c {
        msg(p, &format!("Chunk selected: {}, {}", lo_c.0, lo_c.1));
    } else {
        msg(p, &format!("Chunks selected: ({}, {}) - ({}, {})", lo_c.0, lo_c.1, hi_c.0, hi_c.1));
    }
    Ok(())
}

pub fn desel(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, _: &[String]) -> Result {
    let s = e.session(p);
    s.pos1 = None;
    s.pos2 = None;
    msg(p, "Selection cleared.");
    Ok(())
}

pub fn size(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let (_, f) = flags(a);
    if f.contains('c') {
        let clip = e.session(p).clipboard.clone().ok_or("Your clipboard is empty.")?;
        let lo: Pos = std::array::from_fn(|i| clip.blocks.iter().map(|(d, _)| d[i]).min().unwrap_or(0));
        let hi: Pos = std::array::from_fn(|i| clip.blocks.iter().map(|(d, _)| d[i]).max().unwrap_or(0));
        let d: Vec<i64> = (0..3).map(|i| i64::from(hi[i] - lo[i] + 1)).collect();
        msg(p, &format!("Clipboard: {} x {} x {}, {} blocks", d[0], d[1], d[2], clip.blocks.len()));
        msg(p, &format!("Offset from where you copied: ({}, {}, {})", lo[0], lo[1], lo[2]));
        return Ok(());
    }
    let (lo, hi) = e.selection(p)?;
    let d: Vec<i64> = (0..3).map(|i| i64::from(hi[i] - lo[i] + 1)).collect();
    msg(p, &format!("Region: ({}, {}, {}) - ({}, {}, {})", lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]));
    msg(p, &format!("Size: {} x {} x {}, {} blocks", d[0], d[1], d[2], d[0] * d[1] * d[2]));
    Ok(())
}

/// `<amount> [reverse amount] [direction]`.
fn amounts(p: &Player, a: &[String]) -> std::result::Result<(i32, i32, Pos), String> {
    let n = number(a.first(), 1)?;
    match a.get(1) {
        Some(w) if w.parse::<i32>().is_ok() => Ok((n, number(Some(w), 0)?, dir(p, a.get(2))?)),
        other => Ok((n, 0, dir(p, other)?)),
    }
}

fn resized(e: &mut HorizonEdit, p: &Player, lo: Pos, hi: Pos, how: &str) -> Result {
    let lo2 = [lo[0].min(hi[0]), lo[1].min(hi[1]).max(MIN_Y), lo[2].min(hi[2])];
    let hi2 = [lo[0].max(hi[0]), lo[1].max(hi[1]).min(MAX_Y), lo[2].max(hi[2])];
    e.set_selection(p, lo2, hi2);
    let volume: i64 = (0..3).map(|i| i64::from(hi2[i] - lo2[i] + 1)).product();
    msg(p, &format!("Region {how}: {volume} blocks."));
    Ok(())
}

pub fn expand(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let (mut lo, mut hi) = e.selection(p)?;
    if a.first().is_some_and(|w| w == "vert") {
        lo[1] = MIN_Y;
        hi[1] = MAX_Y;
    } else {
        let (n, back, d) = amounts(p, a)?;
        for i in 0..3 {
            match d[i] {
                1 => {
                    hi[i] += n;
                    lo[i] -= back;
                }
                -1 => {
                    lo[i] -= n;
                    hi[i] += back;
                }
                _ => {}
            }
        }
    }
    resized(e, p, lo, hi, "expanded")
}

pub fn contract(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let (mut lo, mut hi) = e.selection(p)?;
    let (n, back, d) = amounts(p, a)?;
    // The face opposite the direction moves towards it (and the face in
    // the direction back by the reverse amount).
    for i in 0..3 {
        match d[i] {
            1 => {
                lo[i] = (lo[i] + n).min(hi[i]);
                hi[i] = (hi[i] - back).max(lo[i]);
            }
            -1 => {
                hi[i] = (hi[i] - n).max(lo[i]);
                lo[i] = (lo[i] + back).min(hi[i]);
            }
            _ => {}
        }
    }
    resized(e, p, lo, hi, "contracted")
}

pub fn shift(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let (mut lo, mut hi) = e.selection(p)?;
    let n = number(a.first(), 1)?;
    let d = dir(p, a.get(1))?;
    for i in 0..3 {
        lo[i] += d[i] * n;
        hi[i] += d[i] * n;
    }
    resized(e, p, lo, hi, "shifted")
}

fn set_out(e: &mut HorizonEdit, p: &Player, a: &[String], sign: i32) -> Result {
    let (rest, f) = flags(a);
    let n = number(rest.first(), 1)? * sign;
    let (mut lo, mut hi) = e.selection(p)?;
    for i in 0..3 {
        let vertical = i == 1;
        if (vertical && f.contains('h')) || (!vertical && f.contains('v')) {
            continue;
        }
        lo[i] -= n;
        hi[i] += n;
    }
    if (0..3).any(|i| lo[i] > hi[i]) {
        return Err("The selection is too small for that.".into());
    }
    resized(e, p, lo, hi, if sign > 0 { "outset" } else { "inset" })
}

pub fn outset(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    set_out(e, p, a, 1)
}

pub fn inset(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    set_out(e, p, a, -1)
}

pub fn count(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    if a.is_empty() {
        return Err("Usage: //count <mask>".into());
    }
    let mask = Mask::parse(&a.join(" "))?;
    let (lo, hi) = e.selection(p)?;
    let region = e.read(p, [lo[0], lo[1] - 1, lo[2]], [hi[0], hi[1] + 1, hi[2]])?;
    let n = boxed(lo, hi)
        .filter(|q| {
            region.get(q[0], q[1], q[2]).is_some_and(|b| {
                mask.test(b, region.get(q[0], q[1] - 1, q[2]), region.get(q[0], q[1] + 1, q[2]))
            })
        })
        .count();
    msg(p, &format!("Counted: {n}"));
    Ok(())
}

pub fn distr(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let (_, f) = flags(a);
    let mut by: HashMap<String, usize> = HashMap::new();
    let mut total = 0;
    let by_state = f.contains('d');
    let key = |b: &str| if by_state { b.to_owned() } else { kind_of(b).to_owned() };
    if f.contains('c') {
        let clip = e.session(p).clipboard.clone().ok_or("Your clipboard is empty.")?;
        for (_, b) in &clip.blocks {
            *by.entry(key(b)).or_default() += 1;
            total += 1;
        }
    } else {
        let (lo, hi) = e.selection(p)?;
        let region = e.read(p, lo, hi)?;
        for q in boxed(lo, hi) {
            if let Some(b) = region.get(q[0], q[1], q[2]) {
                *by.entry(key(b)).or_default() += 1;
                total += 1;
            }
        }
    }
    let mut list: Vec<(String, usize)> = by.into_iter().collect();
    list.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    msg(p, &format!("# total blocks: {total}"));
    for (kind, n) in list.into_iter().take(12) {
        let pct = n as f64 * 100.0 / total.max(1) as f64;
        p.send_message(&format!("§7{n:>8}  §f{pct:5.1}%  §e{}", kind.trim_start_matches("minecraft:")));
    }
    Ok(())
}
