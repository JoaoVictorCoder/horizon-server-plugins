//! Forja's web editor, server side: the page and the API it calls, served
//! by Forja's HTTP server under `/editor/<token>/`.
//!
//! `/forja editor` makes a token (valid for some hours) and gives the
//! player a link. The page reads and writes the files of
//! `plugins/Forja/packs/` (textures, models, YAML) through the API, and asks
//! Forja to reload. Everything here is plain file work: it runs on the HTTP
//! server's threads, never calls the game server, and only ever touches
//! files inside the packs folder, with the extensions it lists.

use crate::content::Content;
use horizon_plugin_api::{Value, yaml};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// The editor's page: one HTML file, scripts and styles inside.
pub const PAGE: &str = include_str!("editor.html");

/// The largest file the editor may write.
pub const MAX_FILE: usize = 8 * 1024 * 1024;

/// Extensions the editor may read and write.
const EXTENSIONS: [&str; 6] = ["png", "json", "yml", "yaml", "mcmeta", "txt"];

/// An open editor session.
pub struct Editor {
    pub token: String,
    pub expires: Instant,
    pub packs: PathBuf,
    /// Set when the page asks for a reload; Forja reloads on its next tick.
    pub reload: Arc<AtomicBool>,
}

/// What the HTTP server sends back.
#[derive(Debug, PartialEq)]
pub struct Response {
    pub status: &'static str,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

impl Response {
    fn json(v: &Value) -> Response {
        Response {
            status: "200 OK",
            content_type: "application/json",
            body: v.to_json().into_bytes(),
        }
    }
    fn error(status: &'static str, why: &str) -> Response {
        Response {
            status,
            content_type: "application/json",
            body: Value::map().with("error", why).to_json().into_bytes(),
        }
    }
    fn ok() -> Response {
        Response::json(&Value::map().with("ok", true))
    }
}

impl Editor {
    pub fn new(token: String, packs: PathBuf, valid: Duration, reload: Arc<AtomicBool>) -> Editor {
        Editor {
            token,
            expires: Instant::now() + valid,
            packs,
            reload,
        }
    }

