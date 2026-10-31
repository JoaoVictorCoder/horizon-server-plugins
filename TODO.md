# TODO

What is left to do, most important first in each section.

## Bugs

- [ ] HorizonEdit: some block writes fail on a real server (a mixed-pattern `//set` or brush underground, y -40 to
      -46, logged as `setblock ... failed`). Find out why: unloaded chunks, the wrong level in place, or a read that
      returned the wrong blocks.
- [ ] Big edits (a 500,000-block `//set`) are untested in a real world: measure how much they hold the server up
      and tune `limits.commands-per-tick`.
- [ ] `plugin_dry_run` hides bugs: every block reads as grass and block names aren't checked. Give its pretend
      world real blocks and a block list.

## Tests

- [ ] Parity with the real WorldEdit, black box: a Paper server with the official WorldEdit beside a Horizon
      server, the same commands from the same flat world, the results compared block by block; each difference
      becomes a test.
- [ ] In-game checks of what depends on aim and a real world: brushes, `/jumpto`, `/thru`, `//hpos`, the super
      pickaxe, `//smooth`, `//forest`.
- [ ] An automatic end-to-end test: a server, a bot and a plugin, run without a person.
- [ ] Exercise in game what only unit tests cover: tab completion, `/plugins reload` with a replaced library,
      scoreboards, boss bars and teams, `EntitySpawnEvent`/`EntityDeathEvent`, inventory editing, changed join and
      quit messages.
- [ ] Run `build.sh` from a clean checkout.

## Plugin API and engine

- [ ] Chunk load and unload events (the server's chunks come from the streaming workers).
- [ ] Block states: the values of each property (`block_states(id)`), for `/tool cycler`.
- [ ] Left clicks in the air, so far tools work with the left button at range.
- [ ] Entities by handle: move them, change their health and name (today: only kill by selector).
- [ ] Block entity data (chest contents, signs, spawners), to read and write.
- [ ] Menus other than chests: hoppers, anvils, dispensers.
- [ ] Biomes, for `//regen`, `//setbiome` and the biome brush.
- [ ] Run plugins' world commands in their level without swapping levels for each line (it costs time).
- [ ] The plugins' `/version` is left out because vanilla has one: rename it (`/pluginversion`) or join vanilla's.
- [ ] Server calls from plugin threads: today only the server thread may call the server.

## HorizonEdit (what WorldEdit has that it doesn't)

- [ ] `/tool cycler` (needs block states above).
- [ ] `//regen`, `//setbiome`, `/brush biome` (need biomes).
- [ ] `-e` (entities) and `-b` (biomes) on `//copy`, `//cut`, `//paste`, `//move`, `//stack`.
- [ ] `-m` (source mask) on `//copy`, `//move`, `//stack`.
- [ ] `//paste -n` (preview) and `-v`.
- [ ] Selections other than cuboids: `//sel poly`, `ellipsoid`, `sphere`, `cyl`, `convex`.
- [ ] Sponge `.schem` files (read and write), `/schem share`.
- [ ] `/brush deform` with expressions, `/tracemask`.
- [ ] `//butcher -p`: spare tamed pets, as WorldEdit does.
- [ ] Redo for trees (`//forest`, `/tool tree`).
- [ ] Copying chests with their contents (needs block entity data).

## New plugins and tools

- [ ] A converter for Paper plugins: start by scoping it (translating `plugin.yml`, then a guide or a Rust
      skeleton from a `.jar`'s classes).
- [ ] A plugin template (`cargo generate` or a template repository).
- [ ] Publish `horizon-plugin-api` on crates.io, so plugins don't need a `git` dependency.

## Repository

- [ ] GitHub releases with built binaries: the server and the example plugins for Linux, then Windows and macOS.
- [ ] CI: the crates' tests and clippy, and a check that `engine/horizon.patch` still applies to its commit.
- [ ] Move the patch to newer Horizon commits as Horizon changes (it is pinned to `396ff1e`).
- [ ] Rename `iniciar.sh` to `start.sh` and `/dragao` to `/dragon`.
- [ ] Give each example plugin (Homes, Dragão, HorizonEdit) a crate of its own.
- [ ] Document HorizonEdit (its commands and how it differs from WorldEdit) and keep a CHANGELOG.
- [ ] Consider contributing to Horizon itself: small pull requests (under ~500 lines, with `/claim` on an issue),
      starting with the hook points.
