//! The server's side of Horizon's plugins (docs/plugins.md): loads the
//! libraries in `plugins/`, orders them by what they depend on, enables
//! them, hands them events from `Lowest` to `Monitor`, runs their commands
//! with permissions, and answers what they ask through a [`Backend`] the
//! game implements.
//!
//! Single-threaded: the host and every call into it stay on the server's
//! thread. Plugins may call back into the host while it hands them
//! something (a listener sending a message, running a command); the host
//! never holds a borrow across a call into a plugin, and a plugin busy in
//! a call is never handed another (its listeners are skipped, its commands
//! wait for the next tick).

pub mod loader;
pub mod permissions;

use horizon_plugin_api::Value;
use horizon_plugin_api::abi::{self, ABI_VERSION, EntryFn, HostApi, PluginApi, RawBuf, RawStr};
use horizon_plugin_api::color;
use horizon_plugin_api::description::{CommandSpec, LoadOrder, PluginDescription};
use horizon_plugin_api::event::EventPriority;
use horizon_plugin_api::types::{
    Block, BlockPos, CommandSender, ItemStack, Location, Menu, Player, ResourcePack, Uuid, Wire,
};
use permissions::Permissions;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::thread::ThreadId;

/// What the game does for plugins. Methods about "the acting player" read
/// the player whose action or command is being handled, whose state the
/// game has swapped in; they answer `None`/0 outside such a call.
pub trait Backend {
    /// The players on the server, in the order they joined.
    fn players(&self) -> Vec<Player>;
    /// The player whose action or command is being handled now.
    fn acting_player(&self) -> Option<Player>;
    /// A line (with `§` codes) in that player's chat.
    fn tell(&mut self, uuid: Uuid, message: &str) -> bool;
    /// A line to every player and the log; how many players got it.
    fn broadcast(&mut self, message: &str) -> usize;
    fn action_bar(&mut self, uuid: Uuid, message: &str) -> bool;
    /// Whether [`console_command`](Self::console_command) can run now (not
    /// inside a player's action, where the other players are out of reach).
    fn can_run_commands(&self) -> bool;
    /// Runs a command as the console (level 4, no output): whether it ran.
    fn console_command(&mut self, line: &str) -> bool;
    /// The block at `pos`, when it can be read now.
    fn block(&self, pos: &BlockPos) -> Option<Block>;
    /// The acting player's main hand.
    fn held_item(&self) -> Option<ItemStack>;
    /// How many plain `item` the acting player carries.
    fn count_items(&mut self, item: &str) -> u32;
    fn take_items(&mut self, item: &str, count: u32) -> u32;
    fn give_items(&mut self, item: &str, count: u32) -> u32;
    /// A player's 36 slots, when not swapped in.
    fn inventory(&self, uuid: Uuid) -> Option<Vec<Option<ItemStack>>>;
    fn open_menu(&mut self, uuid: Uuid, menu: &Menu) -> Option<i32>;
    fn set_menu_items(&mut self, uuid: Uuid, items: &[Option<ItemStack>]);
    fn close_menu(&mut self, uuid: Uuid);
    /// A line in the server's log (`INFO`, `WARN`, `ERROR`, `DEBUG`).
    fn log(&mut self, level: &str, line: &str);
    /// The mobs of `world` (when the server holds it now), of `kind` if given.
    fn entities(&self, world: &str, kind: Option<&str>) -> Vec<horizon_plugin_api::types::Entity> {
        let _ = (world, kind);
        Vec::new()
    }
    /// Whether a block named `id` (`minecraft:stone`) exists; None when
    /// the server can't tell.
    fn block_exists(&self, id: &str) -> Option<bool> {
        let _ = id;
        None
    }
    /// The overworld's time of day in ticks (`/time query daytime`, 0 at
    /// sunrise, counting on past 24000), when the server can tell.
    fn day_time(&self) -> Option<i64> {
        None
    }
    /// Makes note blocks keep the state they were placed in (no tuning, no
    /// instrument from below, no redstone), so a plugin can draw custom
    /// blocks as their states.
    fn set_note_blocks_inert(&mut self, inert: bool) {
        let _ = inert;
    }
    /// Offers `uuid` a resource pack (`Player.setResourcePack`): whether
    /// they are on to get it.
    fn resource_pack(&mut self, uuid: Uuid, pack: &ResourcePack) -> bool {
        let _ = (uuid, pack);
        false
    }
    /// The server's version, for `/version` and `Server::version`.
    fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_owned()
    }
}

fn is_sha1(hash: &str) -> bool {
    hash.len() == 40 && hash.chars().all(|c| c.is_ascii_hexdigit())
}

/// The message a player without a command's permission gets (Bukkit's).
const NO_PERMISSION: &str = "§cI'm sorry, but you do not have permission to perform this command. Please contact the server administrators if you believe that this is a mistake.";

struct LoadedPlugin {
    description: PluginDescription,
    api: PluginApi,
    /// Pointed at by the plugin; kept as long as it.
    _host_api: Box<HostApi>,
    data_folder: PathBuf,
    /// The file it came from, or `built in`.
    source: String,
    enabled: Cell<bool>,
    busy: Cell<bool>,
}

impl Drop for LoadedPlugin {
    fn drop(&mut self) {
        // SAFETY: the instance the entry point made, destroyed once.
        unsafe { (self.api.destroy)(self.api.instance) };
    }
}

#[derive(Clone, Debug)]
struct Listener {
    plugin: usize,
    id: u64,
    priority: EventPriority,
    ignore_cancelled: bool,
}

#[derive(Clone, Debug)]
struct Command {
    /// `None` for the host's own (`/plugins`).
    plugin: Option<usize>,
    spec: CommandSpec,
}

enum Deferred {
    Console(String),
    Command { sender: CommandSender, line: String },
}

