//! Furniture: a model drawn by an `item_display` entity, with hitboxes of
//! its own (docs/forja.md, "Hitboxes, for furniture"):
//!
//! - `shulker`: an invisible shulker with no AI, solid to players. Its
//!   `scale` sets its size (a cube), its `peek` (0 to 100) stretches it up to
//!   twice that upward: the open shell is part of its box.
//! - `barrier`: a barrier block, solid, on the block grid.
//! - `interaction`: an interaction entity of any width and height, which
//!   takes clicks and hits but isn't solid.
//!
//! A piece placed in the world is an [`Instance`]: its entities carry the tag
//! `forja_<uid>`, and Forja finds the piece a click hit by the entity's kind
//! and place.

use crate::content::{Look, resource_id, snbt};
use horizon_plugin_api::{Value, color};

/// A hitbox of a piece of furniture, placed relative to its anchor (the
/// bottom centre of the block it stands in) before it is turned.
#[derive(Clone, Debug, PartialEq)]
pub enum Hitbox {
    Shulker {
        offset: [f64; 3],
        scale: f64,
        peek: u8,
    },
    Barrier {
        offset: [i32; 3],
    },
    Interaction {
        offset: [f64; 3],
        width: f64,
        height: f64,
    },
}

/// How a placed piece may be turned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rotation {
    /// Facing the player, in steps of 90°.
    Four,
    /// In steps of 45°.
    Eight,
    /// Exactly where the player looks.
    Free,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FurnitureDef {
    pub id: String,
    pub pack: String,
    pub owner: Option<String>,
    /// The model the display entity draws: the furniture's item model.
    pub look: Look,
    /// The display's scale on each axis.
    pub scale: [f64; 3],
    /// Where the display sits from the anchor (default: the block's centre).
    pub offset: [f64; 3],
    pub rotation: Rotation,
    pub hitboxes: Vec<Hitbox>,
    pub drops_self: bool,
}

fn floats<const N: usize>(v: Option<&Value>, default: [f64; N]) -> Result<[f64; N], String> {
    let Some(v) = v else {
        return Ok(default);
    };
    if let Some(x) = v.as_f64() {
        return Ok([x; N]);
    }
    let list = v.as_list().ok_or("a number or a list of numbers")?;
    if list.len() != N {
        return Err(format!("{N} numbers"));
    }
    let mut out = default;
    for (o, x) in out.iter_mut().zip(list) {
        *o = x.as_f64().ok_or("a number")?;
    }
    Ok(out)
}

fn hitbox(v: &Value) -> Result<Hitbox, String> {
    let kind = v
        .get("type")
        .and_then(Value::to_text)
        .ok_or("a hitbox needs a `type`: shulker, barrier or interaction")?;
    let num = |k: &str, d: f64| v.get(k).and_then(Value::as_f64).unwrap_or(d);
    Ok(match kind.as_str() {
        "shulker" => {
            let scale = num("scale", 1.0);
            if !(0.0625..=16.0).contains(&scale) {
                return Err(format!("shulker scale {scale}: 0.0625 to 16"));
            }
            Hitbox::Shulker {
                offset: floats(v.get("offset"), [0.0; 3])?,
                scale,
                peek: num("peek", 0.0).clamp(0.0, 100.0) as u8,
            }
        }
        "barrier" => {
            let o = floats(v.get("offset"), [0.0; 3])?;
            Hitbox::Barrier {
                offset: o.map(|x| x.round() as i32),
            }
        }
        "interaction" => Hitbox::Interaction {
            offset: floats(v.get("offset"), [0.0; 3])?,
            width: num("width", 1.0).clamp(0.01, 64.0),
            height: num("height", 1.0).clamp(0.01, 64.0),
        },
        other => {
            return Err(format!(
                "`{other}` is not a hitbox (shulker, barrier, interaction)"
            ));
        }
    })
}

