//! What plugins handle: players, places, blocks, items, entities, worlds
//! and command senders. Each converts to and from the ABI's JSON.

use crate::server::{call, server};
use crate::value::Value;
use std::fmt;

/// A player's or entity's UUID.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Uuid(pub u128);

impl Uuid {
    /// Reads `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`, with or without dashes.
    pub fn parse(text: &str) -> Option<Uuid> {
        let hex: String = text.chars().filter(|&c| c != '-').collect();
        if hex.len() != 32 {
            return None;
        }
        u128::from_str_radix(&hex, 16).ok().map(Uuid)
    }
}

impl fmt::Display for Uuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let h = format!("{:032x}", self.0);
        write!(
            f,
            "{}-{}-{}-{}-{}",
            &h[0..8],
            &h[8..12],
            &h[12..16],
            &h[16..20],
            &h[20..32]
        )
    }
}

/// A converter between a type and the ABI's JSON.
pub trait Wire: Sized {
    fn to_wire(&self) -> Value;
    fn from_wire(v: &Value) -> Option<Self>;
}

impl Wire for Uuid {
    fn to_wire(&self) -> Value {
        Value::Str(self.to_string())
    }
    fn from_wire(v: &Value) -> Option<Self> {
        Uuid::parse(v.as_str()?)
    }
}
impl Wire for String {
    fn to_wire(&self) -> Value {
        Value::Str(self.clone())
    }
    fn from_wire(v: &Value) -> Option<Self> {
        v.to_text()
    }
}
impl Wire for bool {
    fn to_wire(&self) -> Value {
        Value::Bool(*self)
    }
    fn from_wire(v: &Value) -> Option<Self> {
        v.as_bool()
    }
}
impl Wire for i32 {
    fn to_wire(&self) -> Value {
        Value::Int(i64::from(*self))
    }
    fn from_wire(v: &Value) -> Option<Self> {
        v.as_i64().and_then(|i| i32::try_from(i).ok())
    }
}
impl Wire for i64 {
    fn to_wire(&self) -> Value {
        Value::Int(*self)
    }
    fn from_wire(v: &Value) -> Option<Self> {
        v.as_i64()
    }
}
impl Wire for f64 {
    fn to_wire(&self) -> Value {
        Value::Float(*self)
    }
    fn from_wire(v: &Value) -> Option<Self> {
        v.as_f64()
    }
}
impl Wire for Value {
    fn to_wire(&self) -> Value {
        self.clone()
    }
    fn from_wire(v: &Value) -> Option<Self> {
        Some(v.clone())
    }
}
impl<T: Wire> Wire for Option<T> {
    fn to_wire(&self) -> Value {
        self.as_ref().map_or(Value::Null, Wire::to_wire)
    }
    fn from_wire(v: &Value) -> Option<Self> {
        if v.is_null() {
            Some(None)
        } else {
            T::from_wire(v).map(Some)
        }
    }
}
impl<T: Wire> Wire for Vec<T> {
    fn to_wire(&self) -> Value {
        Value::List(self.iter().map(Wire::to_wire).collect())
    }
    fn from_wire(v: &Value) -> Option<Self> {
        v.as_list()?.iter().map(T::from_wire).collect()
    }
}

/// `minecraft:stone` for `stone`; ids with a namespace stay as they are.
pub fn namespaced(id: &str) -> String {
    let id = id.trim().to_ascii_lowercase();
    if id.contains(':') {
        id
    } else {
        format!("minecraft:{id}")
    }
}

/// The three vanilla worlds' names.
pub mod worlds {
    pub const OVERWORLD: &str = "minecraft:overworld";
    pub const NETHER: &str = "minecraft:the_nether";
    pub const END: &str = "minecraft:the_end";
}

/// A place in a world, with a facing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Location {
    /// `minecraft:overworld`, `minecraft:the_nether`, `minecraft:the_end`.
    pub world: String,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
}

