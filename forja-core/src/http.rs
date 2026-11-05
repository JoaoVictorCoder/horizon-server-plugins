//! A small HTTP server for the resource pack, on a thread of its own: it
//! answers `GET /<sha1>.zip` with that pack and nothing else. It never
//! calls the server, so it may run off the server's thread.
//!
//! Packs are kept by their hash: after `/forja reload` the players who got
//! the last pack's URL can still download it while the new one goes out.
//!
//! It also serves the web editor under `/editor/<token>/` while one is open
//! ([`crate::editor`]).

use crate::editor::{Editor, MAX_FILE, Response};

use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// How many packs it keeps, newest first.
const KEPT: usize = 4;

type Packs = Arc<Mutex<Vec<(String, Arc<Vec<u8>>)>>>;
type Editors = Arc<Mutex<Option<Editor>>>;

pub struct PackServer {
    stop: Arc<AtomicBool>,
    packs: Packs,
    editor: Editors,
    thread: Option<JoinHandle<()>>,
    pub port: u16,
}

impl PackServer {
    /// Listens on `bind:port` (port 0: any free one).
    pub fn start(bind: &str, port: u16) -> io::Result<PackServer> {
        let listener = TcpListener::bind((bind, port))?;
        let port = listener.local_addr()?.port();
        listener.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let packs: Packs = Arc::default();
        let editor: Editors = Arc::default();
        let thread = {
            let (stop, packs, editor) = (stop.clone(), packs.clone(), editor.clone());
            std::thread::Builder::new()
                .name("forja-http".into())
                .spawn(move || serve(listener, &stop, &packs, &editor))?
        };
        Ok(PackServer {
            stop,
            packs,
            editor,
            thread: Some(thread),
            port,
        })
    }

    /// Serves `zip` as `/<sha1>.zip`, besides the last few.
    pub fn publish(&self, sha1: &str, zip: Arc<Vec<u8>>) {
        let mut packs = self.packs.lock().unwrap_or_else(|e| e.into_inner());
        packs.retain(|(h, _)| h != sha1);
        packs.insert(0, (sha1.to_owned(), zip));
        packs.truncate(KEPT);
    }

    /// Opens the web editor (closing the one before): its pages answer under
    /// `/editor/<token>/` until it expires.
    pub fn open_editor(&self, editor: Editor) {
        *self.editor.lock().unwrap_or_else(|e| e.into_inner()) = Some(editor);
    }

    pub fn close_editor(&self) {
        *self.editor.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

impl Drop for PackServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn serve(listener: TcpListener, stop: &AtomicBool, packs: &Packs, editor: &Editors) {
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                let (packs, editor) = (packs.clone(), editor.clone());
                let _ = std::thread::Builder::new()
                    .name("forja-http-conn".into())
                    .spawn(move || {
                        let _ = answer(stream, &packs, &editor);
                    });
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => std::thread::sleep(Duration::from_millis(200)),
        }
    }
}

fn answer(mut stream: TcpStream, packs: &Packs, editor: &Editors) -> io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(60)))?;
    let mut request = Vec::new();
    let mut buf = [0u8; 1024];
    while !request.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = stream.read(&mut buf)?;
        if n == 0 || request.len() > 16 * 1024 {
            return Ok(());
        }
        request.extend_from_slice(&buf[..n]);
    }
    let end = request
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map_or(request.len(), |p| p + 4);
    let text = String::from_utf8_lossy(&request[..end]).into_owned();
    let mut words = text.lines().next().unwrap_or("").split_whitespace();
    let method = words.next().unwrap_or("").to_owned();
    let path = words.next().unwrap_or("").to_owned();
    if path.starts_with("/editor/") {
        let length: usize = text
            .lines()
            .filter_map(|l| l.split_once(':'))
            .find(|(k, _)| k.trim().eq_ignore_ascii_case("content-length"))
            .and_then(|(_, v)| v.trim().parse().ok())
            .unwrap_or(0);
        if length > MAX_FILE {
            return respond(&mut stream, "413 Payload Too Large", "8 MiB at most\n");
        }
        let mut body = request[end..].to_vec();
        while body.len() < length {
            let n = stream.read(&mut buf)?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&buf[..n]);
        }
        body.truncate(length);
        let reply = editor
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .and_then(|e| e.handle(&method, &path, &body));
        return match reply {
            Some(r) => send(&mut stream, &r),
            None => respond(
                &mut stream,
                "404 Not Found",
                "No editor here: open one in game with /forja editor.\n",
            ),
        };
    }
    let method = method.as_str();
    let found = lookup(packs, &path);
    match (method, found) {
        ("GET" | "HEAD", Some(zip)) => {
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/zip\r\nContent-Length: {}\r\nCache-Control: public, max-age=31536000, immutable\r\nConnection: close\r\n\r\n",
                zip.len()
            );
            stream.write_all(head.as_bytes())?;
            if method == "GET" {
                stream.write_all(&zip)?;
            }
        }
        ("GET" | "HEAD", None) => respond(&mut stream, "404 Not Found", "no such pack\n")?,
        _ => respond(&mut stream, "405 Method Not Allowed", "GET only\n")?,
    }
    stream.flush()
}

