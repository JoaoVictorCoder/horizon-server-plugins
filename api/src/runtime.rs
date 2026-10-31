//! A plugin's side of the ABI: the [`Plugin`] trait, the [`Ctx`] its code
//! registers listeners, commands and tasks with, and the entry point
//! [`horizon_plugin!`](crate::horizon_plugin) exports.
//!
//! Every callback gets the plugin itself (`&mut P`) and its `Ctx`, so
//! listeners and tasks reach the plugin's state with no `Rc`. Callbacks run
//! on the server's thread; a panic in one is caught, logged, and the
//! server goes on.

use crate::abi::{self, ABI_VERSION, HostApi, PluginApi, RawBuf, RawStr};
use crate::config::Config;
use crate::description::{CommandSpec, PluginDescription};
use crate::event::{Event, EventPriority};
use crate::server::{Logger, Server, call, server, with_host};
use crate::types::{CommandSender, Wire};
use crate::value::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// A server plugin. Construct it in [`horizon_plugin!`](crate::horizon_plugin);
/// register what it does in `on_enable`.
pub trait Plugin: Sized + 'static {
    /// The plugin was loaded, before any plugin is enabled.
    fn on_load(&mut self, ctx: &mut Ctx<Self>) {
        let _ = ctx;
    }
    /// The plugin is enabled: register listeners, commands and tasks.
    fn on_enable(&mut self, ctx: &mut Ctx<Self>) {
        let _ = ctx;
    }
    /// The plugin is disabled (the server stops): save what it keeps. Its
    /// tasks are cancelled after this.
    fn on_disable(&mut self, ctx: &mut Ctx<Self>) {
        let _ = ctx;
    }
}

/// A scheduled task's id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TaskId(pub u64);

/// A message from another plugin ([`Server::message_plugin`]).
#[derive(Clone, Debug, PartialEq)]
pub struct PluginMessage {
    /// The plugin that sent it.
    pub from: String,
    pub channel: String,
    pub message: Value,
}

type Handler<P> = Box<dyn FnMut(&mut P, &mut Ctx<P>, &Value, bool) -> Option<(Value, bool)>>;
type Executor<P> = Box<dyn FnMut(&mut P, &mut Ctx<P>, &CommandSender, &str, &[String]) -> bool>;
type Task<P> = Box<dyn FnMut(&mut P, &mut Ctx<P>)>;
type Inbox<P> = Arc<Mutex<Vec<Box<dyn FnOnce(&mut P, &mut Ctx<P>) + Send>>>>;
type MessageHandler<P> = Box<dyn FnMut(&mut P, &mut Ctx<P>, &PluginMessage) -> Value>;
type Completer<P> = Box<dyn FnMut(&mut P, &mut Ctx<P>, &CommandSender, &str, &[String]) -> Vec<String>>;

enum Pending<P> {
    Listener {
        id: u64,
        event: &'static str,
        priority: EventPriority,
        ignore_cancelled: bool,
        handler: Handler<P>,
    },
    Command(CommandSpec, Executor<P>),
    Executor(String, Executor<P>),
    Task {
        id: u64,
        delay: u64,
        period: Option<u64>,
        task: Task<P>,
    },
    Cancel(u64),
    CancelAll,
    Message(String, MessageHandler<P>),
    Completer(String, Completer<P>),
}

/// What a plugin's code registers with and reaches the server through.
pub struct Ctx<P> {
    description: PluginDescription,
    data_folder: PathBuf,
    config: Option<Config>,
    default_config: Option<String>,
    pending: Vec<Pending<P>>,
    next_id: u64,
    tick: u64,
    inbox: Inbox<P>,
    current_task: Option<TaskId>,
}

impl<P: Plugin> Ctx<P> {
    fn new(description: PluginDescription) -> Self {
        Ctx {
            description,
            data_folder: PathBuf::new(),
            config: None,
            default_config: None,
            pending: Vec::new(),
            next_id: 1,
            tick: 0,
            inbox: Arc::new(Mutex::new(Vec::new())),
            current_task: None,
        }
    }

