//! Events, as Bukkit's: a listener registers for one type at a priority,
//! listeners run from `Lowest` to `Monitor` across every plugin, and a
//! cancellable event a listener cancels doesn't happen. `Monitor` only
//! watches: what it changes is not used.

use crate::types::{
    Block, BlockFace, BlockPos, Entity, GameMode, ItemStack, Location, Player, Wire,
};
use crate::value::Value;

/// When a listener runs among the others for its event.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EventPriority {
    Lowest,
    Low,
    #[default]
    Normal,
    High,
    Highest,
    /// Last, to see the outcome; changes are ignored.
    Monitor,
}

impl EventPriority {
    pub const ALL: [EventPriority; 6] = [
        EventPriority::Lowest,
        EventPriority::Low,
        EventPriority::Normal,
        EventPriority::High,
        EventPriority::Highest,
        EventPriority::Monitor,
    ];
    pub fn name(self) -> &'static str {
        match self {
            EventPriority::Lowest => "lowest",
            EventPriority::Low => "low",
            EventPriority::Normal => "normal",
            EventPriority::High => "high",
            EventPriority::Highest => "highest",
            EventPriority::Monitor => "monitor",
        }
    }
    pub fn parse(text: &str) -> Option<EventPriority> {
        EventPriority::ALL
            .into_iter()
            .find(|p| p.name().eq_ignore_ascii_case(text))
    }
}

/// An event plugins can listen to.
pub trait Event: Sized + 'static {
    /// Its name on the wire (`block_break`).
    const NAME: &'static str;
    /// Whether a listener can stop it.
    const CANCELLABLE: bool;
    fn from_data(data: &Value) -> Option<Self>;
    fn to_data(&self) -> Value;
    /// Whether an earlier listener cancelled it.
    fn cancelled(&self) -> bool;
    #[doc(hidden)]
    fn set_cancelled_flag(&mut self, cancelled: bool);
}

/// An event a listener can stop.
pub trait Cancellable: Event {
    fn is_cancelled(&self) -> bool {
        self.cancelled()
    }
    fn set_cancelled(&mut self, cancelled: bool) {
        self.set_cancelled_flag(cancelled);
    }
}