struct Shared {
    dir: PathBuf,
    thread: ThreadId,
    plugins: RefCell<Vec<Rc<LoadedPlugin>>>,
    /// Load failures, shown by `/plugins`.
    failed: RefCell<Vec<(String, String)>>,
    listeners: RefCell<HashMap<String, Vec<Listener>>>,
    /// By name, alias and `plugin:name`.
    commands: RefCell<BTreeMap<String, Command>>,
    /// Set once the game asked for the command names.
    names_taken: Cell<bool>,
    /// `/plugins reload` asked; the game reloads at a tick's start.
    reload_requested: Cell<bool>,
    /// Loads so far: each copies the libraries to a folder of its own, so a
    /// library changed on disk loads as new code.
    generation: Cell<u32>,
    permissions: RefCell<Permissions>,
    deferred: RefCell<Vec<Deferred>>,
    seen: RefCell<Vec<Player>>,
    tick: Cell<i64>,
    backend: Cell<Option<*mut (dyn Backend + 'static)>>,
}

/// The plugins of one server run. Cheap to clone; not `Send`.
#[derive(Clone)]
pub struct PluginHost {
    shared: Rc<Shared>,
}

impl PluginHost {
    /// A host for the plugins in `dir` (`plugins/`), with nothing loaded.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        let shared = Rc::new(Shared {
            dir: dir.into(),
            thread: std::thread::current().id(),
            plugins: RefCell::new(Vec::new()),
            failed: RefCell::new(Vec::new()),
            listeners: RefCell::new(HashMap::new()),
            commands: RefCell::new(BTreeMap::new()),
            names_taken: Cell::new(false),
            reload_requested: Cell::new(false),
            generation: Cell::new(0),
            permissions: RefCell::new(Permissions::default()),
            deferred: RefCell::new(Vec::new()),
            seen: RefCell::new(Vec::new()),
            tick: Cell::new(0),
            backend: Cell::new(None),
        });
        let host = PluginHost { shared };
        for (name, aliases, description) in [
            ("plugins", vec!["pl"], "Lists the server's plugins"),
            (
                "version",
                vec!["ver"],
                "Shows the server's version, or a plugin's",
            ),
        ] {
            let mut spec = CommandSpec::new(name).description(description);
            spec.aliases = aliases.into_iter().map(str::to_owned).collect();
            let _ = host.shared.add_command(None, spec);
        }
        host
    }

    pub fn dir(&self) -> &Path {
        &self.shared.dir
    }

    /// Runs `f` with `backend` reachable from plugins' calls.
    fn enter<R>(&self, backend: &mut dyn Backend, f: impl FnOnce(&Shared) -> R) -> R {
        let ptr: *mut (dyn Backend + '_) = backend;
        // SAFETY: only the lifetime is erased. The pointer is used only
        // while `f` runs, during which `backend`'s borrow is held here, and
        // the previous one is put back after.
        let ptr: *mut (dyn Backend + 'static) = unsafe { std::mem::transmute(ptr) };
        struct Restore<'a>(&'a Shared, Option<*mut (dyn Backend + 'static)>);
        impl Drop for Restore<'_> {
            fn drop(&mut self) {
                self.0.backend.set(self.1);
            }
        }
        let _restore = Restore(&self.shared, self.shared.backend.replace(Some(ptr)));
        f(&self.shared)
    }

    /// Loads every plugin library in the folder and `permissions.yml`,
    /// then each plugin's `on_load`, in dependency order. Failures are
    /// logged and listed by `/plugins`.
    pub fn load_all(&self, backend: &mut dyn Backend) {
        self.enter(backend, |s| {
            let entries = s.scan();
            s.load(entries);
        });
    }

    /// Whether `/plugins reload` asked for a reload; asking clears it.
    pub fn take_reload_request(&self) -> bool {
        self.shared.reload_requested.replace(false)
    }

    /// Disables every plugin, forgets them, and loads and enables the
    /// folder's again (new libraries, configs and `permissions.yml`). The
    /// old libraries stay in memory: their threads may still run. Commands
    /// a plugin adds only now reach players once the game rebuilds their
    /// command tree (after [`command_names`](Self::command_names)).
    pub fn reload(&self, backend: &mut dyn Backend) {
        self.enter(backend, |s| {
            s.log("WARN", "[Plugins] Reloading: plugins that keep state outside their folders may misbehave until a restart");
            s.disable_all();
            s.listeners.borrow_mut().clear();
            s.commands.borrow_mut().retain(|_, c| c.plugin.is_none());
            s.failed.borrow_mut().clear();
            *s.permissions.borrow_mut() = Permissions::default();
            s.names_taken.set(false);
            // Dropping them destroys them.
            let old = std::mem::take(&mut *s.plugins.borrow_mut());
            drop(old);
            let entries = s.scan();
            s.load(entries);
            s.enable(None);
            let n = s.plugins.borrow().len();
            s.log("INFO", &format!("[Plugins] Reload complete: {n} plugin(s)"));
        });
    }

    /// A player joined: `PlayerJoinEvent` with the vanilla line (`message`),
    /// and what is said instead (None: nothing).
    pub fn player_joined(&self, backend: &mut dyn Backend, player: &Player, message: &str) -> Option<String> {
        self.enter(backend, |s| {
            {
                let mut seen = s.seen.borrow_mut();
                if !seen.iter().any(|p| p.uuid == player.uuid) {
                    seen.push(player.clone());
                }
            }
            let data = Value::map()
                .with("player", player.to_wire())
                .with("join_message", message);
            let (out, _) = s.fire("player_join", data, false);
            match out.get("join_message") {
                Some(v) => v.to_text(),
                None => Some(message.to_owned()),
            }
        })
    }

    /// A player left: `PlayerQuitEvent`, as [`player_joined`](Self::player_joined).
    pub fn player_left(&self, backend: &mut dyn Backend, player: &Player, message: &str) -> Option<String> {
        self.enter(backend, |s| {
            s.seen.borrow_mut().retain(|p| p.uuid != player.uuid);
            let data = Value::map()
                .with("player", player.to_wire())
                .with("quit_message", message);
            let (out, _) = s.fire("player_quit", data, false);
            match out.get("quit_message") {
                Some(v) => v.to_text(),
                None => Some(message.to_owned()),
            }
        })
    }

    /// What `line` (a plugin's command being typed, no slash) completes to:
    /// where the completed word starts (a byte offset in `line`) and the
    /// words. None when it is not a plugin's command or the sender may not
    /// run it.
    pub fn complete(
        &self,
        backend: &mut dyn Backend,
        sender: CommandSender,
        line: &str,
    ) -> Option<(usize, Vec<String>)> {
        let cmd = self.shared.lookup(line)?;
        let (label, rest) = line.split_once(' ')?;
        let start = line.rfind(' ').map_or(line.len(), |i| i + 1);
        let mut args: Vec<String> = rest.split_whitespace().map(str::to_owned).collect();
        if line.ends_with(' ') {
            args.push(String::new());
        }
        let typed = args.last().cloned().unwrap_or_default().to_ascii_lowercase();
        let words = self.enter(backend, |s| s.completions(&cmd, &sender, label, &args))?;
        let mut out: Vec<String> = words
            .into_iter()
            .filter(|w| w.to_ascii_lowercase().starts_with(&typed))
            .collect();
        out.dedup();
        Some((start, out))
    }

    /// Loads plugins linked into the server itself (and tests'), as if
    /// they were in the folder.
    pub fn load_entries(&self, backend: &mut dyn Backend, entries: Vec<(EntryFn, String)>) {
        self.enter(backend, |s| s.load(entries));
    }

    /// Enables the plugins of `order` (or all) not yet enabled, in load order.
    pub fn enable(&self, backend: &mut dyn Backend, order: Option<LoadOrder>) {
        self.enter(backend, |s| s.enable(order));
    }

    /// Disables every plugin, last loaded first.
    pub fn disable_all(&self, backend: &mut dyn Backend) {
        self.enter(backend, |s| s.disable_all());
    }

    /// A server tick starts: what plugins left for it (commands asked for
    /// inside a player's action), their schedulers, `ServerTickStartEvent`.
    pub fn tick_start(&self, backend: &mut dyn Backend, tick: i64) {
        self.enter(backend, |s| {
            s.tick.set(tick);
            s.run_deferred();
            let plugins = s.plugins.borrow().clone();
            let payload = Value::map().with("tick", tick);
            for (i, p) in plugins.iter().enumerate() {
                if p.enabled.get() {
                    s.report(i, "tick", s.send(i, "tick", &payload));
                }
            }
            s.fire("server_tick_start", Value::map().with("tick", tick), false);
        });
    }

    /// A server tick ended: `ServerTickEndEvent`, then what players did
    /// since the last tick (joined, left, moved, died, changed world or
    /// game mode).
    pub fn tick_end(&self, backend: &mut dyn Backend, tick: i64) {
        self.enter(backend, |s| {
            s.fire("server_tick_end", Value::map().with("tick", tick), false);
            s.watch_players();
        });
    }

    /// Whether any enabled plugin listens to `event`: hot hooks ask this
    /// before building an event.
    pub fn listens(&self, event: &str) -> bool {
        let plugins = self.shared.plugins.borrow();
        self.shared
            .listeners
            .borrow()
            .get(event)
            .is_some_and(|l| l.iter().any(|l| plugins[l.plugin].enabled.get()))
    }

    /// Hands `event` (`block_break`) to its listeners: the event as the
    /// last non-monitor listener left it, and whether it was cancelled.
    pub fn fire(&self, backend: &mut dyn Backend, event: &str, data: Value) -> (Value, bool) {
        if !self.listens(event) {
            return (data, false);
        }
        self.enter(backend, |s| s.fire(event, data, false))
    }

    /// The commands plugins added (names and aliases, not `plugin:name`),
    /// for the server's command tree. Asked once: commands added later run
    /// from the console and other plugins, but players can't type them
    /// until the server restarts.
    pub fn command_names(&self) -> Vec<String> {
        self.shared.names_taken.set(true);
        self.shared
            .commands
            .borrow()
            .keys()
            .filter(|k| !k.contains(':'))
            .cloned()
            .collect()
    }

    /// Whether `line`'s first word (no slash) is a plugin's command.
    pub fn has_command(&self, line: &str) -> bool {
        self.shared.lookup(line).is_some()
    }

    /// Runs `line` (no slash) for `sender` when it is a plugin's command:
    /// whether it was one.
    pub fn command(&self, backend: &mut dyn Backend, sender: CommandSender, line: &str) -> bool {
        if !self.has_command(line) {
            return false;
        }
        self.enter(backend, |s| s.run_command(None, sender, line));
        true
    }

    /// Whether `player` has `node`.
    pub fn has_permission(&self, player: &Player, node: &str) -> bool {
        self.shared.permissions.borrow().has(player, node)
    }

    /// Reads `permissions.yml` again.
    pub fn reload_permissions(&self, backend: &mut dyn Backend) {
        let path = self.shared.dir.join("permissions.yml");
        if let Err(e) = self.shared.permissions.borrow_mut().load_file(&path) {
            backend.log("ERROR", &format!("[Plugins] {e}"));
        }
    }

    /// The plugins loaded: name, version, enabled.
    pub fn plugins(&self) -> Vec<(String, String, bool)> {
        self.shared
            .plugins
            .borrow()
            .iter()
            .map(|p| {
                (
                    p.description.name.clone(),
                    p.description.version.clone(),
                    p.enabled.get(),
                )
            })
            .collect()
    }
}

unsafe extern "C" fn host_call(
    ctx: *mut c_void,
    plugin: u32,
    method: RawStr,
    args: RawStr,
) -> RawBuf {
    // SAFETY: `ctx` is the `Shared` an `Rc` keeps alive while any plugin
    // is (plugins are dropped with it).
    let shared = unsafe { &*ctx.cast::<Shared>() };
    let reply = if std::thread::current().id() != shared.thread {
        Value::map().with("error", "the server is only reachable on its own thread")
    } else {
        // SAFETY: the plugin passes text that lives for the call.
        let (method, args) = unsafe { (method.as_str(), args.as_str()) };
        let args = Value::from_json(args).unwrap_or_default();
        let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            shared.call(plugin as usize, method, &args)
        }));
        match out {
            Ok(Ok(v)) => Value::map().with("ok", v),
            Ok(Err(e)) => Value::map().with("error", e),
            Err(_) => Value::map().with("error", format!("the server failed in `{method}`")),
        }
    };
    RawBuf::from_string(reply.to_json())
}

