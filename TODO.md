# TODO

What is left to do, most important first in each section.

## Bugs

- [ ] `plugin_dry_run` hides bugs: every block reads as grass and block names aren't checked. Give its pretend
      world real blocks and a block list.

## Tests

- [ ] An automatic end-to-end test: a server, a bot and a plugin, run without a person.
- [ ] Exercise in game what only unit tests cover: tab completion, `/plugins reload` with a replaced library,
      scoreboards, boss bars and teams, `EntitySpawnEvent`/`EntityDeathEvent`, inventory editing, changed join and
      quit messages.
- [ ] Run `build.sh` from a clean checkout.

## Plugin API and engine

- [ ] Chunk load and unload events (the server's chunks come from the streaming workers).
- [ ] Block states: the values of each property (`block_states(id)`), for HorizonEdit's `/tool cycler`.
- [ ] Left clicks in the air, so far tools work with the left button at range.
- [ ] Entities by handle: move them, change their health and name (today: only kill by selector).
- [ ] Block entity data (chest contents, signs, spawners), to read and write.
- [ ] Menus other than chests: hoppers, anvils, dispensers.
- [ ] Biomes, for HorizonEdit's `//regen`, `//setbiome` and the biome brush.
- [ ] Run plugins' world commands in their level without swapping levels for each line (it costs time).
- [ ] The plugins' `/version` is left out because vanilla has one: rename it (`/pluginversion`) or join vanilla's.
- [x] Server calls from plugin threads: they wait for the next tick, where the server's thread answers them.
- [ ] Reads that don't wait for a tick: a snapshot (players, tick, plugins) any thread reads at once.

## New plugins and tools

- [x] A converter for Paper plugins: [paper2horizon](https://github.com/JoaoVictorCoder/paper2horizon) turns a
      jar into a crate of this API, with what's left to port marked. Next there: translating simple method bodies.
- [ ] A plugin template (`cargo generate` or a template repository).
- [ ] Publish `horizon-plugin-api` on crates.io, so plugins don't need a `git` dependency.

## Repository

- [ ] GitHub releases with built binaries: the server and the example plugins for Linux, then Windows and macOS.
- [ ] CI: the crates' tests and clippy, and a check that `engine/horizon.patch` still applies to its commit.
- [ ] Move the patch to newer Horizon commits as Horizon changes (it is pinned to `396ff1e`).
- [ ] Rename `iniciar.sh` to `start.sh` and `/dragao` to `/dragon`.
- [ ] Keep a CHANGELOG.
- [ ] Consider contributing to Horizon itself: small pull requests (under ~500 lines, with `/claim` on an issue),
      starting with the hook points.