    fn id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn server(&self) -> Server {
        server()
    }
    pub fn logger(&self) -> Logger {
        server().logger()
    }
    pub fn description(&self) -> &PluginDescription {
        &self.description
    }
    /// `plugins/<name>/`, where the plugin keeps its files. Made on first use.
    pub fn data_folder(&self) -> &Path {
        let _ = std::fs::create_dir_all(&self.data_folder);
        &self.data_folder
    }
    /// The server tick the plugin last saw.
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// Listens to `E` at normal priority.
    pub fn on<E: Event>(&mut self, handler: impl FnMut(&mut P, &mut Ctx<P>, &mut E) + 'static) {
        self.listen(EventPriority::Normal, false, handler);
    }

    /// Listens to `E` at `priority`; with `ignore_cancelled`, not once an
    /// earlier listener cancelled it.
    pub fn listen<E: Event>(
        &mut self,
        priority: EventPriority,
        ignore_cancelled: bool,
        mut handler: impl FnMut(&mut P, &mut Ctx<P>, &mut E) + 'static,
    ) {
        let id = self.id();
        let handler: Handler<P> = Box::new(move |plugin, ctx, data, cancelled| {
            let mut event = E::from_data(data)?;
            event.set_cancelled_flag(cancelled);
            handler(plugin, ctx, &mut event);
            Some((event.to_data(), event.cancelled()))
        });
        self.pending.push(Pending::Listener {
            id,
            event: E::NAME,
            priority,
            ignore_cancelled,
            handler,
        });
    }

    /// Adds a command and what runs it: `executor(plugin, ctx, sender,
    /// label, args)` returns false to show the usage.
    pub fn command(
        &mut self,
        spec: CommandSpec,
        executor: impl FnMut(&mut P, &mut Ctx<P>, &CommandSender, &str, &[String]) -> bool + 'static,
    ) {
        self.pending
            .push(Pending::Command(spec, Box::new(executor)));
    }

    /// What runs a command `plugin.yml` declares (`getCommand(name).setExecutor`).
    pub fn set_executor(
        &mut self,
        name: &str,
        executor: impl FnMut(&mut P, &mut Ctx<P>, &CommandSender, &str, &[String]) -> bool + 'static,
    ) {
        self.pending.push(Pending::Executor(
            name.to_ascii_lowercase(),
            Box::new(executor),
        ));
    }

    /// What a command's arguments complete to as a player types them
    /// (`TabCompleter`): `completer(plugin, ctx, sender, label, args)`, the
    /// last argument the one being typed (maybe empty). The server keeps
    /// those that start with it. Without one, a command completes to the
    /// online players' names.
    pub fn set_tab_completer(
        &mut self,
        name: &str,
        completer: impl FnMut(&mut P, &mut Ctx<P>, &CommandSender, &str, &[String]) -> Vec<String> + 'static,
    ) {
        self.pending
            .push(Pending::Completer(name.to_ascii_lowercase(), Box::new(completer)));
    }

    /// Answers other plugins' messages on `channel`.
    pub fn on_message(
        &mut self,
        channel: &str,
        handler: impl FnMut(&mut P, &mut Ctx<P>, &PluginMessage) -> Value + 'static,
    ) {
        self.pending
            .push(Pending::Message(channel.to_owned(), Box::new(handler)));
    }

    fn schedule(&mut self, delay: u64, period: Option<u64>, task: Task<P>) -> TaskId {
        let id = self.id();
        self.pending.push(Pending::Task {
            id,
            delay: delay.max(1),
            period: period.map(|p| p.max(1)),
            task,
        });
        TaskId(id)
    }

    /// Runs `task` at the next tick (`runTask`).
    pub fn run_task(&mut self, task: impl FnOnce(&mut P, &mut Ctx<P>) + 'static) -> TaskId {
        self.run_later(1, task)
    }

    /// Runs `task` in `delay` ticks (20 a second; `runTaskLater`).
    pub fn run_later(
        &mut self,
        delay: u64,
        task: impl FnOnce(&mut P, &mut Ctx<P>) + 'static,
    ) -> TaskId {
        let mut task = Some(task);
        self.schedule(
            delay,
            None,
            Box::new(move |p, ctx| {
                if let Some(t) = task.take() {
                    t(p, ctx);
                }
            }),
        )
    }