/// The order plugins load in: what they depend on first, and those
/// `load_before` names after; ties by name. Plugins whose hard
/// dependencies are missing (or loop) are left out, with why.
fn load_order(descs: &[PluginDescription]) -> (Vec<usize>, Vec<(usize, String)>) {
    let index: HashMap<String, usize> = descs
        .iter()
        .enumerate()
        .map(|(i, d)| (d.name.to_ascii_lowercase(), i))
        .collect();
    let find = |n: &String| index.get(&n.to_ascii_lowercase()).copied();
    let mut alive = vec![true; descs.len()];
    let mut failed = Vec::new();
    // Missing hard dependencies, until nothing more drops out.
    loop {
        let mut changed = false;
        for (i, d) in descs.iter().enumerate() {
            if !alive[i] {
                continue;
            }
            let missing: Vec<&String> = d
                .depend
                .iter()
                .filter(|n| find(n).is_none_or(|j| !alive[j]))
                .collect();
            if !missing.is_empty() {
                alive[i] = false;
                changed = true;
                let names: Vec<&str> = missing.iter().map(|s| s.as_str()).collect();
                failed.push((i, format!("needs {} (depend)", names.join(", "))));
            }
        }
        if !changed {
            break;
        }
    }
    // Edges: before → after.
    let mut after: Vec<Vec<usize>> = vec![Vec::new(); descs.len()];
    let mut incoming = vec![0usize; descs.len()];
    let mut edge = |a: usize, b: usize, after: &mut Vec<Vec<usize>>| {
        if a != b && alive[a] && alive[b] && !after[a].contains(&b) {
            after[a].push(b);
            incoming[b] += 1;
        }
    };
    for (i, d) in descs.iter().enumerate() {
        for n in d.depend.iter().chain(&d.soft_depend) {
            if let Some(j) = find(n) {
                edge(j, i, &mut after);
            }
        }
        for n in &d.load_before {
            if let Some(j) = find(n) {
                edge(i, j, &mut after);
            }
        }
    }
    let key = |i: usize| {
        (
            descs[i].load != LoadOrder::Startup,
            descs[i].name.to_ascii_lowercase(),
        )
    };
    let mut ready: Vec<usize> = (0..descs.len())
        .filter(|&i| alive[i] && incoming[i] == 0)
        .collect();
    let mut order = Vec::new();
    while !ready.is_empty() {
        ready.sort_by_key(|&i| std::cmp::Reverse(key(i)));
        let i = ready.pop().unwrap_or_default();
        order.push(i);
        for &j in &after[i] {
            incoming[j] -= 1;
            if incoming[j] == 0 {
                ready.push(j);
            }
        }
    }
    for (i, &a) in alive.iter().enumerate() {
        if a && !order.contains(&i) {
            failed.push((i, "its dependencies loop".to_owned()));
        }
    }
    (order, failed)
}

