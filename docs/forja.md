# Forja

Forja makes **server-side mods**: content with textures, models and sounds of its own that vanilla clients see
without installing anything. It's in the spirit of Paper's CraftEngine, ItemsAdder and Oraxen, written from
scratch for Horizon (no code from them). The server builds a resource pack from Forja's packs, serves it over HTTP
and sends it to each player as they join.

Forja is a plugin like any other (`forja/` in this repository). `./build.sh` puts `libforja.so` in
`dist/plugins/`; copy it to the server's `plugins/`.

> **Status: phase 1.** Custom items with their own textures or models, the resource pack built, served and sent
> again on `/forja reload`. Custom blocks, furniture, recipes and GUIs come next ([roadmap](#roadmap)).

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

A plugin can recognise Forja's items without depending on Forja:

```rust
ctx.on(|_, _, e: &mut PlayerInteractEvent| {
    if e.item.as_ref().and_then(|i| i.data("forja:id")) == Some("exemplo:espada_rubi") {
        e.player.send_action_bar("§cThe ruby glows.");
    }
});
```

## Roadmap

1. **Items and the pack** (done): packs, items with textures or models, the resource pack built, served and sent.
2. **More for items**: recipes (shaped, shapeless, furnace), item behaviours in YAML (on use: a command, a sound,
   an effect), sounds (`sounds.json` from the packs), armour with its own textures (`equippable`), emojis and
   GUI backgrounds (fonts).
3. **Custom blocks** that exist on the server: registered as new blocks, with their own hardness, drops and
   states, and drawn on clients as an unused vanilla block state (note blocks, tripwire, mushroom blocks...) that
   the pack retextures. Horizon's engine is patched here, so the swap happens where chunks and block changes are
   written, not by rewriting packets. Needs the block registry to take blocks outside the game's data.
4. **Block behaviours**: crops, strippable logs, seats, facing.
5. **Furniture**: `item_display` entities with a model and a hitbox (needs entities by handle).

## Limits

- A vanilla client knows only vanilla blocks, items and entities. Forja's content is drawn as those, with the
  pack's textures; a client without the pack sees the vanilla thing underneath.
- `components` are given as written; Horizon applies those it models, and keeps the others on the item.