    /// Runs `task` in `delay` ticks, then every `period` ticks until
    /// cancelled (`runTaskTimer`); `ctx.current_task()` is its id inside.
    pub fn run_timer(
        &mut self,
        delay: u64,
        period: u64,
        task: impl FnMut(&mut P, &mut Ctx<P>) + 'static,
    ) -> TaskId {
        self.schedule(delay, Some(period), Box::new(task))
    }

    /// The task running now, inside a task.
    pub fn current_task(&self) -> Option<TaskId> {
        self.current_task
    }

    pub fn cancel_task(&mut self, id: TaskId) {
        self.pending.push(Pending::Cancel(id.0));
    }

    pub fn cancel_all_tasks(&mut self) {
        self.pending.push(Pending::CancelAll);
    }

    /// Runs `work` on a thread of its own (`runTaskAsynchronously`); it
    /// comes back to the server's thread through its [`SyncHandle`].
    pub fn run_async(&mut self, work: impl FnOnce(SyncHandle<P>) + Send + 'static) {
        let handle = self.sync_handle();
        let name = format!("{} async", self.description.name);
        let spawned = std::thread::Builder::new()
            .name(name)
            .spawn(move || work(handle));
        if let Err(e) = spawned {
            self.logger()
                .severe(&format!("could not start a thread: {e}"));
        }
    }

    /// A handle any thread can send work back to the server's thread with.
    pub fn sync_handle(&self) -> SyncHandle<P> {
        SyncHandle {
            inbox: Arc::clone(&self.inbox),
        }
    }

    /// The plugin's `config.yml`, read on first use, with the defaults
    /// [`save_default_config`](Self::save_default_config) gave.
    pub fn config(&mut self) -> &mut Config {
        if self.config.is_none() {
            let path = self.data_folder.join("config.yml");
            let mut config = Config::load(&path).unwrap_or_else(|e| {
                server().logger().severe(&format!("config.yml: {e}"));
                Config::new(&path)
            });
            if let Some(defaults) = &self.default_config
                && let Err(e) = config.set_defaults_yaml(defaults)
            {
                server()
                    .logger()
                    .severe(&format!("the default config.yml: {e}"));
            }
            self.config = Some(config);
        }
        self.config.get_or_insert_with(Config::default)
    }

    /// Ships `defaults` (`include_str!("../config.yml")`) as the plugin's
    /// default `config.yml`: written to the data folder when there is none,
    /// and the fallback for every value.
    pub fn save_default_config(&mut self, defaults: &str) {
        let path = self.data_folder.join("config.yml");
        if !path.exists() {
            let _ = std::fs::create_dir_all(&self.data_folder);
            if let Err(e) = std::fs::write(&path, defaults) {
                self.logger()
                    .severe(&format!("could not write {}: {e}", path.display()));
            }
        }
        self.default_config = Some(defaults.to_owned());
        self.config = None;
    }

    /// Reads `config.yml` again.
    pub fn reload_config(&mut self) {
        self.config = None;
        let _ = self.config();
    }

    /// Saves `config.yml`.
    pub fn save_config(&mut self) {
        if let Err(e) = self.config().save() {
            self.logger().severe(&e);
        }
    }
}

/// Sends work back to the server's thread from any thread, where it runs
/// at the next tick with the plugin and its `Ctx`.
pub struct SyncHandle<P> {
    inbox: Inbox<P>,
}

impl<P> Clone for SyncHandle<P> {
    fn clone(&self) -> Self {
        SyncHandle {
            inbox: Arc::clone(&self.inbox),
        }
    }
}

impl<P> SyncHandle<P> {
    pub fn run(&self, work: impl FnOnce(&mut P, &mut Ctx<P>) + Send + 'static) {
        if let Ok(mut inbox) = self.inbox.lock() {
            inbox.push(Box::new(work));
        }
    }
}

struct ScheduledTask<P> {
    at: u64,
    period: Option<u64>,
    task: Task<P>,
}

struct Runtime<P: Plugin> {
    plugin: P,
    ctx: Ctx<P>,
    listeners: HashMap<u64, Handler<P>>,
    commands: HashMap<String, Executor<P>>,
    aliases: HashMap<String, String>,
    messages: HashMap<String, MessageHandler<P>>,
    completers: HashMap<String, Completer<P>>,
    tasks: BTreeMap<u64, ScheduledTask<P>>,
    cancelled: HashSet<u64>,
}