impl Location {
    pub fn new(world: &str, x: f64, y: f64, z: f64) -> Self {
        Location {
            world: world.to_owned(),
            x,
            y,
            z,
            yaw: 0.0,
            pitch: 0.0,
        }
    }
    pub fn with_rotation(mut self, yaw: f32, pitch: f32) -> Self {
        self.yaw = yaw;
        self.pitch = pitch;
        self
    }
    pub fn block_x(&self) -> i32 {
        self.x.floor() as i32
    }
    pub fn block_y(&self) -> i32 {
        self.y.floor() as i32
    }
    pub fn block_z(&self) -> i32 {
        self.z.floor() as i32
    }
    /// The squared distance to `other`; infinite across worlds.
    pub fn distance_squared(&self, other: &Location) -> f64 {
        if self.world != other.world {
            return f64::INFINITY;
        }
        let (dx, dy, dz) = (self.x - other.x, self.y - other.y, self.z - other.z);
        dx * dx + dy * dy + dz * dz
    }
    pub fn distance(&self, other: &Location) -> f64 {
        self.distance_squared(other).sqrt()
    }
    /// The same place moved by `(dx, dy, dz)`.
    pub fn add(&self, dx: f64, dy: f64, dz: f64) -> Location {
        Location {
            x: self.x + dx,
            y: self.y + dy,
            z: self.z + dz,
            ..self.clone()
        }
    }
    /// The block this place is in.
    pub fn block(&self) -> BlockPos {
        BlockPos::new(&self.world, self.block_x(), self.block_y(), self.block_z())
    }
    /// The world it is in.
    pub fn world(&self) -> World {
        World::new(&self.world)
    }
    /// Where it looks, to the nearest face: up or down past 45° of pitch,
    /// else by yaw (0 is south, 90 west, 180 north, 270 east).
    pub fn facing(&self) -> BlockFace {
        if self.pitch < -45.0 {
            return BlockFace::Up;
        }
        if self.pitch > 45.0 {
            return BlockFace::Down;
        }
        match (self.yaw.rem_euclid(360.0) / 90.0).round() as i32 % 4 {
            0 => BlockFace::South,
            1 => BlockFace::West,
            2 => BlockFace::North,
            _ => BlockFace::East,
        }
    }
}

impl Wire for Location {
    fn to_wire(&self) -> Value {
        Value::map()
            .with("world", &self.world)
            .with("x", self.x)
            .with("y", self.y)
            .with("z", self.z)
            .with("yaw", self.yaw)
            .with("pitch", self.pitch)
    }
    fn from_wire(v: &Value) -> Option<Self> {
        Some(Location {
            world: v.get("world")?.to_text()?,
            x: v.get("x")?.as_f64()?,
            y: v.get("y")?.as_f64()?,
            z: v.get("z")?.as_f64()?,
            yaw: v.get("yaw").and_then(Value::as_f64).unwrap_or(0.0) as f32,
            pitch: v.get("pitch").and_then(Value::as_f64).unwrap_or(0.0) as f32,
        })
    }
}

/// A block's place.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct BlockPos {
    pub world: String,
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl BlockPos {
    pub fn new(world: &str, x: i32, y: i32, z: i32) -> Self {
        BlockPos {
            world: world.to_owned(),
            x,
            y,
            z,
        }
    }
    pub fn offset(&self, dx: i32, dy: i32, dz: i32) -> BlockPos {
        BlockPos {
            world: self.world.clone(),
            x: self.x + dx,
            y: self.y + dy,
            z: self.z + dz,
        }
    }
    /// The block's centre at its bottom face, where a player stands on it.
    pub fn location(&self) -> Location {
        Location::new(
            &self.world,
            f64::from(self.x) + 0.5,
            f64::from(self.y),
            f64::from(self.z) + 0.5,
        )
    }
    /// The block there now. Only in a tick (a scheduled task, a tick event)
    /// or inside a player's own event; `None` elsewhere or if unloaded.
    pub fn block(&self) -> Option<Block> {
        let v = call(
            "get_block",
            Value::map()
                .with("world", &self.world)
                .with("x", self.x)
                .with("y", self.y)
                .with("z", self.z),
        )
        .ok()?;
        Block::from_wire(&v)
    }
    /// Sets the block (`stone`, `minecraft:oak_stairs[facing=east]`) as
    /// `/setblock` does: at the next tick when inside a player's event.
    pub fn set_type(&self, block: &str) -> bool {
        server().dispatch_command(&format!(
            "execute in {} run setblock {} {} {} {}",
            self.world,
            self.x,
            self.y,
            self.z,
            namespaced(block)
        ))
    }
}

impl Wire for BlockPos {
    fn to_wire(&self) -> Value {
        Value::map()
            .with("world", &self.world)
            .with("x", self.x)
            .with("y", self.y)
            .with("z", self.z)
    }
    fn from_wire(v: &Value) -> Option<Self> {
        Some(BlockPos {
            world: v.get("world")?.to_text()?,
            x: i32::from_wire(v.get("x")?)?,
            y: i32::from_wire(v.get("y")?)?,
            z: i32::from_wire(v.get("z")?)?,
        })
    }
}

/// A block: where it is and what it is.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Block {
    pub pos: BlockPos,
    /// `minecraft:stone`.
    pub kind: String,
    /// Its state's properties (`facing: east`); empty when not known.
    pub properties: Vec<(String, String)>,
}