/// A `furniture:` entry: the definition and the item that places it.
pub fn furniture_def(
    key: &str,
    v: &Value,
    pack: &str,
    namespace: &str,
) -> Result<(FurnitureDef, crate::content::ItemDef), String> {
    let id = resource_id(key, namespace)?;
    let ns = id.split(':').next().unwrap_or(namespace).to_owned();
    let text = |k: &str| v.get(k).and_then(Value::to_text);
    let look = match (text("texture"), text("model")) {
        (Some(_), Some(_)) => return Err("give it a `texture` or a `model`, not both".into()),
        (Some(t), None) => Look::Texture {
            texture: resource_id(&t, &ns)?,
            parent: "minecraft:item/generated".into(),
        },
        (None, Some(m)) => Look::Model(resource_id(&m, &ns)?),
        (None, None) => return Err("give it a `model` (or a `texture`)".into()),
    };
    let rotation = match text("rotation").as_deref() {
        None | Some("4") => Rotation::Four,
        Some("8") => Rotation::Eight,
        Some("free") => Rotation::Free,
        Some(r) => return Err(format!("rotation `{r}`: 4, 8 or free")),
    };
    let mut hitboxes = Vec::new();
    for h in v.get("hitbox").and_then(Value::as_list).unwrap_or_default() {
        hitboxes.push(hitbox(h)?);
    }
    // Something to click and hit: an interaction box the size of a block.
    if !hitboxes
        .iter()
        .any(|h| matches!(h, Hitbox::Interaction { .. }))
    {
        hitboxes.push(Hitbox::Interaction {
            offset: [0.0; 3],
            width: 1.0,
            height: 1.0,
        });
    }
    let def = FurnitureDef {
        id: id.clone(),
        pack: pack.to_owned(),
        owner: text("owner"),
        look: look.clone(),
        scale: floats(v.get("scale"), [1.0; 3])?,
        offset: floats(v.get("offset"), [0.0, 0.5, 0.0])?,
        rotation,
        hitboxes,
        drops_self: text("drops").as_deref() != Some("none"),
    };
    let material = resource_id(
        &text("material").unwrap_or_else(|| "paper".into()),
        "minecraft",
    )?;
    let mut components = Vec::new();
    if let Some(map) = v.get("components").and_then(Value::as_map) {
        for (k, c) in map {
            components.push((resource_id(k, "minecraft")?, snbt(c)));
        }
    }
    let item = crate::content::ItemDef {
        id,
        pack: pack.to_owned(),
        material,
        name: text("name").map(|n| color::translate('&', &n)),
        lore: v
            .get("lore")
            .map(Value::to_strings)
            .unwrap_or_default()
            .iter()
            .map(|l| color::translate('&', l))
            .collect(),
        look,
        components,
    };
    Ok((def, item))
}

/// The yaw a piece faces when a player looking at `player_yaw` places it:
/// toward the player, snapped to the definition's steps.
pub fn placed_yaw(rotation: Rotation, player_yaw: f32) -> f32 {
    let toward = (player_yaw + 180.0).rem_euclid(360.0);
    let snapped = match rotation {
        Rotation::Four => (toward / 90.0).round() * 90.0,
        Rotation::Eight => (toward / 45.0).round() * 45.0,
        Rotation::Free => toward,
    };
    snapped.rem_euclid(360.0)
}

/// `offset` turned by `yaw` (Minecraft's: 0 faces south, 90 west), so a
/// piece's parts stay where they belong as it turns.
pub fn turn(offset: [f64; 3], yaw: f32) -> [f64; 3] {
    let r = f64::from(yaw).to_radians();
    let (s, c) = r.sin_cos();
    let [x, y, z] = offset;
    let (nx, nz) = (x * c - z * s, x * s + z * c);
    // Whole steps come out exact.
    let tidy = |v: f64| {
        let r = (v * 1e6).round() / 1e6;
        if r == 0.0 { 0.0 } else { r }
    };
    [tidy(nx), y, tidy(nz)]
}

/// One piece of furniture in the world.
#[derive(Clone, Debug, PartialEq)]
pub struct Instance {
    pub uid: u64,
    pub id: String,
    pub world: String,
    /// Bottom centre of the block it stands in.
    pub anchor: [f64; 3],
    pub yaw: f32,
    /// Its barrier blocks.
    pub barriers: Vec<[i32; 3]>,
    /// Its entities that take clicks: kind and place.
    pub parts: Vec<(String, [f64; 3])>,
}

