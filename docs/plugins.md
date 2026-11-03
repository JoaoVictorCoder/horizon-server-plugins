# Server plugins

A dedicated server runs plugins, the way Paper does: drop a plugin into the server's `plugins/` folder and
restart. Plugins are written in Rust against `horizon-plugin-api`, an API shaped like Bukkit and Paper's. It has
a `plugin.yml`, `on_enable` and `on_disable`, listeners with priorities and cancelling, commands with
permissions, a scheduler counted in ticks, `config.yml`, and `§` and `&` colour codes. Paper's own jars don't
run: they are Java bytecode written against Bukkit's classes. A Paper plugin's logic ports to this API almost
line for line.

Single player and LAN worlds load no plugins. Plugins don't change what clients need: vanilla clients join as
usual.

## Using plugins

- Put the library in `plugins/`, beside `server.properties`: `libhomes.so` on Linux, `homes.dll` on Windows,
  `libhomes.dylib` on macOS. A library is built for one system.
- Plugins are native code and run with the server's rights, as Paper's do. Install only plugins you trust.
- At start the server loads every plugin in dependency order, enables them once the world is ready, and logs each
  one (`[Homes] Enabling Homes v1.0.0`). A plugin that fails to load is logged with the reason. `/plugins` lists
  the plugins: green for enabled, red for disabled, dark red for those that failed. `/version <plugin>` shows a
  plugin's description.
- Each plugin keeps its files in `plugins/<Name>/` (its `config.yml`...). Backups (`horizon-server backup`) hold
  `plugins/` without the libraries.
- Permissions are in `plugins/permissions.yml`, written on first start:

  ```yaml
  groups:
    default:
      permissions: [homes.use]
    admin:
      inherit: [default]
      permissions: ['*', -homes.delete]   # a leading - takes a permission away
  users:
    Steve:
      groups: [admin]
  ```

  Players without a group are in `default`. A permission set nowhere keeps the default its plugin declares
  (`true`, `false`, `op`, `not op`). One no plugin declares is for operators. Operators are the players in
  `ops.json`, at any level. `homes.*` and `*` act as wildcards only in this file.
- The console runs plugins' commands too, as the console sender, which has every permission.
- `/plugins reload` (operators and the console) disables every plugin and loads the folder again at the next
  tick: new libraries, configs and `permissions.yml`. Each load copies the libraries elsewhere first, so a
  library can be replaced while the server runs. The old code stays in memory until the server stops. A command
  a reload adds reaches the players' command tree at once. Plugins that keep state outside their folders may
  misbehave until a restart, as on Paper.

## Writing a plugin

A plugin is a `cdylib` crate:

```toml
[package]
name = "homes"
version = "1.0.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
horizon-plugin-api = { git = "https://github.com/JoaoVictorCoder/horizon-server-plugins" }
```

`plugin.yml` at the crate's root is Bukkit's: `name`, `version`, `description`, `authors`, `website`,
`depend`, `softdepend`, `loadbefore`, `commands` (with `aliases`, `usage`, `permission`,
`permission-message`), `permissions` (with `default` and `children`), `default-permission`. `main` is ignored:
the type named in `horizon_plugin!` is the main.

```rust
use horizon_plugin_api::prelude::*;

#[derive(Default)]
struct Homes {
    sets: u32,
}

impl Plugin for Homes {
    fn on_enable(&mut self, ctx: &mut Ctx<Self>) {
        ctx.save_default_config(include_str!("../config.yml"));
        ctx.on(|_, ctx, e: &mut PlayerJoinEvent| {
            let text = ctx.config().string_or("welcome", "&aHi %player%!").replace("%player%", &e.player.name);
            e.player.send_message(&color::translate('&', &text));
        });
        ctx.listen(EventPriority::High, true, |_, _, e: &mut BlockBreakEvent| {
            if e.block.is("bedrock") && !e.player.has_permission("homes.bedrock") {
                e.set_cancelled(true);
            }
        });
        // A command plugin.yml declares.
        ctx.set_executor("sethome", |plugin, _, sender, _label, _args| {
            plugin.sets += 1;
            sender.send_message("§aSet.");
            true // false shows the command's usage
        });
        ctx.run_timer(20, 20 * 60, |_, ctx| {
            ctx.server().broadcast_message("§7Another minute.");
        });
    }
}

horizon_plugin!(Homes::default(), plugin_yml!());
```

`cargo build --release` makes `target/release/libhomes.so`. A full example is in
`api/examples/homes/`: `/sethome`, `/home`, `/delhome`, a `/homes` chest menu, a limit
from `config.yml`, homes kept in `homes.yml`, and a protected spawn. Try a plugin without a server:

```sh
cargo build -p horizon-plugin-api --example homes
mkdir -p /tmp/plugins && cp target/debug/examples/libhomes.so /tmp/plugins/
cargo run -p horizon-plugin-host --example plugin_dry_run -- /tmp/plugins "sethome base" homes "home base"
```

### What Bukkit names, here

