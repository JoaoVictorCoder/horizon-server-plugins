//! Loads the plugin libraries in a folder against a pretend server and
//! plays a few minutes of it: a player joins, runs each plugin's commands,
//! breaks and places blocks at spawn, walks, leaves. What the plugins do is
//! printed. A plugin author's check before copying a plugin to a server:
//!
//! ```sh
//! cargo build -p horizon-plugin-api --example homes
//! mkdir -p /tmp/plugins && cp target/debug/examples/libhomes.so /tmp/plugins/
//! cargo run -p horizon-plugin-host --example plugin_dry_run -- /tmp/plugins sethome home homes "home base"
//! ```
//!
//! Arguments after the folder are commands the player runs, in order.

use horizon_plugin_api::Value;
use horizon_plugin_api::event::{BlockBreakEvent, BlockPlaceEvent, Event};
use horizon_plugin_api::types::{
    Block, BlockFace, BlockPos, CommandSender, GameMode, ItemStack, Location, Menu, Player, Uuid,
    worlds,
};
use horizon_plugin_host::{Backend, PluginHost};

struct Pretend {
    players: Vec<Player>,
    acting: Option<Player>,
}

impl Backend for Pretend {
    fn players(&self) -> Vec<Player> {
        self.players.clone()
    }
    fn acting_player(&self) -> Option<Player> {
        self.acting.clone()
    }
    fn tell(&mut self, uuid: Uuid, message: &str) -> bool {
        let name = self
            .players
            .iter()
            .find(|p| p.uuid == uuid)
            .map_or("?", |p| p.name.as_str());
        println!("  -> {name}: {}", horizon_plugin_api::color::strip(message));
        true
    }
    fn broadcast(&mut self, message: &str) -> usize {
        println!(
            "  -> everyone: {}",
            horizon_plugin_api::color::strip(message)
        );
        self.players.len()
    }
    fn action_bar(&mut self, _: Uuid, message: &str) -> bool {
        println!(
            "  -> action bar: {}",
            horizon_plugin_api::color::strip(message)
        );
        true
    }
    fn can_run_commands(&self) -> bool {
        self.acting.is_none()
    }
    fn console_command(&mut self, line: &str) -> bool {
        println!("  $ {line}");
        // A teleport moves the pretend player too.
        let words: Vec<&str> = line.split_whitespace().collect();
        if let ["execute", "in", world, "run", "tp", who, x, y, z, ..] = words[..]
            && let Some(p) = self.players.iter_mut().find(|p| p.name == who)
        {
            p.location = Location::new(
                world,
                x.parse().unwrap_or(0.0),
                y.parse().unwrap_or(0.0),
                z.parse().unwrap_or(0.0),
            );
        }
        true
    }
    fn block(&self, pos: &BlockPos) -> Option<Block> {
        Some(Block {
            pos: pos.clone(),
            kind: "minecraft:grass_block".into(),
            properties: vec![],
        })
    }
    fn held_item(&self) -> Option<ItemStack> {
        Some(ItemStack::new("stone", 64))
    }
    fn count_items(&mut self, _: &str) -> u32 {
        0
    }
    fn take_items(&mut self, _: &str, _: u32) -> u32 {
        0
    }
    fn give_items(&mut self, item: &str, n: u32) -> u32 {
        println!("  -> gave {n} {item}");
        n
    }
    fn inventory(&self, _: Uuid) -> Option<Vec<Option<ItemStack>>> {
        Some(vec![None; 36])
    }
    fn open_menu(&mut self, _: Uuid, menu: &Menu) -> Option<i32> {
        println!(
            "  -> menu \"{}\" ({} rows):",
            horizon_plugin_api::color::strip(&menu.title),
            menu.rows
        );
        for (slot, item) in menu.items.iter().enumerate() {
            if let Some(i) = item {
                let name = i.name.as_deref().map(horizon_plugin_api::color::strip);
                println!("     [{slot}] {} {}", i.kind, name.unwrap_or_default());
            }
        }
        Some(1)
    }
    fn set_menu_items(&mut self, _: Uuid, _: &[Option<ItemStack>]) {}
    fn close_menu(&mut self, _: Uuid) {
        println!("  -> menu closed");
    }
    fn log(&mut self, level: &str, line: &str) {
        println!("[{level}] {line}");
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(dir) = args.next() else {
        eprintln!("usage: plugin_dry_run <plugins folder> [command...]");
        std::process::exit(2);
    };
    let commands: Vec<String> = args.collect();
    let host = PluginHost::new(&dir);
    let steve = Player {
        uuid: Uuid(0x069a79f444e94726a5befca90e38aaf5),
        name: "Steve".into(),
        location: Location::new(worlds::OVERWORLD, 100.5, 70.0, -40.5),
        game_mode: GameMode::Survival,
        health: 20.0,
        op_level: 0,
    };
    let mut server = Pretend {
        players: Vec::new(),
        acting: None,
    };
    host.load_all(&mut server);
    host.enable(&mut server, None);
    println!("loaded: {:?}", host.plugins());
    let mut tick = 0;
    let mut step = |server: &mut Pretend| {
        tick += 1;
        host.tick_start(server, tick);
        host.tick_end(server, tick);
    };
    println!("== Steve joins");
    server.players.push(steve.clone());
    step(&mut server);
    for c in &commands {
        println!("== /{c}");
        let me = server.players[0].clone();
        server.acting = Some(me.clone());
        host.command(&mut server, CommandSender::Player(me), c);
        server.acting = None;
        step(&mut server);
        if host.listens("inventory_click") {
            let me = server.players[0].clone();
            host.fire(
                &mut server,
                "inventory_click",
                Value::map()
                    .with("player", horizon_plugin_api::types::Wire::to_wire(&me))
                    .with("window", 1)
                    .with("slot", 0)
                    .with("button", 0)
                    .with("mode", 0),
            );
            host.fire(
                &mut server,
                "inventory_close",
                Value::map()
                    .with("player", horizon_plugin_api::types::Wire::to_wire(&me))
                    .with("window", 1),
            );
            step(&mut server);
        }
    }
    println!("== Steve breaks and places at spawn");
    let me = server.players[0].clone();
    let at = BlockPos::new(worlds::OVERWORLD, 2, 64, 3);
    server.acting = Some(me.clone());
    let block = Block {
        pos: at.clone(),
        kind: "minecraft:grass_block".into(),
        properties: vec![],
    };
    let (_, cancelled) = host.fire(
        &mut server,
        BlockBreakEvent::NAME,
        BlockBreakEvent::new(me.clone(), block).to_data(),
    );
    println!("  break cancelled: {cancelled}");
    let (_, cancelled) = host.fire(
        &mut server,
        BlockPlaceEvent::NAME,
        BlockPlaceEvent::new(
            me.clone(),
            at.offset(0, 1, 0),
            at,
            BlockFace::Up,
            ItemStack::new("stone", 1),
        )
        .to_data(),
    );
    println!("  place cancelled: {cancelled}");
    server.acting = None;
    for _ in 0..40 {
        step(&mut server);
    }
    println!("== Steve leaves");
    server.players.clear();
    step(&mut server);
    host.disable_all(&mut server);
}
