//! The host with plugins linked in, against a pretend server.

use super::*;
use horizon_plugin_api::prelude::*;
use horizon_plugin_api::runtime;
use std::sync::atomic::{AtomicU32, Ordering};

#[derive(Default)]
struct Mock {
    players: Vec<Player>,
    acting: Option<Player>,
    told: Vec<(Uuid, String)>,
    commands: Vec<String>,
    logs: Vec<String>,
    in_player_hook: bool,
    given: Vec<(String, u32)>,
}

impl Backend for Mock {
    fn players(&self) -> Vec<Player> {
        self.players.clone()
    }
    fn acting_player(&self) -> Option<Player> {
        self.acting.clone()
    }
    fn tell(&mut self, uuid: Uuid, message: &str) -> bool {
        self.told.push((uuid, message.to_owned()));
        true
    }
    fn broadcast(&mut self, message: &str) -> usize {
        self.logs.push(format!("BROADCAST {message}"));
        self.players.len()
    }
    fn action_bar(&mut self, _: Uuid, _: &str) -> bool {
        true
    }
    fn can_run_commands(&self) -> bool {
        !self.in_player_hook
    }
    fn console_command(&mut self, line: &str) -> bool {
        self.commands.push(line.to_owned());
        true
    }
    fn block(&self, pos: &BlockPos) -> Option<Block> {
        Some(Block {
            pos: pos.clone(),
            kind: "minecraft:stone".into(),
            properties: vec![],
        })
    }
    fn held_item(&self) -> Option<ItemStack> {
        None
    }
    fn count_items(&mut self, _: &str) -> u32 {
        7
    }
    fn take_items(&mut self, _: &str, n: u32) -> u32 {
        n.min(7)
    }
    fn give_items(&mut self, item: &str, n: u32) -> u32 {
        self.given.push((item.to_owned(), n));
        n
    }
    fn inventory(&self, _: Uuid) -> Option<Vec<Option<ItemStack>>> {
        None
    }
    fn open_menu(&mut self, _: Uuid, _: &Menu) -> Option<i32> {
        Some(3)
    }
    fn set_menu_items(&mut self, _: Uuid, _: &[Option<ItemStack>]) {}
    fn close_menu(&mut self, _: Uuid) {}
    fn log(&mut self, level: &str, line: &str) {
        self.logs.push(format!("{level} {line}"));
    }
}

fn player(name: &str, n: u128, op: u8) -> Player {
    Player {
        uuid: Uuid(n),
        name: name.into(),
        location: Location::new(worlds::OVERWORLD, 0.5, 64.0, 0.5),
        game_mode: GameMode::Survival,
        health: 20.0,
        op_level: op,
    }
}

fn break_event(p: &Player, block: &str) -> Value {
    BlockBreakEvent::new(
        p.clone(),
        Block {
            pos: BlockPos::new(worlds::OVERWORLD, 1, 63, 0),
            kind: format!("minecraft:{block}"),
            properties: vec![],
        },
    )
    .to_data()
}

// A protection plugin: bedrock stays unless you may break it.
#[derive(Default)]
struct Guard {
    ticks_seen: u32,
    joins: Vec<String>,
}

