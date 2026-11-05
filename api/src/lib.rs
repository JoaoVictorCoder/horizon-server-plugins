//! Server plugins for Horizon, in Rust, shaped like Bukkit and Paper's
//! plugin API: a `plugin.yml`, `on_enable`/`on_disable`, listeners with
//! priorities and cancelling, commands with permissions, a scheduler
//! counted in ticks, `config.yml`, and `§`/`&` colour codes.
//!
//! A plugin is a `cdylib` crate that drops into the server's `plugins/`
//! folder like a Paper jar (docs/plugins.md):
//!
//! ```no_run
//! use horizon_plugin_api::prelude::*;
//!
//! #[derive(Default)]
//! struct Greeter {
//!     greeted: u32,
//! }
//!
//! impl Plugin for Greeter {
//!     fn on_enable(&mut self, ctx: &mut Ctx<Self>) {
//!         ctx.save_default_config("greeting: '&aWelcome, %player%!'\n");
//!         ctx.on(|plugin: &mut Greeter, ctx, e: &mut PlayerJoinEvent| {
//!             plugin.greeted += 1;
//!             let text = ctx.config().string_or("greeting", "Hi").replace("%player%", &e.player.name);
//!             e.player.send_message(&color::translate('&', &text));
//!         });
//!         ctx.listen(EventPriority::High, true, |_, _, e: &mut BlockBreakEvent| {
//!             if e.block.is("bedrock") && !e.player.has_permission("greeter.bedrock") {
//!                 e.set_cancelled(true);
//!             }
//!         });
//!         ctx.command(CommandSpec::new("greeted"), |plugin, _, sender, _, _| {
//!             sender.send_message(&format!("{} players greeted", plugin.greeted));
//!             true
//!         });
//!         ctx.run_timer(20 * 60, 20 * 60, |_, ctx| {
//!             ctx.server().broadcast_message("§7A minute went by.");
//!         });
//!     }
//! }
//!
//! horizon_plugin!(Greeter::default(), PluginDescription::new("Greeter", "1.0.0"));
//! ```

pub mod abi;
pub mod color;
pub mod config;
pub mod description;
pub mod event;
pub mod runtime;
pub mod scoreboard;
pub mod server;
pub mod types;
pub mod value;
pub mod yaml;

pub use config::Config;
pub use description::{CommandSpec, LoadOrder, Permission, PermissionDefault, PluginDescription};
pub use runtime::{Ctx, Plugin, PluginMessage, SyncHandle, TaskId};
pub use server::{Logger, PluginInfo, Server, server};
pub use value::Value;

/// Everything a plugin usually needs.
pub mod prelude {
    pub use crate::color::{self, ChatColor};
    pub use crate::event::*;
    pub use crate::types::{
        Block, BlockFace, BlockPos, CommandSender, Entity, GameMode, ItemStack, Location, Menu,
        Player, ResourcePack, Uuid, World, worlds,
    };
    pub use crate::scoreboard::{BarColor, BarStyle, BossBar, DisplaySlot, Objective, Team};
    pub use crate::{
        CommandSpec, Config, Ctx, LoadOrder, Permission, PermissionDefault, Plugin,
        PluginDescription, PluginMessage, Server, SyncHandle, TaskId, Value, horizon_plugin,
        plugin_yml, server,
    };
}

/// Exports a plugin: `horizon_plugin!(constructor, description)`, once in
/// a `cdylib` crate. The description is a [`PluginDescription`], often
/// [`plugin_yml!()`].
#[macro_export]
macro_rules! horizon_plugin {
    ($plugin:expr, $description:expr $(,)?) => {
        /// The entry point the server looks for.
        ///
        /// # Safety
        /// Called by the server with its `HostApi`.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn horizon_plugin_entry_v1(
            host: *const $crate::abi::HostApi,
        ) -> $crate::abi::PluginApi {
            // SAFETY: the server keeps `host` alive for the plugin's life.
            unsafe { $crate::runtime::entry(host, || $plugin, || $description) }
        }
    };
}

/// The crate's own `plugin.yml`, read when the plugin loads.
#[macro_export]
macro_rules! plugin_yml {
    () => {
        $crate::PluginDescription::from_yaml(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/plugin.yml"
        )))
        .expect("plugin.yml")
    };
}
