//! Shapes: spheres and ellipsoids, cylinders, pyramids, and any shape an
//! expression describes.

use crate::expr;
use crate::pattern::Pattern;
use crate::{HorizonEdit, Pos, boxed, flags, here, msg, number};
use horizon_plugin_api::prelude::*;
use std::collections::HashMap;

type Result = std::result::Result<(), String>;

/// The blocks of an ellipsoid around `c`; only its shell when `hollow`.
pub fn ellipsoid(c: Pos, r: [f64; 3], hollow: bool) -> Vec<Pos> {
    let reach: Pos = std::array::from_fn(|i| r[i].ceil() as i32);
    let inside = |q: [f64; 3], shrink: f64| {
        (0..3).map(|i| (q[i] / (r[i] + 0.5 - shrink).max(0.01)).powi(2)).sum::<f64>() <= 1.0
    };
    boxed([c[0] - reach[0], c[1] - reach[1], c[2] - reach[2]], [c[0] + reach[0], c[1] + reach[1], c[2] + reach[2]])
        .filter(|q| {
            let d = [f64::from(q[0] - c[0]), f64::from(q[1] - c[1]), f64::from(q[2] - c[2])];
            inside(d, 0.0) && (!hollow || !inside(d, 1.0))
        })
        .collect()
}

/// The blocks of an elliptic cylinder standing on `c`, `height` tall.
pub fn cylinder(c: Pos, rx: f64, rz: f64, height: i32, hollow: bool) -> Vec<Pos> {
    let (ax, az) = (rx.ceil() as i32, rz.ceil() as i32);
    let inside = |dx: f64, dz: f64, shrink: f64| {
        (dx / (rx + 0.5 - shrink).max(0.01)).powi(2) + (dz / (rz + 0.5 - shrink).max(0.01)).powi(2) <= 1.0
    };
    boxed([c[0] - ax, c[1], c[2] - az], [c[0] + ax, c[1] + height - 1, c[2] + az])
        .filter(|q| {
            let (dx, dz) = (f64::from(q[0] - c[0]), f64::from(q[2] - c[2]));
            inside(dx, dz, 0.0) && (!hollow || !inside(dx, dz, 1.0))
        })
        .collect()
}

/// `r` or `rx,ry,rz` (`rx,rz` for cylinders).
fn radii(text: Option<&String>, n: usize) -> std::result::Result<Vec<f64>, String> {
    let text = text.ok_or("A radius is needed.")?;
    let parts: Vec<f64> = text
        .split(',')
        .map(|s| s.trim().parse::<f64>().map_err(|_| format!("Not a radius: {text}")))
        .collect::<std::result::Result<_, _>>()?;
    let out = match parts.len() {
        1 => vec![parts[0]; n],
        k if k == n => parts,
        _ => return Err(format!("Give one radius or {n}: {text}")),
    };
    if out.iter().any(|r| !(0.0..=128.0).contains(r)) {
        return Err("A radius is 0 to 128.".into());
    }
    Ok(out)
}

fn sphere_cmd(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String], hollow: bool) -> Result {
    let (rest, f) = flags(a);
    let pattern = Pattern::parse(rest.first().ok_or("Usage: //sphere <pattern> <r[,r,r]> [-r]")?)?;
    let r = radii(rest.get(1), 3)?;
    let mut c = here(p);
    if f.contains('r') {
        // Raised: it sits on the ground instead of around the player.
        c[1] += r[1].ceil() as i32;
    }
    let changes = e.fill_with(ellipsoid(c, [r[0], r[1], r[2]], hollow), &pattern);
    let n = e.apply(ctx, p, changes)?;
    msg(p, &format!("{n} blocks have been created."));
    Ok(())
}

pub fn sphere(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    sphere_cmd(e, ctx, p, a, false)
}

pub fn hsphere(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    sphere_cmd(e, ctx, p, a, true)
}

fn cyl_cmd(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String], hollow: bool) -> Result {
    let pattern = Pattern::parse(a.first().ok_or("Usage: //cyl <pattern> <r[,r]> [height]")?)?;
    let r = radii(a.get(1), 2)?;
    let h = number(a.get(2), 1)?;
    if !(1..=384).contains(&h) {
        return Err("The height is 1 to 384.".into());
    }
    let changes = e.fill_with(cylinder(here(p), r[0], r[1], h, hollow), &pattern);
    let n = e.apply(ctx, p, changes)?;
    msg(p, &format!("{n} blocks have been created."));
    Ok(())
}

