//! Engenho's machines as data, with no server: what each holds, what it
//! makes of what, and one tick of each. The plugin (lib.rs) puts them in the
//! world; everything here is tested on its own.
//!
//! Energy is counted in E. Generators make it, batteries keep it, machines
//! spend it, and each tick generators and batteries hand it to the
//! machines and batteries beside them.

use horizon_plugin_api::Value;

/// How many items one slot holds.
pub const STACK: u32 = 64;
/// Upgrades of one kind a machine takes.
pub const MAX_UPGRADES: u32 = 4;

pub const SPEED_UPGRADE: &str = "engenho:upgrade_velocidade";
pub const EFFICIENCY_UPGRADE: &str = "engenho:upgrade_eficiencia";

/// What a machine is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Grinds an ore into two dusts, an ingot into one.
    Grinder,
    /// Smelts with energy.
    Furnace,
    /// Presses ingots into plates.
    Press,
    /// Burns fuel for energy.
    Generator,
    /// Energy from the sun, by day.
    Solar,
    /// Keeps energy; gives it out of its front.
    Battery,
}

impl Kind {
    pub const ALL: [Kind; 6] = [
        Kind::Grinder,
        Kind::Furnace,
        Kind::Press,
        Kind::Generator,
        Kind::Solar,
        Kind::Battery,
    ];

    /// Its block's id.
    pub fn block(self) -> &'static str {
        match self {
            Kind::Grinder => "engenho:moedor",
            Kind::Furnace => "engenho:forno_eletrico",
            Kind::Press => "engenho:prensa",
            Kind::Generator => "engenho:gerador",
            Kind::Solar => "engenho:painel_solar",
            Kind::Battery => "engenho:bateria",
        }
    }

    pub fn from_block(id: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.block() == id)
    }

    pub fn name(self) -> &'static str {
        match self {
            Kind::Grinder => "Moedor",
            Kind::Furnace => "Forno Elétrico",
            Kind::Press => "Prensa",
            Kind::Generator => "Gerador a Combustão",
            Kind::Solar => "Painel Solar",
            Kind::Battery => "Bateria",
        }
    }

    /// The energy it holds at most.
    pub fn capacity(self) -> u64 {
        match self {
            Kind::Grinder | Kind::Furnace | Kind::Press => 20_000,
            Kind::Generator => 40_000,
            Kind::Solar => 10_000,
            Kind::Battery => 1_000_000,
        }
    }

    /// Energy a tick it spends while working (machines) or makes (generators).
    pub fn rate(self) -> u64 {
        match self {
            Kind::Grinder => 20,
            Kind::Furnace => 16,
            Kind::Press => 30,
            Kind::Generator => 40,
            Kind::Solar => 8,
            Kind::Battery => 0,
        }
    }

    /// Ticks one operation takes, without upgrades.
    pub fn ticks(self) -> u32 {
        match self {
            Kind::Grinder => 200,
            Kind::Furnace => 100,
            Kind::Press => 160,
            _ => 0,
        }
    }

    /// Energy a tick it hands to each neighbour at most.
    pub fn output_rate(self) -> u64 {
        match self {
            Kind::Generator => 80,
            Kind::Solar => 16,
            Kind::Battery => 1_000,
            _ => 0,
        }
    }

    /// Whether it turns items into items.
    pub fn processes(self) -> bool {
        matches!(self, Kind::Grinder | Kind::Furnace | Kind::Press)
    }

    /// Whether it takes energy from its neighbours.
    pub fn accepts_energy(self) -> bool {
        self.processes() || self == Kind::Battery
    }

    /// Whether it gives energy to its neighbours (a battery: from its front).
    pub fn gives_energy(self) -> bool {
        matches!(self, Kind::Generator | Kind::Solar | Kind::Battery)
    }

    /// What it makes of `input`: the result and how many.
    pub fn recipe(self, input: &str) -> Option<(&'static str, u32)> {
        let table: &[(&str, &str, u32)] = match self {
            Kind::Grinder => GRINDER,
            Kind::Furnace => FURNACE,
            Kind::Press => PRESS,
            _ => &[],
        };
        table
            .iter()
            .find(|(i, _, _)| *i == input)
            .map(|(_, out, n)| (*out, *n))
    }
}