| Bukkit / Paper | horizon-plugin-api |
|---|---|
| `JavaPlugin.onEnable/onDisable/onLoad` | `Plugin::on_enable/on_disable/on_load(&mut self, ctx)` |
| `@EventHandler(priority, ignoreCancelled)` | `ctx.listen::<E>(priority, ignore_cancelled, \|plugin, ctx, e\| ...)`, `ctx.on` |
| `getCommand("x").setExecutor` | `ctx.set_executor("x", ...)`; a command not in plugin.yml: `ctx.command(CommandSpec::new("x"), ...)` |
| `BukkitScheduler.runTask/Later/Timer` | `ctx.run_task`, `ctx.run_later`, `ctx.run_timer`, `ctx.cancel_task` |
| `runTaskAsynchronously` | `ctx.run_async(\|sync\| ... sync.run(\|plugin, ctx\| ...))` |
| `getConfig()`, `saveDefaultConfig()` | `ctx.config()`, `ctx.save_default_config(text)`, `ctx.save_config()` |
| `Bukkit.getServer()` | `server()` or `ctx.server()` |
| `Player.sendMessage/teleport/kick/hasPermission` | the same on `Player`: `send_message`, `teleport`, `kick`, `has_permission` |
| `Bukkit.dispatchCommand(console, ...)` | `server().dispatch_command(...)` |
| `ServicesManager`, plugin messaging | `server().message_plugin(name, channel, value)` and `ctx.on_message(channel, ...)` |
| `ChatColor.translateAlternateColorCodes` | `color::translate('&', ...)` |

Every callback gets the plugin itself and its `Ctx`, so listeners reach the plugin's fields without `Rc`. A panic
in a callback is caught and logged, and the server goes on.

### Events

| Event | When | Cancel |
|---|---|---|
| `PlayerJoinEvent`, `PlayerQuitEvent` | a player is in the world, left it; change `join_message` / `quit_message`, or `None` for none | |
| `PlayerMoveEvent` | a player moved (seen once a tick) | sends them back |
| `PlayerChangedWorldEvent`, `PlayerDeathEvent`, `PlayerRespawnEvent`, `PlayerGameModeChangeEvent` | seen once a tick | |
| `AsyncPlayerChatEvent` | a chat message; change `message` or `format` | nobody sees it |
| `PlayerInteractEvent` | right click on a block (before the block's use) or in the air (the item's use); left click on a block, as digging starts | nothing happens (nothing is dug) |
| `BlockBreakEvent` | a player breaks a block | the block stays, with its drops |
| `BlockPlaceEvent` | a player is about to place a block | nothing is placed |
| `PlayerBucketFillEvent`, `PlayerBucketEmptyEvent` | a bucket is filled or emptied | the bucket does nothing |
| `PrePlayerAttackEntityEvent` | a player hits an entity | no damage, no knockback |
| `EntitySpawnEvent` | a mob is about to come into the world | it doesn't |
| `EntityDeathEvent` | a mob died, before its loot | |
| `EntityExplodeEvent` | an explosion is about to break `blocks` (take out those to spare) | spares every block |
| `BlockFromToEvent`, `BlockSpreadEvent` | a fluid flows, fire spreads | the block stays |
| `BlockPistonEvent` | a piston pushes or pulls | nothing moves |
| `InventoryClickEvent`, `InventoryCloseEvent` | in a chest menu a plugin opened (`Player::open_menu`) | |
| `ServerTickStartEvent`, `ServerTickEndEvent`, `ServerLoadEvent`, `WorldLoadEvent`, `WorldUnloadEvent`, `PluginEnableEvent`, `PluginDisableEvent` | the server's own | |

They come from the native mods' hook points (Horizon's docs/native-mods.md), at the same places in the game.

### What to know

- **Inside a player's event or command** the game holds that player's state. Their own items are at hand
  (`Player::give`, `take`, `count`, `item_in_hand`). Lines to them and to other players are sent at once
  or, when the others are out of reach, at the next tick. Console commands (`dispatch_command`, `teleport`,
  `kick`, `World::set_block`...) run at the next tick's start; they return `true` for "queued". Open menus from
  `ctx.run_task`, as the Homes example does.
- **Blocks** read (`BlockPos::block`) in a tick (a scheduled task, a tick event) or in a player's own event.
- **Commands** are registered in `on_enable` (or in `plugin.yml`); the players' command tree is built at
  the first tick and after a reload. They get the rest of the line split at spaces. `ctx.set_tab_completer`
  completes their arguments as a player types (without one: the online players' names). A name may start
  with `/`, which makes WorldEdit's `//set`. Two plugins with the same name: the first keeps it, the other
  answers to `/plugin:name`.
- **Scoreboards, teams and boss bars**: `Objective`, `Team`, `BossBar` (`horizon_plugin_api::scoreboard`),
  through vanilla's commands, so the world keeps them.
- **Inventories**: `Player::set_slot`, `set_equipment`, `clear_inventory`, `give_item` (with its name and lore),
  `inventory`; chest menus with `open_menu`.
- **Entities**: `World::entities` lists the mobs of the world the server holds at the moment (a player's own,
  inside their event); `World::kill_entities` takes a selector's filter.
- **Where players look**: `player.location.yaw`, `pitch` and `facing()`.
- Many `Player` and `World` methods run vanilla commands: `set_game_mode`, `send_title`, `play_sound`,
  `add_effect`, `spawn_entity`, `spawn_particle`, `set_time`, `set_weather`, `create_explosion`.
- **Threads**: a `run_async` thread reaches the server like the server's own thread does (`server()`,
  `player.send_message`...); each call waits for the next tick's start or end, where the server's thread answers
  it (`callSyncMethod`). For threads the plugin starts itself, run their code in `sync.attach(|| ...)`. For many
  calls at once, or to change the plugin's state, send one closure back with `sync.run(|plugin, ctx| ...)`.
  Registering listeners and commands stays on the server's thread. Never make the server's thread wait for such
  a thread (joining it, a lock it holds): its call waits in turn, and fails after 10 s. Once the plugin is
  disabled, its threads' calls fail at once.
- **ABI**: plugins talk to the server through a C ABI with JSON messages (`horizon_plugin_api::abi`). A plugin
  built with any Rust loads in any server of the same ABI version (now 1).

### Not yet

Chunk events; menus other than chests (hoppers, anvils...); moving or changing a mob by its handle (only
`kill_entities` by selector); block entity data (a copied chest is empty).