impl Instance {
    /// The piece `def` placed at `anchor` facing `yaw`.
    pub fn new(uid: u64, def: &FurnitureDef, world: &str, anchor: [f64; 3], yaw: f32) -> Instance {
        let mut barriers = Vec::new();
        let mut parts = Vec::new();
        for h in &def.hitboxes {
            match h {
                Hitbox::Barrier { offset } => {
                    let o = turn(offset.map(f64::from), yaw);
                    barriers.push([
                        (anchor[0] + o[0]).floor() as i32,
                        (anchor[1] + o[1]).floor() as i32,
                        (anchor[2] + o[2]).floor() as i32,
                    ]);
                }
                Hitbox::Shulker { offset, .. } => {
                    parts.push(("minecraft:shulker".into(), add(anchor, turn(*offset, yaw))));
                }
                Hitbox::Interaction { offset, .. } => {
                    parts.push((
                        "minecraft:interaction".into(),
                        add(anchor, turn(*offset, yaw)),
                    ));
                }
            }
        }
        Instance {
            uid,
            id: def.id.clone(),
            world: world.to_owned(),
            anchor,
            yaw,
            barriers,
            parts,
        }
    }

    pub fn tag(&self) -> String {
        format!("forja_{}", self.uid)
    }

    /// Whether an entity of `kind` at `at` is one of its parts.
    pub fn has_part(&self, kind: &str, at: [f64; 3]) -> bool {
        self.parts
            .iter()
            .any(|(k, p)| k == kind && (0..3).all(|i| (p[i] - at[i]).abs() < 0.05))
    }

    pub fn has_barrier(&self, at: [i32; 3]) -> bool {
        self.barriers.contains(&at)
    }

    /// The block it stands in.
    pub fn block(&self) -> [i32; 3] {
        self.anchor.map(|x| x.floor() as i32)
    }

    /// The commands that make its entities and blocks, for `item` (the
    /// furniture's item, as `/give` writes it: `minecraft:paper[...]`).
    pub fn spawn_commands(
        &self,
        def: &FurnitureDef,
        item_id: &str,
        item_components: &str,
    ) -> Vec<String> {
        let tags = format!("Tags:[\"forja\",\"{}\"]", self.tag());
        let w = &self.world;
        let mut out = Vec::new();
        let d = add(self.anchor, turn(def.offset, self.yaw));
        let [sx, sy, sz] = def.scale;
        out.push(format!(
            "execute in {w} run summon minecraft:item_display {} {} {} {{{tags},item:{{id:\"{item_id}\",count:1,components:{{{item_components}}}}},item_display:\"none\",Rotation:[{:.2}f,0f],transformation:{{left_rotation:[0f,0f,0f,1f],right_rotation:[0f,0f,0f,1f],translation:[0f,0f,0f],scale:[{sx}f,{sy}f,{sz}f]}}}}",
            num(d[0]),
            num(d[1]),
            num(d[2]),
            self.yaw
        ));
        for h in &def.hitboxes {
            match h {
                Hitbox::Shulker {
                    offset,
                    scale,
                    peek,
                } => {
                    let p = add(self.anchor, turn(*offset, self.yaw));
                    out.push(format!(
                        "execute in {w} run summon minecraft:shulker {} {} {} {{{tags},NoAI:1b,Silent:1b,Invulnerable:1b,PersistenceRequired:1b,NoGravity:1b,AttachFace:0b,Peek:{peek}b,Color:16b,active_effects:[{{id:\"minecraft:invisibility\",amplifier:0b,duration:-1,show_particles:0b}}],attributes:[{{id:\"minecraft:scale\",base:{scale}d}}]}}",
                        num(p[0]),
                        num(p[1]),
                        num(p[2])
                    ));
                }
                Hitbox::Interaction {
                    offset,
                    width,
                    height,
                } => {
                    let p = add(self.anchor, turn(*offset, self.yaw));
                    out.push(format!(
                        "execute in {w} run summon minecraft:interaction {} {} {} {{{tags},width:{width}f,height:{height}f,response:1b}}",
                        num(p[0]),
                        num(p[1]),
                        num(p[2])
                    ));
                }
                Hitbox::Barrier { .. } => {}
            }
        }
        for [x, y, z] in &self.barriers {
            out.push(format!(
                "execute in {w} run setblock {x} {y} {z} minecraft:barrier"
            ));
        }
        out
    }

    /// The commands that take it out of the world.
    pub fn remove_commands(&self) -> Vec<String> {
        let w = &self.world;
        let mut out = vec![format!("execute in {w} run kill @e[tag={}]", self.tag())];
        for [x, y, z] in &self.barriers {
            out.push(format!(
                "execute in {w} if block {x} {y} {z} minecraft:barrier run setblock {x} {y} {z} minecraft:air"
            ));
        }
        out
    }

