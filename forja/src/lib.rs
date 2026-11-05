//! Forja: server-side mods for Horizon, in the spirit of Paper's
//! CraftEngine, ItemsAdder or Oraxen. Packs in `plugins/Forja/packs/` add
//! items with textures of their own; Forja builds the resource pack from
//! them, serves it over HTTP and sends it to players as they join, so
//! vanilla clients see the new items without installing anything.
//!
//! - [`content`]: what the packs define (`config/*.yml`).
//! - [`pack`]: the resource pack built from them ([`zip`], [`sha1`]).
//! - [`http`]: the server that hands the pack out.
//! - [`blocks`]: custom blocks, drawn as note block states.
//!
//! Other plugins own the blocks they define (`owner:` in YAML): Forja tells
//! them when one is placed, broken or used, on the `forja:block` channel,
//! and answers them on `forja:item`, `forja:set_block` and `forja:state`
//! (docs/forja.md).

pub use forja_core::{blocks, content, furniture, http, pack, sha1, zip};

use blocks::{Allocation, NoteState};
use content::{BlockDef, Content};
use horizon_plugin_api::prelude::*;
use horizon_plugin_api::types::Wire;
use std::fs;
use std::path::{Path, PathBuf};

/// Blocks a custom block may be placed into.
const REPLACEABLE: &[&str] = &[
    "minecraft:air",
    "minecraft:cave_air",
    "minecraft:void_air",
    "minecraft:short_grass",
    "minecraft:fern",
    "minecraft:dead_bush",
    "minecraft:snow",
    "minecraft:water",
];

/// The id clients keep Forja's pack under: a new one replaces it.
const PACK_ID: Uuid = Uuid(0x6f72_6a61_2d70_4b63_8a5e_464f_524a_4100);

