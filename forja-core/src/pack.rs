//! Builds the resource pack: every pack's `resources/` merged, plus what
//! the items need and nobody wrote (their item model definitions in
//! `assets/<ns>/items/` and flat models from a texture), as a zip.

use crate::blocks::{Allocation, NoteState};
use crate::content::{BlockLook, Content, Look, block_model_id};
use crate::{sha1, zip};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

/// The resource pack format of Minecraft 26.3, major and minor.
pub const FORMAT: (u32, u32) = (97, 1);

pub struct Built {
    pub zip: Arc<Vec<u8>>,
    /// Lower-case hex.
    pub sha1: String,
    pub files: usize,
    pub warnings: Vec<String>,
}

/// The pack for `content`, described as `description`; custom blocks are
/// drawn as the note block states `states` gives them.
pub fn build(content: &Content, description: &str, states: &Allocation) -> Built {
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut warnings = Vec::new();
    // Who put each file in, for conflicts.
    let mut owner: BTreeMap<String, String> = BTreeMap::new();
    for pack in &content.packs {
        let root = pack.dir.join("resources");
        let mut found = Vec::new();
        walk(&root, &root, &mut found);
        found.sort();
        for (path, full) in found {
            if path == "pack.mcmeta" {
                warnings.push(format!(
                    "{}: resources/pack.mcmeta is ignored, Forja writes its own",
                    pack.name
                ));
                continue;
            }
            if let Some(first) = owner.get(&path) {
                warnings.push(format!(
                    "{path}: in {first} and {}; {first}'s is kept",
                    pack.name
                ));
                continue;
            }
            match fs::read(&full) {
                Ok(bytes) => {
                    files.insert(path.clone(), bytes);
                    owner.insert(path, pack.name.clone());
                }
                Err(e) => warnings.push(format!("{}: {e}", full.display())),
            }
        }
    }
    if !content.blocks.is_empty() {
        block_files(content, states, &mut files, &mut warnings);
    }
    for item in &content.items {
        let (ns, path) = item.id.split_once(':').unwrap_or(("minecraft", &item.id));
        let model = match &item.look {
            Look::Vanilla => continue,
            Look::Model(m) => {
                if !files.contains_key(&model_file(m)) {
                    warnings.push(format!(
                        "{}: its model {m} is not in any pack ({})",
                        item.id,
                        model_file(m)
                    ));
                }
                m.clone()
            }
            Look::Texture { texture, parent } => {
                let (tns, tpath) = texture.split_once(':').unwrap_or(("minecraft", texture));
                let texture_file = format!("assets/{tns}/textures/{tpath}.png");
                if !files.contains_key(&texture_file) && tns != "minecraft" {
                    warnings.push(format!(
                        "{}: its texture {texture_file} is not in any pack",
                        item.id
                    ));
                }
                let model = format!("{ns}:item/{path}");
                files.entry(model_file(&model)).or_insert_with(|| {
                    format!(
                        "{{\n  \"parent\": {},\n  \"textures\": {{\n    \"layer0\": {}\n  }}\n}}\n",
                        json_string(parent),
                        json_string(texture)
                    )
                    .into_bytes()
                });
                model
            }
        };
        files.entry(format!("assets/{ns}/items/{path}.json")).or_insert_with(|| {
            format!(
                "{{\n  \"model\": {{\n    \"type\": \"minecraft:model\",\n    \"model\": {}\n  }}\n}}\n",
                json_string(&model)
            )
            .into_bytes()
        });
    }
    let mcmeta = format!(
        "{{\n  \"pack\": {{\n    \"description\": {},\n    \"min_format\": [{}, 0],\n    \"max_format\": [{}, {}]\n  }}\n}}\n",
        json_string(description),
        FORMAT.0,
        FORMAT.0,
        FORMAT.1
    );
    files.insert("pack.mcmeta".to_owned(), mcmeta.into_bytes());
    let list: Vec<(String, Vec<u8>)> = files.into_iter().collect();
    let bytes = zip::write(&list);
    Built {
        sha1: sha1::hex(&bytes),
        zip: Arc::new(bytes),
        files: list.len(),
        warnings,
    }
}