/// The grinder: ores and raw metal into two dusts, ingots into one.
pub const GRINDER: &[(&str, &str, u32)] = &[
    ("minecraft:raw_iron", "engenho:po_ferro", 2),
    ("minecraft:iron_ore", "engenho:po_ferro", 2),
    ("minecraft:deepslate_iron_ore", "engenho:po_ferro", 2),
    ("minecraft:raw_gold", "engenho:po_ouro", 2),
    ("minecraft:gold_ore", "engenho:po_ouro", 2),
    ("minecraft:deepslate_gold_ore", "engenho:po_ouro", 2),
    ("minecraft:raw_copper", "engenho:po_cobre", 2),
    ("minecraft:copper_ore", "engenho:po_cobre", 6),
    ("minecraft:deepslate_copper_ore", "engenho:po_cobre", 6),
    ("engenho:cobalto_bruto", "engenho:po_cobalto", 2),
    ("minecraft:iron_ingot", "engenho:po_ferro", 1),
    ("minecraft:gold_ingot", "engenho:po_ouro", 1),
    ("minecraft:copper_ingot", "engenho:po_cobre", 1),
    ("engenho:lingote_cobalto", "engenho:po_cobalto", 1),
    ("minecraft:cobblestone", "minecraft:gravel", 1),
    ("minecraft:gravel", "minecraft:sand", 1),
    ("minecraft:bone", "minecraft:bone_meal", 5),
    ("minecraft:blaze_rod", "minecraft:blaze_powder", 4),
];

/// The electric furnace.
pub const FURNACE: &[(&str, &str, u32)] = &[
    ("engenho:po_ferro", "minecraft:iron_ingot", 1),
    ("engenho:po_ouro", "minecraft:gold_ingot", 1),
    ("engenho:po_cobre", "minecraft:copper_ingot", 1),
    ("engenho:po_cobalto", "engenho:lingote_cobalto", 1),
    ("engenho:cobalto_bruto", "engenho:lingote_cobalto", 1),
    ("minecraft:raw_iron", "minecraft:iron_ingot", 1),
    ("minecraft:raw_gold", "minecraft:gold_ingot", 1),
    ("minecraft:raw_copper", "minecraft:copper_ingot", 1),
    ("minecraft:sand", "minecraft:glass", 1),
    ("minecraft:red_sand", "minecraft:glass", 1),
    ("minecraft:cobblestone", "minecraft:stone", 1),
    ("minecraft:stone", "minecraft:smooth_stone", 1),
    ("minecraft:clay_ball", "minecraft:brick", 1),
    ("minecraft:netherrack", "minecraft:nether_brick", 1),
    ("minecraft:cactus", "minecraft:green_dye", 1),
    ("minecraft:wet_sponge", "minecraft:sponge", 1),
    ("minecraft:potato", "minecraft:baked_potato", 1),
    ("minecraft:beef", "minecraft:cooked_beef", 1),
    ("minecraft:porkchop", "minecraft:cooked_porkchop", 1),
    ("minecraft:chicken", "minecraft:cooked_chicken", 1),
    ("minecraft:mutton", "minecraft:cooked_mutton", 1),
    ("minecraft:cod", "minecraft:cooked_cod", 1),
    ("minecraft:salmon", "minecraft:cooked_salmon", 1),
    ("minecraft:rabbit", "minecraft:cooked_rabbit", 1),
    ("minecraft:kelp", "minecraft:dried_kelp", 1),
    ("minecraft:oak_log", "minecraft:charcoal", 1),
    ("minecraft:spruce_log", "minecraft:charcoal", 1),
    ("minecraft:birch_log", "minecraft:charcoal", 1),
    ("minecraft:jungle_log", "minecraft:charcoal", 1),
    ("minecraft:acacia_log", "minecraft:charcoal", 1),
    ("minecraft:dark_oak_log", "minecraft:charcoal", 1),
    ("minecraft:mangrove_log", "minecraft:charcoal", 1),
    ("minecraft:cherry_log", "minecraft:charcoal", 1),
];