impl Plugin for Guard {
    fn on_enable(&mut self, ctx: &mut Ctx<Self>) {
        ctx.save_default_config("protected: [bedrock, barrier]\n");
        ctx.listen(
            EventPriority::High,
            false,
            |_, ctx, e: &mut BlockBreakEvent| {
                let protected = ctx.config().get_strings("protected");
                if protected.iter().any(|b| e.block.is(b))
                    && !e.player.has_permission("guard.bypass")
                {
                    e.set_cancelled(true);
                    e.player.send_message("§cProtected.");
                }
            },
        );
        ctx.on(|plugin: &mut Guard, _, e: &mut PlayerJoinEvent| {
            plugin.joins.push(e.player.name.clone());
            e.player.send_message(&color::translate('&', "&aWelcome!"));
        });
        ctx.on(|_, _, e: &mut PlayerMoveEvent| {
            if e.to.y < 0.0 {
                e.set_cancelled(true);
            }
        });
        ctx.run_timer(1, 2, |plugin, ctx| {
            plugin.ticks_seen += 1;
            if plugin.ticks_seen == 3 {
                let id = ctx.current_task().expect("inside a task");
                ctx.cancel_task(id);
            }
        });
        ctx.command(
            CommandSpec::new("guard")
                .alias("g")
                .permission("guard.admin")
                .usage("/<command> <count|run>"),
            |plugin, ctx, sender, _, args| match args.first().map(String::as_str) {
                Some("count") => {
                    sender.send_message(&format!(
                        "ticks {} joins {}",
                        plugin.ticks_seen,
                        plugin.joins.len()
                    ));
                    true
                }
                Some("run") => {
                    // Its own command, from inside its command: next tick.
                    ctx.server().dispatch_command("guard count");
                    ctx.server().dispatch_command("say hi");
                    true
                }
                _ => false,
            },
        );
    }
}

unsafe extern "C" fn guard_entry(host: *const HostApi) -> PluginApi {
    unsafe {
        runtime::entry(host, Guard::default, || {
            PluginDescription::new("Guard", "1.0")
                .permission(Permission::new("guard.admin").default(PermissionDefault::Op))
                .permission(Permission::new("guard.bypass").default(PermissionDefault::False))
        })
    }
}

// Watches breaks at monitor, and tries (in vain) to undo a cancel.
#[derive(Default)]
struct Watcher;

static MONITOR_SAW_CANCELLED: AtomicU32 = AtomicU32::new(0);

impl Plugin for Watcher {
    fn on_enable(&mut self, ctx: &mut Ctx<Self>) {
        ctx.listen(
            EventPriority::Monitor,
            false,
            |_, _, e: &mut BlockBreakEvent| {
                if e.is_cancelled() {
                    MONITOR_SAW_CANCELLED.fetch_add(1, Ordering::SeqCst);
                }
                e.set_cancelled(false);
            },
        );
        // Runs before Guard's High listener, so it can't see Guard's cancel.
        ctx.listen(EventPriority::Low, true, |_, _, e: &mut BlockBreakEvent| {
            if e.block.is("barrier") {
                e.set_cancelled(true);
            }
        });
    }
}

unsafe extern "C" fn watcher_entry(host: *const HostApi) -> PluginApi {
    unsafe {
        runtime::entry(
            host,
            || Watcher,
            || PluginDescription::new("Watcher", "0.1").soft_depend("Guard"),
        )
    }
}

// An economy other plugins ask for balances.
#[derive(Default)]
struct Economy {
    balance: i64,
}

impl Plugin for Economy {
    fn on_load(&mut self, _: &mut Ctx<Self>) {
        self.balance = 100;
    }
    fn on_enable(&mut self, ctx: &mut Ctx<Self>) {
        ctx.on_message("balance", |plugin: &mut Economy, _, m| {
            Value::map()
                .with("balance", plugin.balance)
                .with("asked_by", &m.from)
        });
        ctx.set_executor("pay", |plugin, _, sender, _, args| {
            let Some(n) = args.first().and_then(|a| a.parse::<i64>().ok()) else {
                return false;
            };
            plugin.balance -= n;
            sender.send_message(&format!("paid {n}"));
            true
        });
        ctx.run_async(|sync| {
            let worked = 2 + 2;
            sync.run(move |plugin: &mut Economy, _| plugin.balance += worked);
        });
    }
}

unsafe extern "C" fn economy_entry(host: *const HostApi) -> PluginApi {
    unsafe {
        runtime::entry(host, Economy::default, || {
            PluginDescription::from_yaml(
                "name: Economy\nversion: 3\ncommands:\n  pay:\n    usage: /<command> <amount>\n    aliases: [give-money]\n",
            )
            .expect("plugin.yml")
        })
    }
}

