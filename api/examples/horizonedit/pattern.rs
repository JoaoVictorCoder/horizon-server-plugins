//! Patterns (what an edit writes), masks (which blocks it touches), and
//! turning or mirroring a block state with the blocks around it.

use horizon_plugin_api::prelude::*;
use horizon_plugin_api::types::namespaced;

/// A block id as commands take it, with its namespace.
pub fn block_id(text: &str) -> String {
    namespaced(text)
}

/// The id without the state: `minecraft:oak_stairs` of `minecraft:oak_stairs[facing=east]`.
pub fn kind_of(state: &str) -> &str {
    state.split('[').next().unwrap_or(state)
}

pub fn is_air(state: &str) -> bool {
    matches!(kind_of(state), "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air")
}

pub fn is_liquid(state: &str) -> bool {
    matches!(kind_of(state), "minecraft:water" | "minecraft:lava")
}

/// Blocks players walk through: air, liquids and small plants.
pub fn is_passable(state: &str) -> bool {
    if is_air(state) || is_liquid(state) {
        return true;
    }
    let k = kind_of(state).trim_start_matches("minecraft:");
    matches!(
        k,
        "short_grass" | "tall_grass" | "fern" | "large_fern" | "dead_bush" | "snow" | "vine"
            | "torch" | "wall_torch" | "redstone_wire" | "rail" | "sugar_cane" | "seagrass"
            | "tall_seagrass" | "fire" | "soul_fire"
    ) || k.ends_with("_flower")
        || k.ends_with("_sapling")
        || k.ends_with("_carpet")
        || k.ends_with("_button")
        || k.ends_with("_pressure_plate")
        || k.ends_with("_tulip")
        || matches!(
            k,
            "dandelion" | "poppy" | "blue_orchid" | "allium" | "azure_bluet" | "oxeye_daisy"
                | "cornflower" | "lily_of_the_valley"
        )
}

/// Blocks renamed since older versions, as players still type them.
const RENAMED: &[(&str, &str)] = &[
    ("grass", "short_grass"),
    ("grass_path", "dirt_path"),
    ("snow_layer", "snow"),
    ("web", "cobweb"),
    ("planks", "oak_planks"),
    ("log", "oak_log"),
    ("wool", "white_wool"),
    ("stained_glass", "white_stained_glass"),
    ("concrete", "white_concrete"),
    ("stonebrick", "stone_bricks"),
];

/// An error for a block the server doesn't have, with what was meant.
pub fn check_block(state: &str) -> Result<(), String> {
    let kind = kind_of(state);
    if server().block_exists(kind) != Some(false) {
        return Ok(());
    }
    let bare = kind.trim_start_matches("minecraft:");
    let hint = RENAMED
        .iter()
        .find(|(old, _)| *old == bare)
        .map(|(_, new)| (*new).to_owned())
        .or_else(|| {
            [format!("{bare}_block"), format!("short_{bare}"), format!("{bare}s")]
                .into_iter()
                .find(|c| server().block_exists(c) == Some(true))
        });
    Err(match hint {
        Some(h) => format!("Unknown block: {bare}. Did you mean {h}?"),
        None => format!("Unknown block: {bare}"),
    })
}

/// A pattern: blocks with weights (`50%stone,30%dirt,gravel`).
#[derive(Clone, Debug)]
pub struct Pattern(pub Vec<(String, u32)>);

impl Pattern {
    pub fn parse(text: &str) -> Result<Pattern, String> {
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
        for (b, _) in &out {
            check_block(b)?;
        }
        Ok(Pattern(out))
    }

    pub fn single(block: &str) -> Pattern {
        Pattern(vec![(block_id(block), 1)])
    }

    pub fn pick(&self, rng: &mut u64) -> &str {
        if self.0.len() == 1 {
            return &self.0[0].0;
        }
        let total: u32 = self.0.iter().map(|(_, w)| w).sum();
        let mut roll = (next(rng) % u64::from(total)) as u32;
        for (b, w) in &self.0 {
            if roll < *w {
                return b;
            }
            roll -= w;
        }
        &self.0[0].0
    }
}

/// xorshift64.
pub fn next(rng: &mut u64) -> u64 {
    *rng ^= *rng << 13;
    *rng ^= *rng >> 7;
    *rng ^= *rng << 17;
    *rng
}

/// A chance in a hundred.
pub fn chance(rng: &mut u64, percent: f64) -> bool {
    (next(rng) % 10_000) as f64 / 100.0 < percent
}

/// One test of a mask.
#[derive(Clone, Debug, PartialEq)]
enum Test {
    /// Any of these blocks (with or without their state).
    Blocks(Vec<String>),
    /// Anything but air.
    Existing,
    /// Not air, a liquid or a small plant.
    Solid,
    /// The block below matches.
    Above(Box<Test>),
    /// The block above matches.
    Below(Box<Test>),
    Not(Box<Test>),
}

