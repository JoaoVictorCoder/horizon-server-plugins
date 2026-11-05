//! What the packs in `plugins/Forja/packs/` define. A pack is a folder:
//!
//! ```text
//! packs/exemplo/
//!   pack.yml              # optional: `enabled: false`, `namespace: exemplo`
//!   config/*.yml          # items (later: blocks, furniture, recipes)
//!   resources/assets/...  # textures, models, sounds, as in a resource pack
//! ```
//!
//! Every `.yml` under `config/` may hold an `items:` map, by id.

use horizon_plugin_api::types::ItemStack;
use horizon_plugin_api::{Value, color, yaml};
use std::fs;
use std::path::{Path, PathBuf};

/// The custom data tag that marks a Forja item with its id.
pub const ID_TAG: &str = "forja:id";

/// How the client draws an item.
#[derive(Clone, Debug, PartialEq)]
pub enum Look {
    /// A flat item from one texture (`exemplo:item/rubi`); `parent` is
    /// `minecraft:item/generated`, or `minecraft:item/handheld` for tools.
    Texture { texture: String, parent: String },
    /// A model the pack has (`exemplo:item/coroa`, from Blockbench...).
    Model(String),
    /// Its material's own look.
    Vanilla,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ItemDef {
    /// `exemplo:rubi`; also its item model's id.
    pub id: String,
    /// The pack's folder name.
    pub pack: String,
    /// The vanilla item underneath: `minecraft:emerald`.
    pub material: String,
    /// With `§` codes.
    pub name: Option<String>,
    pub lore: Vec<String>,
    pub look: Look,
    /// More components, as SNBT.
    pub components: Vec<(String, String)>,
}

impl ItemDef {
    /// The item, `amount` of it, marked with its id.
    pub fn stack(&self, amount: u32) -> ItemStack {
        let mut stack = ItemStack::new(&self.material, amount).with_data(ID_TAG, &self.id);
        if self.look != Look::Vanilla {
            stack = stack.with_model(&self.id);
        }
        if let Some(name) = &self.name {
            // `item_name`, unlike `custom_name`, is not in italics and an
            // anvil can still rename the item.
            stack = stack.with_component("item_name", &format!("{{text:{}}}", snbt_string(name)));
        }
        for line in &self.lore {
            stack = stack.lore_line(line);
        }
        for (id, value) in &self.components {
            stack = stack.with_component(id, value);
        }
        stack
    }
}

/// How a block (or one of its states) looks: a model of the pack, or
/// textures by slot (`all`; `front`, `side`, `top`, `bottom`; `end`, `side`;
/// or each face: `north`, `south`, `east`, `west`, `up`, `down`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BlockLook {
    pub model: Option<String>,
    pub textures: Vec<(String, String)>,
}

impl BlockLook {
    /// This look with `over`'s model and textures on top.
    pub fn with(&self, over: &BlockLook) -> BlockLook {
        let mut out = self.clone();
        if over.model.is_some() {
            out.model = over.model.clone();
            out.textures.clear();
        }
        for (slot, tex) in &over.textures {
            match out.textures.iter_mut().find(|(s, _)| s == slot) {
                Some((_, t)) => *t = tex.clone(),
                None => out.textures.push((slot.clone(), tex.clone())),
            }
        }
        out
    }
}

/// A custom block: placed by the item of the same id, drawn as a note block
/// state for each combination of its properties.
#[derive(Clone, Debug, PartialEq)]
pub struct BlockDef {
    /// `engenho:moedor`.
    pub id: String,
    pub pack: String,
    /// The plugin told when the block is placed, broken or used.
    pub owner: Option<String>,
    /// In order; `facing` is set from where the player looks.
    pub properties: Vec<(String, Vec<String>)>,
    pub look: BlockLook,
    /// Looks for the states a predicate matches (`active=true`), in order.
    pub variants: Vec<(Vec<(String, String)>, BlockLook)>,
    /// Whether breaking it gives its item back (`drops: self`, the default).
    pub drops_self: bool,
}

impl BlockDef {
    /// The look of the state `props`.
    pub fn look_of(&self, props: &[(String, String)]) -> BlockLook {
        let mut look = self.look.clone();
        for (when, over) in &self.variants {
            if when.iter().all(|kv| props.contains(kv)) {
                look = look.with(over);
            }
        }
        look
    }

