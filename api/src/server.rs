//! The server as a plugin sees it (`Bukkit.getServer()`): players, the
//! console, broadcasts, the other plugins, the log.

use crate::abi::{HostApi, RawStr};
use crate::description::PluginDescription;
use crate::types::{CommandSender, Player, Uuid, Wire, World, worlds};
use crate::value::Value;
use std::cell::Cell;

thread_local! {
    /// The server of the plugin whose code runs now on this thread: set
    /// for each message the server hands a plugin.
    static CURRENT: Cell<*const HostApi> = const { Cell::new(std::ptr::null()) };
}

/// Makes `host` the current server while `f` runs.
pub(crate) fn with_host<R>(host: *const HostApi, f: impl FnOnce() -> R) -> R {
    struct Restore(*const HostApi);
    impl Drop for Restore {
        fn drop(&mut self) {
            CURRENT.with(|c| c.set(self.0));
        }
    }
    let _restore = Restore(CURRENT.with(|c| c.replace(host)));
    f()
}

/// Calls a server method. Only on the server's thread, inside a message
/// the server handed the plugin: elsewhere (a thread of the plugin's own)
/// it fails; use [`SyncHandle`](crate::SyncHandle) to come back.
pub fn call(method: &str, args: Value) -> Result<Value, String> {
    let host = CURRENT.with(Cell::get);
    if host.is_null() {
        return Err(format!(
            "`{method}`: the server is only reachable on its own thread (use a SyncHandle)"
        ));
    }
    let args = args.to_json();
    // SAFETY: the server keeps its `HostApi` alive until the plugin is
    // destroyed, and only sets it as current while handing it a message.
    let reply = unsafe {
        let host = &*host;
        let buf = (host.call)(
            host.ctx,
            host.plugin,
            RawStr::new(method),
            RawStr::new(&args),
        );
        let text = buf.to_string_lossy();
        (host.free)(buf);
        text
    };
    // The server answers `{"ok": value}` or `{"error": "why"}`.
    let v = Value::from_json(&reply).map_err(|e| format!("`{method}`: bad answer: {e}"))?;
    if let Some(e) = v.get("error").and_then(Value::to_text) {
        return Err(e);
    }
    Ok(v.get("ok").cloned().unwrap_or(Value::Null))
}

/// The server.
pub fn server() -> Server {
    Server { _private: () }
}

/// A handle on the server; get it with [`server()`] or `ctx.server()`.
#[derive(Clone, Copy, Debug)]
pub struct Server {
    _private: (),
}

/// A plugin, as the server lists them.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginInfo {
    pub description: PluginDescription,
    pub enabled: bool,
}

impl Server {
    fn info(&self) -> Value {
        call("server_info", Value::map()).unwrap_or_default()
    }
    /// `Horizon`.
    pub fn name(&self) -> String {
        self.info()
            .get("name")
            .and_then(Value::to_text)
            .unwrap_or_default()
    }
    /// The server's own version.
    pub fn version(&self) -> String {
        self.info()
            .get("version")
            .and_then(Value::to_text)
            .unwrap_or_default()
    }
    /// The Minecraft version it plays (`26.3`).
    pub fn minecraft_version(&self) -> String {
        self.info()
            .get("minecraft_version")
            .and_then(Value::to_text)
            .unwrap_or_default()
    }
    /// Server ticks since the world opened.
    pub fn current_tick(&self) -> i64 {
        self.info().get("tick").and_then(Value::as_i64).unwrap_or(0)
    }

    pub fn online_players(&self) -> Vec<Player> {
        call("players", Value::map())
            .ok()
            .and_then(|v| Vec::<Player>::from_wire(&v))
            .unwrap_or_default()
    }
    /// The online player named `name`, in any case.
    pub fn player(&self, name: &str) -> Option<Player> {
        self.online_players()
            .into_iter()
            .find(|p| p.name.eq_ignore_ascii_case(name))
    }
    /// The online player named `name` or, failing that, the only one whose
    /// name starts with it (`Bukkit.getPlayer`).
    pub fn match_player(&self, name: &str) -> Option<Player> {
        let players = self.online_players();
        if let Some(p) = players.iter().find(|p| p.name.eq_ignore_ascii_case(name)) {
            return Some(p.clone());
        }
        let lower = name.to_ascii_lowercase();
        let mut starts = players
            .into_iter()
            .filter(|p| p.name.to_ascii_lowercase().starts_with(&lower));
        let first = starts.next()?;
        starts.next().is_none().then_some(first)
    }
    pub fn player_by_uuid(&self, uuid: Uuid) -> Option<Player> {
        self.online_players().into_iter().find(|p| p.uuid == uuid)
    }