    pub fn to_value(&self) -> Value {
        let list = |v: &[f64]| Value::List(v.iter().map(|x| Value::from(*x)).collect());
        Value::map()
            .with("uid", self.uid as i64)
            .with("id", &self.id)
            .with("world", &self.world)
            .with("anchor", list(&self.anchor))
            .with("yaw", f64::from(self.yaw))
    }

    /// An instance read back, rebuilt from its definition.
    pub fn from_value(v: &Value, def: &FurnitureDef) -> Option<Instance> {
        let a = v.get("anchor")?.as_list()?;
        let anchor = [
            a.first()?.as_f64()?,
            a.get(1)?.as_f64()?,
            a.get(2)?.as_f64()?,
        ];
        Some(Instance::new(
            v.get("uid")?.as_i64()? as u64,
            def,
            &v.get("world")?.to_text()?,
            anchor,
            v.get("yaw")?.as_f64()? as f32,
        ))
    }
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// A coordinate for a command: short, and never `-0`.
fn num(x: f64) -> String {
    let r = (x * 1000.0).round() / 1000.0;
    let r = if r == 0.0 { 0.0 } else { r };
    format!("{r}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_plugin_api::yaml;

    fn def(yml: &str) -> FurnitureDef {
        furniture_def("mesa", &yaml::parse(yml).unwrap(), "exemplo", "exemplo")
            .unwrap()
            .0
    }

    #[test]
    fn reads_furniture() {
        let d = def(
            "model: item/mesa\nhitbox:\n  - type: shulker\n    scale: 0.5\n    peek: 100\n    offset: [0, 0, 1]\n  - type: barrier\n    offset: [1, 0, 0]\n",
        );
        assert_eq!(d.look, Look::Model("exemplo:item/mesa".into()));
        assert_eq!(d.rotation, Rotation::Four);
        // An interaction box is added to take clicks.
        assert_eq!(d.hitboxes.len(), 3);
        assert!(matches!(d.hitboxes[0], Hitbox::Shulker { scale, peek: 100, .. } if scale == 0.5));
        assert!(
            furniture_def(
                "x",
                &yaml::parse("model: m\nhitbox:\n  - type: cubo\n").unwrap(),
                "p",
                "p"
            )
            .is_err()
        );
    }

    #[test]
    fn turns_and_snaps() {
        assert_eq!(placed_yaw(Rotation::Four, 10.0), 180.0);
        assert_eq!(placed_yaw(Rotation::Four, 100.0), 270.0);
        assert_eq!(placed_yaw(Rotation::Eight, 140.0), 315.0);
        assert_eq!(turn([0.0, 1.0, 1.0], 0.0), [0.0, 1.0, 1.0]);
        assert_eq!(turn([0.0, 0.0, 1.0], 90.0), [-1.0, 0.0, 0.0]);
        assert_eq!(turn([1.0, 0.0, 0.0], 180.0), [-1.0, 0.0, 0.0]);
    }

    #[test]
    fn instances_find_their_parts_and_spawn() {
        let d = def(
            "model: item/mesa\nhitbox:\n  - type: shulker\n    scale: 0.5\n    offset: [0, 0, 1]\n  - type: barrier\n    offset: [1, 0, 0]\n",
        );
        let i = Instance::new(7, &d, "minecraft:overworld", [10.5, 64.0, -3.5], 90.0);
        // Turned 90°: +z goes to -x, +x to +z.
        assert!(i.has_part("minecraft:shulker", [9.5, 64.0, -3.5]));
        assert!(i.has_part("minecraft:interaction", [10.5, 64.0, -3.5]));
        assert!(!i.has_part("minecraft:shulker", [10.5, 64.0, -3.5]));
        assert!(i.has_barrier([10, 64, -3]));
        let cmds = i.spawn_commands(
            &d,
            "minecraft:paper",
            "\"minecraft:item_model\":\"exemplo:mesa\"",
        );
        assert_eq!(cmds.len(), 4);
        assert!(cmds[0].contains("summon minecraft:item_display 10.5 64.5 -3.5"));
        assert!(cmds[0].contains("Tags:[\"forja\",\"forja_7\"]"));
        assert!(cmds[1].contains("summon minecraft:shulker 9.5 64 -3.5"));
        assert!(cmds[1].contains("base:0.5d"));
        assert!(cmds[3].ends_with("setblock 10 64 -3 minecraft:barrier"));
        assert!(i.remove_commands()[0].contains("kill @e[tag=forja_7]"));
        assert_eq!(Instance::from_value(&i.to_value(), &d), Some(i));
    }
}