    /// Its first state: the first value of each property.
    pub fn default_state(&self) -> Vec<(String, String)> {
        self.properties
            .iter()
            .filter_map(|(k, vs)| Some((k.clone(), vs.first()?.clone())))
            .collect()
    }

    /// `props` with each missing or wrong property set to its first value.
    pub fn complete(&self, props: &[(String, String)]) -> Vec<(String, String)> {
        self.properties
            .iter()
            .filter_map(|(k, vs)| {
                let v = props
                    .iter()
                    .find(|(p, v)| p == k && vs.contains(v))
                    .map(|(_, v)| v.clone())
                    .or_else(|| vs.first().cloned())?;
                Some((k.clone(), v))
            })
            .collect()
    }
}

/// Everything the packs define.
#[derive(Clone, Debug, Default)]
pub struct Content {
    /// Pack folders, in load order (by name).
    pub packs: Vec<PackDir>,
    pub items: Vec<ItemDef>,
    pub blocks: Vec<BlockDef>,
    /// What was wrong, to log.
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct PackDir {
    pub name: String,
    pub namespace: String,
    pub dir: PathBuf,
}

impl Content {
    pub fn item(&self, id: &str) -> Option<&ItemDef> {
        self.items.iter().find(|i| i.id == id).or_else(|| {
            // Without a namespace: the only item of that name.
            let mut found = self
                .items
                .iter()
                .filter(|i| i.id.split(':').nth(1) == Some(id));
            let first = found.next()?;
            found.next().is_none().then_some(first)
        })
    }

    pub fn block(&self, id: &str) -> Option<&BlockDef> {
        self.blocks.iter().find(|b| b.id == id)
    }

    /// The Forja item a stack is, by its mark.
    pub fn item_of(&self, stack: &ItemStack) -> Option<&ItemDef> {
        let id = stack.data(ID_TAG)?;
        self.items.iter().find(|i| i.id == id)
    }

    /// Reads every pack in `dir`.
    pub fn load(dir: &Path) -> Content {
        let mut content = Content::default();
        let mut dirs: Vec<PathBuf> = match fs::read_dir(dir) {
            Ok(entries) => entries
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect(),
            Err(e) => {
                content.warnings.push(format!("{}: {e}", dir.display()));
                return content;
            }
        };
        dirs.sort();
        for pack_dir in dirs {
            content.load_pack(&pack_dir);
        }
        content
    }

    fn load_pack(&mut self, dir: &Path) {
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let meta = match fs::read_to_string(dir.join("pack.yml")) {
            Ok(text) => match yaml::parse(&text) {
                Ok(v) => v,
                Err(e) => {
                    self.warnings.push(format!("{name}/pack.yml: {e}"));
                    return;
                }
            },
            Err(_) => Value::Null,
        };
        if meta.get("enabled").and_then(Value::as_bool) == Some(false) {
            return;
        }
        let namespace = meta
            .get("namespace")
            .and_then(Value::to_text)
            .unwrap_or_else(|| name.to_ascii_lowercase());
        if !valid_namespace(&namespace) {
            self.warnings.push(format!(
                "{name}: the namespace `{namespace}` may only have a-z, 0-9, _, - and ."
            ));
            return;
        }
        self.packs.push(PackDir {
            name: name.clone(),
            namespace: namespace.clone(),
            dir: dir.to_owned(),
        });
        let mut files = Vec::new();
        yml_files(&dir.join("config"), &mut files);
        files.sort();
        for file in files {
            let shown = file
                .strip_prefix(dir)
                .map(|p| format!("{name}/{}", p.display()))
                .unwrap_or_else(|_| file.display().to_string());
            let text = match fs::read_to_string(&file) {
                Ok(t) => t,
                Err(e) => {
                    self.warnings.push(format!("{shown}: {e}"));
                    continue;
                }
            };
            match yaml::parse(&text) {
                Ok(v) => self.load_config(&shown, &name, &namespace, &v),
                Err(e) => self.warnings.push(format!("{shown}: {e}")),
            }
        }
    }