    /// Answers a request whose path starts with `/editor/`; None when the
    /// path is not the editor's (wrong token, expired).
    pub fn handle(&self, method: &str, target: &str, body: &[u8]) -> Option<Response> {
        let (path, query) = target.split_once('?').unwrap_or((target, ""));
        let rest = path.strip_prefix("/editor/")?;
        let (token, route) = rest.split_once('/').unwrap_or((rest, ""));
        if token != self.token || Instant::now() > self.expires || self.token.len() < 16 {
            return None;
        }
        let param = |name: &str| {
            query
                .split('&')
                .filter_map(|kv| kv.split_once('='))
                .find(|(k, _)| *k == name)
                .map(|(_, v)| percent_decode(v))
        };
        Some(match (method, route) {
            ("GET", "") => Response {
                status: "200 OK",
                content_type: "text/html; charset=utf-8",
                body: PAGE.as_bytes().to_vec(),
            },
            ("GET", "api/tree") => Response::json(&self.tree()),
            ("GET", "api/content") => Response::json(&content_summary(&Content::load(&self.packs))),
            ("GET", "api/file") => match param("path").and_then(|p| self.resolve(&p)) {
                Some(file) => match fs::read(&file) {
                    Ok(bytes) => Response {
                        status: "200 OK",
                        content_type: content_type(&file),
                        body: bytes,
                    },
                    Err(_) => Response::error("404 Not Found", "no such file"),
                },
                None => Response::error("400 Bad Request", "not a file the editor may read"),
            },
            ("PUT", "api/file") => {
                let Some(file) = param("path").and_then(|p| self.resolve(&p)) else {
                    return Some(Response::error(
                        "400 Bad Request",
                        "not a file the editor may write",
                    ));
                };
                if body.len() > MAX_FILE {
                    return Some(Response::error("413 Payload Too Large", "8 MiB at most"));
                }
                if file.extension().is_some_and(|e| e == "png") && !body.starts_with(b"\x89PNG") {
                    return Some(Response::error("400 Bad Request", "not a PNG"));
                }
                if let Some(parent) = file.parent()
                    && fs::create_dir_all(parent).is_err()
                {
                    return Some(Response::error(
                        "500 Internal Server Error",
                        "could not make the folder",
                    ));
                }
                match fs::write(&file, body) {
                    Ok(()) => Response::ok(),
                    Err(e) => Response::error("500 Internal Server Error", &e.to_string()),
                }
            }
            ("DELETE", "api/file") => match param("path").and_then(|p| self.resolve(&p)) {
                Some(file) => match fs::remove_file(&file) {
                    Ok(()) => Response::ok(),
                    Err(_) => Response::error("404 Not Found", "no such file"),
                },
                None => Response::error("400 Bad Request", "not a file the editor may delete"),
            },
            // A pack's config/editor.yml, as JSON: what the editor made.
            ("GET", "api/defs") => {
                let Some(file) = param("pack").and_then(|p| self.defs_file(&p)) else {
                    return Some(Response::error("400 Bad Request", "no such pack"));
                };
                let v = fs::read_to_string(&file)
                    .ok()
                    .and_then(|t| yaml::parse(&t).ok())
                    .unwrap_or_else(Value::map);
                Response::json(&v)
            }
            ("PUT", "api/defs") => {
                let Some(file) = param("pack").and_then(|p| self.defs_file(&p)) else {
                    return Some(Response::error("400 Bad Request", "no such pack"));
                };
                let Ok(v) = Value::from_json(&String::from_utf8_lossy(body)) else {
                    return Some(Response::error("400 Bad Request", "not JSON"));
                };
                let text = format!(
                    "# Written by Forja's web editor (/forja editor). Edit it there, or here by hand.\n{}",
                    yaml::to_string(&v)
                );
                if let Some(parent) = file.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                match fs::write(&file, text) {
                    Ok(()) => Response::ok(),
                    Err(e) => Response::error("500 Internal Server Error", &e.to_string()),
                }
            }
            ("POST", "api/pack") => {
                let Some(name) = param("name").filter(|n| valid_name(n)) else {
                    return Some(Response::error(
                        "400 Bad Request",
                        "a pack name: a-z, 0-9, _ and -",
                    ));
                };
                let dir = self.packs.join(&name);
                if dir.exists() {
                    return Some(Response::error("409 Conflict", "that pack exists"));
                }
                let made = fs::create_dir_all(dir.join("config"))
                    .and_then(|()| fs::create_dir_all(dir.join("resources/assets").join(&name)))
                    .and_then(|()| fs::write(dir.join("pack.yml"), format!("namespace: {name}\n")));
                match made {
                    Ok(()) => Response::ok(),
                    Err(e) => Response::error("500 Internal Server Error", &e.to_string()),
                }
            }
            ("POST", "api/reload") => {
                self.reload.store(true, Ordering::Relaxed);
                Response::ok()
            }
            _ => Response::error("404 Not Found", "no such route"),
        })
    }

    /// The packs and their files.
    fn tree(&self) -> Value {
        let mut packs = Vec::new();
        let mut dirs: Vec<PathBuf> = fs::read_dir(&self.packs)
            .map(|r| {
                r.filter_map(Result::ok)
                    .map(|e| e.path())
                    .filter(|p| p.is_dir())
                    .collect()
            })
            .unwrap_or_default();
        dirs.sort();
        for dir in dirs {
            let name = dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let namespace = fs::read_to_string(dir.join("pack.yml"))
                .ok()
                .and_then(|t| yaml::parse(&t).ok())
                .and_then(|v| v.get("namespace").and_then(Value::to_text))
                .unwrap_or_else(|| name.to_ascii_lowercase());
            let mut files = Vec::new();
            walk(&dir, &dir, &mut files);
            files.sort();
            packs.push(
                Value::map()
                    .with("name", &name)
                    .with("namespace", &namespace)
                    .with(
                        "files",
                        Value::List(files.into_iter().map(Value::from).collect()),
                    ),
            );
        }
        Value::map().with("packs", Value::List(packs))
    }

    /// `pack/resources/...` inside the packs folder, when the editor may use it.
    fn resolve(&self, rel: &str) -> Option<PathBuf> {
        if !safe_path(rel) {
            return None;
        }
        let ext = rel.rsplit_once('.')?.1.to_ascii_lowercase();
        if !EXTENSIONS.contains(&ext.as_str()) {
            return None;
        }
        Some(self.packs.join(rel))
    }

