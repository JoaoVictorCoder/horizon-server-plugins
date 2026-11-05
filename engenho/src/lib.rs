//! Engenho: a tech mod for Horizon servers, made on Forja. Machines grind,
//! smelt and press with energy that generators make and batteries keep;
//! an engineering table makes the parts. Forja draws Engenho's blocks and
//! items and tells Engenho when one of its blocks is placed, broken or used.
//!
//! - [`machines`]: the machines as data, and their recipes (tested alone).
//! - Here: the machines in the world, their menus, ticks and save file.

pub mod machines;

use forja_core::content::{Content, ItemDef};
use horizon_plugin_api::prelude::*;
use horizon_plugin_api::types::Wire;
use machines::{Kind, Machine, Stack};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// The pack, embedded by build.rs: (path under packs/, bytes).
const PACK: &[(&str, &[u8])] = include!(concat!(env!("OUT_DIR"), "/pack_files.rs"));

/// A block in the world: world, x, y, z.
type Key = (String, i32, i32, i32);

fn key(pos: &BlockPos) -> Key {
    (pos.world.clone(), pos.x, pos.y, pos.z)
}

fn pos_of(k: &Key) -> BlockPos {
    BlockPos::new(&k.0, k.1, k.2, k.3)
}

/// What a menu a player has open shows.
#[derive(Clone, Debug, PartialEq)]
enum MenuOf {
    Machine(Key),
    Table,
}

struct OpenMenu {
    player: Uuid,
    window: i32,
    of: MenuOf,
}

/// Menu slots of a machine's menu (3 rows).
const SLOT_INPUT: usize = 10;
const SLOT_PROGRESS: usize = 12;
const SLOT_OUTPUT: usize = 14;
const SLOT_ENERGY: usize = 16;
const SLOT_SPEED: usize = 20;
const SLOT_EFFICIENCY: usize = 21;
const SLOT_INFO: usize = 24;

#[derive(Default)]
pub struct Engenho {
    /// Engenho's items, read from its own pack, to build them as Forja does.
    items: Content,
    machines: HashMap<Key, Machine>,
    menus: Vec<OpenMenu>,
    /// Players whose click moved items this tick: their inventory changes at
    /// the next tick's start, so another click this tick would see it stale.
    busy: Vec<Uuid>,
    dirty: bool,
    tick: u64,
    rng: u64,
}

impl Engenho {
    fn save_file(ctx: &Ctx<Self>) -> PathBuf {
        ctx.data_folder().join("machines.json")
    }

    fn load_machines(&mut self, ctx: &Ctx<Self>) {
        let Ok(text) = fs::read_to_string(Self::save_file(ctx)) else {
            return;
        };
        let Ok(v) = Value::from_json(&text) else {
            ctx.logger()
                .severe("machines.json is damaged: no machines loaded");
            return;
        };
        for entry in v.as_list().unwrap_or_default() {
            let pos = entry.get("pos").and_then(BlockPos::from_wire);
            let machine = entry.get("machine").and_then(Machine::from_value);
            if let (Some(pos), Some(m)) = (pos, machine) {
                self.machines.insert(key(&pos), m);
            }
        }
        ctx.logger()
            .info(&format!("{} machine(s) loaded", self.machines.len()));
    }

    fn save(&mut self, ctx: &Ctx<Self>) {
        let list: Vec<Value> = self
            .machines
            .iter()
            .map(|(k, m)| {
                Value::map()
                    .with("pos", pos_of(k).to_wire())
                    .with("machine", m.to_value())
            })
            .collect();
        let _ = fs::create_dir_all(ctx.data_folder());
        match fs::write(Self::save_file(ctx), Value::List(list).to_json()) {
            Ok(()) => self.dirty = false,
            Err(e) => ctx
                .logger()
                .severe(&format!("could not write machines.json: {e}")),
        }
    }