impl Shared {
    // The backend is the game's, lent for the call from it, not part of `self`.
    #[allow(clippy::mut_from_ref)]
    fn backend(&self) -> Result<&mut dyn Backend, String> {
        match self.backend.get() {
            // SAFETY: set by `PluginHost::enter` for the length of a call
            // from the game, which lends the backend for it; each use is
            // short and never overlaps another.
            Some(b) => Ok(unsafe { &mut *b }),
            None => Err("the server is not running plugins right now".to_owned()),
        }
    }

    /// The plugin libraries in the folder, each copied to this load's own
    /// folder first (a library is never opened where it lies, so it can be
    /// replaced while the server runs), opened. Failures are logged.
    fn scan(&self) -> Vec<(EntryFn, String)> {
        let generation = self.generation.get() + 1;
        self.generation.set(generation);
        let copies = std::env::temp_dir()
            .join(format!("horizon-plugins-{}", std::process::id()))
            .join(generation.to_string());
        let mut files: Vec<PathBuf> = std::fs::read_dir(&self.dir)
            .map(|d| {
                d.filter_map(|e| e.ok().map(|e| e.path()))
                    .filter(|p| p.extension().is_some_and(|x| x == loader::EXTENSION))
                    .collect()
            })
            .unwrap_or_default();
        files.sort();
        let mut entries = Vec::new();
        for f in files {
            let source = f
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            let copy = copies.join(&source);
            let opened = std::fs::create_dir_all(&copies)
                .and_then(|()| std::fs::copy(&f, &copy))
                .map_err(|e| format!("could not copy it: {e}"))
                .and_then(|_| loader::open(&copy));
            match opened {
                Ok(entry) => entries.push((entry, source)),
                Err(e) => {
                    self.log("ERROR", &format!("[Plugins] Could not load {source}: {e}"));
                    self.failed.borrow_mut().push((source, e));
                }
            }
        }
        entries
    }

    /// The words `cmd`'s arguments complete to: the plugin's completer's,
    /// else the online players' names.
    fn completions(&self, cmd: &Command, sender: &CommandSender, label: &str, args: &[String]) -> Option<Vec<String>> {
        if let Some(node) = &cmd.spec.permission
            && let CommandSender::Player(p) = sender
            && !self.permissions.borrow().has(p, node)
        {
            return None;
        }
        let names = || -> Vec<String> {
            self.backend()
                .map(|b| b.players().into_iter().map(|p| p.name).collect())
                .unwrap_or_default()
        };
        let Some(i) = cmd.plugin else {
            return Some(match (cmd.spec.name.as_str(), args.len()) {
                ("plugins", 1) => vec!["reload".to_owned()],
                ("version", 1) => self
                    .plugins
                    .borrow()
                    .iter()
                    .map(|p| p.description.name.clone())
                    .collect(),
                _ => Vec::new(),
            });
        };
        if !self.plugin(i).is_some_and(|p| p.enabled.get()) {
            return None;
        }
        let payload = Value::map()
            .with("command", &cmd.spec.name)
            .with("label", label.rsplit(':').next().unwrap_or(label))
            .with("args", args.to_vec())
            .with("sender", sender.to_wire());
        match self.send(i, "tab", &payload) {
            Ok(Value::Null) => Some(names()),
            Ok(v) => Some(v.to_strings()),
            Err(_) => None,
        }
    }

    fn log(&self, level: &str, line: &str) {
        match self.backend() {
            Ok(b) => b.log(level, line),
            Err(_) => eprintln!("[{level}] {line}"),
        }
    }

    fn plugin(&self, i: usize) -> Option<Rc<LoadedPlugin>> {
        self.plugins.borrow().get(i).cloned()
    }

    fn plugin_name(&self, i: usize) -> String {
        self.plugin(i)
            .map_or_else(|| format!("plugin {i}"), |p| p.description.name.clone())
    }

    /// Hands plugin `i` a message; `Err` if it is busy or answers one.
    fn send(&self, i: usize, kind: &str, payload: &Value) -> Result<Value, String> {
        let p = self.plugin(i).ok_or("no such plugin")?;
        Self::send_to(&p, kind, payload)
    }

    fn send_to(p: &LoadedPlugin, kind: &str, payload: &Value) -> Result<Value, String> {
        if p.busy.get() {
            return Err(format!("{} is busy", p.description.name));
        }
        p.busy.set(true);
        let json = payload.to_json();
        // SAFETY: the plugin's own functions on its own instance; the
        // answer is freed by the plugin's `free`.
        let reply = unsafe {
            let buf = (p.api.handle)(p.api.instance, RawStr::new(kind), RawStr::new(&json));
            let text = buf.to_string_lossy();
            (p.api.free)(buf);
            text
        };
        p.busy.set(false);
        let v = Value::from_json(&reply).map_err(|e| format!("bad answer: {e}"))?;
        if let Some(e) = v.get("error").and_then(Value::to_text) {
            return Err(e);
        }
        Ok(v.get("ok").cloned().unwrap_or_default())
    }

    fn report(&self, i: usize, what: &str, r: Result<Value, String>) {
        if let Err(e) = r {
            self.log("ERROR", &format!("[{}] {what}: {e}", self.plugin_name(i)));
        }
    }

