//! Forja: server-side mods for Horizon, in the spirit of Paper's
//! CraftEngine, ItemsAdder or Oraxen. Packs in `plugins/Forja/packs/` add
//! items with textures of their own; Forja builds the resource pack from
//! them, serves it over HTTP and sends it to players as they join, so
//! vanilla clients see the new items without installing anything.
//!
//! - [`content`]: what the packs define (`config/*.yml`).
//! - [`pack`]: the resource pack built from them ([`zip`], [`sha1`]).
//! - [`http`]: the server that hands the pack out.

pub mod content;
pub mod http;
pub mod pack;
pub mod sha1;
pub mod zip;

use content::Content;
use horizon_plugin_api::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};

/// The id clients keep Forja's pack under: a new one replaces it.
const PACK_ID: Uuid = Uuid(0x6f72_6a61_2d70_4b63_8a5e_464f_524a_4100);

/// The example pack, written on the first start.
const EXAMPLE: &[(&str, &[u8])] = &[
    (
        "exemplo/pack.yml",
        include_bytes!("../packs/exemplo/pack.yml"),
    ),
    (
        "exemplo/config/items.yml",
        include_bytes!("../packs/exemplo/config/items.yml"),
    ),
    (
        "exemplo/resources/assets/exemplo/textures/item/rubi.png",
        include_bytes!("../packs/exemplo/resources/assets/exemplo/textures/item/rubi.png"),
    ),
    (
        "exemplo/resources/assets/exemplo/textures/item/espada_rubi.png",
        include_bytes!("../packs/exemplo/resources/assets/exemplo/textures/item/espada_rubi.png"),
    ),
    (
        "exemplo/resources/assets/exemplo/textures/item/picareta_rubi.png",
        include_bytes!("../packs/exemplo/resources/assets/exemplo/textures/item/picareta_rubi.png"),
    ),
];

#[derive(Default)]
pub struct Forja {
    content: Content,
    pack: Option<pack::Built>,
    http: Option<http::PackServer>,
    /// The URL players download the pack from.
    url: Option<String>,
}

impl Forja {
    fn packs_dir(ctx: &Ctx<Self>) -> PathBuf {
        ctx.data_folder().join("packs")
    }

    /// Reads the packs and builds the resource pack again: whether the pack changed.
    fn load(&mut self, ctx: &mut Ctx<Self>) -> bool {
        let log = ctx.logger();
        let dir = Self::packs_dir(ctx);
        self.content = Content::load(&dir);
        for w in &self.content.warnings {
            log.warning(w);
        }
        let description = color::translate(
            '&',
            &ctx.config()
                .string_or("resource-pack.description", "&6Forja"),
        );
        let built = pack::build(&self.content, &description);
        for w in &built.warnings {
            log.warning(w);
        }
        if let Err(e) = fs::write(ctx.data_folder().join("pack.zip"), &*built.zip) {
            log.warning(&format!("could not write pack.zip: {e}"));
        }
        log.info(&format!(
            "{} pack(s), {} item(s); resource pack: {} files, {} KiB, sha1 {}",
            self.content.packs.len(),
            self.content.items.len(),
            built.files,
            built.zip.len().div_ceil(1024),
            built.sha1
        ));
        let changed = self.pack.as_ref().map(|p| &p.sha1) != Some(&built.sha1);
        if let Some(server) = &self.http {
            server.publish(&built.sha1, built.zip.clone());
        }
        self.url = pack_url(ctx, &built.sha1, self.http.as_ref().map(|h| h.port));
        self.pack = Some(built);
        changed
    }

    fn resource_pack(&self, ctx: &mut Ctx<Self>) -> Option<ResourcePack> {
        let built = self.pack.as_ref()?;
        let prompt = ctx.config().string_or("resource-pack.prompt", "");
        Some(ResourcePack {
            id: PACK_ID,
            url: self.url.clone()?,
            sha1: built.sha1.clone(),
            required: ctx.config().bool_or("resource-pack.required", false),
            prompt: (!prompt.is_empty()).then(|| color::translate('&', &prompt)),
        })
    }