/// A mask: blocks an edit may change (`stone,dirt`, `!air`, `#existing`,
/// `#solid`, `>grass_block` over grass, `<stone` under stone). Tests
/// separated by spaces must all pass.
#[derive(Clone, Debug, PartialEq)]
pub struct Mask(Vec<Test>);

fn matches_any(list: &[String], state: &str) -> bool {
    list.iter().any(|m| if m.contains('[') { m == state } else { m == kind_of(state) })
}

impl Mask {
    pub fn parse(text: &str) -> Result<Mask, String> {
        fn one(word: &str) -> Result<Test, String> {
            Ok(if let Some(rest) = word.strip_prefix('!') {
                Test::Not(Box::new(one(rest)?))
            } else if let Some(rest) = word.strip_prefix('>') {
                Test::Above(Box::new(one(rest)?))
            } else if let Some(rest) = word.strip_prefix('<') {
                Test::Below(Box::new(one(rest)?))
            } else if word == "#existing" {
                Test::Existing
            } else if word == "#solid" {
                Test::Solid
            } else if word.starts_with('#') {
                return Err(format!("Unknown mask: {word} (#existing, #solid)"));
            } else {
                let list: Vec<String> = word.split(',').filter(|b| !b.is_empty()).map(block_id).collect();
                if list.is_empty() {
                    return Err("An empty mask.".into());
                }
                for b in &list {
                    check_block(b)?;
                }
                Test::Blocks(list)
            })
        }
        let tests = text.split_whitespace().map(one).collect::<Result<Vec<_>, _>>()?;
        if tests.is_empty() {
            return Err("An empty mask.".into());
        }
        Ok(Mask(tests))
    }

    /// Whether the mask looks at the blocks above or below.
    pub fn needs_neighbours(&self) -> bool {
        fn n(t: &Test) -> bool {
            match t {
                Test::Above(_) | Test::Below(_) => true,
                Test::Not(i) => n(i),
                _ => false,
            }
        }
        self.0.iter().any(n)
    }

    /// Whether `state` passes, with the states below and above it.
    pub fn test(&self, state: &str, below: Option<&str>, above: Option<&str>) -> bool {
        fn t(test: &Test, s: &str, below: Option<&str>, above: Option<&str>) -> bool {
            match test {
                Test::Blocks(list) => matches_any(list, s),
                Test::Existing => !is_air(s),
                Test::Solid => !is_passable(s),
                Test::Above(i) => below.is_some_and(|b| t(i, b, None, None)),
                Test::Below(i) => above.is_some_and(|a| t(i, a, None, None)),
                Test::Not(i) => !t(i, s, below, above),
            }
        }
        self.0.iter().all(|x| t(x, state, below, above))
    }
}

fn props(state: &str) -> (&str, Vec<(String, String)>) {
    match state.split_once('[') {
        None => (state, Vec::new()),
        Some((kind, rest)) => (
            kind,
            rest.trim_end_matches(']')
                .split(',')
                .filter_map(|p| p.split_once('='))
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect(),
        ),
    }
}

fn join(kind: &str, mut props: Vec<(String, String)>) -> String {
    if props.is_empty() {
        return kind.to_owned();
    }
    props.sort();
    let p: Vec<String> = props.into_iter().map(|(k, v)| format!("{k}={v}")).collect();
    format!("{kind}[{}]", p.join(","))
}

const SIDES: [&str; 4] = ["north", "east", "south", "west"];

/// A state turned `quarters` times clockwise (seen from above): `facing`,
/// `axis`, `rotation` and the sides of fences and walls turn with it.
pub fn rotate_state(state: &str, quarters: u8) -> String {
    let (kind, list) = props(state);
    if list.is_empty() {
        return state.to_owned();
    }
    let turn = |side: &str| -> String {
        match SIDES.iter().position(|s| *s == side) {
            Some(i) => SIDES[(i + quarters as usize) % 4].to_owned(),
            None => side.to_owned(),
        }
    };
    let out = list
        .into_iter()
        .map(|(k, v)| match k.as_str() {
            "facing" | "horizontal_facing" => (k, turn(&v)),
            "axis" if quarters % 2 == 1 => (
                k,
                match v.as_str() {
                    "x" => "z".to_owned(),
                    "z" => "x".to_owned(),
                    _ => v,
                },
            ),
            "rotation" => {
                let r = v.parse::<u32>().map_or(v.clone(), |r| ((r + 4 * u32::from(quarters)) % 16).to_string());
                (k, r)
            }
            "north" | "east" | "south" | "west" => (turn(&k), v),
            _ => (k, v),
        })
        .collect();
    join(kind, out)
}

