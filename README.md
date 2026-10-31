# Horizon Server Plugins

A **Minecraft: Java Edition 26.3** server in Rust with **Paper-style plugins**, built on
[Horizon](https://github.com/horizon-rust/horizon) by **Ludovic Marie and the Horizon contributors**.

> This repository holds **only the server side**: the plugin API, the host that loads `plugins/`, example
> plugins, and a small patch to Horizon's engine. The game, the server engine and everything else are Horizon's
> ([credits](CREDITS.md)). Report Horizon's own bugs to [its repository](https://github.com/horizon-rust/horizon/issues).

## What's in it

- **Rust plugins** shaped like Paper's: `plugin.yml`, `on_enable`/`on_disable`, events with priorities and
  cancelling, commands with permissions and tab completion, a tick scheduler, `config.yml`, `&a`/`§a` colours.
- **`plugins/permissions.yml`**: groups, inheritance, `-permission` to deny, `homes.*` wildcards.
- **`/plugins`** and **`/plugins reload`**: reload plugins and their configs without a restart.
- **Events**: join/quit (with their messages), move, death, chat, clicks, block break/place, buckets, attacks,
  mob spawns and deaths, explosions, fluids and fire, pistons, chest menus, ticks.
- **Example plugins** (`api/examples/`):
  - **Homes**: `/sethome`, `/home`, `/delhome`, `/homes` (a menu), spawn protection.
  - **Dragão**: `/dragao [colour]` (or `/dragon`) builds a little dragon out of blocks.
  - **HorizonEdit**: most of WorldEdit's commands, written from scratch (no WorldEdit code): the wand,
    `//hpos1`, `//chunk`, `//set`, `//replace`, `//overlay`, `//naturalize`, `//smooth`, `//hollow`, `//line`,
    `//forest`, `//sphere`, `//cyl`, `//pyramid`, `//generate` (expressions), `//copy`/`//paste`/`//rotate`/
    `//flip`, schematics (`//schem save|load`), `//undo`/`//redo`, `//fill`, `//drain`, `//snow`, `//butcher`,
    `//calc`, `/ascend`, `/jumpto`, `/thru`, brushes (`/brush sphere|cylinder|smooth|clipboard`), tools
    (`/tool tree|repl|info`), the super pickaxe (`/sp`), masks (`//gmask`) and `//limit`. `//help` lists them.
    Not there: `//regen`, biomes and snapshots, which need more of the engine than plugins reach.

Full API documentation: [docs/plugins.md](docs/plugins.md).

Paper `.jar` files do **not** run here: they are Java bytecode written against Bukkit's classes. A Paper plugin
ports to this API almost line for line (see the table in `docs/plugins.md`).

## Building the server

You need [Rust](https://rustup.rs), `curl`, `tar`, `git` and a C compiler.

```sh
./build.sh
```

The script downloads Horizon at the commit in `engine/HORIZON_COMMIT`, adds the plugin crates, applies
`engine/horizon.patch` and compiles only the server (no sound, no controllers). The result:

- `dist/horizon-server-engine`: the server;
- `dist/plugins/`: the example plugins.

The first build takes a while (about 10 minutes); later ones are quick.

## Running it

1. **Game files.** The server needs the 26.3 data, prepared from **your own** copy of Minecraft; nothing of
   Mojang's is distributed here. The easy way: install the [Horizon launcher](https://playhorizon.dev) and play
   once. It leaves the files in
   `~/.local/share/horizon-launcher/server-runtime/game-files/horizon-client/26.3`.
   Details: [Horizon's hosting.md](https://github.com/horizon-rust/horizon/blob/main/docs/hosting.md).
2. **Server folder.** `server/` already has `iniciar.sh` (the start script). Copy the plugins you want into
   `server/plugins/`:

   ```sh
   cp dist/plugins/* server/plugins/
   cd server && ./iniciar.sh --initSettings   # writes server.properties and eula.txt
   ```

   Read and accept Mojang's EULA in `eula.txt` (`eula=true`) and adjust `server.properties`.
3. **Start.** `./iniciar.sh`. The console is the terminal itself: `op YourName`, `plugins`, `stop`...

Game files elsewhere: `RESOURCES=/path/to/26.3 ./iniciar.sh`.

## Writing a plugin

```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
horizon-plugin-api = { git = "https://github.com/JoaoVictorCoder/horizon-server-plugins" }
```

```rust
use horizon_plugin_api::prelude::*;

#[derive(Default)]
struct Hello;

impl Plugin for Hello {
    fn on_enable(&mut self, ctx: &mut Ctx<Self>) {
        ctx.on(|_, _, e: &mut PlayerJoinEvent| {
            e.player.send_message(&color::translate('&', "&aWelcome!"));
        });
    }
}

horizon_plugin!(Hello, PluginDescription::new("Hello", "1.0.0"));
```

`cargo build --release` makes `libhello.so`, which goes in `plugins/`. To try plugins without a server:

```sh
cargo run -p horizon-plugin-host --example plugin_dry_run -- path/to/plugins "command arg"
```

## Layout

| Folder | What it is |
|---|---|
| `api/` | `horizon-plugin-api`: the plugin API (no dependencies, a C ABI) and the examples |
| `host/` | `horizon-plugin-host`: loads `plugins/`, events, commands, permissions, reload |
| `engine/` | `horizon.patch` (the changes to Horizon's engine) and the commit it applies to |
| `server/` | the server folder, with `iniciar.sh` |
| `docs/plugins.md` | the API documentation |
| `TODO.md` | what is left to do |

## License and credits

MIT, like Horizon. See [LICENSE](LICENSE) and [CREDITS.md](CREDITS.md). Minecraft is a trademark of Mojang
Studios; this project is not affiliated with Mojang, Microsoft, Horizon, Paper or WorldEdit.
