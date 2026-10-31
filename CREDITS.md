# Credits

## Horizon

This repository builds on **[Horizon](https://github.com/horizon-rust/horizon)**, Minecraft: Java Edition 26.3
rewritten in Rust, by **Ludovic Marie and the Horizon contributors**, under the MIT license (see [LICENSE](LICENSE)).
It holds none of Horizon's code: `build.sh` downloads Horizon and `engine/horizon.patch` changes a few of its
files. The game, the dedicated server engine, the native mod hook points the plugins are built on and the vanilla
parity work are all theirs.
Thank you for making it open.

- Upstream: <https://github.com/horizon-rust/horizon>
- Standalone server downloads: <https://github.com/horizon-rust/horizon-server>
- Horizon's own credits and third-party notices: its
  [THIRD_PARTY_NOTICES.md](https://github.com/horizon-rust/horizon/blob/main/THIRD_PARTY_NOTICES.md)

## The plugin layer (this repository)

By **JoaoVictorCoder**, written with Claude Code (an AI coding agent):

- `api/` (`horizon-plugin-api`): the API plugins are written against, and its C ABI.
- `host/` (`horizon-plugin-host`): loading `plugins/`, events, commands, permissions, reload.
- `engine/horizon.patch`: `game/plugins.rs` and small calls beside Horizon's own hook points.
- Example plugins in `api/examples/`: Homes, Dragão.

## Projects whose design it follows

No code from these projects is included. The plugin API follows their designs, which their users know:

- **Bukkit / Spigot / [Paper](https://papermc.io)**: `plugin.yml`, events and priorities, the scheduler,
  permissions and their defaults, command executors.

## Minecraft

Minecraft is a trademark of Mojang Studios. Neither Horizon nor this project is affiliated with Mojang or Microsoft.
Nothing of Mojang's is in this repository: Horizon prepares the game's files from the server owner's own copy.
