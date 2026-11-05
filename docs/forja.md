# Forja

Forja makes **server-side mods**: content with textures, models and sounds of its own that vanilla clients see
without installing anything. It's in the spirit of Paper's CraftEngine, ItemsAdder and Oraxen, written from
scratch for Horizon (no code from them). The server builds a resource pack from Forja's packs, serves it over HTTP
and sends it to each player as they join.

Forja is a plugin like any other (`forja/` in this repository). `./build.sh` puts `libforja.so` in
`dist/plugins/`; copy it to the server's `plugins/`.

> **Status:** custom items with their own textures or models, custom blocks with states (drawn as note block
> states), the resource pack built, served and sent again on `/forja reload`, and channels for other plugins to
> own blocks. [Engenho](engenho.md), a tech mod, is built on it. Furniture and recipes come next
> ([roadmap](#roadmap)).

## Packs

Everything Forja adds comes from `plugins/Forja/packs/`, one folder per pack. On the first start Forja writes an
example, `packs/exemplo/`, with three items:

```text
plugins/Forja/
  config.yml                 # the resource pack's settings
  pack.zip                   # the last pack built, to look at or host elsewhere
  packs/
    exemplo/
      pack.yml               # optional: namespace, enabled
      config/
        items.yml            # any .yml under config/, in folders too
      resources/             # merged into the resource pack as it is
        assets/exemplo/textures/item/rubi.png
        assets/exemplo/models/item/coroa.json
        assets/exemplo/sounds/...
```

- `resources/` is laid out like a resource pack's root. Anything in it goes into the pack: textures, models,
  sounds, fonts, languages, and overrides of vanilla's own files (`assets/minecraft/...`). `pack.mcmeta` is
  written by Forja; hidden files (`.DS_Store`) are left out.
- Packs load in alphabetical order of their folders. When two packs have the same file, the first keeps it and
  the log says so.
- `pack.yml`: `namespace` (default: the folder's name in lower case) is the namespace of the pack's ids;
  `enabled: false` leaves the pack out.

## Items

```yaml
# packs/exemplo/config/items.yml
items:
  rubi:                          # exemplo:rubi; "outro:rubi" names another namespace
    material: emerald            # the vanilla item underneath
    name: '&cRuby'               # & colour codes; not in italics
    lore:
      - '&7A gem from deep below.'
    texture: item/rubi           # assets/exemplo/textures/item/rubi.png
    components:                  # more item components, as /give takes them
      rarity: rare
      max_stack_size: 16

  espada_rubi:
    material: diamond_sword
    name: '&cRuby Sword'
    texture: item/espada_rubi
    parent: handheld             # held like a tool (default: generated, like a gem)

  coroa:
    material: golden_helmet
    model: item/coroa            # a model of the pack: assets/exemplo/models/item/coroa.json
```

| Key | What it is |
|---|---|
| `material` | **Required.** The vanilla item it is to the server: how it stacks, breaks blocks, is eaten, worn... |
| `texture` | A flat item drawn from one texture. Forja writes the model (`assets/<ns>/models/item/<id>.json`) unless the pack has one. |
| `parent` | With `texture`: `generated` (default), `handheld` (tools), or any model id. |
| `model` | Instead of `texture`: a model the pack has, such as a Blockbench export. |
| `name`, `lore` | With `&` codes. The name is the `item_name` component: not in italics, and an anvil can still rename it. |
| `components` | Any other components, written in YAML and given to `/give` as SNBT. Horizon applies those it models. |

With neither `texture` nor `model`, the item keeps its material's look (a renamed stick with lore, say).

For each item with a look, Forja writes the item model definition `assets/<ns>/items/<id>.json` (unless the pack
has one, for `select` or `range_dispatch` models), and gives the item the component `item_model=<ns>:<id>`.
Each Forja item carries `custom_data={"forja:id":"<ns>:<id>"}`, so Forja and other plugins know it again
wherever it goes: `stack.data("forja:id")`.

## Blocks

```yaml
# packs/exemplo/config/blocks.yml
blocks:
  maquina:
    owner: MeuPlugin              # told when it is placed, broken or used (optional)
    name: '&fMáquina'
    material: paper               # the item that places it (default: paper)
    properties:
      facing: [north, east, south, west]   # set from where the player looks
      active: ['false', 'true']
    textures:
      front: block/maquina_frente
      side: block/maquina_lado
      top: block/maquina_topo
      bottom: block/maquina_base
    variants:
      active=true:                # the states this predicate matches look like this
        textures:
          front: block/maquina_frente_on
  bloco_rubi:
    texture: block/bloco_rubi     # the same texture on every face
```

| Key | What it is |
|---|---|
| `texture` | One texture on every face (`cube_all`). |
| `textures` | By slot: `front`, `side`, `top`, `bottom` (`orientable`, turned by `facing`); `end`, `side` (`cube_column`); or each face `north`, `south`, `east`, `west`, `up`, `down` (`cube`); `particle`. |
| `model` | A block model of the pack instead, turned by `facing`. |
| `properties` | Its states: each combination of values is one. 64 states at most per block. `facing` (`north`...`west`, `up`, `down`) is set when it is placed. |
| `variants` | Looks for some states: a predicate (`active=true`, `facing=up,lit=true`) and the textures or model it changes. |
| `owner` | A plugin told about the block (see below). |
| `drops` | `self` (default): breaking it gives its item back; `none`: its owner decides. |
| `name`, `lore`, `material`, `components` | Of the item that places it, which has the block's id. |

**How it works.** A vanilla client knows only vanilla blocks, so each state of each custom block is drawn as a
**note block state**. Forja turns note blocks *inert* (a patch to the engine: no tuning, no instrument from the
block below, no redstone; a placed note block is always the plain one). The 1296 states with a note from 1 to 24
are then free; the pack's `note_block.json` draws each with its block's model. The world keeps the note block
states themselves, so blocks survive restarts with no data of their own, and `plugins/Forja/states.txt` keeps which
state is which block: **keep that file** with the world.

- It is a real, solid, full block: collision, light, pistons and explosions treat it as a note block.
- Breaking it takes a note block's time (an axe is best) and gives its item to the player.
- Vanilla note blocks still work as decoration, but don't play; note blocks tuned before Forja turned them inert
  may show as custom blocks.

## Commands

`/forja` needs `forja.admin` (operators by default).

| Command | What it does |
|---|---|
| `/forja give <player> <item> [amount]` | Gives a Forja item. The item is its id, or the name alone when no other pack has it. |
| `/forja get <item> [amount]` | Gives it to yourself. |
| `/forja list [pack]` | The packs and their items. |
| `/forja pack` | The resource pack: files, size, SHA-1 and URL. |
| `/forja reload` | Reads `config.yml` and the packs again and rebuilds the pack. If it changed, it is sent to every player at once: their textures change without leaving. |

## The resource pack

```yaml
# plugins/Forja/config.yml
resource-pack:
  send: true                 # offer it as players join
  required: false            # declining a required pack means not playing here
  prompt: '&6This server has textures of its own.'
  description: '&6Forja'
  http:
    enabled: true
    host: 0.0.0.0
    port: 8163
  public-url: ''             # http://play.example.com:8163
```

- Forja runs a small HTTP server (port `8163`) that hands out the pack as `/<sha1>.zip`. The name changes with
  the content, so a client never mistakes an old pack for a new one, and keeps it cached while it doesn't
  change. The last few packs stay available, for players who got an older URL.
- **Open the port** to players, as you do the game's. `public-url` is the address they reach it at; empty, Forja
  uses the machine's address on its network, which only works on a LAN. Behind a domain or a proxy, set
  `public-url: http://play.example.com:8163`.
- To host the pack elsewhere (a CDN), turn `http.enabled` off, upload `plugins/Forja/pack.zip`, and set
  `public-url` to the file's full URL. Upload it again after each change.
- Forja sends the pack in game (Paper's `Player.setResourcePack`), so `server.properties` needs no
  `resource-pack`. Leave it empty: a pack set there is a second pack, under Forja's.

## For plugin developers

The API pieces Forja uses are open to any plugin:

- `ItemStack::with_model`, `with_data`/`data`, `with_component`: the `item_model` and `custom_data`
  components, and others for `/give`. Items read from the server (`item_in_hand`, events) carry their model and
  custom data.
- `Player::send_resource_pack(&ResourcePack)`: offers a player a pack.

- `Server::set_note_blocks_inert(bool)`, `Server::day_time()` and `is_day()`.
- `Menu::title` may be a JSON text component (`{"text":"...","font":"ns:gui"}`): a font from the pack draws a
  menu's background.
- Block events (`BlockBreakEvent`, `PlayerInteractEvent`) carry the block's state properties.

A plugin can recognise Forja's items without depending on Forja:

```rust
ctx.on(|_, _, e: &mut PlayerInteractEvent| {
    if e.item.as_ref().and_then(|i| i.data("forja:id")) == Some("exemplo:espada_rubi") {
        e.player.send_action_bar("§cThe ruby glows.");
    }
});
```

### Owning blocks

A plugin that owns blocks (`owner: MeuPlugin`) declares `depend: [Forja]` and answers Forja on the
`forja:block` channel. The message is `{action, block, pos, properties, player}`; `action` is:

- `placed`: after the block is set.
- `broken`: after it is removed (its item was given back unless `drops: none`).
- `used`: a right click. Answer `true` to say the plugin handled it (a menu opened...): the click does nothing
  else. Answer anything else and a block item in hand is placed against it.

Forja's own channels, for any plugin (`server().message_plugin("Forja", channel, message)`):

| Channel | Message | Answer |
|---|---|---|
| `forja:item` | `{id, amount}` | the item, as `ItemStack` on the wire, or null |
| `forja:set_block` | `{block, pos, properties}` | whether the block was set (at the next tick's start) |
| `forja:state` | `{block, properties}` | the note block state it is drawn as (`minecraft:note_block[...]`), for `/setblock` |

Inside its answer to `forja:block`, a plugin can't call Forja back (Forja is busy with the event): do it in
`ctx.run_task`. A plugin can also read its own pack with Forja's library, `forja-core` (Engenho builds its
items with it).

A plugin can ship its content as a Forja pack: write it into `plugins/Forja/packs/<name>/` in `on_load`, which
runs for every plugin before any is enabled (Engenho does).

## Hitboxes, for furniture

Blocks drawn as note block states are full cubes. Furniture (chairs, tables, lamps, anything not a cube) needs a
model drawn by a display entity and a hitbox of its own. What a vanilla client can collide with, from Horizon's
code (which follows vanilla's) and what other furniture plugins do:

| Hitbox | Solid? | Size and place | Notes |
|---|---|---|---|
| **Barrier block** | yes, a full block | on the block grid only | Invisible, no light blocked. Can't be broken in survival: the plugin breaks it. The simplest solid hitbox; Oraxen and Crucible use barriers for collision ([Crucible](https://git.mythiccraft.io/mythiccraft/mythiccrucible/-/wikis/Furniture/), [Oraxen](https://docs.oraxen.com/creating-content/furniture/display-entities)). |
| **Shulker** (invisible, no AI) | yes: players and mobs collide with living shulkers (`canBeCollidedWith`) | a cube anywhere, off the grid. **Its open shell is part of the box**: the box grows toward the face it is attached to by the lid's lift (`getPhysicalPeek`: `Peek` 0 → 1 block, 100 → 2 blocks, smoothly between), so one shulker is 1×1×1 up to 1×2×1, along any of the six axes. Vanilla also scales the whole box with `minecraft:scale` | CraftEngine offers it as its hard-collision hitbox ([CraftEngine](https://xiao-momi.github.io/craft-engine-wiki/configuration/furniture/variants)). With no AI the peek goal doesn't run, so a `Peek` set in its data stays: a fixed height. **On Horizon** both work: the peek box (`mobs/shulker.rs` follows `Shulker.getProgressDeltaAabb` and pushes what it grows into) and `scale` (tried on a server). |
| **Happy ghast** (adult, still) | only to stand on: solid to a player whose feet are above its top, or while a player stands on it (`canBeCollidedWith`, the still timeout) | a 4×4 top, scaled with `minecraft:scale` | Nexo uses it as a scalable collision hitbox from 1.21.6 ([Nexo](https://docs.nexomc.com/mechanics/furniture-mechanic/hitbox/ghast-hitbox)); good for platforms and floors, not walls. |
| **Interaction entity** | no | any width and height, anywhere | Takes clicks and hits only; the usual way to make furniture clickable ([Nexo](https://docs.nexomc.com/mechanics/furniture-mechanic/hitbox/interaction-hitbox)). |
| **Boat** | yes | fixed | Collides like the shulker, but drifts and can be ridden: rarely useful. |

**Plan for Forja's furniture:** an `item_display` with the model; an `interaction` entity to click and break it;
barriers for the solid parts on the grid, and shulkers for parts off it. A box of any size is shulkers: `scale`
sets its width (and the base height), the peek stretches it up to twice that along one axis, and a few shulkers
cover what one can't; happy ghasts give large surfaces to stand on. The engine needs entities by handle (spawn,
move, remove, a click event on interaction entities).

## Roadmap

1. **Items and the pack** (done).
2. **Blocks** (done): states drawn as note block states, placing, breaking, owners.
3. **More for items**: recipes (shaped, shapeless, furnace) and item behaviours in YAML, sounds
   (`sounds.json`), armour with its own textures (`equippable`), emojis and GUI backgrounds (fonts).
4. **Furniture**: display entities with the hitboxes above.
5. **Block behaviours**: hardness and the right tool, crops, strippable logs, seats; more carriers than note blocks
   (tripwire, mushroom blocks) for blocks that are not full or let light through.

## Limits

- A vanilla client knows only vanilla blocks, items and entities. Forja's content is drawn as those, with the
  pack's textures; a client without the pack sees the vanilla thing underneath.
- `components` are given as written; Horizon applies those it models, and keeps the others on the item.