impl<P: Plugin> Runtime<P> {
    /// Applies what the last callback registered.
    fn flush(&mut self) {
        while !self.ctx.pending.is_empty() {
            for p in std::mem::take(&mut self.ctx.pending) {
                match p {
                    Pending::Listener {
                        id,
                        event,
                        priority,
                        ignore_cancelled,
                        handler,
                    } => {
                        self.listeners.insert(id, handler);
                        let _ = call(
                            "register_listener",
                            Value::map()
                                .with("id", id)
                                .with("event", event)
                                .with("priority", priority.name())
                                .with("ignore_cancelled", ignore_cancelled),
                        );
                    }
                    Pending::Command(spec, executor) => {
                        self.register_command(&spec);
                        self.commands.insert(spec.name.clone(), executor);
                        if !self
                            .ctx
                            .description
                            .commands
                            .iter()
                            .any(|c| c.name == spec.name)
                        {
                            self.ctx.description.commands.push(spec);
                        }
                    }
                    Pending::Executor(name, executor) => {
                        let name = self.aliases.get(&name).cloned().unwrap_or(name);
                        if self.ctx.description.commands.iter().any(|c| c.name == name) {
                            self.commands.insert(name, executor);
                        } else {
                            self.ctx.logger().warning(&format!(
                                "set_executor(\"{name}\"): plugin.yml declares no such command"
                            ));
                        }
                    }
                    Pending::Task {
                        id,
                        delay,
                        period,
                        task,
                    } => {
                        self.tasks.insert(
                            id,
                            ScheduledTask {
                                at: self.ctx.tick + delay,
                                period,
                                task,
                            },
                        );
                    }
                    Pending::Cancel(id) => {
                        self.tasks.remove(&id);
                        self.cancelled.insert(id);
                    }
                    Pending::CancelAll => {
                        self.cancelled.extend(self.tasks.keys().copied());
                        self.tasks.clear();
                        if let Some(id) = self.ctx.current_task {
                            self.cancelled.insert(id.0);
                        }
                    }
                    Pending::Message(channel, handler) => {
                        self.messages.insert(channel, handler);
                    }
                    Pending::Completer(name, completer) => {
                        let name = self.aliases.get(&name).cloned().unwrap_or(name);
                        self.completers.insert(name, completer);
                    }
                }
            }
        }
    }

    fn register_command(&mut self, spec: &CommandSpec) {
        for alias in &spec.aliases {
            self.aliases.insert(alias.clone(), spec.name.clone());
        }
        if let Err(e) = call("register_command", spec.to_value()) {
            self.ctx
                .logger()
                .warning(&format!("command /{}: {e}", spec.name));
        }
    }

    /// Runs `f` with the plugin and its ctx, catching a panic.
    fn guarded<R>(&mut self, what: &str, f: impl FnOnce(&mut P, &mut Ctx<P>) -> R) -> Option<R> {
        let (plugin, ctx) = (&mut self.plugin, &mut self.ctx);
        let out = catch_unwind(AssertUnwindSafe(|| f(plugin, ctx)));
        if out.is_err() {
            self.ctx.logger().severe(&format!("{what} panicked"));
        }
        self.flush();
        out.ok()
    }