fn send(stream: &mut TcpStream, r: &Response) -> io::Result<()> {
    let head = format!(
        "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        r.status,
        r.content_type,
        r.body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(&r.body)?;
    stream.flush()
}

fn respond(stream: &mut TcpStream, status: &str, body: &str) -> io::Result<()> {
    let text = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(text.as_bytes())
}

/// `/<sha1>.zip` (a query after it is ignored), or `/pack.zip` for the newest.
fn lookup(packs: &Packs, path: &str) -> Option<Arc<Vec<u8>>> {
    let path = path.split('?').next().unwrap_or("");
    let name = path.strip_prefix('/')?.strip_suffix(".zip")?;
    let packs = packs.lock().unwrap_or_else(|e| e.into_inner());
    let by_hash: BTreeMap<&str, &Arc<Vec<u8>>> =
        packs.iter().map(|(h, z)| (h.as_str(), z)).collect();
    if name == "pack" {
        return packs.first().map(|(_, z)| z.clone());
    }
    by_hash.get(name).map(|z| (*z).clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get(port: u16, path: &str) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(s, "GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        out
    }

    #[test]
    fn serves_packs_by_hash() {
        let server = PackServer::start("127.0.0.1", 0).unwrap();
        server.publish("aaaa", Arc::new(b"first".to_vec()));
        server.publish("bbbb", Arc::new(b"second".to_vec()));
        let a = get(server.port, "/aaaa.zip");
        assert!(a.starts_with("HTTP/1.1 200"), "{a}");
        assert!(a.ends_with("first"));
        assert!(get(server.port, "/bbbb.zip?x=1").ends_with("second"));
        assert!(get(server.port, "/pack.zip").ends_with("second"));
        assert!(get(server.port, "/cccc.zip").starts_with("HTTP/1.1 404"));
        assert!(get(server.port, "/../etc/passwd").starts_with("HTTP/1.1 404"));
    }

    #[test]
    fn serves_the_editor_with_its_token() {
        let server = PackServer::start("127.0.0.1", 0).unwrap();
        let dir = std::env::temp_dir().join(format!("forja-http-editor-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("p")).unwrap();
        assert!(get(server.port, "/editor/abcdefabcdefabcdef/").starts_with("HTTP/1.1 404"));
        server.open_editor(Editor::new(
            "abcdefabcdefabcdef".into(),
            dir.clone(),
            Duration::from_secs(60),
            Arc::default(),
        ));
        let page = get(server.port, "/editor/abcdefabcdefabcdef/");
        assert!(
            page.starts_with("HTTP/1.1 200") && page.contains("<html"),
            "{}",
            &page[..200.min(page.len())]
        );
        // A body comes through.
        let mut s = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
        let body = "{\"a\":1}";
        write!(
            s,
            "PUT /editor/abcdefabcdefabcdef/api/file?path=p/x.json HTTP/1.1\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        assert!(out.starts_with("HTTP/1.1 200"), "{out}");
        assert_eq!(std::fs::read_to_string(dir.join("p/x.json")).unwrap(), body);
        server.close_editor();
        assert!(get(server.port, "/editor/abcdefabcdefabcdef/").starts_with("HTTP/1.1 404"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