/// The example pack, written on the first start.
const EXAMPLE: &[(&str, &[u8])] = &[
    (
        "exemplo/config/furniture.yml",
        include_bytes!("../packs/exemplo/config/furniture.yml"),
    ),
    (
        "exemplo/resources/assets/exemplo/models/item/mesa.json",
        include_bytes!("../packs/exemplo/resources/assets/exemplo/models/item/mesa.json"),
    ),
    (
        "exemplo/resources/assets/exemplo/models/item/banquinho.json",
        include_bytes!("../packs/exemplo/resources/assets/exemplo/models/item/banquinho.json"),
    ),
    (
        "exemplo/resources/assets/exemplo/textures/block/madeira.png",
        include_bytes!("../packs/exemplo/resources/assets/exemplo/textures/block/madeira.png"),
    ),
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
    /// Which note block state each custom block state is.
    states: Allocation,
    /// Players who placed a block this tick: their hand changes at the next
    /// tick's start, so a second click this tick would place for free.
    placing: Vec<Uuid>,
    /// The furniture in the world.
    furniture: Vec<furniture::Instance>,
    /// Saved pieces whose definition is gone: kept, to come back with it.
    orphans: Vec<Value>,
    next_uid: u64,
    /// Set by the web editor's "apply": Forja reloads at its next check.
    editor_reload: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Forja {
    fn packs_dir(ctx: &Ctx<Self>) -> PathBuf {
        ctx.data_folder().join("packs")
    }

    /// `/forja reload`: the config and the packs again, and the new pack to
    /// every player if it changed. (changed, players sent to, warnings)
    fn reload_all(&mut self, ctx: &mut Ctx<Self>) -> (bool, usize, usize) {
        ctx.reload_config();
        let changed = self.load(ctx);
        let warnings =
            self.content.warnings.len() + self.pack.as_ref().map_or(0, |p| p.warnings.len());
        let mut sent = 0;
        if changed {
            for p in ctx.server().online_players() {
                if self.send_pack(ctx, &p) {
                    sent += 1;
                }
            }
        }
        (changed, sent, warnings)
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
        // Each state of each block keeps the note block state it had; new
        // ones take free states.
        let states_file = ctx.data_folder().join("states.txt");
        self.states = Allocation::load(&states_file);
        let mut full = false;
        for block in &self.content.blocks {
            for props in blocks::combinations(&block.properties) {
                if self.states.assign(&block.id, &props).is_none() {
                    full = true;
                }
            }
        }
        if full {
            log.severe("every note block state is taken: some blocks have no look");
        }
        if self.states.dirty
            && let Err(e) = self.states.save(&states_file)
        {
            log.severe(&format!("could not write states.txt: {e}"));
        }
        self.load_furniture(ctx);
        if !self.content.blocks.is_empty() {
            // Note blocks keep their state: the others draw custom blocks.
            ctx.server().set_note_blocks_inert(true);
        }
        let built = pack::build(&self.content, &description, &self.states);
        for w in &built.warnings {
            log.warning(w);
        }
        if let Err(e) = fs::write(ctx.data_folder().join("pack.zip"), &*built.zip) {
            log.warning(&format!("could not write pack.zip: {e}"));
        }
        log.info(&format!(
            "{} pack(s), {} item(s), {} block(s); resource pack: {} files, {} KiB, sha1 {}",
            self.content.packs.len(),
            self.content.items.len(),
            self.content.blocks.len(),
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

    /// The custom block a block is, with its state: a note block in one of
    /// the states Forja gave out.
    fn custom_block(&self, block: &Block) -> Option<(&BlockDef, Vec<(String, String)>)> {
        if block.kind != "minecraft:note_block" {
            return None;
        }
        let note = NoteState::from_properties(&block.properties)?;
        let (id, props) = self.states.block_of(note)?;
        Some((self.content.block(&id)?, props))
    }

    /// Tells a block's owner what happened to it (`placed`, `broken`,
    /// `used`): its answer, or Null without an owner.
    fn notify(
        &self,
        ctx: &Ctx<Self>,
        def: &BlockDef,
        action: &str,
        pos: &BlockPos,
        props: &[(String, String)],
        player: &Player,
    ) -> Value {
        let Some(owner) = &def.owner else {
            return Value::Null;
        };
        let message = Value::map()
            .with("action", action)
            .with("block", &def.id)
            .with("pos", pos.to_wire())
            .with("properties", props_value(props))
            .with("player", player.to_wire());
        match ctx.server().message_plugin(owner, "forja:block", message) {
            Ok(v) => v,
            Err(e) => {
                ctx.logger()
                    .warning(&format!("{} ({owner}) on `{action}`: {e}", def.id));
                Value::Null
            }
        }
    }

    /// Sets the block at `pos` to `def` in the state `props`.
    fn set_block(&self, def: &BlockDef, pos: &BlockPos, props: &[(String, String)]) -> bool {
        let props = def.complete(props);
        match self.states.get(&def.id, &props) {
            Some(note) => pos.set_type(&note.block_string()),
            None => false,
        }
    }

    /// A player right-clicked `clicked` on `face` holding `def`'s item:
    /// places it where a block would go. Whether it was placed.
    fn place(
        &mut self,
        ctx: &mut Ctx<Self>,
        player: &Player,
        def: &BlockDef,
        clicked: &Block,
        face: BlockFace,
    ) -> bool {
        if self.placing.contains(&player.uuid) {
            return false;
        }
        let target = if REPLACEABLE.contains(&clicked.kind.as_str()) {
            clicked.pos.clone()
        } else {
            let [dx, dy, dz] = face.offset();
            clicked.pos.offset(dx, dy, dz)
        };
        let free = target
            .block()
            .is_some_and(|b| REPLACEABLE.contains(&b.kind.as_str()));
        let feet = player.location.block();
        let in_player = target.world == feet.world
            && target.x == feet.x
            && target.z == feet.z
            && (target.y == feet.y || target.y == feet.y + 1);
        if !free || in_player || !(-64..320).contains(&target.y) {
            return false;
        }
        let mut props = def.default_state();
        if let Some((_, values)) = def.properties.iter().find(|(k, _)| k == "facing") {
            // The block's front looks at the player.
            let [dx, dy, dz] = player.location.facing().offset();
            let toward = BlockFace::from_offset([-dx, -dy, -dz]).map(BlockFace::name);
            let horizontal = {
                let level = player
                    .location
                    .clone()
                    .with_rotation(player.location.yaw, 0.0);
                let [dx, _, dz] = level.facing().offset();
                BlockFace::from_offset([-dx, 0, -dz]).map(BlockFace::name)
            };
            let pick = toward
                .filter(|f| values.iter().any(|v| v == f))
                .or(horizontal.filter(|f| values.iter().any(|v| v == f)));
            if let Some(f) = pick {
                for (k, v) in &mut props {
                    if k == "facing" {
                        *v = f.to_owned();
                    }
                }
            }
        }
        if !self.set_block(def, &target, &props) {
            return false;
        }
        if player.game_mode != GameMode::Creative
            && let Some(item) = self.content.item(&def.id)
        {
            let left = player
                .item_in_hand()
                .map_or(0, |s| s.amount.saturating_sub(1));
            let stack = (left > 0).then(|| item.stack(left));
            player.set_equipment("mainhand", stack.as_ref());
        }
        self.placing.push(player.uuid);
        ctx.run_task(|forja, _| forja.placing.clear());
        play_at(&target, "minecraft:block.stone.place");
        self.notify(ctx, def, "placed", &target, &props, player);
        true
    }

    // ---- Furniture ----------------------------------------------------------

    fn furniture_file(ctx: &Ctx<Self>) -> PathBuf {
        ctx.data_folder().join("furniture.json")
    }

    fn load_furniture(&mut self, ctx: &Ctx<Self>) {
        self.furniture.clear();
        self.orphans.clear();
        let Ok(text) = fs::read_to_string(Self::furniture_file(ctx)) else {
            return;
        };
        let Ok(v) = Value::from_json(&text) else {
            ctx.logger()
                .severe("furniture.json is damaged: no furniture loaded");
            return;
        };
        for entry in v.as_list().unwrap_or_default() {
            let id = entry.get("id").and_then(Value::to_text).unwrap_or_default();
            match self
                .content
                .furniture(&id)
                .and_then(|d| furniture::Instance::from_value(entry, d))
            {
                Some(i) => {
                    self.next_uid = self.next_uid.max(i.uid + 1);
                    self.furniture.push(i);
                }
                None => {
                    if let Some(uid) = entry.get("uid").and_then(Value::as_i64) {
                        self.next_uid = self.next_uid.max(uid as u64 + 1);
                    }
                    self.orphans.push(entry.clone());
                }
            }
        }
    }

    fn save_furniture(&self, ctx: &Ctx<Self>) {
        let mut list: Vec<Value> = self.furniture.iter().map(|i| i.to_value()).collect();
        list.extend(self.orphans.iter().cloned());
        if let Err(e) = fs::write(Self::furniture_file(ctx), Value::List(list).to_json()) {
            ctx.logger()
                .severe(&format!("could not write furniture.json: {e}"));
        }
    }

    /// The piece one of whose entities is `entity`.
    fn piece_of_entity(&self, entity: &Entity) -> Option<usize> {
        let l = &entity.location;
        self.furniture
            .iter()
            .position(|i| i.world == l.world && i.has_part(&entity.kind, [l.x, l.y, l.z]))
    }

    /// The piece one of whose barriers is at `pos`.
    fn piece_of_barrier(&self, pos: &BlockPos) -> Option<usize> {
        self.furniture
            .iter()
            .position(|i| i.world == pos.world && i.has_barrier([pos.x, pos.y, pos.z]))
    }

    fn notify_piece(
        &self,
        ctx: &Ctx<Self>,
        piece: &furniture::Instance,
        action: &str,
        player: &Player,
    ) -> Value {
        let Some(owner) = self
            .content
            .furniture(&piece.id)
            .and_then(|d| d.owner.clone())
        else {
            return Value::Null;
        };
        let [x, y, z] = piece.block();
        let message = Value::map()
            .with("action", action)
            .with("furniture", &piece.id)
            .with("uid", piece.uid as i64)
            .with("pos", BlockPos::new(&piece.world, x, y, z).to_wire())
            .with("yaw", f64::from(piece.yaw))
            .with("player", player.to_wire());
        ctx.server()
            .message_plugin(&owner, "forja:furniture", message)
            .unwrap_or(Value::Null)
    }

    /// A player right-clicked `clicked` on `face` holding `def`'s item.
    fn place_furniture(
        &mut self,
        ctx: &mut Ctx<Self>,
        player: &Player,
        def: &furniture::FurnitureDef,
        clicked: &Block,
        face: BlockFace,
    ) -> bool {
        if self.placing.contains(&player.uuid) {
            return false;
        }
        let target = if REPLACEABLE.contains(&clicked.kind.as_str()) {
            clicked.pos.clone()
        } else {
            let [dx, dy, dz] = face.offset();
            clicked.pos.offset(dx, dy, dz)
        };
        let anchor = [
            f64::from(target.x) + 0.5,
            f64::from(target.y),
            f64::from(target.z) + 0.5,
        ];
        let yaw = furniture::placed_yaw(def.rotation, player.location.yaw);
        let piece = furniture::Instance::new(self.next_uid, def, &target.world, anchor, yaw);
        let free = |p: &BlockPos| {
            p.block()
                .is_some_and(|b| REPLACEABLE.contains(&b.kind.as_str()))
        };
        let feet = player.location.block();
        let blocks: Vec<[i32; 3]> = std::iter::once([target.x, target.y, target.z])
            .chain(piece.barriers.iter().copied())
            .collect();
        for [x, y, z] in &blocks {
            let p = BlockPos::new(&target.world, *x, *y, *z);
            let in_player = p.x == feet.x && p.z == feet.z && (p.y == feet.y || p.y == feet.y + 1);
            if !free(&p) || (in_player && piece.has_barrier([p.x, p.y, p.z])) {
                return false;
            }
        }
        if self
            .furniture
            .iter()
            .any(|i| i.world == piece.world && i.block() == piece.block())
        {
            return false;
        }
        let components = format!("\"minecraft:item_model\":\"{}\"", def.id);
        let material = self
            .content
            .item(&def.id)
            .map_or("minecraft:paper".to_owned(), |i| i.material.clone());
        for line in piece.spawn_commands(def, &material, &components) {
            ctx.server().dispatch_command(&line);
        }
        if player.game_mode != GameMode::Creative
            && let Some(item) = self.content.item(&def.id)
        {
            let left = player
                .item_in_hand()
                .map_or(0, |s| s.amount.saturating_sub(1));
            let stack = (left > 0).then(|| item.stack(left));
            player.set_equipment("mainhand", stack.as_ref());
        }
        self.next_uid += 1;
        self.placing.push(player.uuid);
        ctx.run_task(|forja, _| forja.placing.clear());
        play_at(&target, "minecraft:block.wood.place");
        self.notify_piece(ctx, &piece, "placed", player);
        self.furniture.push(piece);
        self.save_furniture(ctx);
        true
    }

    /// Takes piece `index` out of the world, its item to `player`.
    fn break_furniture(&mut self, ctx: &mut Ctx<Self>, index: usize, player: &Player) {
        let piece = self.furniture.remove(index);
        for line in piece.remove_commands() {
            ctx.server().dispatch_command(&line);
        }
        let [x, y, z] = piece.block();
        play_at(
            &BlockPos::new(&piece.world, x, y, z),
            "minecraft:block.wood.break",
        );
        let drops = self
            .content
            .furniture(&piece.id)
            .is_some_and(|d| d.drops_self);
        if drops
            && player.game_mode != GameMode::Creative
            && let Some(item) = self.content.item(&piece.id)
        {
            player.give_item(&item.stack(1));
        }
        self.notify_piece(ctx, &piece, "broken", player);
        self.save_furniture(ctx);
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

/// Block properties as a map on the wire.
fn props_value(props: &[(String, String)]) -> Value {
    Value::Map(
        props
            .iter()
            .map(|(k, v)| (k.clone(), Value::from(v)))
            .collect(),
    )
}

fn props_of(v: Option<&Value>) -> Vec<(String, String)> {
    v.and_then(Value::as_map)
        .unwrap_or_default()
        .iter()
        .filter_map(|(k, v)| Some((k.clone(), v.to_text()?)))
        .collect()
}

/// A sound at a block, for the players near it.
fn play_at(pos: &BlockPos, sound: &str) {
    server().dispatch_command(&format!(
        "execute in {} run playsound {sound} block @a {} {} {}",
        pos.world,
        f64::from(pos.x) + 0.5,
        f64::from(pos.y) + 0.5,
        f64::from(pos.z) + 0.5
    ));
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
        // The example, once: other plugins may have written their packs
        // first, and a deleted example stays deleted.
        let marker = ctx.data_folder().join(".example-written");
        if !marker.exists() {
            match write_example(&dir) {
                Ok(()) => {
                    let _ = fs::write(&marker, "packs/exemplo was written once\n");
                    ctx.logger().info("wrote the example pack to packs/exemplo");
                }
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

        // The web editor's "apply": reloaded here, on the server's thread.
        ctx.run_timer(20, 10, |forja, ctx| {
            if forja
                .editor_reload
                .swap(false, std::sync::atomic::Ordering::Relaxed)
            {
                let (changed, sent, warnings) = forja.reload_all(ctx);
                let line = format!(
                    "§6Forja§7: reloaded from the web editor: {} item(s), {} block(s), {} piece(s) of furniture{}{}.",
                    forja.content.items.len(),
                    forja.content.blocks.len(),
                    forja.content.furniture.len(),
                    if changed { format!(", new pack sent to {sent} player(s)") } else { String::new() },
                    if warnings > 0 { format!(", §e{warnings} warning(s) in the log§7") } else { String::new() }
                );
                ctx.logger().info(&color::strip(&line));
                ctx.server().broadcast(&line, "forja.admin");
            }
        });

        ctx.on(|forja: &mut Forja, ctx, e: &mut PlayerJoinEvent| {
            if !forja.send_pack(ctx, &e.player) {
                // Not in reach yet: a second later.
                let player = e.player.clone();
                ctx.run_later(20, move |forja, ctx| {
                    forja.send_pack(ctx, &player);
                });
            }
        });

        // Custom blocks: using one tells its owner; a block's item places it.
        ctx.listen(
            EventPriority::Normal,
            true,
            |forja: &mut Forja, ctx, e: &mut PlayerInteractEvent| {
                if e.action != "right_click_block" {
                    return;
                }
                let (Some(block), Some(face)) = (e.block.clone(), e.face) else {
                    return;
                };
                // A piece of furniture's barrier: the piece is used.
                if block.is("barrier")
                    && let Some(i) = forja.piece_of_barrier(&block.pos)
                {
                    e.set_cancelled(true);
                    let piece = forja.furniture[i].clone();
                    forja.notify_piece(ctx, &piece, "used", &e.player);
                    return;
                }
                if let Some((def, props)) = forja.custom_block(&block) {
                    let def = def.clone();
                    let used = forja.notify(ctx, &def, "used", &block.pos, &props, &e.player);
                    if used.as_bool() == Some(true) {
                        e.set_cancelled(true);
                        return;
                    }
                }
                let held = e
                    .item
                    .as_ref()
                    .and_then(|i| forja.content.item_of(i))
                    .map(|i| i.id.clone());
                if let Some(def) = held
                    .as_deref()
                    .and_then(|id| forja.content.furniture(id))
                    .cloned()
                {
                    e.set_cancelled(true);
                    forja.place_furniture(ctx, &e.player, &def, &block, face);
                    return;
                }
                let Some(def) = held
                    .as_deref()
                    .and_then(|id| forja.content.block(id))
                    .cloned()
                else {
                    return;
                };
                // Its item is not the block: whatever happens, the item's own
                // use doesn't.
                e.set_cancelled(true);
                forja.place(ctx, &e.player, &def, &block, face);
            },
        );
        ctx.listen(
            EventPriority::Normal,
            true,
            |forja: &mut Forja, ctx, e: &mut BlockBreakEvent| {
                let Some((def, props)) = forja.custom_block(&e.block) else {
                    return;
                };
                let def = def.clone();
                // The note block itself would drop: Forja breaks it instead.
                e.set_cancelled(true);
                let pos = e.block.pos.clone();
                pos.set_type("minecraft:air");
                play_at(&pos, "minecraft:block.stone.break");
                if def.drops_self
                    && e.player.game_mode != GameMode::Creative
                    && let Some(item) = forja.content.item(&def.id)
                {
                    e.player.give_item(&item.stack(1));
                }
                forja.notify(ctx, &def, "broken", &pos, &props, &e.player);
            },
        );

        // Furniture: a right click on one of its entities uses it, a hit
        // breaks it, and so does a hit on one of its barriers.
        ctx.listen(
            EventPriority::Normal,
            true,
            |forja: &mut Forja, ctx, e: &mut PlayerInteractEntityEvent| {
                if let Some(i) = forja.piece_of_entity(&e.entity) {
                    e.set_cancelled(true);
                    let piece = forja.furniture[i].clone();
                    forja.notify_piece(ctx, &piece, "used", &e.player);
                }
            },
        );
        ctx.listen(
            EventPriority::Normal,
            true,
            |forja: &mut Forja, ctx, e: &mut PrePlayerAttackEntityEvent| {
                if let Some(i) = forja.piece_of_entity(&e.entity) {
                    e.set_cancelled(true);
                    forja.break_furniture(ctx, i, &e.player);
                }
            },
        );
        ctx.listen(
            EventPriority::Normal,
            true,
            |forja: &mut Forja, ctx, e: &mut PlayerInteractEvent| {
                if e.action != "left_click_block" {
                    return;
                }
                if let Some(block) = e.block.clone()
                    && block.is("barrier")
                    && let Some(i) = forja.piece_of_barrier(&block.pos)
                {
                    e.set_cancelled(true);
                    forja.break_furniture(ctx, i, &e.player);
                }
            },
        );
        ctx.listen(
            EventPriority::Normal,
            true,
            |forja: &mut Forja, ctx, e: &mut BlockBreakEvent| {
                if e.block.is("barrier")
                    && let Some(i) = forja.piece_of_barrier(&e.block.pos)
                {
                    e.set_cancelled(true);
                    forja.break_furniture(ctx, i, &e.player);
                }
            },
        );

        // What other plugins ask of Forja.
        ctx.on_message("forja:item", |forja, _, m| {
            let id = m
                .message
                .get("id")
                .and_then(Value::to_text)
                .unwrap_or_default();
            let amount = m
                .message
                .get("amount")
                .and_then(Value::as_i64)
                .map_or(1, |n| n.clamp(1, 99) as u32);
            forja
                .content
                .item(&id)
                .map_or(Value::Null, |i| i.stack(amount).to_wire())
        });
        ctx.on_message("forja:state", |forja, _, m| {
            let id = m
                .message
                .get("block")
                .and_then(Value::to_text)
                .unwrap_or_default();
            let Some(def) = forja.content.block(&id) else {
                return Value::Null;
            };
            let props = def.complete(&props_of(m.message.get("properties")));
            forja
                .states
                .get(&def.id, &props)
                .map_or(Value::Null, |n| Value::from(n.block_string()))
        });
        ctx.on_message("forja:set_block", |forja, _, m| {
            let id = m
                .message
                .get("block")
                .and_then(Value::to_text)
                .unwrap_or_default();
            let (Some(def), Some(pos)) = (
                forja.content.block(&id),
                m.message.get("pos").and_then(BlockPos::from_wire),
            ) else {
                return Value::Bool(false);
            };
            Value::Bool(forja.set_block(def, &pos, &props_of(m.message.get("properties"))))
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
                "editor" => {
                    let Some(server) = &forja.http else {
                        sender.send_message(
                            "§cThe editor is served by Forja's HTTP server: turn on resource-pack.http in config.yml.",
                        );
                        return true;
                    };
                    let hours = ctx.config().f64_or("editor.hours", 2.0).clamp(0.1, 48.0);
                    let token = forja_core::editor::new_token();
                    server.open_editor(forja_core::editor::Editor::new(
                        token.clone(),
                        Self::packs_dir(ctx),
                        std::time::Duration::from_secs_f64(hours * 3600.0),
                        forja.editor_reload.clone(),
                    ));
                    let base = forja
                        .url
                        .as_deref()
                        .and_then(|u| u.rsplit_once('/'))
                        .map(|(b, _)| b.to_owned())
                        .unwrap_or_default();
                    let link = format!("{base}/editor/{token}/");
                    sender.send_message("§6Forja editor§7: open this link in a browser (anyone with it can edit the packs):");
                    sender.send_message(&format!("§f{link}"));
                    sender.send_message(&format!(
                        "§7It works for {hours:.1} h; §f/forja editor§7 again makes a new link and closes this one."
                    ));
                    ctx.logger().info(&format!("{} opened the web editor", sender.name()));
                    true
                }
                "reload" => {
                    let (changed, sent, warnings) = forja.reload_all(ctx);
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
                0 | 1 => ["give", "get", "list", "pack", "reload", "editor"]
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

    fn on_disable(&mut self, ctx: &mut Ctx<Self>) {
        self.save_furniture(ctx);
        // Stops the HTTP server and frees its port, for /plugins reload.
        self.http = None;
    }
}

horizon_plugin!(Forja::default(), plugin_yml!());
