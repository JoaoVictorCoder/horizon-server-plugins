# Engenho

Engenho is a tech mod for Horizon servers, built on [Forja](forja.md): machines that grind, smelt and press with
energy, generators and batteries, upgrades, and an engineering table for its parts. Vanilla clients see it through
Forja's resource pack; nothing is installed on them. Its names, numbers and textures are its own.

Put `libengenho.so` (`engenho.dll` on Windows) in `plugins/` beside Forja's. On its first start Engenho writes its
content into `plugins/Forja/packs/engenho/`, and Forja draws it.

## Getting started

1. **Cobalt.** Mine stone, andesite, diorite or granite below y 48 (1.5%), or deepslate and tuff (3%): sometimes
   you get **raw cobalt** with the stone. (Cobalt ore in new chunks needs chunk events in the server; until then,
   this is how it is found. The chances are in `plugins/Engenho/config.yml`.)
2. **Engineering table.** Right-click a crafting table holding an **iron block**: it becomes an engineering
   table. Right-click it to see what it makes; click a recipe to make it from what you carry.
3. **The first parts**: a cobalt ingot (2 raw cobalt + 1 coal), a circuit, a machine casing.
4. **Power**: a combustion generator burns what a furnace burns. Put machines **beside** it: each generator and
   battery hands energy to the machines and batteries touching it, every tick.
5. **Machines**: right-click to open. Click an item in your inventory to put it in; click the input, output or an
   upgrade to take it back. The menu shows progress and energy as it works.

## Machines

| Machine | Does | Energy | Time |
|---|---|---|---|
| Moedor (grinder) | raw ore and ores into **2 dusts** (copper ore into 6), ingots into 1 dust, cobblestone → gravel → sand, bones, blaze rods | 20 E/t, holds 20 kE | 10 s |
| Forno Elétrico (electric furnace) | dusts into ingots, raw ore, sand, food, logs into charcoal... | 16 E/t, 20 kE | 5 s |
| Prensa (press) | iron, copper and cobalt ingots into plates | 30 E/t, 20 kE | 8 s |
| Gerador a Combustão (generator) | burns fuel: coal 80 s, coal block 800 s, planks and logs 15 s... | makes 40 E/t, holds 40 kE, gives 80 E/t a side | |
| Painel Solar (solar panel) | energy by day | makes 8 E/t, gives 16 E/t a side | |
| Bateria (battery) | keeps 1 ME; takes energy in on its sides and back, gives it out of its **front** (1000 E/t) | | |

So grinding and smelting ore doubles it: one raw iron → 2 iron dust → 2 iron ingots.

**Upgrades** go in a processing machine, up to 4 of each:

- **Velocidade** (speed): each one takes a share off the time (4: a third of it) and costs more energy a tick.
- **Eficiência** (efficiency): each one cuts the energy a tick.

## Engineering table recipes

| Makes | From |
|---|---|
| Cobalt ingot | 2 raw cobalt, 1 coal |
| Circuit | 4 redstone, 2 copper ingots, 1 iron ingot |
| Machine casing | 6 iron ingots, 2 cobalt ingots |
| Generator | casing, circuit, furnace |
| Grinder | casing, circuit, 4 flint, 1 diamond |
| Electric furnace | casing, circuit, furnace, 4 copper ingots |
| Press | casing, circuit, 2 pistons, 1 iron block |
| Solar panel | casing, circuit, 3 glass, 2 cobalt plates |
| Battery | casing, 2 circuits, 4 redstone blocks, 4 copper plates |
| Speed upgrade | circuit, 8 redstone, 2 cobalt plates |
| Efficiency upgrade | circuit, 8 lapis lazuli, 2 iron plates |

## Commands

`/engenho give <player> <item> [amount]` (`engenho.admin`, operators) and `/engenho info`. Items are named
without `engenho:` (`moedor`, `lingote_cobalto`...); tab completes them.

## What is kept where

- `plugins/Engenho/machines.json`: each machine's energy, items, progress and upgrades, saved every minute and
  when the server stops.
- The blocks themselves are in the world (as Forja's note block states, see `plugins/Forja/states.txt`).

## Limits, for now

- Machines tick wherever they are, loaded or not (the server doesn't tell plugins about chunks yet).
- Menus are click-to-move: items can't be dragged with the cursor into a plugin's menu.
- Machines don't push or pull items into chests and hoppers (block entities aren't reachable by plugins yet), and
  energy passes only between blocks that touch: cables come next.
- Breaking a machine gives its item and its contents to the player who broke it, in their inventory.

## Next

Cables for energy, item pipes, cobalt ore in new chunks (chunk events), machine tiers, more machines (an ore
washer for 3× ore, an alloy smelter), menu backgrounds drawn with a font, and the generator taking lava.