impl Block {
    pub fn is(&self, id: &str) -> bool {
        self.kind == namespaced(id)
    }
    /// The block as commands write it: `minecraft:oak_stairs[facing=east,half=bottom]`.
    pub fn state_string(&self) -> String {
        state_string(&self.kind, &self.properties)
    }
    pub fn is_air(&self) -> bool {
        matches!(
            self.kind.as_str(),
            "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
        )
    }
    pub fn property(&self, key: &str) -> Option<&str> {
        self.properties
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

impl Wire for Block {
    fn to_wire(&self) -> Value {
        let mut props = Value::map();
        for (k, v) in &self.properties {
            props.set(k, v.as_str());
        }
        self.pos
            .to_wire()
            .with("type", &self.kind)
            .with("properties", props)
    }
    fn from_wire(v: &Value) -> Option<Self> {
        Some(Block {
            pos: BlockPos::from_wire(v)?,
            kind: v.get("type")?.to_text()?,
            properties: v
                .get("properties")
                .and_then(Value::as_map)
                .map(|m| {
                    m.iter()
                        .filter_map(|(k, v)| Some((k.clone(), v.to_text()?)))
                        .collect()
                })
                .unwrap_or_default(),
        })
    }
}

/// A face of a block, or the direction it points.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BlockFace {
    Down,
    #[default]
    Up,
    North,
    South,
    West,
    East,
}

impl BlockFace {
    pub fn from_offset(d: [i32; 3]) -> Option<BlockFace> {
        Some(match d {
            [0, -1, 0] => BlockFace::Down,
            [0, 1, 0] => BlockFace::Up,
            [0, 0, -1] => BlockFace::North,
            [0, 0, 1] => BlockFace::South,
            [-1, 0, 0] => BlockFace::West,
            [1, 0, 0] => BlockFace::East,
            _ => return None,
        })
    }
    pub fn offset(self) -> [i32; 3] {
        match self {
            BlockFace::Down => [0, -1, 0],
            BlockFace::Up => [0, 1, 0],
            BlockFace::North => [0, 0, -1],
            BlockFace::South => [0, 0, 1],
            BlockFace::West => [-1, 0, 0],
            BlockFace::East => [1, 0, 0],
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            BlockFace::Down => "down",
            BlockFace::Up => "up",
            BlockFace::North => "north",
            BlockFace::South => "south",
            BlockFace::West => "west",
            BlockFace::East => "east",
        }
    }
}

impl Wire for BlockFace {
    fn to_wire(&self) -> Value {
        Value::Str(self.name().to_owned())
    }
    fn from_wire(v: &Value) -> Option<Self> {
        Some(match v.as_str()? {
            "down" => BlockFace::Down,
            "up" => BlockFace::Up,
            "north" => BlockFace::North,
            "south" => BlockFace::South,
            "west" => BlockFace::West,
            "east" => BlockFace::East,
            _ => return None,
        })
    }
}

/// A stack of items.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemStack {
    /// `minecraft:diamond`.
    pub kind: String,
    pub amount: u32,
    /// A custom name, with `§` codes.
    pub name: Option<String>,
    /// Lines under the name, with `§` codes.
    pub lore: Vec<String>,
}

impl ItemStack {
    pub fn new(kind: &str, amount: u32) -> Self {
        ItemStack {
            kind: namespaced(kind),
            amount,
            ..Default::default()
        }
    }
    pub fn named(mut self, name: &str) -> Self {
        self.name = Some(name.to_owned());
        self
    }
    pub fn lore_line(mut self, line: &str) -> Self {
        self.lore.push(line.to_owned());
        self
    }
    pub fn is(&self, id: &str) -> bool {
        self.kind == namespaced(id)
    }
    /// Whether it has no name or lore, the only items the server counts,
    /// takes and gives by kind.
    pub fn is_plain(&self) -> bool {
        self.name.is_none() && self.lore.is_empty()
    }
    /// The item as `/give` and `/item` read it, with its name and lore as
    /// components: `minecraft:diamond[custom_name={text:"§bShiny"}]`.
    pub fn command_arg(&self) -> String {
        if self.is_plain() {
            return self.kind.clone();
        }
        let text = |s: &str| format!("{{text:{}}}", snbt_string(s));
        let mut parts = Vec::new();
        if let Some(n) = &self.name {
            parts.push(format!("custom_name={}", text(n)));
        }
        if !self.lore.is_empty() {
            let lines: Vec<String> = self.lore.iter().map(|l| text(l)).collect();
            parts.push(format!("lore=[{}]", lines.join(",")));
        }
        format!("{}[{}]", self.kind, parts.join(","))
    }
}

/// `s` as a quoted SNBT string.
pub fn snbt_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

impl Wire for ItemStack {
    fn to_wire(&self) -> Value {
        Value::map()
            .with("type", &self.kind)
            .with("amount", self.amount)
            .with("name", self.name.clone())
            .with("lore", self.lore.clone())
    }
    fn from_wire(v: &Value) -> Option<Self> {
        Some(ItemStack {
            kind: v.get("type")?.to_text()?,
            amount: v
                .get("amount")
                .and_then(Value::as_i64)
                .map_or(1, |a| a.clamp(0, i64::from(u32::MAX)) as u32),
            name: v.get("name").and_then(Value::to_text),
            lore: v.get("lore").map(Value::to_strings).unwrap_or_default(),
        })
    }
}