    /// `n` of `id`: Engenho's items as Forja builds them, vanilla ones plain.
    fn stack(&self, id: &str, n: u32) -> ItemStack {
        match self.items.items.iter().find(|i| i.id == id) {
            Some(def) => def.stack(n),
            None => ItemStack::new(id, n),
        }
    }

    fn def(&self, id: &str) -> Option<&ItemDef> {
        self.items.items.iter().find(|i| i.id == id)
    }

    /// What an item is, for recipes: an Engenho item by its mark, a vanilla
    /// item by its kind when nothing was done to it (not renamed, not
    /// enchanted...), else nothing.
    fn id_of(stack: &ItemStack) -> Option<String> {
        if let Some(id) = stack.data(forja_core::content::ID_TAG) {
            return Some(id.to_owned());
        }
        stack.is_plain().then(|| stack.kind.clone())
    }

    fn give(&self, player: &Player, id: &str, mut n: u32) {
        while n > 0 {
            let take = n.min(machines::STACK);
            player.give_item(&self.stack(id, take));
            n -= take;
        }
    }

    /// Draws a machine's block on or off.
    fn show_active(ctx: &Ctx<Self>, k: &Key, m: &Machine) {
        let message = Value::map()
            .with("block", m.kind.block())
            .with("pos", pos_of(k).to_wire())
            .with(
                "properties",
                Value::map()
                    .with("facing", &m.facing)
                    .with("active", if m.active { "true" } else { "false" }),
            );
        let _ = ctx
            .server()
            .message_plugin("Forja", "forja:set_block", message);
    }

    /// One tick of every machine, then energy to their neighbours.
    fn tick_all(&mut self, ctx: &mut Ctx<Self>) {
        self.tick += 1;
        let day = ctx.server().is_day();
        let mut changed = Vec::new();
        for (k, m) in &mut self.machines {
            let was = m.active;
            m.tick(day);
            if m.active != was {
                changed.push(k.clone());
            }
        }
        // Energy: each giver hands some to each neighbour that takes it.
        let givers: Vec<Key> = self
            .machines
            .iter()
            .filter(|(_, m)| m.kind.gives_energy() && m.energy > 0)
            .map(|(k, _)| k.clone())
            .collect();
        for k in givers {
            for (d, side) in machines::SIDES {
                let n = (k.0.clone(), k.1 + d[0], k.2 + d[1], k.3 + d[2]);
                if n == k || !self.machines.contains_key(&n) {
                    continue;
                }
                let Some(mut from) = self.machines.remove(&k) else {
                    break;
                };
                if let Some(to) = self.machines.get_mut(&n) {
                    machines::transfer(&mut from, to, side);
                }
                self.machines.insert(k.clone(), from);
            }
        }
        for k in changed {
            if let Some(m) = self.machines.get(&k) {
                Self::show_active(ctx, &k, m);
            }
        }
        if !self.machines.is_empty() {
            self.dirty = true;
        }
        if self.tick % 10 == 0 {
            self.refresh_menus();
        }
        if self.tick % (20 * 60) == 0 && self.dirty {
            self.save(ctx);
        }
    }