    fn handle(&mut self, kind: &str, payload: &Value) -> Result<Value, String> {
        match kind {
            "describe" => Ok(self.ctx.description.to_value()),
            "load" => {
                if let Some(dir) = payload.get("data_folder").and_then(Value::as_str) {
                    self.ctx.data_folder = PathBuf::from(dir);
                }
                self.guarded("on_load", |p, ctx| p.on_load(ctx));
                Ok(Value::Null)
            }
            "enable" => {
                for spec in self.ctx.description.commands.clone() {
                    self.register_command(&spec);
                }
                self.guarded("on_enable", |p, ctx| p.on_enable(ctx));
                Ok(Value::Null)
            }
            "disable" => {
                self.guarded("on_disable", |p, ctx| p.on_disable(ctx));
                self.tasks.clear();
                self.listeners.clear();
                Ok(Value::Null)
            }
            "event" => {
                let id = payload.get("listener").and_then(Value::as_i64).unwrap_or(0) as u64;
                let data = payload.get("data").cloned().unwrap_or_default();
                let cancelled = payload
                    .get("cancelled")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let Some(mut handler) = self.listeners.remove(&id) else {
                    return Err(format!("no listener {id}"));
                };
                let event = payload
                    .get("event")
                    .and_then(Value::to_text)
                    .unwrap_or_default();
                let out = self
                    .guarded(&format!("a listener of {event}"), |p, ctx| {
                        handler(p, ctx, &data, cancelled)
                    })
                    .flatten();
                self.listeners.entry(id).or_insert(handler);
                Ok(match out {
                    Some((data, cancelled)) => {
                        Value::map().with("data", data).with("cancelled", cancelled)
                    }
                    None => Value::map().with("data", data).with("cancelled", cancelled),
                })
            }
            "command" => {
                let name = payload
                    .get("command")
                    .and_then(Value::to_text)
                    .unwrap_or_default();
                let label = payload
                    .get("label")
                    .and_then(Value::to_text)
                    .unwrap_or_else(|| name.clone());
                let args = payload
                    .get("args")
                    .map(Value::to_strings)
                    .unwrap_or_default();
                let sender = payload
                    .get("sender")
                    .and_then(CommandSender::from_wire)
                    .unwrap_or(CommandSender::Console);
                let name = self.aliases.get(&name).cloned().unwrap_or(name);
                let Some(mut executor) = self.commands.remove(&name) else {
                    sender.send_message(&format!(
                        "§cUnknown command. /{label} is declared by {} but nothing runs it.",
                        self.ctx.description.name
                    ));
                    return Ok(Value::Bool(true));
                };
                let ok = self.guarded(&format!("/{name}"), |p, ctx| {
                    executor(p, ctx, &sender, &label, &args)
                });
                self.commands.entry(name.clone()).or_insert(executor);
                match ok {
                    Some(true) => {}
                    Some(false) => {
                        let usage = self
                            .ctx
                            .description
                            .commands
                            .iter()
                            .find(|c| c.name == name)
                            .map(|c| c.usage.replace("<command>", &label))
                            .unwrap_or_else(|| format!("/{label}"));
                        sender.send_message(&format!("§c{usage}"));
                    }
                    None => sender.send_message(
                        "§cAn internal error occurred while attempting to perform this command",
                    ),
                }
                Ok(Value::Bool(true))
            }
            "tab" => {
                let name = payload.get("command").and_then(Value::to_text).unwrap_or_default();
                let label = payload.get("label").and_then(Value::to_text).unwrap_or_else(|| name.clone());
                let args = payload.get("args").map(Value::to_strings).unwrap_or_default();
                let sender = payload
                    .get("sender")
                    .and_then(CommandSender::from_wire)
                    .unwrap_or(CommandSender::Console);
                let name = self.aliases.get(&name).cloned().unwrap_or(name);
                let Some(mut completer) = self.completers.remove(&name) else {
                    // No completer: the host offers the players' names.
                    return Ok(Value::Null);
                };
                let out = self.guarded(&format!("completing /{name}"), |p, ctx| {
                    completer(p, ctx, &sender, &label, &args)
                });
                self.completers.entry(name).or_insert(completer);
                Ok(out.map_or(Value::Null, Value::from))
            }
            "message" => {
                let msg = PluginMessage {
                    from: payload
                        .get("from")
                        .and_then(Value::to_text)
                        .unwrap_or_default(),
                    channel: payload
                        .get("channel")
                        .and_then(Value::to_text)
                        .unwrap_or_default(),
                    message: payload.get("message").cloned().unwrap_or_default(),
                };
                let Some(mut handler) = self.messages.remove(&msg.channel) else {
                    return Err(format!(
                        "{} has no channel `{}`",
                        self.ctx.description.name, msg.channel
                    ));
                };
                let out = self.guarded(&format!("channel {}", msg.channel), |p, ctx| {
                    handler(p, ctx, &msg)
                });
                self.messages.entry(msg.channel.clone()).or_insert(handler);
                out.ok_or_else(|| format!("channel `{}` panicked", msg.channel))
            }
            "tick" => {
                if let Some(t) = payload.get("tick").and_then(Value::as_i64) {
                    self.ctx.tick = t.max(0) as u64;
                }
                self.tick();
                Ok(Value::Null)
            }
            other => Err(format!("unknown message `{other}`")),
        }
    }

