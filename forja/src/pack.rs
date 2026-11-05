//! Builds the resource pack: every pack's `resources/` merged, plus what
//! the items need and nobody wrote (their item model definitions in
//! `assets/<ns>/items/` and flat models from a texture), as a zip.

use crate::content::{Content, Look};
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

/// The pack for `content`, described as `description`.
pub fn build(content: &Content, description: &str) -> Built {
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
        let built = build(&content, "Forja");
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
        assert_eq!(build(&content, "Forja").sha1, built.sha1);
        fs::remove_dir_all(&dir).unwrap();
    }
}