/// The press.
pub const PRESS: &[(&str, &str, u32)] = &[
    ("minecraft:iron_ingot", "engenho:placa_ferro", 1),
    ("minecraft:copper_ingot", "engenho:placa_cobre", 1),
    ("engenho:lingote_cobalto", "engenho:placa_cobalto", 1),
];

/// Ticks of burning an item gives the generator (vanilla's fuel values).
pub fn burn_ticks(item: &str) -> Option<u32> {
    let t = match item {
        "minecraft:coal" | "minecraft:charcoal" => 1600,
        "minecraft:coal_block" => 16_000,
        "minecraft:blaze_rod" => 2400,
        "minecraft:dried_kelp_block" => 4000,
        "minecraft:stick" => 100,
        "minecraft:bamboo" => 50,
        i if i.ends_with("_planks") => 300,
        i if i.ends_with("_log") || i.ends_with("_wood") || i.ends_with("_stem") => 300,
        i if i.ends_with("_sapling") => 100,
        _ => return None,
    };
    Some(t)
}

/// Some items in one slot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stack {
    /// `minecraft:raw_iron`, `engenho:po_ferro`.
    pub id: String,
    pub count: u32,
}

impl Stack {
    pub fn new(id: &str, count: u32) -> Stack {
        Stack {
            id: id.to_owned(),
            count,
        }
    }
}

/// Adds `n` of `id` to a slot: how many fit.
pub fn put(slot: &mut Option<Stack>, id: &str, n: u32) -> u32 {
    match slot {
        None => {
            let fit = n.min(STACK);
            if fit > 0 {
                *slot = Some(Stack::new(id, fit));
            }
            fit
        }
        Some(s) if s.id == id => {
            let fit = n.min(STACK - s.count.min(STACK));
            s.count += fit;
            fit
        }
        Some(_) => 0,
    }
}

/// One machine in the world.
#[derive(Clone, Debug, PartialEq)]
pub struct Machine {
    pub kind: Kind,
    /// Where its front looks: `north`, `east`, `south`, `west`.
    pub facing: String,
    pub energy: u64,
    /// What it works on (processing machines) or burns (the generator).
    pub input: Option<Stack>,
    pub output: Option<Stack>,
    /// Ticks into the current operation.
    pub progress: u32,
    /// The generator's fuel left, in ticks, and how long the item burned.
    pub burn: u32,
    pub burn_total: u32,
    pub speed: u32,
    pub efficiency: u32,
    /// Whether it worked on its last tick (its block shows it).
    pub active: bool,
}

impl Machine {
    pub fn new(kind: Kind, facing: &str) -> Machine {
        Machine {
            kind,
            facing: facing.to_owned(),
            energy: 0,
            input: None,
            output: None,
            progress: 0,
            burn: 0,
            burn_total: 0,
            speed: 0,
            efficiency: 0,
            active: false,
        }
    }

    /// Ticks one operation takes: each speed upgrade takes a share off.
    pub fn ticks(&self) -> u32 {
        (self.kind.ticks() * 4 / (4 + 2 * self.speed)).max(1)
    }

    /// Energy a working tick costs: speed costs more, efficiency less.
    pub fn usage(&self) -> u64 {
        let base = self.kind.rate() * u64::from(10 + 5 * self.speed);
        (base / u64::from(10 + 4 * self.efficiency)).max(1)
    }

    /// Room for `n` more energy.
    pub fn room(&self) -> u64 {
        self.kind.capacity().saturating_sub(self.energy)
    }