    fn load(&self, entries: Vec<(EntryFn, String)>) {
        let path = self.dir.join("permissions.yml");
        if let Err(e) = self.permissions.borrow_mut().load_file(&path) {
            self.log("ERROR", &format!("[Plugins] {e}"));
        }
        // Start each one, and read its description.
        let mut started: Vec<LoadedPlugin> = Vec::new();
        let base = self.plugins.borrow().len();
        for (entry, source) in entries {
            let host_api = Box::new(HostApi {
                abi: ABI_VERSION,
                ctx: std::ptr::from_ref(self).cast_mut().cast::<c_void>(),
                plugin: (base + started.len()) as u32,
                call: host_call,
                free: abi::free_buf,
            });
            // SAFETY: the plugin's entry point, with a HostApi kept as long
            // as the plugin.
            let api = unsafe { entry(&*host_api) };
            let fail = |why: String| {
                self.log(
                    "ERROR",
                    &format!("[Plugins] Could not load {source}: {why}"),
                );
                self.failed.borrow_mut().push((source.clone(), why));
            };
            if api.abi != ABI_VERSION {
                fail(format!(
                    "it was made for plugin ABI {}, this server runs {ABI_VERSION}",
                    api.abi
                ));
                continue;
            }
            let mut plugin = LoadedPlugin {
                description: PluginDescription::default(),
                api,
                _host_api: host_api,
                data_folder: PathBuf::new(),
                source: source.clone(),
                enabled: Cell::new(false),
                busy: Cell::new(false),
            };
            if plugin.api.instance.is_null() {
                fail("it failed to start (a panic in its constructor or description?)".into());
                continue;
            }
            // Not in `plugins` yet: its calls back find no plugin of its
            // number, which only its log lines' name needs.
            let describe = Self::send_to(&plugin, "describe", &Value::map());
            match describe.and_then(|v| PluginDescription::from_value(&v)) {
                Ok(d) => {
                    let same = |o: &PluginDescription| o.name.eq_ignore_ascii_case(&d.name);
                    let taken = started.iter().any(|p| same(&p.description))
                        || self.plugins.borrow().iter().any(|p| same(&p.description));
                    if taken {
                        fail(format!("another plugin is named {}", d.name));
                        continue;
                    }
                    plugin.data_folder = self.dir.join(&d.name);
                    plugin.description = d;
                    started.push(plugin);
                }
                Err(e) => fail(format!("its description: {e}")),
            }
        }
        // Put them in order. Their HostApi numbers are where each was
        // started; renumber to where it lands.
        let descs: Vec<PluginDescription> = started.iter().map(|p| p.description.clone()).collect();
        let (order, failed) = load_order(&descs);
        for (i, why) in failed {
            self.log(
                "ERROR",
                &format!("[Plugins] Could not load {}: {why}", started[i].source),
            );
            self.failed
                .borrow_mut()
                .push((started[i].source.clone(), why));
        }
        let mut slots: Vec<Option<LoadedPlugin>> = started.into_iter().map(Some).collect();
        for i in order {
            let Some(mut p) = slots[i].take() else {
                continue;
            };
            let at = self.plugins.borrow().len();
            p._host_api.plugin = at as u32;
            for perm in &p.description.permissions {
                self.permissions.borrow_mut().declare(perm);
            }
            self.plugins.borrow_mut().push(Rc::new(p));
            let p = self.plugin(at).expect("just pushed");
            self.log(
                "INFO",
                &format!(
                    "[{}] Loading {}",
                    p.description.name,
                    p.description.full_name()
                ),
            );
            let payload =
                Value::map().with("data_folder", p.data_folder.to_string_lossy().into_owned());
            self.report(at, "on_load", self.send(at, "load", &payload));
        }
        // The rest (left out) are dropped, which destroys them.
    }

    fn enable(&self, order: Option<LoadOrder>) {
        let plugins = self.plugins.borrow().clone();
        for (i, p) in plugins.iter().enumerate() {
            if p.enabled.get() || order.is_some_and(|o| o != p.description.load) {
                continue;
            }
            self.log(
                "INFO",
                &format!(
                    "[{}] Enabling {}",
                    p.description.name,
                    p.description.full_name()
                ),
            );
            p.enabled.set(true);
            if let Err(e) = self.send(i, "enable", &Value::map()) {
                self.log("ERROR", &format!("[{}] on_enable: {e}", p.description.name));
                self.disable(i);
                continue;
            }
            self.fire(
                "plugin_enable",
                Value::map().with("plugin", &p.description.name),
                false,
            );
        }
        if order.is_none_or(|o| o == LoadOrder::PostWorld) {
            self.fire("server_load", Value::map(), false);
        }
    }

    fn disable(&self, i: usize) {
        let Some(p) = self.plugin(i) else {
            return;
        };
        if !p.enabled.get() {
            return;
        }
        self.log(
            "INFO",
            &format!(
                "[{}] Disabling {}",
                p.description.name,
                p.description.full_name()
            ),
        );
        self.report(i, "on_disable", self.send(i, "disable", &Value::map()));
        p.enabled.set(false);
        for list in self.listeners.borrow_mut().values_mut() {
            list.retain(|l| l.plugin != i);
        }
        self.fire(
            "plugin_disable",
            Value::map().with("plugin", &p.description.name),
            false,
        );
    }

    fn disable_all(&self) {
        let n = self.plugins.borrow().len();
        for i in (0..n).rev() {
            self.disable(i);
        }
    }

    fn fire(&self, event: &str, mut data: Value, mut cancelled: bool) -> (Value, bool) {
        let list = self
            .listeners
            .borrow()
            .get(event)
            .cloned()
            .unwrap_or_default();
        for l in list {
            let Some(p) = self.plugin(l.plugin) else {
                continue;
            };
            if !p.enabled.get() || (l.ignore_cancelled && cancelled) {
                continue;
            }
            if p.busy.get() {
                // Its own action led here (a command it ran set off the
                // event): it can't be handed another call inside that one.
                continue;
            }
            let payload = Value::map()
                .with("listener", l.id)
                .with("event", event)
                .with("data", data.clone())
                .with("cancelled", cancelled);
            match self.send(l.plugin, "event", &payload) {
                Ok(v) if l.priority != EventPriority::Monitor => {
                    if let Some(d) = v.get("data") {
                        data = d.clone();
                    }
                    if let Some(c) = v.get("cancelled").and_then(Value::as_bool) {
                        cancelled = c;
                    }
                }
                Ok(_) => {}
                Err(e) => self.log(
                    "ERROR",
                    &format!("[{}] a listener of {event}: {e}", p.description.name),
                ),
            }
        }
        (data, cancelled)
    }