    /// One config file, already parsed.
    pub fn load_config(&mut self, file: &str, pack: &str, namespace: &str, v: &Value) {
        for section in v.as_map().unwrap_or_default() {
            match section.0.as_str() {
                "items" => {
                    for (key, def) in section.1.as_map().unwrap_or_default() {
                        let at = format!("{file}: items.{key}");
                        match item_def(key, def, pack, namespace) {
                            Ok(item) if self.items.iter().any(|i| i.id == item.id) => {
                                self.warnings
                                    .push(format!("{at}: `{}` is defined twice", item.id));
                            }
                            Ok(item) => self.items.push(item),
                            Err(e) => self.warnings.push(format!("{at}: {e}")),
                        }
                    }
                }
                "blocks" => {
                    for (key, def) in section.1.as_map().unwrap_or_default() {
                        let at = format!("{file}: blocks.{key}");
                        match block_def(key, def, pack, namespace) {
                            Ok((block, _)) if self.blocks.iter().any(|b| b.id == block.id) => {
                                self.warnings
                                    .push(format!("{at}: `{}` is defined twice", block.id));
                            }
                            Ok((_, item)) if self.items.iter().any(|i| i.id == item.id) => {
                                self.warnings.push(format!(
                                    "{at}: an item `{}` exists already; a block's item has its id",
                                    item.id
                                ));
                            }
                            Ok((block, item)) => {
                                self.blocks.push(block);
                                self.items.push(item);
                            }
                            Err(e) => self.warnings.push(format!("{at}: {e}")),
                        }
                    }
                }
                other => self.warnings.push(format!(
                    "{file}: `{other}` is not something Forja reads (yet)"
                )),
            }
        }
    }
}

fn yml_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.filter_map(Result::ok) {
        let p = e.path();
        if p.is_dir() {
            yml_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "yml" || x == "yaml") {
            out.push(p);
        }
    }
}

pub fn valid_namespace(ns: &str) -> bool {
    !ns.is_empty()
        && ns
            .chars()
            .all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_' | '-' | '.'))
}

pub fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && !path
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
        && path
            .chars()
            .all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_' | '-' | '.' | '/'))
}

/// `id` with `namespace` when it has none, checked.
pub fn resource_id(id: &str, namespace: &str) -> Result<String, String> {
    let (ns, path) = id.split_once(':').unwrap_or((namespace, id));
    if !valid_namespace(ns) || !valid_path(path) {
        return Err(format!(
            "`{id}` is not a valid id: lower case a-z, 0-9, _, -, . (and / in the path)"
        ));
    }
    Ok(format!("{ns}:{path}"))
}

fn item_def(key: &str, v: &Value, pack: &str, namespace: &str) -> Result<ItemDef, String> {
    let id = resource_id(key, namespace)?;
    let text = |k: &str| v.get(k).and_then(Value::to_text);
    let material = text("material").ok_or("it needs a `material` (the vanilla item underneath)")?;
    let material = resource_id(&material, "minecraft")?;
    let look = match (text("texture"), text("model")) {
        (Some(_), Some(_)) => return Err("give it a `texture` or a `model`, not both".into()),
        (Some(t), None) => {
            let parent = match text("parent").as_deref() {
                None | Some("generated") => "minecraft:item/generated".to_owned(),
                Some("handheld") => "minecraft:item/handheld".to_owned(),
                Some(other) => resource_id(other, "minecraft")?,
            };
            Look::Texture {
                texture: resource_id(&t, id.split(':').next().unwrap_or(namespace))?,
                parent,
            }
        }
        (None, Some(m)) => Look::Model(resource_id(&m, id.split(':').next().unwrap_or(namespace))?),
        (None, None) => Look::Vanilla,
    };
    let mut components = Vec::new();
    if let Some(map) = v.get("components").and_then(Value::as_map) {
        for (k, c) in map {
            components.push((resource_id(k, "minecraft")?, snbt(c)));
        }
    }
    Ok(ItemDef {
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
    })
}

/// The model id a block's look `index` is drawn with: `ns:block/path` for
/// the first, `ns:block/path_<n>` for the others.
pub fn block_model_id(block: &str, index: usize) -> String {
    let (ns, path) = block.split_once(':').unwrap_or(("minecraft", block));
    if index == 0 {
        format!("{ns}:block/{path}")
    } else {
        format!("{ns}:block/{path}_{index}")
    }
}

const TEXTURE_SLOTS: [&str; 13] = [
    "all", "front", "side", "top", "bottom", "end", "north", "south", "east", "west", "up", "down",
    "particle",
];