// Needs Economy: loads after it, and asks it on enable.
#[derive(Default)]
struct Shop;

impl Plugin for Shop {
    fn on_enable(&mut self, ctx: &mut Ctx<Self>) {
        let answer = ctx
            .server()
            .message_plugin("Economy", "balance", Value::Null)
            .expect("Economy answers");
        ctx.logger().info(&format!(
            "balance {} for {}",
            answer.get("balance").and_then(Value::as_i64).unwrap_or(-1),
            answer
                .get("asked_by")
                .and_then(Value::as_str)
                .unwrap_or("?")
        ));
        assert!(
            ctx.server()
                .message_plugin("Shop", "x", Value::Null)
                .is_err()
        );
        // Shop's own command clashes with Economy's alias: it keeps `shop:give-money`.
        ctx.command(CommandSpec::new("give-money"), |_, _, s, _, _| {
            s.send_message("shop's");
            true
        });
    }
}

unsafe extern "C" fn shop_entry(host: *const HostApi) -> PluginApi {
    unsafe {
        runtime::entry(
            host,
            || Shop,
            || PluginDescription::new("Shop", "1").depend("Economy"),
        )
    }
}

unsafe extern "C" fn broken_entry(host: *const HostApi) -> PluginApi {
    unsafe {
        runtime::entry(
            host,
            || Shop,
            || PluginDescription::new("Broken", "1").depend("Missing"),
        )
    }
}

unsafe extern "C" fn panics_entry(host: *const HostApi) -> PluginApi {
    unsafe {
        runtime::entry(
            host,
            || -> Shop { panic!("constructor failed") },
            || PluginDescription::new("Panics", "1"),
        )
    }
}

fn host_with_plugins(dir: &Path, mock: &mut Mock) -> PluginHost {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        dir.join("permissions.yml"),
        "groups:\n  default:\n    permissions: []\nusers:\n  Builder:\n    permissions: [guard.bypass]\n",
    )
    .unwrap();
    let host = PluginHost::new(dir);
    host.load_entries(
        mock,
        vec![
            (shop_entry as EntryFn, "shop".into()),
            (broken_entry as EntryFn, "broken".into()),
            (watcher_entry as EntryFn, "watcher".into()),
            (economy_entry as EntryFn, "economy".into()),
            (guard_entry as EntryFn, "guard".into()),
            (panics_entry as EntryFn, "panics".into()),
        ],
    );
    host.enable(mock, None);
    host
}