/// An entity other than a player, as an event saw it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Entity {
    /// The entity's network id (`Entity.getId`), for this server run.
    pub id: i32,
    /// `minecraft:cow`, `minecraft:item_frame`, `minecraft:player`.
    pub kind: String,
    pub location: Location,
}

impl Wire for Entity {
    fn to_wire(&self) -> Value {
        Value::map()
            .with("id", self.id)
            .with("type", &self.kind)
            .with("location", self.location.to_wire())
    }
    fn from_wire(v: &Value) -> Option<Self> {
        Some(Entity {
            id: i32::from_wire(v.get("id")?)?,
            kind: v.get("type")?.to_text()?,
            location: Location::from_wire(v.get("location")?)?,
        })
    }
}

/// A game mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GameMode {
    #[default]
    Survival,
    Creative,
    Adventure,
    Spectator,
}

impl GameMode {
    pub fn id(self) -> i32 {
        self as i32
    }
    pub fn from_id(id: i32) -> GameMode {
        match id {
            1 => GameMode::Creative,
            2 => GameMode::Adventure,
            3 => GameMode::Spectator,
            _ => GameMode::Survival,
        }
    }
    /// Its name in `/gamemode`.
    pub fn name(self) -> &'static str {
        match self {
            GameMode::Survival => "survival",
            GameMode::Creative => "creative",
            GameMode::Adventure => "adventure",
            GameMode::Spectator => "spectator",
        }
    }
    pub fn parse(text: &str) -> Option<GameMode> {
        Some(match text.to_ascii_lowercase().as_str() {
            "survival" | "s" | "0" => GameMode::Survival,
            "creative" | "c" | "1" => GameMode::Creative,
            "adventure" | "a" | "2" => GameMode::Adventure,
            "spectator" | "sp" | "3" => GameMode::Spectator,
            _ => return None,
        })
    }
}

impl Wire for GameMode {
    fn to_wire(&self) -> Value {
        Value::Int(i64::from(self.id()))
    }
    fn from_wire(v: &Value) -> Option<Self> {
        Some(GameMode::from_id(i32::from_wire(v)?))
    }
}

/// A player on the server, as they were when the plugin got them; methods
/// act on the player as they are now. [`refresh`](Player::refresh) reads
/// them again.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Player {
    pub uuid: Uuid,
    pub name: String,
    pub location: Location,
    pub game_mode: GameMode,
    /// 20 at full; 0 once dead.
    pub health: f32,
    /// The operator level, 0 to 4 (`ops.json`); 0 for a player who is not
    /// an operator.
    pub op_level: u8,
}

impl Wire for Player {
    fn to_wire(&self) -> Value {
        Value::map()
            .with("uuid", self.uuid.to_wire())
            .with("name", &self.name)
            .with("location", self.location.to_wire())
            .with("game_mode", self.game_mode.id())
            .with("health", self.health)
            .with("op_level", self.op_level)
    }
    fn from_wire(v: &Value) -> Option<Self> {
        Some(Player {
            uuid: Uuid::from_wire(v.get("uuid")?)?,
            name: v.get("name")?.to_text()?,
            location: Location::from_wire(v.get("location")?)?,
            game_mode: v
                .get("game_mode")
                .and_then(GameMode::from_wire)
                .unwrap_or_default(),
            health: v.get("health").and_then(Value::as_f64).unwrap_or(20.0) as f32,
            op_level: v
                .get("op_level")
                .and_then(Value::as_i64)
                .map_or(0, |l| l.clamp(0, 4) as u8),
        })
    }
}