    /// A line to every player and the log; how many players got it.
    pub fn broadcast_message(&self, message: &str) -> usize {
        call("broadcast", Value::map().with("message", message))
            .ok()
            .and_then(|v| v.as_i64())
            .map_or(0, |n| n.max(0) as usize)
    }
    /// A line to the players with `permission`.
    pub fn broadcast(&self, message: &str, permission: &str) -> usize {
        let mut n = 0;
        for p in self.online_players() {
            if p.has_permission(permission) && p.send_message(message) {
                n += 1;
            }
        }
        n
    }
    /// Runs `line` (no slash) as the console: a plugin's command goes to its
    /// plugin, any other to the game at operator level 4, with no output.
    /// Inside a player's event it runs at the next tick's start; true then
    /// means it was queued.
    pub fn dispatch_command(&self, line: &str) -> bool {
        call(
            "dispatch_command",
            Value::map().with("line", line.trim_start_matches('/')),
        )
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    }
    pub fn console_sender(&self) -> CommandSender {
        CommandSender::Console
    }

    pub fn worlds(&self) -> Vec<World> {
        [worlds::OVERWORLD, worlds::NETHER, worlds::END]
            .into_iter()
            .map(World::new)
            .collect()
    }
    pub fn world(&self, name: &str) -> Option<World> {
        let w = World::new(name);
        self.worlds().into_iter().find(|x| *x == w)
    }

    /// Every plugin the server loaded.
    pub fn plugins(&self) -> Vec<PluginInfo> {
        let Ok(Value::List(list)) = call("plugins", Value::map()) else {
            return Vec::new();
        };
        list.iter()
            .filter_map(|p| {
                Some(PluginInfo {
                    description: PluginDescription::from_value(p.get("description")?).ok()?,
                    enabled: p.get("enabled")?.as_bool()?,
                })
            })
            .collect()
    }
    pub fn plugin(&self, name: &str) -> Option<PluginInfo> {
        self.plugins()
            .into_iter()
            .find(|p| p.description.name.eq_ignore_ascii_case(name))
    }
    pub fn is_plugin_enabled(&self, name: &str) -> bool {
        self.plugin(name).is_some_and(|p| p.enabled)
    }
    /// Sends `message` to another plugin (`target`), which hears it as a
    /// [`PluginMessage`](crate::runtime::PluginMessage); its answer. How
    /// plugins offer services to each other (Bukkit's `ServicesManager`).
    pub fn message_plugin(
        &self,
        target: &str,
        channel: &str,
        message: Value,
    ) -> Result<Value, String> {
        call(
            "plugin_message",
            Value::map()
                .with("target", target)
                .with("channel", channel)
                .with("message", message),
        )
    }

    pub fn logger(&self) -> Logger {
        Logger { _private: () }
    }

    /// Bans `name` (`/ban`).
    pub fn ban(&self, name: &str, reason: &str) -> bool {
        self.dispatch_command(&format!("ban {name} {reason}"))
    }
    pub fn pardon(&self, name: &str) -> bool {
        self.dispatch_command(&format!("pardon {name}"))
    }
    pub fn whitelist_add(&self, name: &str) -> bool {
        self.dispatch_command(&format!("whitelist add {name}"))
    }
    pub fn whitelist_remove(&self, name: &str) -> bool {
        self.dispatch_command(&format!("whitelist remove {name}"))
    }
    /// Saves every world (`/save-all`).
    pub fn save_all(&self) -> bool {
        self.dispatch_command("save-all")
    }
}

/// The plugin's log, in the server's console with the plugin's name.
#[derive(Clone, Copy, Debug)]
pub struct Logger {
    _private: (),
}

impl Logger {
    fn log(&self, level: &str, message: &str) {
        if call(
            "log",
            Value::map().with("level", level).with("message", message),
        )
        .is_err()
        {
            eprintln!("[{level}] {message}");
        }
    }
    pub fn info(&self, message: &str) {
        self.log("INFO", message);
    }
    pub fn warning(&self, message: &str) {
        self.log("WARN", message);
    }
    pub fn severe(&self, message: &str) {
        self.log("ERROR", message);
    }
    pub fn fine(&self, message: &str) {
        self.log("DEBUG", message);
    }
}