    /// Whether `id` can go in its input slot.
    pub fn accepts(&self, id: &str) -> bool {
        match self.kind {
            Kind::Generator => burn_ticks(id).is_some(),
            k if k.processes() => k.recipe(id).is_some(),
            _ => false,
        }
    }

    /// One tick, by day or not: whether it is working.
    pub fn tick(&mut self, day: bool) -> bool {
        let active = match self.kind {
            Kind::Grinder | Kind::Furnace | Kind::Press => self.tick_process(),
            Kind::Generator => self.tick_generator(),
            Kind::Solar => {
                if day {
                    self.energy = (self.energy + self.kind.rate()).min(self.kind.capacity());
                }
                day
            }
            Kind::Battery => false,
        };
        self.active = active;
        active
    }

    fn tick_process(&mut self) -> bool {
        let Some(input) = &self.input else {
            self.progress = 0;
            return false;
        };
        let Some((out, n)) = self.kind.recipe(&input.id) else {
            self.progress = 0;
            return false;
        };
        let room = match &self.output {
            None => true,
            Some(o) => o.id == out && o.count + n <= STACK,
        };
        let usage = self.usage();
        if !room || self.energy < usage {
            return false;
        }
        self.energy -= usage;
        self.progress += 1;
        if self.progress >= self.ticks() {
            self.progress = 0;
            put(&mut self.output, out, n);
            if let Some(i) = &mut self.input {
                i.count -= 1;
                if i.count == 0 {
                    self.input = None;
                }
            }
        }
        true
    }

    fn tick_generator(&mut self) -> bool {
        if self.burn == 0 && self.room() > 0 {
            // A new fuel item.
            if let Some(fuel) = &mut self.input
                && let Some(t) = burn_ticks(&fuel.id)
            {
                fuel.count -= 1;
                if fuel.count == 0 {
                    self.input = None;
                }
                self.burn = t;
                self.burn_total = t;
            }
        }
        if self.burn == 0 {
            return false;
        }
        self.burn -= 1;
        self.energy = (self.energy + self.kind.rate()).min(self.kind.capacity());
        true
    }

    /// The items it holds, to give back when it is broken.
    pub fn contents(&self) -> Vec<Stack> {
        let mut out: Vec<Stack> = self.input.iter().chain(&self.output).cloned().collect();
        if self.speed > 0 {
            out.push(Stack::new(SPEED_UPGRADE, self.speed));
        }
        if self.efficiency > 0 {
            out.push(Stack::new(EFFICIENCY_UPGRADE, self.efficiency));
        }
        out
    }

    pub fn to_value(&self) -> Value {
        let stack = |s: &Option<Stack>| match s {
            Some(s) => Value::map().with("id", &s.id).with("count", s.count),
            None => Value::Null,
        };
        Value::map()
            .with("kind", self.kind.block())
            .with("facing", &self.facing)
            .with("energy", self.energy as i64)
            .with("input", stack(&self.input))
            .with("output", stack(&self.output))
            .with("progress", self.progress)
            .with("burn", self.burn)
            .with("burn_total", self.burn_total)
            .with("speed", self.speed)
            .with("efficiency", self.efficiency)
    }

    pub fn from_value(v: &Value) -> Option<Machine> {
        let kind = Kind::from_block(&v.get("kind")?.to_text()?)?;
        let int = |k: &str| v.get(k).and_then(Value::as_i64).unwrap_or(0).max(0);
        let stack = |k: &str| {
            let s = v.get(k)?;
            let count = s.get("count")?.as_i64()?.clamp(1, i64::from(STACK)) as u32;
            Some(Stack::new(&s.get("id")?.to_text()?, count))
        };
        let mut m = Machine::new(kind, &v.get("facing")?.to_text()?);
        m.energy = (int("energy") as u64).min(kind.capacity());
        m.input = stack("input");
        m.output = stack("output");
        m.progress = int("progress") as u32;
        m.burn = int("burn") as u32;
        m.burn_total = int("burn_total") as u32;
        m.speed = (int("speed") as u32).min(MAX_UPGRADES);
        m.efficiency = (int("efficiency") as u32).min(MAX_UPGRADES);
        Some(m)
    }
}

