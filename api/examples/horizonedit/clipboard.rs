//! The clipboard: copy, cut, paste, turn, mirror, and keep it in files
//! (`plugins/HorizonEdit/schematics/<name>.hschem`, a text format of this
//! plugin's own).

use crate::pattern::{Pattern, flip_state, is_air, rotate_state, tip_state};
use crate::{Clipboard, HorizonEdit, Placed, Pos, add, boxed, dir, flags, here, in_world, msg, number};
use horizon_plugin_api::prelude::*;
use std::path::PathBuf;
use std::sync::Mutex;

type Result = std::result::Result<(), String>;

static FOLDER: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn set_folder(path: PathBuf) {
    if let Ok(mut f) = FOLDER.lock() {
        *f = Some(path);
    }
}

fn folder() -> std::result::Result<PathBuf, String> {
    FOLDER
        .lock()
        .ok()
        .and_then(|f| f.clone())
        .ok_or_else(|| "No schematics folder.".to_owned())
}

fn copy_region(e: &mut HorizonEdit, p: &Player) -> std::result::Result<(Pos, Pos, usize), String> {
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
    e.session(p).clipboard = Some(Clipboard { blocks, origin });
    Ok((lo, hi, n))
}

pub fn copy(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, _: &[String]) -> Result {
    let (_, _, n) = copy_region(e, p)?;
    msg(p, &format!("{n} blocks copied."));
    Ok(())
}

/// `//cut [leave]`: what is left behind is `leave` (air without one).
pub fn cut(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let leave = match a.first() {
        Some(t) => Pattern::parse(t)?,
        None => Pattern::single("air"),
    };
    let (lo, hi, n) = copy_region(e, p)?;
    let changes = e.fill_with(boxed(lo, hi), &leave);
    e.apply(ctx, p, changes)?;
    msg(p, &format!("{n} blocks cut."));
    Ok(())
}

pub fn clipboard(e: &mut HorizonEdit, p: &Player) -> std::result::Result<Clipboard, String> {
    e.session(p)
        .clipboard
        .clone()
        .ok_or_else(|| "Your clipboard is empty. Use //copy first.".to_owned())
}

/// The clipboard's blocks placed with their origin at `at`.
pub fn placed(clip: &Clipboard, at: Pos, skip_air: bool) -> Vec<Placed> {
    clip.blocks
        .iter()
        .filter(|(_, b)| !(skip_air && is_air(b)))
        .map(|(d, b)| (add(at, *d), b.clone()))
        .filter(|(q, _)| in_world(q))
        .collect()
}

pub fn paste(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let (_, f) = flags(a);
    let clip = clipboard(e, p)?;
    let at = if f.contains('o') { clip.origin } else { here(p) };
    let changes = placed(&clip, at, f.contains('a'));
    if f.contains('s') && !changes.is_empty() {
        let lo: Pos = std::array::from_fn(|i| changes.iter().map(|(q, _)| q[i]).min().unwrap_or(0));
        let hi: Pos = std::array::from_fn(|i| changes.iter().map(|(q, _)| q[i]).max().unwrap_or(0));
        e.set_selection(p, lo, hi);
    }
    let n = e.apply(ctx, p, changes)?;
    msg(p, &format!("The clipboard has been pasted at ({}, {}, {}): {n} blocks.", at[0], at[1], at[2]));
    Ok(())
}

/// Quarter turns of `deg` degrees.
fn quarters(deg: i32) -> std::result::Result<u8, String> {
    if deg % 90 != 0 {
        return Err("Rotate by multiples of 90 degrees.".into());
    }
    Ok((deg.rem_euclid(360) / 90) as u8)
}

/// `//rotate <y> [x] [z]`: degrees around the vertical axis, then the
/// east-west one, then the north-south one.
pub fn rotate(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let (y, x, z) = (
        quarters(number(a.first(), 90)?)?,
        quarters(number(a.get(1), 0)?)?,
        quarters(number(a.get(2), 0)?)?,
    );
    let s = e.session(p);
    let clip = s.clipboard.as_mut().ok_or("Your clipboard is empty. Use //copy first.")?;
    for (d, b) in clip.blocks.iter_mut() {
        for _ in 0..y {
            *d = [-d[2], d[1], d[0]];
        }
        *b = rotate_state(b, y);
        for _ in 0..x {
            // Around x: up turns north.
            *d = [d[0], d[2], -d[1]];
        }
        *b = tip_state(b, 0, x);
        for _ in 0..z {
            // Around z: up turns east (and east down).
            *d = [d[1], -d[0], d[2]];
        }
        *b = tip_state(b, 2, z);
    }
    msg(p, &format!("The clipboard copy has been rotated ({}, {}, {}).", u16::from(y) * 90, u16::from(x) * 90, u16::from(z) * 90));
    Ok(())
}

pub fn flip(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let d = dir(p, a.first())?;
    let axis = (0..3).find(|&i| d[i] != 0).unwrap_or(0);
    let s = e.session(p);
    let clip = s.clipboard.as_mut().ok_or("Your clipboard is empty. Use //copy first.")?;
    for (off, b) in clip.blocks.iter_mut() {
        off[axis] = -off[axis];
        *b = flip_state(b, axis);
    }
    msg(p, &format!("The clipboard copy has been flipped along {}.", ["x", "y", "z"][axis]));
    Ok(())
}