    /// A pseudo-random number in 0..1.
    fn random(&mut self) -> f64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 11) as f64 / (1u64 << 53) as f64
    }

    // ---- Menus -------------------------------------------------------------

    fn gui(&self, name: &str) -> ItemStack {
        self.stack(&format!("engenho:{name}"), 1)
    }

    fn filler(&self) -> ItemStack {
        self.gui("gui_vazio")
    }

    /// A machine's menu, as it is now.
    fn machine_menu(&self, m: &Machine) -> Menu {
        let mut menu = Menu::new(3, &format!("§8{}", m.kind.name()));
        for slot in 0..27 {
            menu.set(slot, Some(self.filler()));
        }
        let shown = |s: &Option<Stack>| s.as_ref().map(|s| self.stack(&s.id, s.count));
        let fill = |value: u64, max: u64, steps: u64| {
            if max == 0 {
                0
            } else {
                (value * steps).div_ceil(max).min(steps)
            }
        };
        let energy = self
            .gui(&format!(
                "gui_energia_{}",
                fill(m.energy, m.kind.capacity(), 8)
            ))
            .named(&format!(
                "§eEnergia: §f{} §7/ {}",
                machines::energy_text(m.energy),
                machines::energy_text(m.kind.capacity())
            ));
        menu.set(SLOT_ENERGY, Some(energy));
        match m.kind {
            k if k.processes() => {
                menu.set(SLOT_INPUT, shown(&m.input).or(Some(self.filler())));
                menu.set(SLOT_OUTPUT, shown(&m.output).or(Some(self.filler())));
                let step = fill(u64::from(m.progress), u64::from(m.ticks()), 8);
                let progress = self.gui(&format!("gui_progresso_{step}")).named(&format!(
                    "§7Progresso: §f{}%",
                    u64::from(m.progress) * 100 / u64::from(m.ticks())
                ));
                menu.set(SLOT_PROGRESS, Some(progress));
                let upgrade = |id: &str, n: u32| {
                    if n == 0 {
                        self.filler().named("§7Sem upgrades")
                    } else {
                        self.stack(id, n)
                    }
                };
                menu.set(SLOT_SPEED, Some(upgrade(machines::SPEED_UPGRADE, m.speed)));
                menu.set(
                    SLOT_EFFICIENCY,
                    Some(upgrade(machines::EFFICIENCY_UPGRADE, m.efficiency)),
                );
                let info = self
                    .stack(m.kind.block(), 1)
                    .named(&format!("§f{}", m.kind.name()))
                    .lore_line(&format!("§7Gasto: §f{} E/t", m.usage()))
                    .lore_line(&format!("§7Tempo: §f{:.1} s", f64::from(m.ticks()) / 20.0))
                    .lore_line("§8Clique num item do seu inventário")
                    .lore_line("§8para colocá-lo; clique aqui para tirar.");
                menu.set(SLOT_INFO, Some(info));
            }
            Kind::Generator => {
                menu.set(SLOT_INPUT, shown(&m.input).or(Some(self.filler())));
                let step = fill(u64::from(m.burn), u64::from(m.burn_total), 4);
                menu.set(
                    SLOT_PROGRESS,
                    Some(
                        self.gui(&format!("gui_chama_{step}"))
                            .named(&format!("§7Queimando: §f{} s", m.burn / 20)),
                    ),
                );
                let info = self
                    .stack(m.kind.block(), 1)
                    .named("§fGerador a Combustão")
                    .lore_line(&format!("§7Gera: §f{} E/t", m.kind.rate()))
                    .lore_line("§8Clique num combustível do seu inventário.");
                menu.set(SLOT_INFO, Some(info));
            }
            _ => {
                let info = self
                    .stack(m.kind.block(), 1)
                    .named(&format!("§f{}", m.kind.name()))
                    .lore_line(&match m.kind {
                        Kind::Solar => format!("§7Gera de dia: §f{} E/t", m.kind.rate()),
                        _ => format!("§7Entrega até: §f{} E/t", m.kind.output_rate()),
                    });
                menu.set(13, Some(info));
            }
        }
        menu
    }

    /// The engineering table's menu: a recipe in each slot.
    fn table_menu(&self, player: &Player) -> Menu {
        let inventory = Self::snapshot(player);
        let mut menu = Menu::new(3, "§8Bancada de Engenharia");
        for (i, recipe) in machines::TABLE.iter().enumerate().take(27) {
            let ok = machines::can_craft(&inventory, recipe);
            let mut item = self.stack(recipe.result, recipe.count);
            item.lore.clear();
            item = item.lore_line(if ok {
                "§aClique para montar"
            } else {
                "§cFaltam ingredientes"
            });
            for (id, n) in recipe.ingredients {
                let name = self
                    .def(id)
                    .and_then(|d| d.name.clone())
                    .unwrap_or_else(|| id.trim_start_matches("minecraft:").replace('_', " "));
                let have: u32 = inventory
                    .iter()
                    .filter(|(i, _)| i == id)
                    .map(|(_, c)| c)
                    .sum();
                let mark = if have >= *n { "§a" } else { "§c" };
                item = item.lore_line(&format!("{mark}{n}× §7{}", color::strip(&name)));
            }
            menu.set(i, Some(item));
        }
        menu
    }

    fn menu_for(&self, of: &MenuOf, player: &Player) -> Option<Menu> {
        match of {
            MenuOf::Machine(k) => Some(self.machine_menu(self.machines.get(k)?)),
            MenuOf::Table => Some(self.table_menu(player)),
        }
    }

    fn open(&mut self, player: &Player, of: MenuOf) {
        let Some(menu) = self.menu_for(&of, player) else {
            return;
        };
        if let Some(window) = player.open_menu(&menu) {
            self.menus.retain(|m| m.player != player.uuid);
            self.menus.push(OpenMenu {
                player: player.uuid,
                window,
                of,
            });
        }
    }

    fn refresh_menus(&self) {
        for open in &self.menus {
            let MenuOf::Machine(k) = &open.of else {
                continue;
            };
            if let (Some(m), Some(p)) = (self.machines.get(k), server().player_by_uuid(open.player))
            {
                p.set_menu_items(&self.machine_menu(m).items);
            }
        }
    }

    fn refresh(&self, player: &Player, of: &MenuOf) {
        if let Some(menu) = self.menu_for(of, player) {
            player.set_menu_items(&menu.items);
        }
    }

    /// A player's 36 slots as (item id, count); items recipes can't use count as nothing.
    fn snapshot(player: &Player) -> Vec<(String, u32)> {
        player
            .inventory()
            .unwrap_or_default()
            .iter()
            .map(|s| match s {
                Some(s) => (Self::id_of(s).unwrap_or_default(), s.amount),
                None => (String::new(), 0),
            })
            .collect()
    }

    /// Sets inventory slot `slot` to `n` of `id` (0: empty).
    fn set_slot(&self, player: &Player, slot: usize, id: &str, n: u32) {
        let stack = (n > 0).then(|| self.stack(id, n));
        player.set_slot(slot as u8, stack.as_ref());
    }

    /// A click in a machine's menu.
    fn click_machine(&mut self, player: &Player, k: &Key, slot: i32) {
        let Some(m) = self.machines.get_mut(k) else {
            return;
        };
        let mut give: Vec<(String, u32)> = Vec::new();
        match usize::try_from(slot) {
            // Out of the machine, into the player's inventory.
            Ok(SLOT_INPUT) => give.extend(m.input.take().map(|s| (s.id, s.count))),
            Ok(SLOT_OUTPUT) => give.extend(m.output.take().map(|s| (s.id, s.count))),
            Ok(SLOT_SPEED) if m.kind.processes() && m.speed > 0 => {
                give.push((machines::SPEED_UPGRADE.into(), m.speed));
                m.speed = 0;
            }
            Ok(SLOT_EFFICIENCY) if m.kind.processes() && m.efficiency > 0 => {
                give.push((machines::EFFICIENCY_UPGRADE.into(), m.efficiency));
                m.efficiency = 0;
            }
            // From the player's inventory into the machine.
            Ok(s) if (27..63).contains(&s) => {
                let index = if s - 27 < 27 { s - 27 + 9 } else { s - 27 - 27 };
                let inventory = player.inventory().unwrap_or_default();
                let Some(Some(stack)) = inventory.get(index) else {
                    return;
                };
                let Some(id) = Self::id_of(stack) else {
                    return;
                };
                let n = stack.amount;
                let moved = if id == machines::SPEED_UPGRADE && m.kind.processes() {
                    let fit = n.min(machines::MAX_UPGRADES - m.speed);
                    m.speed += fit;
                    fit
                } else if id == machines::EFFICIENCY_UPGRADE && m.kind.processes() {
                    let fit = n.min(machines::MAX_UPGRADES - m.efficiency);
                    m.efficiency += fit;
                    fit
                } else if m.accepts(&id) {
                    machines::put(&mut m.input, &id, n)
                } else {
                    0
                };
                if moved == 0 {
                    return;
                }
                let left = n - moved;
                let stack = (left > 0).then(|| self_stack(&self.items, &id, left));
                player.set_slot(index as u8, stack.as_ref());
            }
            _ => return,
        }
        for (id, n) in give {
            self.give(player, &id, n);
        }
        self.dirty = true;
        self.busy.push(player.uuid);
        let of = MenuOf::Machine(k.clone());
        self.refresh(player, &of);
    }

    /// A click on a recipe of the engineering table.
    fn click_table(&mut self, player: &Player, slot: i32) {
        let Some(recipe) = usize::try_from(slot)
            .ok()
            .and_then(|s| machines::TABLE.get(s))
        else {
            return;
        };
        let mut inventory = Self::snapshot(player);
        if !machines::can_craft(&inventory, recipe) {
            player.send_action_bar("§cFaltam ingredientes.");
            return;
        }
        for slot in machines::take_ingredients(&mut inventory, recipe) {
            let (id, n) = inventory[slot].clone();
            self.set_slot(player, slot, &id, n);
        }
        self.give(player, recipe.result, recipe.count);
        player.play_sound("minecraft:block.anvil.use", 0.5, 1.4);
        self.busy.push(player.uuid);
    }
}