/// The six neighbours of a block, as offsets with the face's name.
pub const SIDES: [([i32; 3], &str); 6] = [
    ([0, 0, -1], "north"),
    ([0, 0, 1], "south"),
    ([1, 0, 0], "east"),
    ([-1, 0, 0], "west"),
    ([0, 1, 0], "up"),
    ([0, -1, 0], "down"),
];

/// Whether `from` gives energy toward `side` (`north`...): generators to every
/// side, a battery only out of its front.
pub fn gives_toward(from: &Machine, side: &str) -> bool {
    match from.kind {
        Kind::Battery => from.facing == side,
        k => k.gives_energy(),
    }
}

/// Whether `to` takes energy coming in from `side` (its own face): machines
/// from any side, a battery from any side but its front.
pub fn takes_from(to: &Machine, side: &str) -> bool {
    match to.kind {
        Kind::Battery => to.facing != side,
        k => k.accepts_energy(),
    }
}

pub fn opposite(side: &str) -> &'static str {
    match side {
        "north" => "south",
        "south" => "north",
        "east" => "west",
        "west" => "east",
        "up" => "down",
        _ => "up",
    }
}

/// Moves energy from `from` to `to` (beside it on `from`'s side `side`): how much.
pub fn transfer(from: &mut Machine, to: &mut Machine, side: &str) -> u64 {
    if !gives_toward(from, side) || !takes_from(to, opposite(side)) {
        return 0;
    }
    let n = from.energy.min(from.kind.output_rate()).min(to.room());
    from.energy -= n;
    to.energy += n;
    n
}

/// `1.2 kE`, `340 E`, `1.0 ME`.
pub fn energy_text(e: u64) -> String {
    if e >= 1_000_000 {
        format!("{:.1} ME", e as f64 / 1e6)
    } else if e >= 1_000 {
        format!("{:.1} kE", e as f64 / 1e3)
    } else {
        format!("{e} E")
    }
}

/// A recipe of the engineering table: what it makes from what.
pub struct TableRecipe {
    pub result: &'static str,
    pub count: u32,
    pub ingredients: &'static [(&'static str, u32)],
}

/// The engineering table's recipes, in the order it shows them.
pub const TABLE: &[TableRecipe] = &[
    TableRecipe {
        result: "engenho:lingote_cobalto",
        count: 1,
        ingredients: &[("engenho:cobalto_bruto", 2), ("minecraft:coal", 1)],
    },
    TableRecipe {
        result: "engenho:circuito",
        count: 1,
        ingredients: &[
            ("minecraft:redstone", 4),
            ("minecraft:copper_ingot", 2),
            ("minecraft:iron_ingot", 1),
        ],
    },
    TableRecipe {
        result: "engenho:carcaca",
        count: 1,
        ingredients: &[("minecraft:iron_ingot", 6), ("engenho:lingote_cobalto", 2)],
    },
    TableRecipe {
        result: "engenho:gerador",
        count: 1,
        ingredients: &[
            ("engenho:carcaca", 1),
            ("engenho:circuito", 1),
            ("minecraft:furnace", 1),
        ],
    },
    TableRecipe {
        result: "engenho:moedor",
        count: 1,
        ingredients: &[
            ("engenho:carcaca", 1),
            ("engenho:circuito", 1),
            ("minecraft:flint", 4),
            ("minecraft:diamond", 1),
        ],
    },
    TableRecipe {
        result: "engenho:forno_eletrico",
        count: 1,
        ingredients: &[
            ("engenho:carcaca", 1),
            ("engenho:circuito", 1),
            ("minecraft:furnace", 1),
            ("minecraft:copper_ingot", 4),
        ],
    },
    TableRecipe {
        result: "engenho:prensa",
        count: 1,
        ingredients: &[
            ("engenho:carcaca", 1),
            ("engenho:circuito", 1),
            ("minecraft:piston", 2),
            ("minecraft:iron_block", 1),
        ],
    },
    TableRecipe {
        result: "engenho:painel_solar",
        count: 1,
        ingredients: &[
            ("engenho:carcaca", 1),
            ("engenho:circuito", 1),
            ("minecraft:glass", 3),
            ("engenho:placa_cobalto", 2),
        ],
    },
    TableRecipe {
        result: "engenho:bateria",
        count: 1,
        ingredients: &[
            ("engenho:carcaca", 1),
            ("engenho:circuito", 2),
            ("minecraft:redstone_block", 4),
            ("engenho:placa_cobre", 4),
        ],
    },
    TableRecipe {
        result: SPEED_UPGRADE,
        count: 1,
        ingredients: &[
            ("engenho:circuito", 1),
            ("minecraft:redstone", 8),
            ("engenho:placa_cobalto", 2),
        ],
    },
    TableRecipe {
        result: EFFICIENCY_UPGRADE,
        count: 1,
        ingredients: &[
            ("engenho:circuito", 1),
            ("minecraft:lapis_lazuli", 8),
            ("engenho:placa_ferro", 2),
        ],
    },
];