impl Player {
    /// The name commands target them by: the name, or the UUID when the
    /// name could be read as something else.
    pub fn target(&self) -> String {
        let plain = !self.name.is_empty()
            && self.name.len() <= 16
            && self
                .name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_');
        if plain {
            self.name.clone()
        } else {
            self.uuid.to_string()
        }
    }
    fn uuid_args(&self) -> Value {
        Value::map().with("uuid", self.uuid.to_wire())
    }
    pub fn world(&self) -> World {
        World::new(&self.location.world)
    }
    pub fn is_op(&self) -> bool {
        self.op_level > 0
    }
    pub fn is_dead(&self) -> bool {
        self.health <= 0.0
    }
    /// The player as they are now; `None` once they left.
    pub fn refresh(&self) -> Option<Player> {
        server().player_by_uuid(self.uuid)
    }
    pub fn is_online(&self) -> bool {
        self.refresh().is_some()
    }
    /// A line in their chat, with `§` codes. False when they aren't on.
    pub fn send_message(&self, message: &str) -> bool {
        call("send_message", self.uuid_args().with("message", message))
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }
    /// A line over their hotbar.
    pub fn send_action_bar(&self, message: &str) -> bool {
        call("action_bar", self.uuid_args().with("message", message))
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }
    /// A title and subtitle on their screen, timings in ticks (Bukkit's
    /// defaults are 10, 70, 20).
    pub fn send_title(&self, title: &str, subtitle: &str, fade_in: i32, stay: i32, fade_out: i32) {
        let t = self.target();
        let s = server();
        s.dispatch_command(&format!("title {t} times {fade_in} {stay} {fade_out}"));
        if !subtitle.is_empty() {
            s.dispatch_command(&format!("title {t} subtitle {}", legacy_json(subtitle)));
        }
        s.dispatch_command(&format!("title {t} title {}", legacy_json(title)));
    }
    /// Moves them, across worlds too. Inside their own event it happens
    /// at the next tick's start, as the console's teleports do.
    pub fn teleport(&self, to: &Location) -> bool {
        call("teleport", self.uuid_args().with("location", to.to_wire()))
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }
    pub fn set_game_mode(&self, mode: GameMode) -> bool {
        server().dispatch_command(&format!("gamemode {} {}", mode.name(), self.target()))
    }
    /// Disconnects them with `reason`.
    pub fn kick(&self, reason: &str) -> bool {
        call("kick", self.uuid_args().with("reason", reason))
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }
    /// Whether they have `node`, by the server's permissions (`permissions.yml`),
    /// the plugins' declared defaults and their operator level.
    pub fn has_permission(&self, node: &str) -> bool {
        call("has_permission", self.uuid_args().with("permission", node))
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }
    /// Gives them `amount` of plain `item`; how many they got. Whatever
    /// doesn't fit drops at their feet when the server gives through
    /// `/give` (outside their own events).
    pub fn give(&self, item: &str, amount: u32) -> u32 {
        call(
            "give_item",
            self.uuid_args()
                .with("item", namespaced(item))
                .with("amount", amount),
        )
        .ok()
        .and_then(|v| v.as_i64())
        .map_or(0, |n| n.max(0) as u32)
    }
    /// Takes up to `amount` of plain `item`; how many it took. Only inside
    /// their own events and commands, where their inventory is at hand.
    pub fn take(&self, item: &str, amount: u32) -> u32 {
        call(
            "take_item",
            self.uuid_args()
                .with("item", namespaced(item))
                .with("amount", amount),
        )
        .ok()
        .and_then(|v| v.as_i64())
        .map_or(0, |n| n.max(0) as u32)
    }
    /// How many plain `item` they carry. Only inside their own events and
    /// commands.
    pub fn count(&self, item: &str) -> u32 {
        call(
            "count_item",
            self.uuid_args().with("item", namespaced(item)),
        )
        .ok()
        .and_then(|v| v.as_i64())
        .map_or(0, |n| n.max(0) as u32)
    }
    /// Their 36 inventory slots (0..9 the hotbar); `None` inside their own
    /// events, where it is swapped out, or once they left.
    pub fn inventory(&self) -> Option<Vec<Option<ItemStack>>> {
        let v = call("inventory", self.uuid_args()).ok()?;
        Vec::<Option<ItemStack>>::from_wire(&v)
    }
    /// The item in their main hand, inside their own events and commands.
    pub fn item_in_hand(&self) -> Option<ItemStack> {
        let v = call("held_item", self.uuid_args()).ok()?;
        ItemStack::from_wire(&v)
    }
    /// Opens a chest menu for them (a GUI): its clicks come as
    /// [`InventoryClickEvent`](crate::event::InventoryClickEvent)s with its
    /// window. The window id, or `None` when they aren't here.
    pub fn open_menu(&self, menu: &Menu) -> Option<i32> {
        let v = call(
            "open_menu",
            self.uuid_args()
                .with("rows", menu.rows)
                .with("title", &menu.title)
                .with("items", menu.items.to_wire()),
        )
        .ok()?;
        i32::from_wire(&v)
    }
    /// Shows `items` in the menu they have open.
    pub fn set_menu_items(&self, items: &[Option<ItemStack>]) {
        let _ = call(
            "set_menu_items",
            self.uuid_args().with("items", items.to_vec().to_wire()),
        );
    }
    /// Closes the menu a plugin opened for them.
    pub fn close_menu(&self) {
        let _ = call("close_menu", self.uuid_args());
    }
    /// Runs `line` (no slash) as they typed it: a plugin's command goes to
    /// its plugin, any other runs as `execute as <player>`.
    pub fn perform_command(&self, line: &str) -> bool {
        call(
            "player_command",
            self.uuid_args().with("line", line.trim_start_matches('/')),
        )
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    }
    /// Plays `sound` (`entity.experience_orb.pickup`) to them only.
    pub fn play_sound(&self, sound: &str, volume: f32, pitch: f32) -> bool {
        let t = self.target();
        server().dispatch_command(&format!(
            "execute as {t} at @s run playsound {} master @s ~ ~ ~ {volume} {pitch}",
            namespaced(sound)
        ))
    }
    /// Gives them a potion effect (`speed`), `amplifier` 0 for level I.
    pub fn add_effect(&self, effect: &str, seconds: u32, amplifier: u8, particles: bool) -> bool {
        server().dispatch_command(&format!(
            "effect give {} {} {seconds} {amplifier} {}",
            self.target(),
            namespaced(effect),
            !particles
        ))
    }
    pub fn clear_effects(&self) -> bool {
        server().dispatch_command(&format!("effect clear {}", self.target()))
    }
    /// Gives them `item`, with its name and lore (`/give`); what doesn't
    /// fit drops at their feet.
    pub fn give_item(&self, item: &ItemStack) -> bool {
        server().dispatch_command(&format!(
            "give {} {} {}",
            self.target(),
            item.command_arg(),
            item.amount.max(1)
        ))
    }
    /// Puts `item` in inventory slot `slot` (0..9 the hotbar, 9..36 the
    /// rest), or empties it.
    pub fn set_slot(&self, slot: u8, item: Option<&ItemStack>) -> bool {
        let at = if slot < 9 {
            format!("hotbar.{slot}")
        } else {
            format!("inventory.{}", slot.saturating_sub(9))
        };
        self.replace_item(&at, item)
    }
    /// Puts `item` on them: `head`, `chest`, `legs`, `feet` or `offhand`.
    pub fn set_equipment(&self, slot: &str, item: Option<&ItemStack>) -> bool {
        let at = match slot {
            "offhand" => "weapon.offhand".to_owned(),
            s => format!("armor.{s}"),
        };
        self.replace_item(&at, item)
    }
    fn replace_item(&self, at: &str, item: Option<&ItemStack>) -> bool {
        let with = item.map_or("minecraft:air".to_owned(), |i| {
            format!("{} {}", i.command_arg(), i.amount.max(1))
        });
        server().dispatch_command(&format!("item replace entity {} {at} with {with}", self.target()))
    }
    /// Empties their inventory (`/clear`).
    pub fn clear_inventory(&self) -> bool {
        server().dispatch_command(&format!("clear {}", self.target()))
    }
    /// Their experience: adds `amount` levels (`/xp`), negative to take.
    pub fn give_levels(&self, amount: i32) -> bool {
        server().dispatch_command(&format!("xp add {} {amount} levels", self.target()))
    }
}

