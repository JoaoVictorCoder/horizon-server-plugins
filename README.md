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
- **Example plugins** (`api/examples/`), small on purpose, to read and copy from:
  - **Homes**: `/sethome`, `/home`, `/delhome`, `/homes` (a menu), spawn protection.
  - **Dragão**: `/dragao [colour]` (or `/dragon`) builds a little dragon out of blocks.

## Forja: server-side mods

**[Forja](docs/forja.md)** (`forja/`) adds content with textures of its own that vanilla clients see without
installing anything, in the spirit of Paper's CraftEngine and ItemsAdder. Packs in `plugins/Forja/packs/` define
items in YAML and bring their textures and models. Forja builds the resource pack, serves it over HTTP and sends
it to players as they join and after `/forja reload`.

```yaml
# plugins/Forja/packs/exemplo/config/items.yml
items:
  rubi:
    material: emerald
    name: '&cRuby'
    texture: item/rubi     # resources/assets/exemplo/textures/item/rubi.png
```

Now: custom items. Next: recipes, custom blocks that exist on the server, furniture ([roadmap](docs/forja.md#roadmap)).

## Plugins built on it

Full plugins live in repositories of their own and depend on `horizon-plugin-api` through `git`:

- **[HorizonEdit](https://github.com/JoaoVictorCoder/horizonedit)**: most of WorldEdit's commands (selections,
  `//set`, `//copy`/`//paste`, shapes, brushes, tools, schematics, undo), written from scratch (no WorldEdit code).
  Build it with `cargo build --release` in its repository and copy the library into `plugins/`.

Made one? Open an issue or a pull request to list it here.

Full API documentation: [docs/plugins.md](docs/plugins.md).

Paper `.jar` files do **not** run here: they are Java bytecode written against Bukkit's classes. A Paper plugin
ports to this API almost line for line (see the table in `docs/plugins.md`), and
[paper2horizon](https://github.com/JoaoVictorCoder/paper2horizon) starts the port from the plugin's jar: it writes
the crate with each listener, command and task in place and what the Java code called beside each.

## Roadmap

Next up (the full list: [TODO.md](TODO.md)):

- An automatic end-to-end test: a server, a bot and a plugin.
- More of the API: chunk events, block states, block entity data (chest contents), entities by handle.
- GitHub releases with built servers and plugins, and CI.

## Building the server

You need [Rust](https://rustup.rs), `curl`, `tar`, `git` and a C compiler.

```sh
./build.sh
```

The script downloads Horizon at the commit in `engine/HORIZON_COMMIT`, adds the plugin crates, applies
`engine/horizon.patch` and compiles only the server (no sound, no controllers). The result:

- `dist/horizon-server-engine`: the server;
- `dist/plugins/`: the example plugins and Forja.

The first build takes a while (about 10 minutes); later ones are quick.

## Running it

1. **Game files.** The server needs the 26.3 data, prepared from **your own** copy of Minecraft; nothing of
   Mojang's is distributed here. The easy way: install the [Horizon launcher](https://playhorizon.dev) and play
   once. It leaves the files in
   `~/.local/share/horizon-launcher/server-runtime/game-files/horizon-client/26.3`.
   Details: [Horizon's hosting.md](https://github.com/horizon-rust/horizon/blob/main/docs/hosting.md).
2. **Server folder.** `server/` already has `iniciar.sh` (the start script). Copy the plugins you want into
   `server/plugins/` (the examples from `dist/plugins/`, and others such as HorizonEdit from their own builds):

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

`cargo build --release` makes `libhello.so`, which goes in `plugins/`. For a whole plugin in its own repository,
see [HorizonEdit](https://github.com/JoaoVictorCoder/horizonedit). To try plugins without a server:

```sh
cargo run -p horizon-plugin-host --example plugin_dry_run -- path/to/plugins "command arg"
```

## Layout

| Folder | What it is |
|---|---|
| `api/` | `horizon-plugin-api`: the plugin API (no dependencies, a C ABI) and the examples (`api/examples/`) |
| `host/` | `horizon-plugin-host`: loads `plugins/`, events, commands, permissions, reload |
| `forja/` | Forja, server-side mods: custom items and the resource pack ([docs/forja.md](docs/forja.md)) |
| `engine/` | `horizon.patch` (the changes to Horizon's engine) and the commit it applies to |
| `server/` | the server folder, with `iniciar.sh` |
| `docs/plugins.md` | the API documentation |
| `docs/forja.md` | Forja's documentation |
| `TODO.md` | what is left to do |

## License and credits

MIT, like Horizon. See [LICENSE](LICENSE) and [CREDITS.md](CREDITS.md). Minecraft is a trademark of Mojang
Studios; this project is not affiliated with Mojang, Microsoft, Horizon, Paper or WorldEdit.