/// Whether `inventory` (item id, count per slot) holds `recipe`'s ingredients.
pub fn can_craft(inventory: &[(String, u32)], recipe: &TableRecipe) -> bool {
    recipe.ingredients.iter().all(|(id, n)| {
        inventory
            .iter()
            .filter(|(i, _)| i == id)
            .map(|(_, c)| *c)
            .sum::<u32>()
            >= *n
    })
}

/// Takes `recipe`'s ingredients out of `inventory`, slot by slot: the slots
/// changed, with what is left in each (0: empty).
pub fn take_ingredients(inventory: &mut [(String, u32)], recipe: &TableRecipe) -> Vec<usize> {
    let mut changed = Vec::new();
    for (id, n) in recipe.ingredients {
        let mut left = *n;
        for (slot, (i, c)) in inventory.iter_mut().enumerate() {
            if left == 0 {
                break;
            }
            if i == id && *c > 0 {
                let take = left.min(*c);
                *c -= take;
                left -= take;
                if !changed.contains(&slot) {
                    changed.push(slot);
                }
            }
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grinder_doubles_ores_with_energy() {
        let mut m = Machine::new(Kind::Grinder, "north");
        m.input = Some(Stack::new("minecraft:raw_iron", 3));
        // No energy, no work.
        assert!(!m.tick(true));
        m.energy = Kind::Grinder.capacity();
        for _ in 0..200 {
            assert!(m.tick(true));
        }
        assert_eq!(m.output, Some(Stack::new("engenho:po_ferro", 2)));
        assert_eq!(m.input, Some(Stack::new("minecraft:raw_iron", 2)));
        assert_eq!(m.energy, 20_000 - 200 * 20);
    }

    #[test]
    fn upgrades_trade_speed_and_energy() {
        let mut m = Machine::new(Kind::Grinder, "north");
        assert_eq!((m.ticks(), m.usage()), (200, 20));
        m.speed = 4;
        assert_eq!((m.ticks(), m.usage()), (66, 60));
        m.efficiency = 4;
        assert_eq!(m.usage(), 23);
        m.speed = 0;
        assert_eq!(m.usage(), 7);
    }

    #[test]
    fn a_full_output_stops_the_machine() {
        let mut m = Machine::new(Kind::Press, "north");
        m.energy = 20_000;
        m.input = Some(Stack::new("minecraft:iron_ingot", 5));
        m.output = Some(Stack::new("minecraft:gold_ingot", 1));
        assert!(!m.tick(true));
        m.output = Some(Stack::new("engenho:placa_ferro", STACK));
        assert!(!m.tick(true));
    }

    #[test]
    fn generator_burns_fuel_until_full() {
        let mut g = Machine::new(Kind::Generator, "north");
        g.input = Some(Stack::new("minecraft:coal", 2));
        assert!(g.tick(false));
        assert_eq!(g.input, Some(Stack::new("minecraft:coal", 1)));
        assert_eq!((g.burn, g.energy), (1599, 40));
        g.energy = Kind::Generator.capacity();
        g.burn = 0;
        // Full: it keeps its fuel.
        assert!(!g.tick(false));
        assert_eq!(g.input, Some(Stack::new("minecraft:coal", 1)));
        assert!(g.accepts("minecraft:oak_planks"));
        assert!(!g.accepts("minecraft:dirt"));
    }

    #[test]
    fn solar_works_by_day() {
        let mut s = Machine::new(Kind::Solar, "north");
        assert!(!s.tick(false));
        assert!(s.tick(true));
        assert_eq!(s.energy, 8);
    }

    #[test]
    fn energy_flows_from_generators_and_out_of_battery_fronts() {
        let mut g = Machine::new(Kind::Generator, "north");
        g.energy = 1000;
        let mut m = Machine::new(Kind::Furnace, "north");
        assert_eq!(transfer(&mut g, &mut m, "east"), 80);
        let mut b = Machine::new(Kind::Battery, "east");
        b.energy = 5000;
        // A battery gives out of its front only...
        assert_eq!(transfer(&mut b, &mut m, "west"), 0);
        assert_eq!(transfer(&mut b, &mut m, "east"), 1000);
        // ...and takes in from its other sides, not its front.
        let mut b2 = Machine::new(Kind::Battery, "north");
        assert_eq!(transfer(&mut g, &mut b2, "south"), 0);
        assert_eq!(transfer(&mut g, &mut b2, "north"), 80);
        // Machines don't give energy.
        assert_eq!(transfer(&mut m, &mut b2, "north"), 0);
    }

    #[test]
    fn machines_round_trip() {
        let mut m = Machine::new(Kind::Furnace, "west");
        m.energy = 1234;
        m.input = Some(Stack::new("engenho:po_ferro", 7));
        m.speed = 2;
        m.progress = 33;
        assert_eq!(Machine::from_value(&m.to_value()), Some(m.clone()));
        assert_eq!(m.contents().len(), 2);
    }

    #[test]
    fn table_takes_ingredients() {
        let recipe = &TABLE[1];
        let mut inv = vec![
            ("minecraft:redstone".to_owned(), 3),
            ("minecraft:copper_ingot".to_owned(), 5),
            ("minecraft:redstone".to_owned(), 2),
            ("minecraft:iron_ingot".to_owned(), 1),
        ];
        assert!(can_craft(&inv, recipe));
        let changed = take_ingredients(&mut inv, recipe);
        assert_eq!(changed, vec![0, 2, 1, 3]);
        assert_eq!(inv[0].1 + inv[2].1, 1);
        assert_eq!(inv[1].1, 3);
        assert!(!can_craft(&inv, recipe));
    }

    #[test]
    fn every_recipe_item_is_known() {
        // Each engenho: item a recipe names is one the pack defines.
        let items = include_str!("../packs/engenho/config/items.yml");
        let blocks = include_str!("../packs/engenho/config/blocks.yml");
        let defined = |id: &str| {
            let name = id.strip_prefix("engenho:").unwrap();
            items.contains(&format!("\n  {name}:")) || blocks.contains(&format!("\n  {name}:"))
        };
        let mut ids: Vec<&str> = Vec::new();
        for t in [GRINDER, FURNACE, PRESS] {
            for (i, o, _) in t {
                ids.push(i);
                ids.push(o);
            }
        }
        for r in TABLE {
            ids.push(r.result);
            ids.extend(r.ingredients.iter().map(|(i, _)| *i));
        }
        for k in Kind::ALL {
            ids.push(k.block());
        }
        for id in ids.into_iter().filter(|i| i.starts_with("engenho:")) {
            assert!(defined(id), "{id} is not in the pack");
        }
    }
}