    fn tick(&mut self) {
        let work: Vec<_> = match self.ctx.inbox.lock() {
            Ok(mut inbox) => std::mem::take(&mut *inbox),
            Err(_) => Vec::new(),
        };
        for w in work {
            self.guarded("a task sent back from another thread", |p, ctx| w(p, ctx));
        }
        let now = self.ctx.tick;
        let due: Vec<u64> = self
            .tasks
            .iter()
            .filter(|(_, t)| t.at <= now)
            .map(|(id, _)| *id)
            .collect();
        for id in due {
            let Some(mut t) = self.tasks.remove(&id) else {
                continue;
            };
            self.ctx.current_task = Some(TaskId(id));
            self.guarded(&format!("task {id}"), |p, ctx| (t.task)(p, ctx));
            self.ctx.current_task = None;
            if let Some(period) = t.period
                && !self.cancelled.contains(&id)
            {
                t.at = now + period;
                self.tasks.insert(id, t);
            }
        }
        self.cancelled.clear();
    }
}

/// What [`horizon_plugin!`](crate::horizon_plugin) exports: builds the
/// plugin and its runtime for the server.
///
/// # Safety
/// `host` must point at a `HostApi` that outlives the returned plugin.
pub unsafe fn entry<P: Plugin>(
    host: *const HostApi,
    make: impl FnOnce() -> P,
    describe: impl FnOnce() -> PluginDescription,
) -> PluginApi {
    let failed = PluginApi {
        abi: ABI_VERSION,
        instance: std::ptr::null_mut(),
        handle: handle::<P>,
        free: abi::free_buf,
        destroy: destroy::<P>,
    };
    // SAFETY: the caller hands a live `HostApi`.
    if host.is_null() || unsafe { (*host).abi } != ABI_VERSION {
        return failed;
    }
    let built = catch_unwind(AssertUnwindSafe(|| {
        with_host(host, || {
            let description = describe();
            Runtime {
                plugin: make(),
                ctx: Ctx::new(description),
                listeners: HashMap::new(),
                commands: HashMap::new(),
                aliases: HashMap::new(),
                messages: HashMap::new(),
                completers: HashMap::new(),
                tasks: BTreeMap::new(),
                cancelled: HashSet::new(),
            }
        })
    }));
    let Ok(runtime) = built else {
        return failed;
    };
    let state = Box::new((host, runtime));
    PluginApi {
        instance: Box::into_raw(state).cast::<c_void>(),
        ..failed
    }
}

unsafe extern "C" fn handle<P: Plugin>(
    instance: *mut c_void,
    kind: RawStr,
    payload: RawStr,
) -> RawBuf {
    let reply = if instance.is_null() {
        Value::map().with("error", "the plugin failed to start")
    } else {
        // SAFETY: `instance` is the box `entry` made, used by one thread at
        // a time (the server's).
        let (host, runtime) = unsafe { &mut *instance.cast::<(*const HostApi, Runtime<P>)>() };
        // SAFETY: the server passes text that lives for the call.
        let (kind, payload) = unsafe { (kind.as_str(), payload.as_str()) };
        let payload = Value::from_json(payload).unwrap_or_default();
        let out = catch_unwind(AssertUnwindSafe(|| {
            with_host(*host, || runtime.handle(kind, &payload))
        }));
        match out {
            Ok(Ok(v)) => Value::map().with("ok", v),
            Ok(Err(e)) => Value::map().with("error", e),
            Err(_) => Value::map().with("error", format!("`{kind}` panicked")),
        }
    };
    RawBuf::from_string(reply.to_json())
}

unsafe extern "C" fn destroy<P: Plugin>(instance: *mut c_void) {
    if !instance.is_null() {
        // SAFETY: the box `entry` made, destroyed once.
        drop(unsafe { Box::from_raw(instance.cast::<(*const HostApi, Runtime<P>)>()) });
    }
}