    fn send_pack(&self, ctx: &mut Ctx<Self>, player: &Player) -> bool {
        if !ctx.config().bool_or("resource-pack.send", true) {
            return true;
        }
        match self.resource_pack(ctx) {
            Some(pack) => player.send_resource_pack(&pack),
            None => true,
        }
    }
}

/// Where players get the pack whose hash is `sha1`.
fn pack_url(ctx: &mut Ctx<Forja>, sha1: &str, port: Option<u16>) -> Option<String> {
    let public = ctx.config().string_or("resource-pack.public-url", "");
    let public = public.trim().trim_end_matches('/');
    match port {
        // Forja's own server: the file is named by its hash, so a new pack
        // gets a new URL and clients never mistake an old one for it.
        Some(port) => {
            let base = if public.is_empty() {
                format!("http://{}:{port}", local_address())
            } else {
                public.to_owned()
            };
            Some(format!("{base}/{sha1}.zip"))
        }
        None if public.is_empty() => {
            ctx.logger().warning(
                "resource-pack.http is off and resource-pack.public-url is empty: no pack is sent",
            );
            None
        }
        None => Some(public.to_owned()),
    }
}

/// This machine's address on its network: the one it would reach the
/// internet from (no packet is sent), or the loopback address.
fn local_address() -> String {
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| {
            s.connect("8.8.8.8:53")?;
            s.local_addr()
        })
        .map(|a| a.ip().to_string())
        .unwrap_or_else(|_| "127.0.0.1".to_owned())
}

/// Writes the example pack when there is no pack folder yet.
fn write_example(dir: &Path) -> std::io::Result<()> {
    for (path, bytes) in EXAMPLE {
        let file = dir.join(path);
        if let Some(parent) = file.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(file, bytes)?;
    }
    Ok(())
}

fn amount(arg: Option<&String>) -> Option<u32> {
    match arg {
        None => Some(1),
        Some(a) => a.parse::<u32>().ok().filter(|n| (1..=6400).contains(n)),
    }
}