    fn defs_file(&self, pack: &str) -> Option<PathBuf> {
        let dir = self.packs.join(pack);
        (valid_name(pack) && dir.is_dir()).then(|| dir.join("config").join("editor.yml"))
    }
}

/// A pack folder's name the editor makes.
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_' | '-'))
}

/// A relative path with no way out: no `..`, no root, no backslashes, no
/// hidden parts, only plain characters.
pub fn safe_path(rel: &str) -> bool {
    !rel.is_empty()
        && rel.len() <= 512
        && !rel.starts_with('/')
        && rel.split('/').all(|part| {
            !part.is_empty()
                && !part.starts_with('.')
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        })
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.filter_map(Result::ok) {
        let p = e.path();
        if p.file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with('.'))
        {
            continue;
        }
        if p.is_dir() {
            walk(root, &p, out);
        } else if let Ok(rel) = p.strip_prefix(root.parent().unwrap_or(root)) {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

fn content_type(file: &Path) -> &'static str {
    match file.extension().and_then(|e| e.to_str()) {
        Some("png") => "image/png",
        Some("json" | "mcmeta") => "application/json",
        _ => "text/plain; charset=utf-8",
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(b) => {
                        out.push(b);
                        i += 3;
                    }
                    Err(_) => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// What the packs define, for the editor's lists.
pub fn content_summary(c: &Content) -> Value {
    use crate::content::Look;
    use crate::furniture::Hitbox;
    let look = |l: &Look| match l {
        Look::Texture { texture, .. } => Value::map().with("texture", texture),
        Look::Model(m) => Value::map().with("model", m),
        Look::Vanilla => Value::map(),
    };
    let items = c
        .items
        .iter()
        .map(|i| {
            Value::map()
                .with("id", &i.id)
                .with("pack", &i.pack)
                .with("material", &i.material)
                .with("name", i.name.clone())
                .with("look", look(&i.look))
        })
        .collect();
    let blocks = c
        .blocks
        .iter()
        .map(|b| Value::map().with("id", &b.id).with("pack", &b.pack))
        .collect();
    let furniture = c
        .furniture
        .iter()
        .map(|f| {
            let hitboxes = f
                .hitboxes
                .iter()
                .map(|h| match h {
                    Hitbox::Shulker {
                        offset,
                        scale,
                        peek,
                    } => Value::map()
                        .with("type", "shulker")
                        .with(
                            "offset",
                            Value::List(offset.iter().map(|x| Value::from(*x)).collect()),
                        )
                        .with("scale", *scale)
                        .with("peek", u32::from(*peek)),
                    Hitbox::Barrier { offset } => Value::map().with("type", "barrier").with(
                        "offset",
                        Value::List(offset.iter().map(|x| Value::from(i64::from(*x))).collect()),
                    ),
                    Hitbox::Interaction {
                        offset,
                        width,
                        height,
                    } => Value::map()
                        .with("type", "interaction")
                        .with(
                            "offset",
                            Value::List(offset.iter().map(|x| Value::from(*x)).collect()),
                        )
                        .with("width", *width)
                        .with("height", *height),
                })
                .collect();
            Value::map()
                .with("id", &f.id)
                .with("pack", &f.pack)
                .with("look", look(&f.look))
                .with("hitboxes", Value::List(hitboxes))
        })
        .collect();
    Value::map()
        .with("items", Value::List(items))
        .with("blocks", Value::List(blocks))
        .with("furniture", Value::List(furniture))
        .with(
            "warnings",
            Value::List(c.warnings.iter().map(Value::from).collect()),
        )
}

/// A fresh token: 32 hex digits from the clock, the process and the address
/// of a new allocation, hashed. Not guessable from outside.
pub fn new_token() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let probe = Box::new(0u8);
    let seed = format!(
        "{}:{}:{:p}:{:?}",
        now.as_nanos(),
        std::process::id(),
        &*probe,
        std::thread::current().id()
    );
    crate::sha1::hex(seed.as_bytes())[..32].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor() -> (Editor, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "forja-editor-{}-{}",
            std::process::id(),
            new_token()
        ));
        fs::create_dir_all(dir.join("exemplo/config")).unwrap();
        fs::write(dir.join("exemplo/pack.yml"), "namespace: exemplo\n").unwrap();
        let e = Editor::new(
            "0123456789abcdef0123".into(),
            dir.clone(),
            Duration::from_secs(60),
            Arc::default(),
        );
        (e, dir)
    }

    #[test]
    fn paths_stay_inside() {
        assert!(safe_path("exemplo/resources/assets/x/textures/a.png"));
        for bad in [
            "../x.png",
            "a/../b.png",
            "/etc/x.png",
            "a//b.png",
            ".hidden/x.png",
            "a\\b.png",
            "",
        ] {
            assert!(!safe_path(bad), "{bad}");
        }
        assert_eq!(percent_decode("a%2Fb+c%41"), "a/b cA");
    }

    #[test]
    fn needs_the_token() {
        let (e, dir) = editor();
        assert!(e.handle("GET", "/editor/wrong/", b"").is_none());
        let page = e
            .handle("GET", "/editor/0123456789abcdef0123/", b"")
            .unwrap();
        assert!(page.content_type.starts_with("text/html"));
        let expired = Editor::new(
            "0123456789abcdef0123".into(),
            dir.clone(),
            Duration::ZERO,
            Arc::default(),
        );
        std::thread::sleep(Duration::from_millis(5));
        assert!(
            expired
                .handle("GET", "/editor/0123456789abcdef0123/", b"")
                .is_none()
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn reads_writes_and_reloads() {
        let (e, dir) = editor();
        let t = "/editor/0123456789abcdef0123/";
        let path = "exemplo%2Fresources%2Fassets%2Fexemplo%2Ftextures%2Fitem%2Fx.png";
        let png = b"\x89PNG\r\n\x1a\nrest";
        let r = e
            .handle("PUT", &format!("{t}api/file?path={path}"), png)
            .unwrap();
        assert_eq!(r.status, "200 OK", "{}", String::from_utf8_lossy(&r.body));
        let r = e
            .handle("GET", &format!("{t}api/file?path={path}"), b"")
            .unwrap();
        assert_eq!((r.content_type, r.body.as_slice()), ("image/png", &png[..]));
        // Not a PNG, not an allowed file, not inside.
        assert_eq!(
            e.handle("PUT", &format!("{t}api/file?path={path}"), b"hello")
                .unwrap()
                .status,
            "400 Bad Request"
        );
        assert_eq!(
            e.handle("PUT", &format!("{t}api/file?path=exemplo/x.exe"), b"x")
                .unwrap()
                .status,
            "400 Bad Request"
        );
        assert_eq!(
            e.handle("GET", &format!("{t}api/file?path=..%2Fsecret.yml"), b"")
                .unwrap()
                .status,
            "400 Bad Request"
        );
        let tree =
            String::from_utf8(e.handle("GET", &format!("{t}api/tree"), b"").unwrap().body).unwrap();
        assert!(
            tree.contains("exemplo/resources/assets/exemplo/textures/item/x.png"),
            "{tree}"
        );
        // Definitions round trip through config/editor.yml.
        let defs = r#"{"items":{"gema":{"material":"emerald","texture":"item/x"}}}"#;
        assert_eq!(
            e.handle("PUT", &format!("{t}api/defs?pack=exemplo"), defs.as_bytes())
                .unwrap()
                .status,
            "200 OK"
        );
        let back = String::from_utf8(
            e.handle("GET", &format!("{t}api/defs?pack=exemplo"), b"")
                .unwrap()
                .body,
        )
        .unwrap();
        assert!(back.contains("\"gema\""), "{back}");
        let content = String::from_utf8(
            e.handle("GET", &format!("{t}api/content"), b"")
                .unwrap()
                .body,
        )
        .unwrap();
        assert!(content.contains("exemplo:gema"), "{content}");
        assert_eq!(
            e.handle("POST", &format!("{t}api/pack?name=novo"), b"")
                .unwrap()
                .status,
            "200 OK"
        );
        assert!(dir.join("novo/pack.yml").exists());
        assert_eq!(
            e.handle("POST", &format!("{t}api/pack?name=..%2Fx"), b"")
                .unwrap()
                .status,
            "400 Bad Request"
        );
        e.handle("POST", &format!("{t}api/reload"), b"").unwrap();
        assert!(e.reload.load(Ordering::Relaxed));
        fs::remove_dir_all(dir).unwrap();
    }
}