/// A state turned `quarters` times around the x axis (`axis` 0) or the
/// z axis (2), as the blocks turn when a clipboard tips over: `facing` and
/// `axis` follow; a state with neither stays as it is.
pub fn tip_state(state: &str, axis: usize, quarters: u8) -> String {
    let (kind, list) = props(state);
    if list.is_empty() || quarters % 4 == 0 {
        return state.to_owned();
    }
    // The four faces a turn around the axis cycles through.
    let ring: [&str; 4] = if axis == 0 {
        ["up", "north", "down", "south"]
    } else {
        ["up", "east", "down", "west"]
    };
    let turn = |face: &str| -> String {
        match ring.iter().position(|f| *f == face) {
            Some(i) => ring[(i + quarters as usize) % 4].to_owned(),
            None => face.to_owned(),
        }
    };
    let swap = if axis == 0 { ("y", "z") } else { ("x", "y") };
    let out = list
        .into_iter()
        .map(|(k, v)| match k.as_str() {
            "facing" => (k, turn(&v)),
            "axis" if quarters % 2 == 1 => {
                let v = if v == swap.0 {
                    swap.1.to_owned()
                } else if v == swap.1 {
                    swap.0.to_owned()
                } else {
                    v
                };
                (k, v)
            }
            _ => (k, v),
        })
        .collect();
    join(kind, out)
}

/// A state mirrored across `axis` (0 x, 1 y, 2 z).
pub fn flip_state(state: &str, axis: usize) -> String {
    let (kind, list) = props(state);
    if list.is_empty() {
        return state.to_owned();
    }
    let mirror = |side: &str| -> String {
        match (axis, side) {
            (0, "east") => "west".into(),
            (0, "west") => "east".into(),
            (2, "north") => "south".into(),
            (2, "south") => "north".into(),
            (1, "up") => "down".into(),
            (1, "down") => "up".into(),
            _ => side.to_owned(),
        }
    };
    let out = list
        .into_iter()
        .map(|(k, v)| match k.as_str() {
            "facing" => (k, mirror(&v)),
            "half" if axis == 1 => (
                k,
                match v.as_str() {
                    "top" => "bottom".into(),
                    "bottom" => "top".into(),
                    "upper" => "upper".into(),
                    _ => v,
                },
            ),
            "type" if axis == 1 && (v == "top" || v == "bottom") => {
                (k, if v == "top" { "bottom".into() } else { "top".into() })
            }
            "shape" if axis != 1 => (
                k,
                match v.as_str() {
                    "inner_left" => "inner_right".into(),
                    "inner_right" => "inner_left".into(),
                    "outer_left" => "outer_right".into(),
                    "outer_right" => "outer_left".into(),
                    _ => v,
                },
            ),
            "hinge" if axis != 1 => (k, if v == "left" { "right".into() } else { "left".into() }),
            "north" | "east" | "south" | "west" | "up" | "down" => (mirror(&k), v),
            _ => (k, v),
        })
        .collect();
    join(kind, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_and_flips() {
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
        assert_eq!(
            flip_state("minecraft:oak_stairs[facing=east,half=bottom,shape=inner_left]", 0),
            "minecraft:oak_stairs[facing=west,half=bottom,shape=inner_right]"
        );
        assert_eq!(
            flip_state("minecraft:oak_stairs[facing=east,half=bottom,shape=straight]", 1),
            "minecraft:oak_stairs[facing=east,half=top,shape=straight]"
        );
        assert_eq!(flip_state("minecraft:oak_slab[type=top]", 1), "minecraft:oak_slab[type=bottom]");
        assert_eq!(tip_state("minecraft:oak_log[axis=y]", 0, 1), "minecraft:oak_log[axis=z]");
        assert_eq!(tip_state("minecraft:piston[extended=false,facing=up]", 2, 1), "minecraft:piston[extended=false,facing=east]");
        assert_eq!(tip_state("minecraft:stone", 0, 1), "minecraft:stone");
    }

    #[test]
    fn masks() {
        let m = Mask::parse("!air").unwrap();
        assert!(m.test("minecraft:stone", None, None));
        assert!(!m.test("minecraft:air", None, None));
        let over_grass = Mask::parse("#existing >grass_block").unwrap();
        assert!(over_grass.needs_neighbours());
        assert!(over_grass.test("minecraft:stone", Some("minecraft:grass_block"), None));
        assert!(!over_grass.test("minecraft:stone", Some("minecraft:dirt"), None));
        assert!(Mask::parse("stone,dirt").unwrap().test("minecraft:dirt", None, None));
        assert!(Mask::parse("#solid").unwrap().test("minecraft:stone", None, None));
        assert!(!Mask::parse("#solid").unwrap().test("minecraft:short_grass", None, None));
        assert!(Mask::parse("#nope").is_err());
    }

    #[test]
    fn patterns() {
        let p = Pattern::parse("50%stone,50%dirt").unwrap();
        let mut rng = 12345;
        let picks: Vec<&str> = (0..200).map(|_| p.pick(&mut rng)).collect();
        assert!(picks.contains(&"minecraft:stone") && picks.contains(&"minecraft:dirt"));
        assert!(Pattern::parse("").is_err());
        assert!(Pattern::parse("x%stone").is_err());
    }
}