#[test]
fn plugins_load_in_dependency_order() {
    let dir = std::env::temp_dir().join(format!("hz-plugins-order-{}", std::process::id()));
    let mut mock = Mock::default();
    let host = host_with_plugins(&dir, &mut mock);
    let names: Vec<String> = host.plugins().into_iter().map(|(n, _, _)| n).collect();
    assert_eq!(names, vec!["Economy", "Guard", "Shop", "Watcher"]);
    assert!(host.plugins().iter().all(|(_, _, on)| *on));
    assert!(
        mock.logs
            .iter()
            .any(|l| l.contains("Could not load broken: needs Missing")),
        "{:?}",
        mock.logs
    );
    assert!(
        mock.logs
            .iter()
            .any(|l| l.contains("Could not load panics"))
    );
    assert!(
        mock.logs
            .iter()
            .any(|l| l == "INFO [Shop] balance 100 for Shop"),
        "{:?}",
        mock.logs
    );
    assert!(
        dir.join("Guard").join("config.yml").exists(),
        "the default config is written"
    );
    let alex = player("Alex", 1, 0);
    assert!(host.command(&mut mock, CommandSender::Player(alex.clone()), "plugins"));
    let (_, list) = mock.told.last().unwrap();
    assert_eq!(
        list,
        "§fPlugins (4): §aEconomy§f, §aGuard§f, §aShop§f, §aWatcher§f, §4panics§f, §4broken"
    );
    host.disable_all(&mut mock);
    assert!(host.plugins().iter().all(|(_, _, on)| !*on));
    let disabling: Vec<&String> = mock
        .logs
        .iter()
        .filter(|l| l.contains("Disabling"))
        .collect();
    assert!(disabling[0].contains("Watcher") && disabling[3].contains("Economy"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn listeners_run_by_priority_and_cancel() {
    let dir = std::env::temp_dir().join(format!("hz-plugins-events-{}", std::process::id()));
    let mut mock = Mock::default();
    let host = host_with_plugins(&dir, &mut mock);
    let (alex, builder) = (player("Alex", 1, 0), player("Builder", 2, 0));
    mock.players = vec![alex.clone(), builder.clone()];

    let (_, cancelled) = host.fire(&mut mock, "block_break", break_event(&alex, "bedrock"));
    assert!(cancelled, "Guard cancels it, and Monitor can't undo that");
    assert_eq!(mock.told.last().map(|t| t.1.as_str()), Some("§cProtected."));
    assert_eq!(MONITOR_SAW_CANCELLED.load(Ordering::SeqCst), 1);

    let (_, cancelled) = host.fire(&mut mock, "block_break", break_event(&builder, "bedrock"));
    assert!(!cancelled, "permissions.yml gives Builder guard.bypass");

    let (_, cancelled) = host.fire(&mut mock, "block_break", break_event(&alex, "stone"));
    assert!(!cancelled);
    // Watcher's Low listener cancels barriers first; Guard's still runs (it
    // doesn't ignore cancelled events) and cancels too.
    let (_, cancelled) = host.fire(&mut mock, "block_break", break_event(&builder, "barrier"));
    assert!(cancelled);
    assert!(!host.listens("player_chat"));
    assert!(host.listens("block_break"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn commands_check_permissions_and_wait_when_busy() {
    let dir = std::env::temp_dir().join(format!("hz-plugins-commands-{}", std::process::id()));
    let mut mock = Mock::default();
    let host = host_with_plugins(&dir, &mut mock);
    let (alex, op) = (player("Alex", 1, 0), player("Opal", 3, 4));
    mock.players = vec![alex.clone(), op.clone()];
    let names = host.command_names();
    for n in [
        "guard",
        "g",
        "pay",
        "give-money",
        "plugins",
        "pl",
        "version",
    ] {
        assert!(names.contains(&n.to_owned()), "{n} in {names:?}");
    }
    assert!(!names.iter().any(|n| n.contains(':')));

    assert!(host.command(
        &mut mock,
        CommandSender::Player(alex.clone()),
        "guard count"
    ));
    assert!(mock.told.last().unwrap().1.starts_with("§cI'm sorry"));
    assert!(host.command(&mut mock, CommandSender::Player(op.clone()), "g"));
    assert_eq!(
        mock.told.last().unwrap().1,
        "§c/g <count|run>",
        "false shows the usage"
    );
    assert!(host.command(&mut mock, CommandSender::Player(op.clone()), "GUARD count"));
    assert_eq!(mock.told.last().unwrap().1, "ticks 0 joins 0");
    assert!(!host.command(&mut mock, CommandSender::Player(op.clone()), "nope"));

    // From inside its own command: `guard count` waits a tick, `say` runs.
    mock.told.clear();
    assert!(host.command(&mut mock, CommandSender::Player(op.clone()), "guard run"));
    assert_eq!(mock.commands, vec!["say hi"]);
    assert!(mock.told.is_empty());
    host.tick_start(&mut mock, 1);
    assert!(
        mock.logs.iter().any(|l| l.ends_with("ticks 0 joins 0")),
        "the console got the answer"
    );

    // plugin.yml's command and its alias, taken first by Economy.
    assert!(host.command(
        &mut mock,
        CommandSender::Player(alex.clone()),
        "give-money 5"
    ));
    assert_eq!(mock.told.last().unwrap().1, "paid 5");
    assert!(host.command(
        &mut mock,
        CommandSender::Player(alex.clone()),
        "shop:give-money"
    ));
    assert_eq!(mock.told.last().unwrap().1, "shop's");
    assert!(host.command(&mut mock, CommandSender::Player(alex.clone()), "pay x"));
    assert_eq!(mock.told.last().unwrap().1, "§c/pay <amount>");
    assert!(host.command(&mut mock, CommandSender::Console, "version Economy"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ticks_run_tasks_and_watch_players() {
    let dir = std::env::temp_dir().join(format!("hz-plugins-ticks-{}", std::process::id()));
    let mut mock = Mock::default();
    let host = host_with_plugins(&dir, &mut mock);
    let op = player("Opal", 3, 4);
    for t in 1..=10 {
        host.tick_start(&mut mock, t);
        host.tick_end(&mut mock, t);
    }
    host.command(&mut mock, CommandSender::Player(op.clone()), "guard count");
    assert_eq!(
        mock.told.last().unwrap().1,
        "ticks 3 joins 0",
        "the timer cancelled itself at 3"
    );

    let mut alex = player("Alex", 1, 0);
    mock.players = vec![alex.clone()];
    host.tick_end(&mut mock, 11);
    assert_eq!(
        mock.told.last().unwrap(),
        &(alex.uuid, "§aWelcome!".to_owned())
    );
    // A move below 0 is cancelled: back with a teleport.
    alex.location.y = -5.0;
    mock.players = vec![alex.clone()];
    host.tick_end(&mut mock, 12);
    assert_eq!(
        mock.commands.last().unwrap(),
        "execute in minecraft:overworld run tp Alex 0.5 64 0.5"
    );
    mock.players.clear();
    host.tick_end(&mut mock, 13);
    host.command(&mut mock, CommandSender::Player(op.clone()), "guard count");
    assert_eq!(mock.told.last().unwrap().1, "ticks 3 joins 1");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn async_work_comes_back_on_a_tick() {
    let dir = std::env::temp_dir().join(format!("hz-plugins-async-{}", std::process::id()));
    let mut mock = Mock::default();
    let host = host_with_plugins(&dir, &mut mock);
    let alex = player("Alex", 1, 0);
    mock.players = vec![alex.clone()];
    std::thread::sleep(std::time::Duration::from_millis(50));
    host.tick_start(&mut mock, 1);
    // 100 - 0 + 4 from the thread; pay 104 shows it is back.
    host.command(&mut mock, CommandSender::Player(alex.clone()), "pay 104");
    let _ = std::fs::remove_dir_all(&dir);
    let v = host
        .shared
        .send(
            0,
            "message",
            &Value::map().with("from", "t").with("channel", "balance"),
        )
        .unwrap();
    assert_eq!(v.get("balance").and_then(Value::as_i64), Some(0));
}

#[test]
fn console_commands_wait_inside_a_players_action() {
    let dir = std::env::temp_dir().join(format!("hz-plugins-defer-{}", std::process::id()));
    let mut mock = Mock::default();
    let host = host_with_plugins(&dir, &mut mock);
    let alex = player("Alex", 1, 0);
    mock.players = vec![alex.clone()];
    mock.acting = Some(alex.clone());
    mock.in_player_hook = true;
    let r = host.enter(&mut mock, |s| {
        let teleported = s.call(
            1,
            "teleport",
            &Value::map().with("uuid", alex.uuid.to_wire()).with(
                "location",
                Location::new("the_nether", 1.0, 70.0, 2.0).to_wire(),
            ),
        );
        let given = s.call(
            1,
            "give_item",
            &Value::map()
                .with("uuid", alex.uuid.to_wire())
                .with("item", "minecraft:diamond")
                .with("amount", 2),
        );
        (teleported, given)
    });
    assert_eq!(r.0, Ok(Value::Bool(true)));
    assert_eq!(r.1, Ok(Value::Int(2)));
    assert_eq!(
        mock.given,
        vec![("minecraft:diamond".to_owned(), 2)],
        "the acting player's own items"
    );
    assert!(mock.commands.is_empty(), "queued");
    mock.in_player_hook = false;
    mock.acting = None;
    host.tick_start(&mut mock, 1);
    assert_eq!(
        mock.commands,
        vec!["execute in minecraft:the_nether run tp Alex 1 70 2 0 0"]
    );
    let err = host.enter(&mut mock, |s| {
        s.call(
            1,
            "count_item",
            &Value::map()
                .with("uuid", alex.uuid.to_wire())
                .with("item", "x"),
        )
    });
    assert!(err.is_err(), "items only of the acting player");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn load_order_handles_soft_and_before() {
    let d = |n: &str| PluginDescription::new(n, "1");
    let descs = vec![
        d("C").depend("A"),
        d("A").soft_depend("Z"),
        d("B").load_before("A"),
        d("D").depend("E"),
        d("E").depend("D"),
        d("S").load(LoadOrder::Startup),
    ];
    let (order, failed) = load_order(&descs);
    let names: Vec<&str> = order.iter().map(|&i| descs[i].name.as_str()).collect();
    assert_eq!(names, vec!["S", "B", "A", "C"]);
    assert_eq!(failed.len(), 2);
    assert!(failed.iter().all(|(_, why)| why.contains("loop")));
}

#[test]
fn commands_complete_and_joins_carry_messages() {
    let dir = std::env::temp_dir().join(format!("hz-plugins-complete-{}", std::process::id()));
    let mut mock = Mock::default();
    let host = host_with_plugins(&dir, &mut mock);
    let (alex, op) = (player("Alex", 1, 0), player("Opal", 3, 4));
    mock.players = vec![alex.clone(), op.clone()];
    // No completer: the players' names, starting with what is typed.
    let (start, words) = host
        .complete(&mut mock, CommandSender::Player(op.clone()), "guard O")
        .unwrap();
    assert_eq!((start, words), (6, vec!["Opal".to_owned()]));
    let (start, words) = host
        .complete(&mut mock, CommandSender::Player(op.clone()), "plugins ")
        .unwrap();
    assert_eq!((start, words), (8, vec!["reload".to_owned()]));
    // Without the permission, nothing.
    assert!(host.complete(&mut mock, CommandSender::Player(alex.clone()), "guard ").is_none());
    assert!(host.complete(&mut mock, CommandSender::Player(op.clone()), "nope ").is_none());

    // Joins: Guard greets, the message stays; the tick after sees no new join.
    host.tick_end(&mut mock, 0);
    let newcomer = player("Neo", 9, 0);
    let said = host.player_joined(&mut mock, &newcomer, "§eNeo joined the game");
    assert_eq!(said.as_deref(), Some("§eNeo joined the game"));
    assert_eq!(mock.told.last().unwrap(), &(newcomer.uuid, "§aWelcome!".to_owned()));
    mock.players.push(newcomer.clone());
    let told = mock.told.len();
    host.tick_end(&mut mock, 1);
    assert_eq!(mock.told.len(), told, "no second welcome");
    let said = host.player_left(&mut mock, &newcomer, "§eNeo left the game");
    assert_eq!(said.as_deref(), Some("§eNeo left the game"));

    // /plugins reload: operators only, at the next tick.
    host.command(&mut mock, CommandSender::Player(alex.clone()), "plugins reload");
    assert!(!host.take_reload_request());
    host.command(&mut mock, CommandSender::Player(op.clone()), "plugins reload");
    assert!(host.take_reload_request());
    // The folder has no libraries: a reload leaves no plugin but the built-ins.
    host.reload(&mut mock);
    assert!(host.plugins().is_empty());
    assert!(host.has_command("plugins") && !host.has_command("guard"));
    assert!(mock.logs.iter().any(|l| l.contains("Disabling Guard")));
    let _ = std::fs::remove_dir_all(&dir);
}