pub fn clear(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, _: &[String]) -> Result {
    e.session(p).clipboard = None;
    msg(p, "Clipboard cleared.");
    Ok(())
}

/// The schematics kept, by name.
pub fn schematic_names() -> Vec<String> {
    let Ok(dir) = folder() else {
        return Vec::new();
    };
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|d| {
            d.filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().to_string_lossy().strip_suffix(".hschem").map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

fn file(name: Option<&String>) -> std::result::Result<PathBuf, String> {
    let name = name.ok_or("A name is needed.")?;
    if name.is_empty() || name.len() > 64 || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err("A schematic's name is letters, digits, _ and -.".into());
    }
    Ok(folder()?.join(format!("{name}.hschem")))
}

/// The clipboard as a file: `hschem 1`, the origin, the palette, then one
/// `dx dy dz index` line per block.
fn write(clip: &Clipboard) -> String {
    let mut palette: Vec<&str> = Vec::new();
    let mut lines = Vec::with_capacity(clip.blocks.len());
    for (d, b) in &clip.blocks {
        let i = palette.iter().position(|x| *x == b).unwrap_or_else(|| {
            palette.push(b);
            palette.len() - 1
        });
        lines.push(format!("{} {} {} {i}", d[0], d[1], d[2]));
    }
    let mut out = format!("hschem 1\norigin {} {} {}\npalette {}\n", clip.origin[0], clip.origin[1], clip.origin[2], palette.len());
    for b in palette {
        out.push_str(b);
        out.push('\n');
    }
    out.push_str(&lines.join("\n"));
    out.push('\n');
    out
}

fn read(text: &str) -> std::result::Result<Clipboard, String> {
    let mut lines = text.lines();
    if lines.next() != Some("hschem 1") {
        return Err("Not a HorizonEdit schematic.".into());
    }
    let bad = || "A damaged schematic.".to_owned();
    let nums = |l: Option<&str>, tag: &str| -> std::result::Result<Vec<i32>, String> {
        l.and_then(|l| l.strip_prefix(tag))
            .ok_or_else(bad)?
            .split_whitespace()
            .map(|n| n.parse::<i32>().map_err(|_| bad()))
            .collect()
    };
    let o = nums(lines.next(), "origin ")?;
    let count = *nums(lines.next(), "palette ")?.first().ok_or_else(bad)? as usize;
    let palette: Vec<String> = lines.by_ref().take(count).map(str::to_owned).collect();
    if palette.len() != count || o.len() != 3 {
        return Err(bad());
    }
    let mut blocks = Vec::new();
    for l in lines {
        let v: Vec<i32> = l.split_whitespace().map(|n| n.parse().map_err(|_| bad())).collect::<std::result::Result<_, _>>()?;
        let [x, y, z, i] = v[..] else {
            return Err(bad());
        };
        blocks.push(([x, y, z], palette.get(i as usize).ok_or_else(bad)?.clone()));
    }
    Ok(Clipboard {
        blocks,
        origin: [o[0], o[1], o[2]],
    })
}

pub fn schematic(e: &mut HorizonEdit, _: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    match a.first().map(String::as_str) {
        Some("save") => {
            let path = file(a.get(1))?;
            let clip = clipboard(e, p)?;
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            std::fs::write(&path, write(&clip)).map_err(|e| format!("Could not save: {e}"))?;
            msg(p, &format!("Schematic {} saved ({} blocks).", a[1], clip.blocks.len()));
        }
        Some("load") => {
            let path = file(a.get(1))?;
            let text = std::fs::read_to_string(&path).map_err(|_| format!("No schematic named {}.", a[1]))?;
            let clip = read(&text)?;
            let n = clip.blocks.len();
            e.session(p).clipboard = Some(clip);
            msg(p, &format!("Schematic {} loaded into your clipboard ({n} blocks). //paste it.", a[1]));
        }
        Some("delete") => {
            let path = file(a.get(1))?;
            std::fs::remove_file(&path).map_err(|_| format!("No schematic named {}.", a[1]))?;
            msg(p, &format!("Schematic {} deleted.", a[1]));
        }
        Some("list") => {
            let names = schematic_names();
            if names.is_empty() {
                msg(p, "No schematics yet: //schem save <name>");
            } else {
                msg(p, &format!("Schematics ({}): {}", names.len(), names.join(", ")));
            }
        }
        _ => return Err("Usage: //schem <save|load|list|delete> [name]".into()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schematics_round_trip() {
        let clip = Clipboard {
            blocks: vec![
                ([0, 0, 0], "minecraft:stone".into()),
                ([1, -2, 3], "minecraft:oak_stairs[facing=east,half=bottom]".into()),
                ([2, 0, 0], "minecraft:stone".into()),
            ],
            origin: [10, 64, -5],
        };
        let back = read(&write(&clip)).unwrap();
        assert_eq!(back.blocks, clip.blocks);
        assert_eq!(back.origin, clip.origin);
        assert!(read("nope").is_err());
        assert!(read("hschem 1\norigin 0 0 0\npalette 1\nminecraft:stone\n0 0 0 5\n").is_err());
    }
}