/// `text` with `§` codes as a JSON text component, for `/title` and
/// `/tellraw`.
pub fn legacy_json(text: &str) -> String {
    Value::map().with("text", text).to_json()
}

/// A chest menu a plugin shows a player.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Menu {
    /// 1 to 6.
    pub rows: u8,
    /// With `§` codes.
    pub title: String,
    /// By slot, `rows * 9` at most.
    pub items: Vec<Option<ItemStack>>,
}

impl Menu {
    pub fn new(rows: u8, title: &str) -> Self {
        let rows = rows.clamp(1, 6);
        Menu {
            rows,
            title: title.to_owned(),
            items: vec![None; usize::from(rows) * 9],
        }
    }
    pub fn size(&self) -> usize {
        usize::from(self.rows) * 9
    }
    pub fn set(&mut self, slot: usize, item: Option<ItemStack>) {
        if slot < self.size() {
            if self.items.len() < self.size() {
                self.items.resize(self.size(), None);
            }
            self.items[slot] = item;
        }
    }
    pub fn get(&self, slot: usize) -> Option<&ItemStack> {
        self.items.get(slot)?.as_ref()
    }
}

/// A world (a dimension).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct World {
    pub name: String,
}

/// `kind[key=value,...]`, as `/setblock` reads a block state.
pub fn state_string(kind: &str, properties: &[(String, String)]) -> String {
    if properties.is_empty() {
        return kind.to_owned();
    }
    let props: Vec<String> = properties.iter().map(|(k, v)| format!("{k}={v}")).collect();
    format!("{kind}[{}]", props.join(","))
}

/// A box of blocks read at once ([`World::region`]): each block's state as
/// `/setblock` writes it, by index into `palette`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Region {
    pub world: String,
    /// The lowest corner.
    pub min: [i32; 3],
    /// Width (x), height (y), depth (z).
    pub size: [i32; 3],
    /// Block states; `""` for a block that could not be read (unloaded).
    pub palette: Vec<String>,
    /// x fastest, then z, then y.
    pub data: Vec<u32>,
}

