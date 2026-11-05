//! Serves the web editor on a packs folder, without a game server, to try
//! it in a browser:
//!
//! `cargo run -p forja-core --example editor_preview -- path/to/packs [port]`
//!
//! It prints the link, and stops with Ctrl+C. "Apply" only prints a line.

use forja_core::editor::Editor;
use forja_core::http::PackServer;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

fn main() {
    let mut args = std::env::args().skip(1);
    let packs = args.next().expect("a packs folder");
    let port: u16 = args.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    let server = PackServer::start("127.0.0.1", port).expect("could not listen");
    let reload = Arc::new(AtomicBool::new(false));
    let token = "preview0123456789abcdef".to_owned();
    server.open_editor(Editor::new(
        token.clone(),
        packs.into(),
        Duration::from_secs(24 * 3600),
        reload.clone(),
    ));
    println!("http://127.0.0.1:{}/editor/{token}/", server.port);
    loop {
        std::thread::sleep(Duration::from_millis(200));
        if reload.swap(false, Ordering::Relaxed) {
            println!("reload asked");
        }
    }
}