/// `n` of `id`, from Engenho's items or vanilla.
fn self_stack(items: &Content, id: &str, n: u32) -> ItemStack {
    match items.items.iter().find(|i| i.id == id) {
        Some(def) => def.stack(n),
        None => ItemStack::new(id, n),
    }
}

impl Plugin for Engenho {
    fn on_load(&mut self, ctx: &mut Ctx<Self>) {
        // Engenho's content goes into Forja's packs before Forja reads them.
        let Some(plugins) = ctx.data_folder().parent() else {
            return;
        };
        let dir = plugins.join("Forja").join("packs");
        for (path, bytes) in PACK {
            let file = dir.join(path);
            if let Some(parent) = file.parent()
                && let Err(e) = fs::create_dir_all(parent)
            {
                ctx.logger().severe(&format!("{}: {e}", parent.display()));
                return;
            }
            if fs::read(&file).ok().as_deref() != Some(*bytes)
                && let Err(e) = fs::write(&file, bytes)
            {
                ctx.logger().severe(&format!("{}: {e}", file.display()));
            }
        }
    }

    fn on_enable(&mut self, ctx: &mut Ctx<Self>) {
        ctx.save_default_config(include_str!("../config.yml"));
        for (path, bytes) in PACK {
            if path.ends_with(".yml") && path.contains("/config/") {
                if let Ok(v) = horizon_plugin_api::yaml::parse(&String::from_utf8_lossy(bytes)) {
                    self.items.load_config(path, "engenho", "engenho", &v);
                }
            }
        }
        self.rng = 0x9E37_79B9_7F4A_7C15 ^ (ctx.server().current_tick() as u64 | 1);
        self.load_machines(ctx);

        ctx.on_message("forja:block", |engenho, ctx, m| {
            let v = &m.message;
            let action = v.get("action").and_then(Value::to_text).unwrap_or_default();
            let block = v.get("block").and_then(Value::to_text).unwrap_or_default();
            let (Some(pos), Some(player)) = (
                v.get("pos").and_then(BlockPos::from_wire),
                v.get("player").and_then(Player::from_wire),
            ) else {
                return Value::Null;
            };
            let k = key(&pos);
            let props = v.get("properties");
            let facing = props
                .and_then(|p| p.get("facing"))
                .and_then(Value::to_text)
                .unwrap_or_else(|| "north".into());
            match action.as_str() {
                "placed" => {
                    if let Some(kind) = Kind::from_block(&block) {
                        engenho.machines.insert(k, Machine::new(kind, &facing));
                        engenho.dirty = true;
                    }
                    Value::Null
                }
                "broken" => {
                    if let Some(m) = engenho.machines.remove(&k) {
                        for s in m.contents() {
                            engenho.give(&player, &s.id, s.count);
                        }
                        engenho.dirty = true;
                    }
                    let of = MenuOf::Machine(k);
                    for open in engenho.menus.iter().filter(|o| o.of == of) {
                        if let Some(p) = ctx.server().player_by_uuid(open.player) {
                            p.close_menu();
                        }
                    }
                    engenho.menus.retain(|o| o.of != of);
                    Value::Null
                }
                "used" => {
                    let of = if block == "engenho:bancada" {
                        MenuOf::Table
                    } else if engenho.machines.contains_key(&k) {
                        MenuOf::Machine(k)
                    } else if let Some(kind) = Kind::from_block(&block) {
                        // A machine the save lost: it starts again empty.
                        engenho
                            .machines
                            .insert(k.clone(), Machine::new(kind, &facing));
                        MenuOf::Machine(k)
                    } else {
                        return Value::Bool(false);
                    };
                    ctx.run_task(move |engenho, _| engenho.open(&player, of));
                    Value::Bool(true)
                }
                _ => Value::Null,
            }
        });

        ctx.on(|engenho: &mut Engenho, _, e: &mut InventoryClickEvent| {
            if engenho.busy.contains(&e.player.uuid) {
                return;
            }
            let Some(open) = engenho
                .menus
                .iter()
                .find(|m| m.player == e.player.uuid && m.window == e.window)
            else {
                return;
            };
            match open.of.clone() {
                MenuOf::Machine(k) => engenho.click_machine(&e.player, &k, e.slot),
                MenuOf::Table => {
                    engenho.click_table(&e.player, e.slot);
                    // Show what can be made now, once the inventory changed.
                }
            }
        });
        ctx.on(|engenho: &mut Engenho, _, e: &mut InventoryCloseEvent| {
            engenho
                .menus
                .retain(|m| !(m.player == e.player.uuid && m.window == e.window));
        });

        // Cobalt: mining stone deep down sometimes gives raw cobalt.
        ctx.listen(
            EventPriority::Monitor,
            true,
            |engenho: &mut Engenho, ctx, e: &mut BlockBreakEvent| {
                let deep = matches!(
                    e.block.kind.as_str(),
                    "minecraft:deepslate" | "minecraft:tuff"
                );
                let stone = matches!(
                    e.block.kind.as_str(),
                    "minecraft:stone"
                        | "minecraft:andesite"
                        | "minecraft:diorite"
                        | "minecraft:granite"
                );
                if !(deep || stone) || e.player.game_mode != GameMode::Survival {
                    return;
                }
                let max_y = ctx.config().int_or("cobalt.max-y", 48);
                if i64::from(e.block.pos.y) > max_y {
                    return;
                }
                let chance = if deep {
                    ctx.config().f64_or("cobalt.chance-deepslate", 0.03)
                } else {
                    ctx.config().f64_or("cobalt.chance-stone", 0.015)
                };
                if engenho.random() < chance {
                    e.player
                        .give_item(&engenho.stack("engenho:cobalto_bruto", 1));
                    e.player.send_action_bar("§9Você encontrou cobalto!");
                }
            },
        );

        // A crafting table with an iron block used on it becomes an engineering table.
        ctx.listen(
            EventPriority::Normal,
            true,
            |engenho: &mut Engenho, ctx, e: &mut PlayerInteractEvent| {
                if e.action != "right_click_block" {
                    return;
                }
                let (Some(block), Some(item)) = (e.block.clone(), e.item.clone()) else {
                    return;
                };
                if !block.is("crafting_table") || !item.is("iron_block") || !item.is_plain() {
                    return;
                }
                e.set_cancelled(true);
                if engenho.busy.contains(&e.player.uuid) {
                    return;
                }
                let message = Value::map()
                    .with("block", "engenho:bancada")
                    .with("pos", block.pos.to_wire());
                let placed = ctx
                    .server()
                    .message_plugin("Forja", "forja:set_block", message)
                    .ok()
                    .and_then(|v| v.as_bool())
                    == Some(true);
                if !placed {
                    return;
                }
                if e.player.game_mode != GameMode::Creative {
                    let left = item.amount.saturating_sub(1);
                    let stack = (left > 0).then(|| ItemStack::new("iron_block", left));
                    e.player.set_equipment("mainhand", stack.as_ref());
                }
                e.player
                    .play_sound("minecraft:block.smithing_table.use", 1.0, 1.0);
                e.player
                    .send_action_bar("§fA mesa virou uma §aBancada de Engenharia§f.");
                engenho.busy.push(e.player.uuid);
            },
        );

        ctx.run_timer(1, 1, |engenho, ctx| {
            engenho.busy.clear();
            engenho.tick_all(ctx);
        });

        ctx.set_executor("engenho", |engenho, ctx, sender, _, args| {
            match args.first().map(String::as_str) {
                Some("give") => {
                    let (Some(name), Some(id)) = (args.get(1), args.get(2)) else {
                        return false;
                    };
                    let id = if id.contains(':') {
                        id.clone()
                    } else {
                        format!("engenho:{id}")
                    };
                    let Some(def) = engenho.def(&id) else {
                        sender.send_message(&format!("§cO Engenho não tem o item {id}."));
                        return true;
                    };
                    let n = args
                        .get(3)
                        .and_then(|a| a.parse::<u32>().ok())
                        .unwrap_or(1)
                        .clamp(1, 640);
                    let Some(p) = ctx.server().match_player(name) else {
                        sender.send_message(&format!("§cNenhum jogador {name} online."));
                        return true;
                    };
                    let id = def.id.clone();
                    engenho.give(&p, &id, n);
                    sender.send_message(&format!("§aDei {n} {id} a {}.", p.name));
                    true
                }
                Some("info") => {
                    let mut by_kind: HashMap<&str, usize> = HashMap::new();
                    for m in engenho.machines.values() {
                        *by_kind.entry(m.kind.name()).or_default() += 1;
                    }
                    sender.send_message(&format!(
                        "§6Engenho§7: {} máquina(s)",
                        engenho.machines.len()
                    ));
                    for (k, n) in by_kind {
                        sender.send_message(&format!("§7- {k}: §f{n}"));
                    }
                    true
                }
                _ => false,
            }
        });
        ctx.set_tab_completer("engenho", |engenho, ctx, _, _, args| match args.len() {
            0 | 1 => vec!["give".into(), "info".into()],
            2 if args[0] == "give" => ctx
                .server()
                .online_players()
                .into_iter()
                .map(|p| p.name)
                .collect(),
            3 if args[0] == "give" => engenho
                .items
                .items
                .iter()
                .filter(|i| !i.id.contains(":gui_"))
                .map(|i| i.id.trim_start_matches("engenho:").to_owned())
                .collect(),
            _ => Vec::new(),
        });
    }

    fn on_disable(&mut self, ctx: &mut Ctx<Self>) {
        for open in &self.menus {
            if let Some(p) = ctx.server().player_by_uuid(open.player) {
                p.close_menu();
            }
        }
        self.save(ctx);
    }
}

horizon_plugin!(Engenho::default(), plugin_yml!());
