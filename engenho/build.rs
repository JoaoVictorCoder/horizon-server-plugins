//! Embeds the pack (packs/engenho) in the plugin: the plugin writes it into
//! Forja's packs as it loads.
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    entries.sort_by_key(|e| e.path());
    for e in entries {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

fn main() {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("packs");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut files = Vec::new();
    walk(&root, &mut files);
    let mut code = String::from("&[\n");
    for f in &files {
        println!("cargo:rerun-if-changed={}", f.display());
        let rel = f
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let _ = writeln!(
            code,
            "    ({rel:?}, include_bytes!({:?})),",
            f.display().to_string()
        );
    }
    code.push(']');
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("pack_files.rs");
    std::fs::write(out, code).unwrap();
}