impl Plugin for Forja {
    fn on_enable(&mut self, ctx: &mut Ctx<Self>) {
        ctx.save_default_config(include_str!("../config.yml"));
        let dir = Self::packs_dir(ctx);
        if !dir.exists() {
            match write_example(&dir) {
                Ok(()) => ctx.logger().info("wrote the example pack to packs/exemplo"),
                Err(e) => ctx.logger().severe(&format!("could not write packs/: {e}")),
            }
        }
        if ctx.config().bool_or("resource-pack.http.enabled", true) {
            let host = ctx.config().string_or("resource-pack.http.host", "0.0.0.0");
            let port = ctx
                .config()
                .int_or("resource-pack.http.port", 8163)
                .clamp(0, 65535) as u16;
            match http::PackServer::start(&host, port) {
                Ok(server) => {
                    ctx.logger().info(&format!(
                        "serving the resource pack on {host}:{}",
                        server.port
                    ));
                    self.http = Some(server);
                }
                Err(e) => ctx.logger().severe(&format!(
                    "could not serve the resource pack on {host}:{port}: {e}"
                )),
            }
        }
        self.load(ctx);
        if let Some(url) = &self.url {
            ctx.logger()
                .info(&format!("players download it from {url}"));
        }

        ctx.on(|forja: &mut Forja, ctx, e: &mut PlayerJoinEvent| {
            if !forja.send_pack(ctx, &e.player) {
                // Not in reach yet: a second later.
                let player = e.player.clone();
                ctx.run_later(20, move |forja, ctx| {
                    forja.send_pack(ctx, &player);
                });
            }
        });

        ctx.set_executor("forja", |forja, ctx, sender, _, args| {
            let sub = args
                .first()
                .map(|s| s.to_ascii_lowercase())
                .unwrap_or_default();
            match sub.as_str() {
                "give" | "get" => {
                    let (target, rest) = if sub == "give" {
                        let Some(name) = args.get(1) else {
                            return false;
                        };
                        match ctx.server().match_player(name) {
                            Some(p) => (p, &args[2..]),
                            None => {
                                sender.send_message(&format!("§cNo player named {name} is on."));
                                return true;
                            }
                        }
                    } else {
                        match sender.player() {
                            Some(p) => (p.clone(), &args[1..]),
                            None => {
                                sender.send_message("§cOnly players can get items; use give.");
                                return true;
                            }
                        }
                    };
                    let Some(id) = rest.first() else { return false };
                    let Some(item) = forja.content.item(id) else {
                        sender
                            .send_message(&format!("§cNo item {id}. §7/forja list§c shows them."));
                        return true;
                    };
                    let Some(n) = amount(rest.get(1)) else {
                        return false;
                    };
                    if target.give_item(&item.stack(n)) {
                        sender.send_message(&format!("§aGave {n} {} to {}.", item.id, target.name));
                    } else {
                        sender.send_message("§cThe server could not give it.");
                    }
                    true
                }
                "list" => {
                    let pack = args.get(1);
                    let items: Vec<&str> = forja
                        .content
                        .items
                        .iter()
                        .filter(|i| pack.is_none_or(|p| &i.pack == p))
                        .map(|i| i.id.as_str())
                        .collect();
                    sender.send_message(&format!(
                        "§6Forja§7: {} pack(s), {} item(s)",
                        forja.content.packs.len(),
                        items.len()
                    ));
                    if !items.is_empty() {
                        sender.send_message(&format!("§f{}", items.join("§7, §f")));
                    }
                    true
                }
                "pack" => {
                    match &forja.pack {
                        Some(p) => {
                            sender.send_message(&format!(
                                "§6Resource pack§7: {} files, {} KiB, sha1 §f{}",
                                p.files,
                                p.zip.len().div_ceil(1024),
                                p.sha1
                            ));
                            sender.send_message(&format!(
                                "§7URL: §f{}",
                                forja
                                    .url
                                    .as_deref()
                                    .unwrap_or("none (see resource-pack in config.yml)")
                            ));
                        }
                        None => sender.send_message("§cNo pack was built."),
                    }
                    true
                }
                "reload" => {
                    ctx.reload_config();
                    let changed = forja.load(ctx);
                    let warnings = forja.content.warnings.len()
                        + forja.pack.as_ref().map_or(0, |p| p.warnings.len());
                    let mut sent = 0;
                    if changed {
                        for p in ctx.server().online_players() {
                            if forja.send_pack(ctx, &p) {
                                sent += 1;
                            }
                        }
                    }
                    sender.send_message(&format!(
                        "§aForja reloaded: {} item(s){}{}.",
                        forja.content.items.len(),
                        if changed {
                            format!(", new pack sent to {sent} player(s)")
                        } else {
                            ", pack unchanged".into()
                        },
                        if warnings > 0 {
                            format!(", §e{warnings} warning(s) in the log§a")
                        } else {
                            String::new()
                        }
                    ));
                    true
                }
                _ => false,
            }
        });

        ctx.set_tab_completer("forja", |forja, ctx, _, _, args| {
            let ids = || {
                forja
                    .content
                    .items
                    .iter()
                    .map(|i| i.id.clone())
                    .collect::<Vec<_>>()
            };
            match args.len() {
                0 | 1 => ["give", "get", "list", "pack", "reload"]
                    .map(String::from)
                    .to_vec(),
                2 => match args[0].as_str() {
                    "give" => ctx
                        .server()
                        .online_players()
                        .into_iter()
                        .map(|p| p.name)
                        .collect(),
                    "get" => ids(),
                    "list" => forja.content.packs.iter().map(|p| p.name.clone()).collect(),
                    _ => Vec::new(),
                },
                3 if args[0] == "give" => ids(),
                _ => Vec::new(),
            }
        });
    }

    fn on_disable(&mut self, _ctx: &mut Ctx<Self>) {
        // Stops the HTTP server and frees its port, for /plugins reload.
        self.http = None;
    }
}

horizon_plugin!(Forja::default(), plugin_yml!());