fn block_look(v: &Value, namespace: &str) -> Result<BlockLook, String> {
    let mut look = BlockLook::default();
    if let Some(m) = v.get("model").and_then(Value::to_text) {
        look.model = Some(resource_id(&m, namespace)?);
    }
    if let Some(t) = v.get("texture").and_then(Value::to_text) {
        look.textures
            .push(("all".into(), resource_id(&t, namespace)?));
    }
    for (slot, t) in v
        .get("textures")
        .and_then(Value::as_map)
        .unwrap_or_default()
    {
        if !TEXTURE_SLOTS.contains(&slot.as_str()) {
            return Err(format!(
                "`{slot}` is not a texture slot ({})",
                TEXTURE_SLOTS.join(", ")
            ));
        }
        let t = t
            .to_text()
            .ok_or_else(|| format!("textures.{slot}: a texture id"))?;
        look.textures
            .push((slot.clone(), resource_id(&t, namespace)?));
    }
    Ok(look)
}

fn block_def(
    key: &str,
    v: &Value,
    pack: &str,
    namespace: &str,
) -> Result<(BlockDef, ItemDef), String> {
    let id = resource_id(key, namespace)?;
    let ns = id.split(':').next().unwrap_or(namespace).to_owned();
    let text = |k: &str| v.get(k).and_then(Value::to_text);
    let mut properties = Vec::new();
    for (name, values) in v
        .get("properties")
        .and_then(Value::as_map)
        .unwrap_or_default()
    {
        let values = values.to_strings();
        if values.is_empty() {
            return Err(format!("properties.{name}: list its values"));
        }
        properties.push((name.clone(), values));
    }
    let states: usize = properties.iter().map(|(_, v)| v.len()).product();
    if states > 64 {
        return Err(format!("{states} states; 64 at most for one block"));
    }
    let look = block_look(v, &ns)?;
    if look.model.is_none() && look.textures.is_empty() {
        return Err("give it a `texture`, `textures` or a `model`".into());
    }
    let mut variants = Vec::new();
    for (when, over) in v
        .get("variants")
        .and_then(Value::as_map)
        .unwrap_or_default()
    {
        let predicate: Vec<(String, String)> = when
            .split(',')
            .filter_map(|kv| kv.split_once('='))
            .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
            .collect();
        for (k, val) in &predicate {
            if !properties.iter().any(|(p, vs)| p == k && vs.contains(val)) {
                return Err(format!(
                    "variants.{when}: no property {k} with the value {val}"
                ));
            }
        }
        variants.push((predicate, block_look(over, &ns)?));
    }
    let block = BlockDef {
        id: id.clone(),
        pack: pack.to_owned(),
        owner: text("owner"),
        properties,
        look,
        variants,
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
    let item = ItemDef {
        id: id.clone(),
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
        // Its item looks like the block in its first state.
        look: Look::Model(block_model_id(&id, 0)),
        components,
    };
    Ok((block, item))
}

/// A YAML value as SNBT, for a component: maps become compounds, lists
/// lists, text quoted strings.
pub fn snbt(v: &Value) -> String {
    match v {
        Value::Null => "{}".to_owned(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => {
            let s = f.to_string();
            if s.contains(['.', 'e', 'E']) {
                format!("{s}d")
            } else {
                format!("{s}.0d")
            }
        }
        Value::Str(s) => snbt_string(s),
        Value::List(items) => format!("[{}]", items.iter().map(snbt).collect::<Vec<_>>().join(",")),
        Value::Map(entries) => format!(
            "{{{}}}",
            entries
                .iter()
                .map(|(k, v)| format!("{}:{}", snbt_string(k), snbt(v)))
                .collect::<Vec<_>>()
                .join(",")
        ),
    }
}

fn snbt_string(s: &str) -> String {
    horizon_plugin_api::types::snbt_string(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ITEMS: &str = r#"
items:
  rubi:
    material: emerald
    name: '&cRubi'
    lore: ['&7Raro']
    texture: item/rubi
    components:
      max_stack_size: 16
      rarity: rare
  outro:espada:
    material: diamond_sword
    texture: item/espada
    parent: handheld
  coroa:
    material: golden_helmet
    model: item/coroa
  simples:
    material: stick
  ruim:
    name: sem material
"#;

    fn content() -> Content {
        let mut c = Content::default();
        c.load_config("x.yml", "exemplo", "exemplo", &yaml::parse(ITEMS).unwrap());
        c
    }

    #[test]
    fn reads_items() {
        let c = content();
        assert_eq!(c.items.len(), 4, "{:?}", c.warnings);
        assert_eq!(c.warnings.len(), 1);
        let rubi = c.item("rubi").unwrap();
        assert_eq!(rubi.id, "exemplo:rubi");
        assert_eq!(rubi.material, "minecraft:emerald");
        assert_eq!(rubi.name.as_deref(), Some("§cRubi"));
        assert_eq!(
            rubi.look,
            Look::Texture {
                texture: "exemplo:item/rubi".into(),
                parent: "minecraft:item/generated".into()
            }
        );
        assert_eq!(
            rubi.components,
            vec![
                ("minecraft:max_stack_size".into(), "16".into()),
                ("minecraft:rarity".into(), "\"rare\"".into())
            ]
        );
        assert_eq!(
            c.item("outro:espada").unwrap().look,
            Look::Texture {
                texture: "outro:item/espada".into(),
                parent: "minecraft:item/handheld".into()
            }
        );
        assert_eq!(
            c.item("coroa").unwrap().look,
            Look::Model("exemplo:item/coroa".into())
        );
        assert_eq!(c.item("simples").unwrap().look, Look::Vanilla);
    }

    #[test]
    fn stacks_carry_their_id() {
        let c = content();
        let stack = c.item("rubi").unwrap().stack(2);
        assert_eq!(stack.amount, 2);
        assert_eq!(stack.model.as_deref(), Some("exemplo:rubi"));
        assert_eq!(
            c.item_of(&stack).map(|i| i.id.as_str()),
            Some("exemplo:rubi")
        );
        let arg = stack.command_arg();
        assert!(arg.starts_with("minecraft:emerald["), "{arg}");
        assert!(
            arg.contains(r#"minecraft:item_name={text:"§cRubi"}"#),
            "{arg}"
        );
        assert!(
            arg.contains(r#"custom_data={"forja:id":"exemplo:rubi"}"#),
            "{arg}"
        );
        assert!(c.item("simples").unwrap().stack(1).model.is_none());
    }

    #[test]
    fn reads_blocks_and_their_items() {
        let yml = r#"
blocks:
  moedor:
    owner: Engenho
    name: '&fGrinder'
    properties:
      facing: [north, east, south, west]
      active: [false, true]
    textures:
      front: block/moedor_front
      side: block/casing
      top: block/casing_top
    variants:
      active=true:
        textures:
          front: block/moedor_front_on
  bloco:
    texture: block/cobalto
  ruim:
    properties:
      x: [a]
    texture: block/x
    variants:
      x=b:
        texture: block/y
"#;
        let mut c = Content::default();
        c.load_config("b.yml", "engenho", "engenho", &yaml::parse(yml).unwrap());
        assert_eq!(c.blocks.len(), 2, "{:?}", c.warnings);
        assert_eq!(c.warnings.len(), 1);
        let m = c.block("engenho:moedor").unwrap();
        assert_eq!(m.owner.as_deref(), Some("Engenho"));
        let on = [
            ("facing".to_owned(), "east".to_owned()),
            ("active".to_owned(), "true".to_owned()),
        ];
        let look = m.look_of(&on);
        assert!(
            look.textures
                .contains(&("front".into(), "engenho:block/moedor_front_on".into()))
        );
        assert!(
            look.textures
                .contains(&("side".into(), "engenho:block/casing".into()))
        );
        assert_eq!(m.default_state()[0], ("facing".into(), "north".into()));
        assert_eq!(
            m.complete(&[("active".into(), "true".into())])[0].1,
            "north"
        );
        let item = c.item("engenho:moedor").unwrap();
        assert_eq!(item.material, "minecraft:paper");
        assert_eq!(item.look, Look::Model("engenho:block/moedor".into()));
    }

    #[test]
    fn ids_are_checked() {
        assert!(
            resource_id("Rubi", "x")
                .unwrap_err()
                .contains("not a valid")
        );
        assert!(resource_id("x:../a", "x").is_err());
        assert_eq!(resource_id("a/b", "x").unwrap(), "x:a/b");
        assert_eq!(
            snbt(&yaml::parse("a: [1, 2.5, true, 'x']").unwrap()),
            r#"{"a":[1,2.5d,true,"x"]}"#
        );
    }
}