/// The models of the custom blocks' looks, and the note block's
/// blockstates drawing each state of theirs with its model.
fn block_files(
    content: &Content,
    states: &Allocation,
    files: &mut BTreeMap<String, Vec<u8>>,
    warnings: &mut Vec<String>,
) {
    // Note block state → its variant (model and rotation).
    let mut variants: BTreeMap<NoteState, String> = BTreeMap::new();
    for block in &content.blocks {
        let mut looks: Vec<BlockLook> = Vec::new();
        for props in crate::blocks::combinations(&block.properties) {
            let look = block.look_of(&props);
            let index = match looks.iter().position(|l| *l == look) {
                Some(i) => i,
                None => {
                    looks.push(look.clone());
                    looks.len() - 1
                }
            };
            let model = block_model_id(&block.id, index);
            if index + 1 == looks.len() && !files.contains_key(&model_file(&model)) {
                if let Some(m) = &look.model
                    && !m.starts_with("minecraft:")
                    && !files.contains_key(&model_file(m))
                {
                    warnings.push(format!(
                        "{}: its model {m} is not in any pack ({})",
                        block.id,
                        model_file(m)
                    ));
                }
                for (_, t) in &look.textures {
                    let (tns, tpath) = t.split_once(':').unwrap_or(("minecraft", t));
                    let file = format!("assets/{tns}/textures/{tpath}.png");
                    if tns != "minecraft" && !files.contains_key(&file) {
                        warnings.push(format!(
                            "{}: its texture {file} is not in any pack",
                            block.id
                        ));
                    }
                }
                files.insert(model_file(&model), block_model_json(&look).into_bytes());
            }
            let Some(note) = states.get(&block.id, &props) else {
                warnings.push(format!(
                    "{}: no note block state for {}",
                    block.id,
                    crate::blocks::state_name(&block.id, &props)
                ));
                continue;
            };
            let facing = props
                .iter()
                .find(|(k, _)| k == "facing")
                .map(|(_, v)| v.as_str());
            let (x, y) = if look.textures.iter().any(|(s, _)| s == "all")
                && look.model.is_none()
                && !look
                    .textures
                    .iter()
                    .any(|(s, _)| s != "all" && s != "particle")
            {
                (0, 0)
            } else {
                rotation(facing)
            };
            let mut variant = format!("{{\"model\": {}", json_string(&model));
            if x != 0 {
                variant.push_str(&format!(", \"x\": {x}"));
            }
            if y != 0 {
                variant.push_str(&format!(", \"y\": {y}"));
            }
            variant.push('}');
            variants.insert(note, variant);
        }
    }
    let mut json = String::from("{\n  \"variants\": {\n");
    let all: Vec<NoteState> = NoteState::all().collect();
    for (i, note) in all.iter().enumerate() {
        let variant = variants
            .get(note)
            .cloned()
            .unwrap_or_else(|| "{\"model\": \"minecraft:block/note_block\"}".to_owned());
        json.push_str(&format!(
            "    {}: {variant}{}\n",
            json_string(&note.key()),
            if i + 1 < all.len() { "," } else { "" }
        ));
    }
    json.push_str("  }\n}\n");
    files.insert(
        "assets/minecraft/blockstates/note_block.json".to_owned(),
        json.into_bytes(),
    );
}

/// The x and y rotations that turn a model facing north to `facing`.
fn rotation(facing: Option<&str>) -> (u32, u32) {
    match facing {
        Some("east") => (0, 90),
        Some("south") => (0, 180),
        Some("west") => (0, 270),
        Some("up") => (270, 0),
        Some("down") => (90, 0),
        _ => (0, 0),
    }
}

/// A block model for `look`: its own model as the parent, or the vanilla
/// template its textures fit (`cube_all`, `orientable`, `cube_column`, `cube`).
fn block_model_json(look: &BlockLook) -> String {
    let tex = |slot: &str| {
        look.textures
            .iter()
            .find(|(s, _)| s == slot)
            .map(|(_, t)| t.clone())
    };
    if let Some(m) = &look.model {
        return format!("{{\n  \"parent\": {}\n}}\n", json_string(m));
    }
    let any = look
        .textures
        .first()
        .map(|(_, t)| t.clone())
        .unwrap_or_default();
    let all = tex("all");
    let side = tex("side")
        .or_else(|| all.clone())
        .unwrap_or_else(|| any.clone());
    let top = tex("top")
        .or_else(|| tex("end"))
        .unwrap_or_else(|| side.clone());
    let (parent, slots): (&str, Vec<(&str, String)>) = if let Some(front) = tex("front") {
        match tex("bottom") {
            Some(bottom) => (
                "minecraft:block/orientable_with_bottom",
                vec![
                    ("front", front),
                    ("side", side.clone()),
                    ("top", top),
                    ("bottom", bottom),
                ],
            ),
            None => (
                "minecraft:block/orientable",
                vec![("front", front), ("side", side.clone()), ("top", top)],
            ),
        }
    } else if let Some(end) = tex("end") {
        (
            "minecraft:block/cube_column",
            vec![("end", end), ("side", side.clone())],
        )
    } else if ["north", "south", "east", "west", "up", "down"]
        .iter()
        .any(|f| tex(f).is_some())
    {
        let face = |f: &str, or: &String| tex(f).unwrap_or_else(|| or.clone());
        let bottom = tex("bottom").unwrap_or_else(|| top.clone());
        (
            "minecraft:block/cube",
            vec![
                ("north", face("north", &side)),
                ("south", face("south", &side)),
                ("east", face("east", &side)),
                ("west", face("west", &side)),
                ("up", face("up", &top)),
                ("down", face("down", &bottom)),
            ],
        )
    } else {
        (
            "minecraft:block/cube_all",
            vec![("all", all.unwrap_or(any))],
        )
    };
    let particle = tex("particle").unwrap_or_else(|| slots[0].1.clone());
    let mut body: Vec<String> = slots
        .iter()
        .map(|(k, v)| format!("    {}: {}", json_string(k), json_string(v)))
        .collect();
    body.push(format!("    \"particle\": {}", json_string(&particle)));
    format!(
        "{{\n  \"parent\": {},\n  \"textures\": {{\n{}\n  }}\n}}\n",
        json_string(parent),
        body.join(",\n")
    )
}