macro_rules! event {
    (@cancel cancellable $name:ident) => {
        impl Cancellable for $name {}
    };
    (@cancel observe $name:ident) => {};
    (@flag cancellable) => { true };
    (@flag observe) => { false };
    (
        $(#[$meta:meta])*
        $kind:ident $name:ident = $id:literal {
            $( $(#[$fmeta:meta])* $field:ident : $ty:ty ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Default, PartialEq)]
        pub struct $name {
            $( $(#[$fmeta])* pub $field: $ty, )*
            cancelled: bool,
        }

        impl $name {
            /// The event with these fields, not cancelled.
            #[allow(clippy::too_many_arguments)]
            pub fn new($( $field: $ty ),*) -> Self {
                $name { $( $field, )* cancelled: false }
            }
        }

        impl Event for $name {
            const NAME: &'static str = $id;
            const CANCELLABLE: bool = event!(@flag $kind);
            fn from_data(data: &Value) -> Option<Self> {
                let _ = data;
                Some($name {
                    $( $field: <$ty as Wire>::from_wire(
                        data.get(stringify!($field)).unwrap_or(&Value::Null),
                    )?, )*
                    cancelled: false,
                })
            }
            fn to_data(&self) -> Value {
                #[allow(unused_mut)]
                let mut v = Value::map();
                $( v.set(stringify!($field), Wire::to_wire(&self.$field)); )*
                v
            }
            fn cancelled(&self) -> bool {
                self.cancelled
            }
            fn set_cancelled_flag(&mut self, cancelled: bool) {
                self.cancelled = cancelled;
            }
        }

        event!(@cancel $kind $name);
    };
}

// What the server and its plugins do.

event! {
    /// A plugin was enabled (also each plugin for itself, after `on_enable`).
    observe PluginEnableEvent = "plugin_enable" { plugin: String }
}
event! {
    /// A plugin was disabled.
    observe PluginDisableEvent = "plugin_disable" { plugin: String }
}
event! {
    /// Every plugin is enabled and the world is open: the server is ready.
    observe ServerLoadEvent = "server_load" {}
}
event! {
    /// A server tick starts, before any world ticks (Paper's
    /// `ServerTickStartEvent`). `tick` counts ticks since the world opened.
    observe ServerTickStartEvent = "server_tick_start" { tick: i64 }
}
event! {
    /// A server tick ended, after every world ticked.
    observe ServerTickEndEvent = "server_tick_end" { tick: i64 }
}
event! {
    /// The server's world opened to players.
    observe WorldLoadEvent = "world_load" {}
}
event! {
    /// The server's world closed, after its last save.
    observe WorldUnloadEvent = "world_unload" {}
}

// Players coming, going and changing.

event! {
    /// A player joined and stands in the world. `join_message` is what
    /// everyone is told (`§e` and the vanilla line); change it, or set it
    /// to `None` for no message.
    observe PlayerJoinEvent = "player_join" { player: Player, join_message: Option<String> }
}
event! {
    /// A player left. `quit_message` as `PlayerJoinEvent`'s `join_message`.
    observe PlayerQuitEvent = "player_quit" { player: Player, quit_message: Option<String> }
}
event! {
    /// A player moved, seen once a tick. Cancelling it sends them back to
    /// `from` (at the next tick's start).
    cancellable PlayerMoveEvent = "player_move" { player: Player, from: Location, to: Location }
}
event! {
    /// A player went to another world; `from` is the one they left.
    observe PlayerChangedWorldEvent = "player_changed_world" { player: Player, from: String }
}
event! {
    /// A player died.
    observe PlayerDeathEvent = "player_death" { player: Player }
}
event! {
    /// A dead player is back.
    observe PlayerRespawnEvent = "player_respawn" { player: Player }
}
event! {
    /// A player's game mode changed (`player` already has the new one).
    observe PlayerGameModeChangeEvent = "player_game_mode_change" { player: Player, previous: GameMode }
}
event! {
    /// A player wrote in chat. Cancel it and nobody sees it; change
    /// `message` or `format` (`<%1$s> %2$s`: `%1$s` the name, `%2$s` the
    /// message, `§` codes allowed) to change what everyone sees.
    cancellable AsyncPlayerChatEvent = "player_chat" { player: Player, message: String, format: String }
}

// What players do to blocks, items and entities.

event! {
    /// A player clicked: `action` is `right_click_block` (with `block` and
    /// `face`, before the block's own use: a chest, a door),
    /// `right_click_air` (with `item`, the held item's use) or
    /// `left_click_block` (with `block`, `face` and `item`, as digging
    /// starts). Cancel it and nothing happens: a left click digs nothing.
    cancellable PlayerInteractEvent = "player_interact" {
        player: Player,
        action: String,
        block: Option<Block>,
        face: Option<BlockFace>,
        item: Option<ItemStack>,
    }
}
event! {
    /// A player breaks a block. Cancel it and the block stays, with its drops.
    cancellable BlockBreakEvent = "block_break" { player: Player, block: Block }
}
event! {
    /// A player is about to place `item` as a block at `block`, against
    /// `against`. Cancel it and nothing is placed. `block` is where a block
    /// lands beside `against`: a replaceable block (grass, snow) there is
    /// placed into instead.
    cancellable BlockPlaceEvent = "block_place" { player: Player, block: BlockPos, against: BlockPos, face: BlockFace, item: ItemStack }
}
event! {
    /// A player fills an empty bucket at `block`. Cancel it and the bucket does nothing.
    cancellable PlayerBucketFillEvent = "player_bucket_fill" { player: Player, block: BlockPos, bucket: ItemStack }
}
event! {
    /// A player empties a bucket at `block`. Cancel it and the bucket does nothing.
    cancellable PlayerBucketEmptyEvent = "player_bucket_empty" { player: Player, block: BlockPos, bucket: ItemStack }
}
event! {
    /// A player right-clicks an entity: a mob, or an interaction entity a
    /// plugin placed (furniture). Cancel it and the click does nothing.
    cancellable PlayerInteractEntityEvent = "player_interact_entity" { player: Player, entity: Entity }
}
event! {
    /// A player hits an entity (Paper's `PrePlayerAttackEntityEvent`). Cancel
    /// it and the blow does nothing: no damage, no knockback.
    cancellable PrePlayerAttackEntityEvent = "player_attack_entity" { player: Player, entity: Entity }
}

// Entities.

event! {
    /// A mob is about to come into the world (natural spawns, eggs,
    /// spawners, `/summon`). Cancel it and it doesn't.
    cancellable EntitySpawnEvent = "entity_spawn" { entity: Entity }
}
event! {
    /// A mob died, before its loot drops.
    observe EntityDeathEvent = "entity_death" { entity: Entity }
}

// The world changing by itself.

event! {
    /// An explosion is about to break `blocks`: take out the ones to spare.
    /// Cancelling it spares them all (entities are still hurt).
    cancellable EntityExplodeEvent = "entity_explode" { location: Location, blocks: Vec<BlockPos> }
}
event! {
    /// A fluid flows from `block` into `to`. Cancel it and `to` stays as it is.
    cancellable BlockFromToEvent = "block_from_to" { block: BlockPos, to: BlockPos }
}
event! {
    /// Fire spreads from `source` into `block`, or burns it. Cancel it and
    /// `block` stays as it is.
    cancellable BlockSpreadEvent = "block_spread" { source: BlockPos, block: BlockPos }
}
event! {
    /// A piston pushes (or pulls) `blocks` one step `direction` and destroys
    /// `destroyed`. Cancel it and nothing moves.
    cancellable BlockPistonEvent = "block_piston" { piston: BlockPos, direction: BlockFace, blocks: Vec<BlockPos>, destroyed: Vec<BlockPos> }
}

// Menus plugins open.

event! {
    /// A click in a menu a plugin opened ([`Player::open_menu`]), which
    /// moved nothing: `slot` counts the menu's slots first, then the
    /// player's inventory (-999 outside); `mode` is the click's kind
    /// (0 pick up, 1 shift click, 2 number key, 3 middle, 4 drop, 5 drag,
    /// 6 double click).
    observe InventoryClickEvent = "inventory_click" { player: Player, window: i32, slot: i32, button: i32, mode: i32 }
}
event! {
    /// A menu a plugin opened closed.
    observe InventoryCloseEvent = "inventory_close" { player: Player, window: i32 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Uuid, worlds};

    #[test]
    fn events_cross_the_wire() {
        let mut e = BlockBreakEvent {
            player: Player {
                uuid: Uuid(5),
                name: "Alex".into(),
                location: Location::new(worlds::OVERWORLD, 0.0, 64.0, 0.0),
                ..Default::default()
            },
            block: Block {
                pos: BlockPos::new(worlds::OVERWORLD, 1, 63, 0),
                kind: "minecraft:stone".into(),
                properties: vec![],
            },
            ..Default::default()
        };
        const { assert!(BlockBreakEvent::CANCELLABLE && !PlayerJoinEvent::CANCELLABLE) };
        let back = BlockBreakEvent::from_data(&e.to_data()).unwrap();
        assert_eq!(back, e);
        e.set_cancelled(true);
        assert!(e.is_cancelled());
        assert!(BlockBreakEvent::from_data(&Value::map()).is_none());
        assert_eq!(
            ServerLoadEvent::from_data(&Value::Null),
            Some(ServerLoadEvent::default())
        );
        let i = PlayerInteractEvent::from_data(
            &Value::map()
                .with("player", e.player.to_wire())
                .with("action", "right_click_air"),
        )
        .unwrap();
        assert_eq!(i.block, None);
        assert_eq!(
            EventPriority::parse("HIGHEST"),
            Some(EventPriority::Highest)
        );
        assert!(EventPriority::Lowest < EventPriority::Monitor);
    }
}
