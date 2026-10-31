//! Homes: an example Horizon plugin. `/sethome [name]`, `/home [name]`,
//! `/delhome <name>`, `/homes` (a menu: click one to go there), a limit
//! from `config.yml`, and a protected spawn. Homes are kept in
//! `plugins/Homes/homes.yml`.

use horizon_plugin_api::prelude::*;

#[derive(Default)]
struct Homes {
    /// `<uuid>.<home>` → place.
    store: Option<Config>,
    /// The `/homes` menu each player has open: its window and its homes by slot.
    menus: Vec<(Uuid, i32, Vec<String>)>,
}

impl Homes {
    fn store(&mut self, ctx: &Ctx<Self>) -> &mut Config {
        self.store.get_or_insert_with(|| {
            let path = ctx.data_folder().join("homes.yml");
            Config::load(&path).unwrap_or_else(|e| {
                ctx.logger().severe(&format!("homes.yml: {e}"));
                Config::new(path)
            })
        })
    }

    fn names(&mut self, ctx: &Ctx<Self>, who: Uuid) -> Vec<String> {
        self.store(ctx).keys(&who.to_string(), false)
    }

    fn get(&mut self, ctx: &Ctx<Self>, who: Uuid, name: &str) -> Option<Location> {
        let v = self.store(ctx).get(&format!("{who}.{name}"))?.clone();
        Some(Location {
            world: v.get("world")?.to_text()?,
            x: v.get("x")?.as_f64()?,
            y: v.get("y")?.as_f64()?,
            z: v.get("z")?.as_f64()?,
            yaw: v.get("yaw").and_then(Value::as_f64).unwrap_or(0.0) as f32,
            pitch: v.get("pitch").and_then(Value::as_f64).unwrap_or(0.0) as f32,
        })
    }

    fn save(&mut self, ctx: &Ctx<Self>) {
        if let Err(e) = self.store(ctx).save() {
            ctx.logger().severe(&e);
        }
    }
}

fn message(ctx: &mut Ctx<Homes>, key: &str, pairs: &[(&str, &str)]) -> String {
    let mut text = ctx.config().string_or(&format!("messages.{key}"), key);
    for (k, v) in pairs {
        text = text.replace(&format!("%{k}%"), v);
    }
    color::translate('&', &text)
}

/// A home's name: the first argument or `home`, lower case.
fn home_name(args: &[String]) -> Option<String> {
    let name = args
        .first()
        .map_or("home", String::as_str)
        .to_ascii_lowercase();
    let ok = name.len() <= 24
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    ok.then_some(name)
}

fn in_spawn(ctx: &mut Ctx<Homes>, pos: &BlockPos) -> bool {
    let r = ctx.config().int_or("spawn-radius", 16);
    r > 0
        && pos.world == worlds::OVERWORLD
        && i64::from(pos.x).abs() <= r
        && i64::from(pos.z).abs() <= r
}