impl Region {
    pub fn volume(&self) -> usize {
        self.size.iter().map(|&s| s.max(0) as usize).product()
    }
    fn index(&self, x: i32, y: i32, z: i32) -> Option<usize> {
        let [dx, dy, dz] = [x - self.min[0], y - self.min[1], z - self.min[2]];
        let [w, h, d] = self.size;
        if dx < 0 || dy < 0 || dz < 0 || dx >= w || dy >= h || dz >= d {
            return None;
        }
        Some(((dy * d + dz) * w + dx) as usize)
    }
    /// The block state at `(x, y, z)` (world coordinates), when read.
    pub fn get(&self, x: i32, y: i32, z: i32) -> Option<&str> {
        let i = *self.data.get(self.index(x, y, z)?)? as usize;
        self.palette.get(i).map(String::as_str).filter(|s| !s.is_empty())
    }
}

impl World {
    /// The blocks from `min` to `max` (both included), read in one call;
    /// see [`BlockPos::block`] for when blocks can be read. At most
    /// 2 000 000 blocks.
    pub fn region(&self, min: [i32; 3], max: [i32; 3]) -> Result<Region, String> {
        let lo = [min[0].min(max[0]), min[1].min(max[1]), min[2].min(max[2])];
        let hi = [min[0].max(max[0]), min[1].max(max[1]), min[2].max(max[2])];
        let v = call(
            "get_blocks",
            Value::map()
                .with("world", &self.name)
                .with("min", lo.to_vec())
                .with("max", hi.to_vec()),
        )?;
        let palette = v.get("palette").map(Value::to_strings).unwrap_or_default();
        let data = v
            .get("data")
            .and_then(Value::as_list)
            .map(|l| l.iter().map(|i| i.as_i64().unwrap_or(0) as u32).collect())
            .unwrap_or_default();
        Ok(Region {
            world: self.name.clone(),
            min: lo,
            size: [hi[0] - lo[0] + 1, hi[1] - lo[1] + 1, hi[2] - lo[2] + 1],
            palette,
            data,
        })
    }

    pub fn new(name: &str) -> Self {
        World {
            name: namespaced(name),
        }
    }
    /// The block at `(x, y, z)`; see [`BlockPos::block`] for when it reads.
    pub fn block_at(&self, x: i32, y: i32, z: i32) -> Option<Block> {
        BlockPos::new(&self.name, x, y, z).block()
    }
    pub fn set_block(&self, x: i32, y: i32, z: i32, block: &str) -> bool {
        BlockPos::new(&self.name, x, y, z).set_type(block)
    }
    /// The players in this world.
    pub fn players(&self) -> Vec<Player> {
        server()
            .online_players()
            .into_iter()
            .filter(|p| p.location.world == self.name)
            .collect()
    }
    /// Spawns `entity` (`zombie`) at `at`.
    pub fn spawn_entity(&self, entity: &str, at: &Location) -> bool {
        server().dispatch_command(&format!(
            "execute in {} run summon {} {} {} {}",
            self.name,
            namespaced(entity),
            at.x,
            at.y,
            at.z
        ))
    }
    /// Sends `particle` (`flame`) around `at`.
    pub fn spawn_particle(
        &self,
        particle: &str,
        at: &Location,
        count: u32,
        spread: f64,
        speed: f64,
    ) -> bool {
        server().dispatch_command(&format!(
            "execute in {} run particle {} {} {} {} {spread} {spread} {spread} {speed} {count}",
            self.name,
            namespaced(particle),
            at.x,
            at.y,
            at.z
        ))
    }
    /// The mobs in this world, when it is the one the server holds at the
    /// moment (a player's own world inside their event); `kind` filters by
    /// type (`zombie`).
    pub fn entities(&self, kind: Option<&str>) -> Vec<Entity> {
        call(
            "entities",
            Value::map()
                .with("world", &self.name)
                .with("type", kind.map(namespaced)),
        )
        .ok()
        .and_then(|v| Vec::<Entity>::from_wire(&v))
        .unwrap_or_default()
    }
    /// Kills the entities a selector's filter picks (`type=zombie,distance=..20`
    /// around `at`), as `/kill @e[...]`.
    pub fn kill_entities(&self, filter: &str, at: &Location) -> bool {
        server().dispatch_command(&format!(
            "execute in {} positioned {} {} {} run kill @e[{filter}]",
            self.name, at.x, at.y, at.z
        ))
    }
    /// Strikes lightning at `at`.
    pub fn strike_lightning(&self, at: &Location) -> bool {
        self.spawn_entity("lightning_bolt", at)
    }
    /// An explosion of `power` (TNT is 4) at `at`, as `/summon tnt` with no fuse.
    pub fn create_explosion(&self, at: &Location, power: u8) -> bool {
        server().dispatch_command(&format!(
            "execute in {} run summon tnt {} {} {} {{fuse:0,explosion_power:{power}}}",
            self.name, at.x, at.y, at.z
        ))
    }
    /// Sets the time of day (0 is sunrise, 6000 noon, 18000 midnight).
    pub fn set_time(&self, time: i64) -> bool {
        server().dispatch_command(&format!("time set {time}"))
    }
    /// `clear`, `rain` or `thunder`, for `seconds` (0: as the game picks).
    pub fn set_weather(&self, weather: &str, seconds: u32) -> bool {
        if seconds == 0 {
            server().dispatch_command(&format!("weather {weather}"))
        } else {
            server().dispatch_command(&format!("weather {weather} {seconds}s"))
        }
    }
}