pub fn cyl(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    cyl_cmd(e, ctx, p, a, false)
}

pub fn hcyl(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    cyl_cmd(e, ctx, p, a, true)
}

/// A square pyramid on `c`, `size` levels tall.
pub fn pyramid_cells(c: Pos, size: i32, hollow: bool) -> Vec<Pos> {
    let mut out = Vec::new();
    for level in 0..size {
        let r = size - 1 - level;
        for dz in -r..=r {
            for dx in -r..=r {
                let edge = dx.abs() == r || dz.abs() == r;
                // A hollow one keeps its sides (and its floor).
                if !hollow || edge || level == 0 {
                    out.push([c[0] + dx, c[1] + level, c[2] + dz]);
                }
            }
        }
    }
    out
}

fn pyramid_cmd(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String], hollow: bool) -> Result {
    let pattern = Pattern::parse(a.first().ok_or("Usage: //pyramid <pattern> <size>")?)?;
    let size = number(a.get(1), 0)?;
    if !(1..=128).contains(&size) {
        return Err("The size is 1 to 128.".into());
    }
    let changes = e.fill_with(pyramid_cells(here(p), size, hollow), &pattern);
    let n = e.apply(ctx, p, changes)?;
    msg(p, &format!("{n} blocks have been created."));
    Ok(())
}

pub fn pyramid(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    pyramid_cmd(e, ctx, p, a, false)
}

pub fn hpyramid(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    pyramid_cmd(e, ctx, p, a, true)
}

/// `//generate <pattern> <expression>`: the blocks of the selection where
/// the expression is true, with `x`, `y`, `z` from -1 to 1 across the
/// selection (`-r`: the world's coordinates). `-h` keeps only the shell.
pub fn generate(e: &mut HorizonEdit, ctx: &mut Ctx<HorizonEdit>, p: &Player, a: &[String]) -> Result {
    let (rest, f) = flags(a);
    let pattern = Pattern::parse(rest.first().ok_or("Usage: //generate <pattern> <expression> [-h] [-r]")?)?;
    if rest.len() < 2 {
        return Err("An expression is needed, e.g. x^2+y^2+z^2<1".into());
    }
    let expr = expr::parse(&rest[1..].join(" "))?;
    let (lo, hi) = e.selection(p)?;
    let raw = f.contains('r');
    let coords = |q: Pos| -> [f64; 3] {
        if raw {
            return [f64::from(q[0]), f64::from(q[1]), f64::from(q[2])];
        }
        std::array::from_fn(|i| {
            let (a, b) = (f64::from(lo[i]), f64::from(hi[i]));
            if b > a { (f64::from(q[i]) - a) / (b - a) * 2.0 - 1.0 } else { 0.0 }
        })
    };
    let mut vars: HashMap<&str, f64> = HashMap::new();
    let mut inside = |q: Pos| -> std::result::Result<bool, String> {
        let [x, y, z] = coords(q);
        vars.insert("x", x);
        vars.insert("y", y);
        vars.insert("z", z);
        Ok(expr.eval(&vars)? > 0.0)
    };
    let mut cells = Vec::new();
    for q in boxed(lo, hi) {
        if !inside(q)? {
            continue;
        }
        if f.contains('h') {
            let shell = [[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]]
                .iter()
                .map(|o| [q[0] + o[0], q[1] + o[1], q[2] + o[2]])
                .any(|n| !(0..3).all(|i| (lo[i]..=hi[i]).contains(&n[i])) || !inside(n).unwrap_or(false));
            if !shell {
                continue;
            }
        }
        cells.push(q);
    }
    let changes = e.fill_with(cells, &pattern);
    let n = e.apply(ctx, p, changes)?;
    msg(p, &format!("{n} blocks have been created."));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_have_the_right_sizes() {
        // Radius 3: about 4/3·π·3.5³ blocks.
        assert_eq!(ellipsoid([0, 0, 0], [3.0; 3], false).len(), 179);
        let hollow = ellipsoid([0, 0, 0], [3.0; 3], true);
        assert!(hollow.len() < 179 && !hollow.contains(&[0, 0, 0]));
        let c = cylinder([0, 0, 0], 2.0, 2.0, 3, false);
        assert_eq!(c.len() % 3, 0);
        assert!(c.contains(&[2, 2, 0]) && !c.contains(&[0, 3, 0]));
        assert_eq!(pyramid_cells([0, 0, 0], 2, false).len(), 9 + 1);
        assert_eq!(pyramid_cells([0, 0, 0], 3, true).len(), 25 + 8 + 1);
    }
}