impl Plugin for Homes {
    fn on_enable(&mut self, ctx: &mut Ctx<Self>) {
        ctx.save_default_config(include_str!("config.yml"));

        ctx.on(|_, ctx, e: &mut PlayerJoinEvent| {
            let text = message(ctx, "welcome", &[("player", &e.player.name)]);
            e.player.send_message(&text);
        });

        // Spawn protection, before other plugins' Normal listeners decide.
        ctx.listen(
            EventPriority::Low,
            true,
            |_, ctx, e: &mut BlockBreakEvent| {
                if in_spawn(ctx, &e.block.pos) && !e.player.has_permission("homes.spawn.build") {
                    e.set_cancelled(true);
                    e.player.send_action_bar(&message(ctx, "spawn", &[]));
                }
            },
        );
        ctx.listen(
            EventPriority::Low,
            true,
            |_, ctx, e: &mut BlockPlaceEvent| {
                if in_spawn(ctx, &e.block) && !e.player.has_permission("homes.spawn.build") {
                    e.set_cancelled(true);
                    e.player.send_action_bar(&message(ctx, "spawn", &[]));
                }
            },
        );
        // Explosions spare the spawn's blocks.
        ctx.listen(
            EventPriority::Low,
            true,
            |_, ctx, e: &mut EntityExplodeEvent| {
                let blocks = std::mem::take(&mut e.blocks);
                e.blocks = blocks.into_iter().filter(|b| !in_spawn(ctx, b)).collect();
            },
        );

        ctx.set_executor("sethome", |plugin, ctx, sender, _, args| {
            let Some(p) = sender.player() else {
                sender.send_message("Only players have homes.");
                return true;
            };
            let Some(name) = home_name(args) else {
                return false;
            };
            let names = plugin.names(ctx, p.uuid);
            let max = ctx.config().int_or("max-homes", 3).max(0) as usize;
            if !names.contains(&name) && names.len() >= max && !p.has_permission("homes.unlimited")
            {
                p.send_message(&message(ctx, "limit", &[("max", &max.to_string())]));
                return true;
            }
            let l = &p.location;
            let key = format!("{}.{name}", p.uuid);
            let store = plugin.store(ctx);
            store.set(&format!("{key}.world"), l.world.as_str());
            store.set(&format!("{key}.x"), l.x);
            store.set(&format!("{key}.y"), l.y);
            store.set(&format!("{key}.z"), l.z);
            store.set(&format!("{key}.yaw"), l.yaw);
            store.set(&format!("{key}.pitch"), l.pitch);
            plugin.save(ctx);
            p.send_message(&message(ctx, "set", &[("home", &name)]));
            true
        });

        ctx.set_executor("home", |plugin, ctx, sender, _, args| {
            let Some(p) = sender.player() else {
                return true;
            };
            let Some(name) = home_name(args) else {
                return false;
            };
            match plugin.get(ctx, p.uuid, &name) {
                Some(to) => {
                    p.teleport(&to);
                    p.play_sound("entity.enderman.teleport", 1.0, 1.0);
                }
                None => {
                    p.send_message(&message(ctx, "gone", &[("home", &name)]));
                }
            }
            true
        });

        ctx.set_executor("delhome", |plugin, ctx, sender, _, args| {
            let (Some(p), Some(name)) = (sender.player(), args.first()) else {
                return false;
            };
            let name = name.to_ascii_lowercase();
            if plugin.get(ctx, p.uuid, &name).is_none() {
                p.send_message(&message(ctx, "gone", &[("home", &name)]));
                return true;
            }
            plugin
                .store(ctx)
                .set(&format!("{}.{name}", p.uuid), Value::Null);
            plugin.save(ctx);
            p.send_message(&format!("§7Deleted §e{name}§7."));
            true
        });

        ctx.set_executor("homes", |plugin, ctx, sender, _, _| {
            let Some(p) = sender.player() else {
                return true;
            };
            let names = plugin.names(ctx, p.uuid);
            if names.is_empty() {
                p.send_message("§7No homes yet: /sethome");
                return true;
            }
            let rows = names.len().div_ceil(9).clamp(1, 6) as u8;
            let mut menu = Menu::new(rows, "§8Your homes");
            for (slot, name) in names.iter().enumerate().take(54) {
                let at = plugin.get(ctx, p.uuid, name).unwrap_or_default();
                menu.set(
                    slot,
                    Some(
                        ItemStack::new("red_bed", 1)
                            .named(&format!("§e{name}"))
                            .lore_line(&format!(
                                "§7{} {} {}",
                                at.block_x(),
                                at.block_y(),
                                at.block_z()
                            )),
                    ),
                );
            }
            // The menu waits for the next tick: inside a player's command
            // their screen is theirs to answer.
            let uuid = p.uuid;
            ctx.run_task(move |plugin: &mut Homes, ctx| {
                let Some(p) = ctx.server().player_by_uuid(uuid) else {
                    return;
                };
                if let Some(window) = p.open_menu(&menu) {
                    plugin.menus.retain(|(u, _, _)| *u != uuid);
                    plugin.menus.push((uuid, window, names));
                }
            });
            true
        });

        ctx.on(|plugin: &mut Homes, ctx, e: &mut InventoryClickEvent| {
            let Some((_, _, names)) = plugin
                .menus
                .iter()
                .find(|(u, w, _)| *u == e.player.uuid && *w == e.window)
            else {
                return;
            };
            let Some(name) = usize::try_from(e.slot)
                .ok()
                .and_then(|s| names.get(s))
                .cloned()
            else {
                return;
            };
            e.player.close_menu();
            if let Some(to) = plugin.get(ctx, e.player.uuid, &name) {
                e.player.teleport(&to);
            }
        });
        ctx.on(|plugin: &mut Homes, _, e: &mut InventoryCloseEvent| {
            plugin
                .menus
                .retain(|(u, w, _)| !(*u == e.player.uuid && *w == e.window));
        });

        ctx.logger()
            .info("Ready: /sethome, /home, /delhome, /homes");
    }

    fn on_disable(&mut self, ctx: &mut Ctx<Self>) {
        if self.store.is_some() {
            self.save(ctx);
        }
    }
}

// A plugin crate of its own keeps plugin.yml at its root and writes
// `plugin_yml!()`; this example sits beside its plugin.yml.
horizon_plugin!(
    Homes::default(),
    PluginDescription::from_yaml(include_str!("plugin.yml")).expect("plugin.yml")
);