/// Who ran a command: a player or the server's console.
#[derive(Clone, Debug, PartialEq)]
pub enum CommandSender {
    Player(Player),
    Console,
}

impl CommandSender {
    pub fn name(&self) -> &str {
        match self {
            CommandSender::Player(p) => &p.name,
            CommandSender::Console => "CONSOLE",
        }
    }
    pub fn player(&self) -> Option<&Player> {
        match self {
            CommandSender::Player(p) => Some(p),
            CommandSender::Console => None,
        }
    }
    pub fn is_op(&self) -> bool {
        match self {
            CommandSender::Player(p) => p.is_op(),
            CommandSender::Console => true,
        }
    }
    pub fn send_message(&self, message: &str) {
        match self {
            CommandSender::Player(p) => {
                p.send_message(message);
            }
            CommandSender::Console => server().logger().info(&crate::color::strip(message)),
        }
    }
    /// The console has every permission.
    pub fn has_permission(&self, node: &str) -> bool {
        match self {
            CommandSender::Player(p) => p.has_permission(node),
            CommandSender::Console => true,
        }
    }
}

impl Wire for CommandSender {
    fn to_wire(&self) -> Value {
        match self {
            CommandSender::Player(p) => p.to_wire().with("kind", "player"),
            CommandSender::Console => Value::map().with("kind", "console"),
        }
    }
    fn from_wire(v: &Value) -> Option<Self> {
        match v.get("kind")?.as_str()? {
            "console" => Some(CommandSender::Console),
            _ => Player::from_wire(v).map(CommandSender::Player),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuids_read_and_write() {
        let u = Uuid::parse("069a79f4-44e9-4726-a5be-fca90e38aaf5").unwrap();
        assert_eq!(u.to_string(), "069a79f4-44e9-4726-a5be-fca90e38aaf5");
        assert_eq!(Uuid::parse("069a79f444e94726a5befca90e38aaf5"), Some(u));
        assert_eq!(Uuid::parse("nope"), None);
    }

    #[test]
    fn players_cross_the_wire() {
        let p = Player {
            uuid: Uuid(7),
            name: "Steve".into(),
            location: Location::new(worlds::NETHER, 1.5, 64.0, -3.25).with_rotation(90.0, 0.0),
            game_mode: GameMode::Creative,
            health: 12.5,
            op_level: 2,
        };
        assert_eq!(Player::from_wire(&p.to_wire()), Some(p.clone()));
        let s = CommandSender::Player(p);
        assert_eq!(CommandSender::from_wire(&s.to_wire()), Some(s));
        assert_eq!(
            CommandSender::from_wire(&CommandSender::Console.to_wire()),
            Some(CommandSender::Console)
        );
    }

    #[test]
    fn targets_fall_back_to_the_uuid() {
        let mut p = Player {
            name: "Alex_1".into(),
            uuid: Uuid(1),
            ..Default::default()
        };
        assert_eq!(p.target(), "Alex_1");
        p.name = "@a".into();
        assert_eq!(p.target(), "00000000-0000-0000-0000-000000000001");
    }

    #[test]
    fn items_blocks_and_places() {
        let i = ItemStack::new("Diamond", 3).named("§bShiny").lore_line("x");
        assert_eq!(i.kind, "minecraft:diamond");
        assert!(!i.is_plain());
        assert_eq!(ItemStack::from_wire(&i.to_wire()), Some(i));
        let b = Block {
            pos: BlockPos::new(worlds::OVERWORLD, 1, -2, 3),
            kind: "minecraft:oak_stairs".into(),
            properties: vec![("facing".into(), "east".into())],
        };
        assert_eq!(Block::from_wire(&b.to_wire()), Some(b.clone()));
        assert_eq!(b.property("facing"), Some("east"));
        let l = Location::new(worlds::OVERWORLD, -0.5, 64.9, 2.0);
        assert_eq!(l.block(), BlockPos::new(worlds::OVERWORLD, -1, 64, 2));
        assert_eq!(l.distance(&l.add(3.0, 4.0, 0.0)), 5.0);
        assert!(
            l.distance(&Location::new(worlds::END, 0.0, 0.0, 0.0))
                .is_infinite()
        );
        let mut m = Menu::new(9, "x");
        assert_eq!(m.rows, 6);
        m.set(53, Some(ItemStack::new("stone", 1)));
        m.set(54, Some(ItemStack::new("stone", 1)));
        assert!(m.get(53).is_some());
        assert_eq!(m.items.len(), 54);
    }
}