/// Where model `ns:path` lives in a pack.
fn model_file(model: &str) -> String {
    let (ns, path) = model.split_once(':').unwrap_or(("minecraft", model));
    format!("assets/{ns}/models/{path}.json")
}

/// The files under `dir`, as (path from `root` with `/`, full path).
fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, std::path::PathBuf)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.filter_map(Result::ok) {
        let p = e.path();
        if p.is_dir() {
            walk(root, &p, out);
        } else if let Ok(rel) = p.strip_prefix(root) {
            let parts: Vec<String> = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            // Hidden files (.DS_Store, editors' backups) stay out.
            if parts.iter().any(|p| p.starts_with('.')) {
                continue;
            }
            out.push((parts.join("/"), p));
        }
    }
}

pub fn json_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::Content;

    #[test]
    fn builds_models_and_item_definitions() {
        let dir = std::env::temp_dir().join(format!("forja-pack-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let pack = dir.join("exemplo");
        fs::create_dir_all(pack.join("config")).unwrap();
        fs::create_dir_all(pack.join("resources/assets/exemplo/textures/item")).unwrap();
        fs::write(
            pack.join("resources/assets/exemplo/textures/item/rubi.png"),
            b"png",
        )
        .unwrap();
        fs::write(pack.join("resources/.DS_Store"), b"x").unwrap();
        fs::write(
            pack.join("config/items.yml"),
            "items:\n  rubi:\n    material: emerald\n    texture: item/rubi\n  sem:\n    material: stone\n    texture: item/nada\n",
        )
        .unwrap();
        let content = Content::load(&dir);
        assert!(content.warnings.is_empty(), "{:?}", content.warnings);
        let built = build(&content, "Forja", &Allocation::default());
        assert_eq!(built.sha1.len(), 40);
        assert_eq!(built.warnings.len(), 1, "{:?}", built.warnings);
        assert!(built.warnings[0].contains("item/nada"));
        // pack.mcmeta, the texture, and a model and definition for each item.
        assert_eq!(built.files, 6);
        let text = String::from_utf8_lossy(&built.zip);
        assert!(text.contains("assets/exemplo/items/rubi.json"));
        assert!(text.contains(r#""model": "exemplo:item/rubi""#));
        assert!(text.contains(r#""layer0": "exemplo:item/rubi""#));
        assert!(!text.contains("DS_Store"));
        // The same packs make the same zip.
        assert_eq!(
            build(&content, "Forja", &Allocation::default()).sha1,
            built.sha1
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn blocks_become_note_block_states() {
        let yml = "blocks:\n  maquina:\n    properties:\n      facing: [north, east]\n      active: [false, true]\n    textures:\n      front: engenho:block/frente\n      side: engenho:block/lado\n    variants:\n      active=true:\n        textures:\n          front: engenho:block/frente_on\n";
        let mut content = Content::default();
        content.load_config(
            "b.yml",
            "engenho",
            "engenho",
            &horizon_plugin_api::yaml::parse(yml).unwrap(),
        );
        let mut states = Allocation::default();
        for block in &content.blocks {
            for props in crate::blocks::combinations(&block.properties) {
                states.assign(&block.id, &props);
            }
        }
        let built = build(&content, "Forja", &states);
        let text = String::from_utf8_lossy(&built.zip);
        // Two looks (off and on), each a model; four states, each a variant.
        assert!(text.contains("assets/engenho/models/block/maquina.json"));
        assert!(text.contains("assets/engenho/models/block/maquina_1.json"));
        assert!(text.contains(r#""parent": "minecraft:block/orientable""#));
        assert!(text.contains(
            r#""instrument=harp,note=3,powered=false": {"model": "engenho:block/maquina", "y": 90}"#
        ));
        assert!(text.contains(
            r#""instrument=harp,note=0,powered=false": {"model": "minecraft:block/note_block"}"#
        ));
        // The block's item is drawn with its first look.
        assert!(text.contains(r#""model": "engenho:block/maquina""#));
        let variants = text.matches("\"instrument=").count();
        assert_eq!(variants, 25 * 27 * 2);
    }
}
