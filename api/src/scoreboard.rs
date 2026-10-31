//! Scoreboards, teams and boss bars, as vanilla keeps them: each call runs
//! the vanilla command (`/scoreboard`, `/team`, `/bossbar`), so the world
//! saves them and every client shows them.

use crate::server::server;
use crate::types::{Player, snbt_string};

fn run(line: String) -> bool {
    server().dispatch_command(&line)
}

/// A name a command takes as one word (objectives, teams): letters,
/// digits and `_-.+`, as vanilla's unquoted strings.
fn word(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_ascii_alphanumeric() || "_-.+".contains(*c))
        .collect()
}

/// `text` as a text component for commands, `§` codes kept.
fn component(text: &str) -> String {
    format!("{{text:{}}}", snbt_string(text))
}

/// Where an objective shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplaySlot {
    Sidebar,
    List,
    BelowName,
}

impl DisplaySlot {
    fn name(self) -> &'static str {
        match self {
            DisplaySlot::Sidebar => "sidebar",
            DisplaySlot::List => "list",
            DisplaySlot::BelowName => "below_name",
        }
    }
}

/// A scoreboard objective.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Objective {
    pub name: String,
}

impl Objective {
    /// Makes it (`criteria` `dummy` for one a plugin keeps), titled
    /// `title` (`§` codes allowed). Making one that exists does nothing.
    pub fn create(name: &str, criteria: &str, title: &str) -> Objective {
        let o = Objective { name: word(name) };
        run(format!("scoreboard objectives add {} {} {}", o.name, word(criteria), component(title)));
        o
    }
    /// One that exists.
    pub fn named(name: &str) -> Objective {
        Objective { name: word(name) }
    }
    pub fn set_title(&self, title: &str) -> bool {
        run(format!("scoreboard objectives modify {} displayname {}", self.name, component(title)))
    }
    /// Shows it in `slot` (the sidebar...).
    pub fn show(&self, slot: DisplaySlot) -> bool {
        run(format!("scoreboard objectives setdisplay {} {}", slot.name(), self.name))
    }
    /// Takes whatever shows in `slot` away.
    pub fn hide(slot: DisplaySlot) -> bool {
        run(format!("scoreboard objectives setdisplay {}", slot.name()))
    }
    /// The score of `holder`: a player's name, or any line of text on a
    /// sidebar (`§aKills`).
    pub fn set(&self, holder: &str, score: i32) -> bool {
        run(format!("scoreboard players set {} {} {score}", holder_arg(holder), self.name))
    }
    pub fn add(&self, holder: &str, amount: i32) -> bool {
        let (verb, n) = if amount < 0 { ("remove", -amount) } else { ("add", amount) };
        run(format!("scoreboard players {verb} {} {} {n}", holder_arg(holder), self.name))
    }
    pub fn reset(&self, holder: &str) -> bool {
        run(format!("scoreboard players reset {} {}", holder_arg(holder), self.name))
    }
    pub fn remove(&self) -> bool {
        run(format!("scoreboard objectives remove {}", self.name))
    }
}

/// A holder as a command reads it: a plain name as it is, anything else quoted.
fn holder_arg(holder: &str) -> String {
    let plain = !holder.is_empty()
        && holder
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-.+#".contains(c));
    if plain { holder.to_owned() } else { snbt_string(holder) }
}

/// A team: a prefix, suffix and colour on its players' names, friendly fire.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Team {
    pub name: String,
}

impl Team {
    pub fn create(name: &str) -> Team {
        let t = Team { name: word(name) };
        run(format!("team add {}", t.name));
        t
    }
    pub fn named(name: &str) -> Team {
        Team { name: word(name) }
    }
    pub fn add_player(&self, player: &Player) -> bool {
        run(format!("team join {} {}", self.name, player.target()))
    }
    pub fn remove_player(player: &Player) -> bool {
        run(format!("team leave {}", player.target()))
    }
    pub fn set_prefix(&self, prefix: &str) -> bool {
        run(format!("team modify {} prefix {}", self.name, component(prefix)))
    }
    pub fn set_suffix(&self, suffix: &str) -> bool {
        run(format!("team modify {} suffix {}", self.name, component(suffix)))
    }
    /// `red`, `gold`, `aqua`... (vanilla's colour names).
    pub fn set_color(&self, color: &str) -> bool {
        run(format!("team modify {} color {}", self.name, word(color)))
    }
    pub fn set_friendly_fire(&self, on: bool) -> bool {
        run(format!("team modify {} friendlyFire {on}", self.name))
    }
    pub fn remove(&self) -> bool {
        run(format!("team remove {}", self.name))
    }
}

/// A boss bar's colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarColor {
    Pink,
    Blue,
    Red,
    Green,
    Yellow,
    Purple,
    White,
}

/// A boss bar's segments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarStyle {
    Progress,
    Notched6,
    Notched10,
    Notched12,
    Notched20,
}

/// A boss bar, by id (`myplugin:timer`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BossBar {
    pub id: String,
}

impl BossBar {
    pub fn create(id: &str, title: &str) -> BossBar {
        let b = BossBar {
            id: crate::types::namespaced(id),
        };
        run(format!("bossbar add {} {}", b.id, component(title)));
        b
    }
    pub fn named(id: &str) -> BossBar {
        BossBar {
            id: crate::types::namespaced(id),
        }
    }
    fn set(&self, what: &str, value: String) -> bool {
        run(format!("bossbar set {} {what} {value}", self.id))
    }
    pub fn set_title(&self, title: &str) -> bool {
        self.set("name", component(title))
    }
    /// How full it is, 0 to 1.
    pub fn set_progress(&self, progress: f64) -> bool {
        self.set("max", "1000".to_owned())
            && self.set("value", ((progress.clamp(0.0, 1.0) * 1000.0).round() as i32).to_string())
    }
    pub fn set_color(&self, color: BarColor) -> bool {
        self.set("color", format!("{color:?}").to_ascii_lowercase())
    }
    pub fn set_style(&self, style: BarStyle) -> bool {
        let s = match style {
            BarStyle::Progress => "progress",
            BarStyle::Notched6 => "notched_6",
            BarStyle::Notched10 => "notched_10",
            BarStyle::Notched12 => "notched_12",
            BarStyle::Notched20 => "notched_20",
        };
        self.set("style", s.to_owned())
    }
    pub fn set_visible(&self, visible: bool) -> bool {
        self.set("visible", visible.to_string())
    }
    /// Who sees it: these players only. They are tagged for the bar
    /// (`hzbar_<id>`), and the bar shows to that tag.
    pub fn show_to(&self, players: &[Player]) -> bool {
        let tag = format!("hzbar_{}", word(&self.id.replace(':', "_")));
        let mut ok = run(format!("tag @a remove {tag}"));
        for p in players {
            ok &= run(format!("tag {} add {tag}", p.target()));
        }
        ok & self.set("players", format!("@a[tag={tag}]"))
    }
    /// Everyone online sees it.
    pub fn show_to_all(&self) -> bool {
        self.set("players", "@a".to_owned())
    }
    pub fn remove(&self) -> bool {
        run(format!("bossbar remove {}", self.id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_holders_are_command_safe() {
        assert_eq!(word("kills; say hi"), "killssayhi");
        assert_eq!(holder_arg("Steve"), "Steve");
        assert_eq!(holder_arg("§aKills"), "\"§aKills\"");
        assert_eq!(component("a\"b"), "{text:\"a\\\"b\"}");
    }
}