    fn add_command(&self, plugin: Option<usize>, spec: CommandSpec) -> Result<(), String> {
        // A leading `/` makes WorldEdit's `//set`: the command `/set`.
        let valid = |n: &str| {
            let n = n.strip_prefix('/').unwrap_or(n);
            !n.is_empty()
                && n.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "_-".contains(c))
        };
        if !valid(&spec.name) {
            return Err(format!(
                "`{}` must be lower-case letters, digits, `_` and `-` (after an optional `/`)",
                spec.name
            ));
        }
        let mut commands = self.commands.borrow_mut();
        let prefix = plugin.map_or_else(
            || "horizon".to_owned(),
            |i| self.plugin_name(i).to_ascii_lowercase(),
        );
        let cmd = Command { plugin, spec };
        commands.insert(format!("{prefix}:{}", cmd.spec.name), cmd.clone());
        for name in std::iter::once(&cmd.spec.name).chain(&cmd.spec.aliases) {
            if !valid(name) {
                continue;
            }
            // The first plugin to take a name keeps it, as Bukkit's
            // fallback prefixes do; the others answer only to `plugin:name`.
            if let Some(other) = commands.get(name)
                && other.plugin != plugin
            {
                let owner = other
                    .plugin
                    .map_or_else(|| "the server".to_owned(), |i| self.plugin_name(i));
                drop(commands);
                self.log(
                    "WARN",
                    &format!(
                        "[{}] /{name} is taken by {owner}: use /{prefix}:{}",
                        plugin.map_or_else(|| "Plugins".to_owned(), |i| self.plugin_name(i)),
                        cmd.spec.name
                    ),
                );
                commands = self.commands.borrow_mut();
                continue;
            }
            commands.insert(name.clone(), cmd.clone());
        }
        drop(commands);
        if self.names_taken.get() && plugin.is_some() {
            self.log(
                "WARN",
                &format!(
                    "[{}] /{} was added after the server built its command tree: players can type it after a restart",
                    self.plugin_name(plugin.unwrap_or_default()),
                    cmd.spec.name
                ),
            );
        }
        Ok(())
    }

    fn lookup(&self, line: &str) -> Option<Command> {
        let name = line.split_whitespace().next()?.to_ascii_lowercase();
        self.commands.borrow().get(&name).cloned()
    }

    /// Runs a plugin's (or the host's) command `line` for `sender`, from
    /// plugin `from` when a plugin asked; waits for the next tick when
    /// the plugin it belongs to is busy.
    fn run_command(&self, from: Option<usize>, sender: CommandSender, line: &str) -> bool {
        let Some(cmd) = self.lookup(line) else {
            return false;
        };
        let mut words = line.split_whitespace();
        let label = words.next().unwrap_or_default().to_ascii_lowercase();
        let args: Vec<String> = words.map(str::to_owned).collect();
        if let Some(node) = &cmd.spec.permission
            && let CommandSender::Player(p) = &sender
            && !self.permissions.borrow().has(p, node)
        {
            let message = cmd
                .spec
                .permission_message
                .as_deref()
                .map_or_else(|| NO_PERMISSION.to_owned(), |m| color::translate('&', m));
            self.tell(&sender, &message);
            return true;
        }
        let Some(i) = cmd.plugin else {
            self.builtin(&sender, &cmd.spec.name, &args);
            return true;
        };
        let Some(p) = self.plugin(i) else {
            return false;
        };
        if !p.enabled.get() {
            self.tell(&sender, &format!("§c{} is disabled.", p.description.name));
            return true;
        }
        if p.busy.get() || from == Some(i) {
            self.deferred.borrow_mut().push(Deferred::Command {
                sender,
                line: line.to_owned(),
            });
            return true;
        }
        let payload = Value::map()
            .with("command", &cmd.spec.name)
            .with("label", label.rsplit(':').next().unwrap_or(&label))
            .with("args", args)
            .with("sender", sender.to_wire());
        let r = self.send(i, "command", &payload);
        if let Err(e) = &r {
            self.tell(
                &sender,
                "§cAn internal error occurred while attempting to perform this command",
            );
            self.log("ERROR", &format!("[{}] /{label}: {e}", p.description.name));
        }
        true
    }

    fn tell(&self, sender: &CommandSender, message: &str) {
        let Ok(b) = self.backend() else {
            return;
        };
        match sender {
            CommandSender::Player(p) => {
                b.tell(p.uuid, message);
            }
            CommandSender::Console => b.log("INFO", &color::strip(message)),
        }
    }

    /// `/plugins` and `/version`.
    fn builtin(&self, sender: &CommandSender, name: &str, args: &[String]) {
        let plugins = self.plugins.borrow().clone();
        match name {
            "plugins" if args.first().is_some_and(|a| a == "reload") => {
                if sender.is_op() {
                    self.reload_requested.set(true);
                    self.tell(sender, "§eReloading the plugins at the next tick...");
                } else {
                    self.tell(sender, NO_PERMISSION);
                }
            }
            "plugins" => {
                let mut names: Vec<String> = plugins
                    .iter()
                    .map(|p| {
                        let c = if p.enabled.get() { "§a" } else { "§c" };
                        format!("{c}{}", p.description.name)
                    })
                    .collect();
                names.extend(self.failed.borrow().iter().map(|(f, _)| format!("§4{f}")));
                self.tell(
                    sender,
                    &format!("§fPlugins ({}): {}", plugins.len(), names.join("§f, ")),
                );
            }
            _ => match args.first() {
                None => {
                    let version = self.backend().map(|b| b.version()).unwrap_or_default();
                    self.tell(
                        sender,
                        &format!("§fThis server is running Horizon {version} (Minecraft 26.3), plugin ABI {ABI_VERSION}"),
                    );
                }
                Some(n) => match plugins
                    .iter()
                    .find(|p| p.description.name.eq_ignore_ascii_case(n))
                {
                    Some(p) => {
                        let d = &p.description;
                        self.tell(sender, &format!("§a{} §fversion §a{}", d.name, d.version));
                        if !d.description.is_empty() {
                            self.tell(sender, &d.description);
                        }
                        if !d.website.is_empty() {
                            self.tell(sender, &format!("Website: §a{}", d.website));
                        }
                        if !d.authors.is_empty() {
                            self.tell(sender, &format!("Authors: §a{}", d.authors.join("§f, §a")));
                        }
                    }
                    None => self.tell(
                        sender,
                        "§cThis server is not running any plugin by that name.",
                    ),
                },
            },
        }
    }

    /// A console command now, or at the next tick inside a player's action.
    fn console(&self, from: Option<usize>, line: &str) -> Result<bool, String> {
        let line = line.trim().trim_start_matches('/');
        if self.lookup(line).is_some() {
            return Ok(self.run_command(from, CommandSender::Console, line));
        }
        let b = self.backend()?;
        if b.can_run_commands() {
            let ok = b.console_command(line);
            if !ok {
                let who = from.map_or_else(|| "Plugins".to_owned(), |i| self.plugin_name(i));
                self.log("WARN", &format!("[{who}] `{line}` failed"));
            }
            Ok(ok)
        } else {
            self.deferred
                .borrow_mut()
                .push(Deferred::Console(line.to_owned()));
            Ok(true)
        }
    }

    fn run_deferred(&self) {
        // What these run may defer more; that waits for the next tick.
        let work = std::mem::take(&mut *self.deferred.borrow_mut());
        for d in work {
            match d {
                Deferred::Console(line) => {
                    if let Ok(b) = self.backend()
                        && !b.console_command(&line)
                    {
                        self.log("WARN", &format!("[Plugins] `{line}` failed"));
                    }
                }
                Deferred::Command { sender, line } => {
                    self.run_command(None, sender, &line);
                }
            }
        }
    }

    fn find_player(&self, uuid: Uuid) -> Result<Player, String> {
        let b = self.backend()?;
        if let Some(p) = b.acting_player().filter(|p| p.uuid == uuid) {
            return Ok(p);
        }
        b.players()
            .into_iter()
            .find(|p| p.uuid == uuid)
            .ok_or_else(|| format!("no player {uuid} on the server"))
    }

    fn acting(&self, uuid: Uuid) -> Result<&mut dyn Backend, String> {
        let b = self.backend()?;
        if b.acting_player().is_some_and(|p| p.uuid == uuid) {
            Ok(b)
        } else {
            Err("only inside that player's own event or command".to_owned())
        }
    }

    /// Players joining, leaving, moving, dying... since the last tick.
    fn watch_players(&self) {
        let Ok(b) = self.backend() else {
            return;
        };
        let now = b.players();
        let before = std::mem::replace(&mut *self.seen.borrow_mut(), now.clone());
        for p in &before {
            if !now.iter().any(|n| n.uuid == p.uuid) {
                self.fire(
                    "player_quit",
                    Value::map().with("player", p.to_wire()),
                    false,
                );
            }
        }
        for p in &now {
            let Some(old) = before.iter().find(|o| o.uuid == p.uuid) else {
                self.fire(
                    "player_join",
                    Value::map().with("player", p.to_wire()),
                    false,
                );
                continue;
            };
            let player = p.to_wire();
            if old.location.world != p.location.world {
                self.fire(
                    "player_changed_world",
                    Value::map()
                        .with("player", player.clone())
                        .with("from", &old.location.world),
                    false,
                );
            } else if old.location.distance_squared(&p.location) > 1e-8 {
                let (_, cancelled) = self.fire(
                    "player_move",
                    Value::map()
                        .with("player", player.clone())
                        .with("from", old.location.to_wire())
                        .with("to", p.location.to_wire()),
                    false,
                );
                if cancelled {
                    // Back where they were, keeping where they look.
                    let l = &old.location;
                    let _ = self.console(
                        None,
                        &format!(
                            "execute in {} run tp {} {} {} {}",
                            l.world,
                            p.target(),
                            l.x,
                            l.y,
                            l.z
                        ),
                    );
                    if let Some(seen) = self.seen.borrow_mut().iter_mut().find(|s| s.uuid == p.uuid)
                    {
                        seen.location = old.location.clone();
                    }
                }
            }
            if old.health > 0.0 && p.health <= 0.0 {
                self.fire(
                    "player_death",
                    Value::map().with("player", player.clone()),
                    false,
                );
            } else if old.health <= 0.0 && p.health > 0.0 {
                self.fire(
                    "player_respawn",
                    Value::map().with("player", player.clone()),
                    false,
                );
            }
            if old.game_mode != p.game_mode {
                self.fire(
                    "player_game_mode_change",
                    Value::map()
                        .with("player", player)
                        .with("previous", old.game_mode.to_wire()),
                    false,
                );
            }
        }
    }

    /// A plugin's call.
    fn call(&self, from: usize, method: &str, args: &Value) -> Result<Value, String> {
        let uuid = || {
            args.get("uuid")
                .and_then(Uuid::from_wire)
                .ok_or_else(|| "`uuid` is missing".to_owned())
        };
        let text = |k: &str| args.get(k).and_then(Value::to_text).unwrap_or_default();
        let amount = || {
            args.get("amount")
                .and_then(Value::as_i64)
                .map_or(1, |n| n.clamp(0, i64::from(u32::MAX)) as u32)
        };
        Ok(match method {
            "log" => {
                let level = match text("level").as_str() {
                    l @ ("WARN" | "ERROR" | "DEBUG") => l.to_owned(),
                    _ => "INFO".to_owned(),
                };
                let name = self
                    .plugin(from)
                    .map(|p| {
                        p.description
                            .prefix
                            .clone()
                            .unwrap_or_else(|| p.description.name.clone())
                    })
                    .unwrap_or_default();
                self.log(&level, &format!("[{name}] {}", text("message")));
                Value::Null
            }
            "server_info" => Value::map()
                .with("name", "Horizon")
                .with(
                    "version",
                    self.backend().map(|b| b.version()).unwrap_or_default(),
                )
                .with("minecraft_version", "26.3")
                .with("tick", self.tick.get()),
            "register_listener" => {
                let event = text("event");
                let listener = Listener {
                    plugin: from,
                    id: args.get("id").and_then(Value::as_i64).unwrap_or(0) as u64,
                    priority: EventPriority::parse(&text("priority")).unwrap_or_default(),
                    ignore_cancelled: args
                        .get("ignore_cancelled")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                };
                let mut listeners = self.listeners.borrow_mut();
                let list = listeners.entry(event).or_default();
                // After those of its priority already there: plugins' order.
                let at = list.partition_point(|l| l.priority <= listener.priority);
                list.insert(at, listener);
                Value::Null
            }
            "register_command" => {
                self.add_command(Some(from), CommandSpec::from_value(args))?;
                Value::Null
            }
            "players" => self.backend()?.players().to_wire(),
            "send_message" => Value::Bool(self.backend()?.tell(uuid()?, &text("message"))),
            "action_bar" => Value::Bool(self.backend()?.action_bar(uuid()?, &text("message"))),
            "day_time" => self
                .backend()?
                .day_time()
                .map_or(Value::Null, Value::from),
            "note_blocks_inert" => {
                let inert = args.get("inert").and_then(Value::as_bool).unwrap_or(true);
                self.backend()?.set_note_blocks_inert(inert);
                Value::Null
            }
            "resource_pack" => {
                let pack = ResourcePack {
                    id: args.get("id").and_then(Uuid::from_wire).unwrap_or_default(),
                    url: text("url"),
                    sha1: text("hash"),
                    required: args
                        .get("required")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    prompt: args.get("prompt").and_then(Value::to_text),
                };
                if pack.url.is_empty() || !(pack.sha1.is_empty() || is_sha1(&pack.sha1)) {
                    return Err("a resource pack needs a url and a 40-digit hex sha1".into());
                }
                Value::Bool(self.backend()?.resource_pack(uuid()?, &pack))
            }
            "broadcast" => Value::from(self.backend()?.broadcast(&text("message"))),
            "dispatch_command" => Value::Bool(self.console(Some(from), &text("line"))?),
            "teleport" => {
                let p = self.find_player(uuid()?)?;
                let l = args
                    .get("location")
                    .and_then(Location::from_wire)
                    .ok_or("`location` is missing")?;
                let world = horizon_plugin_api::types::namespaced(&l.world);
                Value::Bool(self.console(
                    Some(from),
                    &format!(
                        "execute in {world} run tp {} {} {} {} {} {}",
                        p.target(),
                        l.x,
                        l.y,
                        l.z,
                        l.yaw,
                        l.pitch
                    ),
                )?)
            }
            "kick" => {
                let p = self.find_player(uuid()?)?;
                let reason = color::strip(&text("reason"));
                Value::Bool(self.console(
                    Some(from),
                    format!("kick {} {reason}", p.target()).trim_end(),
                )?)
            }
            "has_permission" => {
                let p = self.find_player(uuid()?)?;
                Value::Bool(self.permissions.borrow().has(&p, &text("permission")))
            }
            "give_item" => {
                let id = uuid()?;
                let item = text("item");
                let n = amount();
                match self.acting(id) {
                    Ok(b) => Value::from(b.give_items(&item, n)),
                    Err(_) => {
                        let p = self.find_player(id)?;
                        if self.console(Some(from), &format!("give {} {item} {n}", p.target()))? {
                            Value::from(n)
                        } else {
                            Value::Int(0)
                        }
                    }
                }
            }
            "take_item" => Value::from(self.acting(uuid()?)?.take_items(&text("item"), amount())),
            "count_item" => Value::from(self.acting(uuid()?)?.count_items(&text("item"))),
            "held_item" => self.acting(uuid()?)?.held_item().to_wire(),
            "inventory" => self.backend()?.inventory(uuid()?).to_wire(),
            "open_menu" => {
                let id = uuid()?;
                let rows = args
                    .get("rows")
                    .and_then(Value::as_i64)
                    .unwrap_or(3)
                    .clamp(1, 6) as u8;
                let mut menu = Menu::new(rows, &text("title"));
                let items = args
                    .get("items")
                    .and_then(Vec::<Option<ItemStack>>::from_wire)
                    .unwrap_or_default();
                for (slot, item) in items.into_iter().enumerate() {
                    menu.set(slot, item);
                }
                self.backend()?.open_menu(id, &menu).to_wire()
            }
            "set_menu_items" => {
                let items = args
                    .get("items")
                    .and_then(Vec::<Option<ItemStack>>::from_wire)
                    .unwrap_or_default();
                self.backend()?.set_menu_items(uuid()?, &items);
                Value::Null
            }
            "close_menu" => {
                self.backend()?.close_menu(uuid()?);
                Value::Null
            }
            "player_command" => {
                let p = self.find_player(uuid()?)?;
                let line = text("line");
                let line = line.trim().trim_start_matches('/');
                if self.lookup(line).is_some() {
                    Value::Bool(self.run_command(Some(from), CommandSender::Player(p), line))
                } else {
                    Value::Bool(self.console(
                        Some(from),
                        &format!("execute as {} at @s run {line}", p.target()),
                    )?)
                }
            }
            "block_exists" => self.backend()?.block_exists(&text("id")).to_wire(),
            "entities" => {
                let kind = args.get("type").and_then(Value::to_text);
                self.backend()?
                    .entities(&text("world"), kind.as_deref())
                    .to_wire()
            }
            "get_block" => {
                let pos = BlockPos::from_wire(args).ok_or("a block position is missing")?;
                self.backend()?.block(&pos).to_wire()
            }
            "get_blocks" => {
                let corner = |k: &str| -> Result<[i32; 3], String> {
                    let l = args.get(k).and_then(Value::as_list).ok_or(format!("`{k}` is missing"))?;
                    let n = |i: usize| l.get(i).and_then(Value::as_i64).map(|v| v as i32);
                    Ok([n(0).unwrap_or(0), n(1).unwrap_or(0), n(2).unwrap_or(0)])
                };
                let (lo, hi) = (corner("min")?, corner("max")?);
                let size: Vec<i64> = (0..3).map(|i| i64::from(hi[i]) - i64::from(lo[i]) + 1).collect();
                if size.iter().any(|&s| s <= 0) || size.iter().product::<i64>() > 2_000_000 {
                    return Err("a region is 1 to 2 000 000 blocks".to_owned());
                }
                let world = text("world");
                let b = self.backend()?;
                let mut palette: Vec<String> = Vec::new();
                let mut index: HashMap<String, u32> = HashMap::new();
                let mut data = Vec::with_capacity(size.iter().product::<i64>() as usize);
                for y in lo[1]..=hi[1] {
                    for z in lo[2]..=hi[2] {
                        for x in lo[0]..=hi[0] {
                            let state = b
                                .block(&BlockPos::new(&world, x, y, z))
                                .map(|bl| bl.state_string())
                                .unwrap_or_default();
                            let next = palette.len() as u32;
                            let i = *index.entry(state.clone()).or_insert_with(|| {
                                palette.push(state);
                                next
                            });
                            data.push(Value::from(i));
                        }
                    }
                }
                Value::map()
                    .with("palette", palette)
                    .with("data", Value::List(data))
            }
            "plugins" => Value::List(
                self.plugins
                    .borrow()
                    .iter()
                    .map(|p| {
                        Value::map()
                            .with("description", p.description.to_value())
                            .with("enabled", p.enabled.get())
                    })
                    .collect(),
            ),
            "plugin_message" => {
                let target = text("target");
                let i = self
                    .plugins
                    .borrow()
                    .iter()
                    .position(|p| p.description.name.eq_ignore_ascii_case(&target))
                    .ok_or_else(|| format!("no plugin {target}"))?;
                if i == from {
                    return Err("a plugin can't message itself".to_owned());
                }
                if !self.plugin(i).is_some_and(|p| p.enabled.get()) {
                    return Err(format!("{target} is not enabled"));
                }
                self.send(
                    i,
                    "message",
                    &Value::map()
                        .with("from", self.plugin_name(from))
                        .with("channel", text("channel"))
                        .with("message", args.get("message").cloned().unwrap_or_default()),
                )?
            }
            "call_event" => {
                let (data, cancelled) = self.fire(
                    &text("event"),
                    args.get("data").cloned().unwrap_or_default(),
                    false,
                );
                Value::map().with("data", data).with("cancelled", cancelled)
            }
            other => return Err(format!("unknown method `{other}`")),
        })
    }
}

#[cfg(test)]
mod tests;
