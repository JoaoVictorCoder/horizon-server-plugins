//! Who has which permission: the defaults plugins declare (`plugin.yml`),
//! then the server's `plugins/permissions.yml` (groups, inherited, and
//! players), with Bukkit's children and the wildcards permission plugins
//! add (`homes.*`, `*`). A permission nobody declared or set is the
//! operators'.

use horizon_plugin_api::description::{Permission, PermissionDefault};
use horizon_plugin_api::types::Player;
use horizon_plugin_api::{Value, yaml};
use std::collections::HashMap;
use std::path::Path;

/// The file written when there is none.
pub const TEMPLATE: &str = "\
# Permissions for the server's plugins.
#
# A permission nobody sets here keeps the default its plugin declares in
# plugin.yml (true, false, op or not op); one no plugin declares is for
# operators (ops.json) only.
#
# Groups list permissions, and may inherit others' (theirs first, so a
# group overrides what it inherits). A leading - takes a permission away.
# `homes.*` gives every permission under homes., and `*` all of them.
# Players without a group are in `default`.
groups:
  default:
    permissions: []
  # admin:
  #   inherit: [default]
  #   permissions:
  #     - '*'
users: {}
  # Steve:
  #   groups: [admin]
  #   permissions:
  #     - -homes.delete
";

#[derive(Clone, Debug, Default)]
struct Group {
    inherit: Vec<String>,
    permissions: Vec<(String, bool)>,
}

#[derive(Clone, Debug, Default)]
struct User {
    groups: Vec<String>,
    permissions: Vec<(String, bool)>,
}

#[derive(Clone, Debug, Default)]
pub struct Permissions {
    declared: HashMap<String, Permission>,
    groups: HashMap<String, Group>,
    /// By lower-case name or UUID.
    users: HashMap<String, User>,
}

fn nodes(v: Option<&Value>) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    match v {
        Some(Value::Map(m)) => {
            for (k, on) in m {
                out.push((k.to_ascii_lowercase(), on.as_bool().unwrap_or(true)));
            }
        }
        Some(v) => {
            for n in v.to_strings() {
                let n = n.trim().to_ascii_lowercase();
                match n.strip_prefix('-') {
                    Some(off) => out.push((off.to_owned(), false)),
                    None => out.push((n, true)),
                }
            }
        }
        None => {}
    }
    out
}

impl Permissions {
    /// Adds a plugin's declared permission; the first declaration wins.
    pub fn declare(&mut self, p: &Permission) {
        let mut p = p.clone();
        p.name = p.name.to_ascii_lowercase();
        for c in &mut p.children {
            c.0 = c.0.to_ascii_lowercase();
        }
        self.declared.entry(p.name.clone()).or_insert(p);
    }

    /// Reads `permissions.yml`'s text, keeping the declared permissions.
    pub fn load_yaml(&mut self, text: &str) -> Result<(), String> {
        let v = yaml::parse(text)?;
        let mut groups = HashMap::new();
        if let Some(Value::Map(m)) = v.get("groups") {
            for (name, g) in m {
                groups.insert(
                    name.to_ascii_lowercase(),
                    Group {
                        inherit: g
                            .get("inherit")
                            .map(Value::to_strings)
                            .unwrap_or_default()
                            .into_iter()
                            .map(|s| s.to_ascii_lowercase())
                            .collect(),
                        permissions: nodes(g.get("permissions")),
                    },
                );
            }
        }
        let mut users = HashMap::new();
        if let Some(Value::Map(m)) = v.get("users") {
            for (who, u) in m {
                users.insert(
                    who.to_ascii_lowercase(),
                    User {
                        groups: u
                            .get("groups")
                            .map(Value::to_strings)
                            .unwrap_or_default()
                            .into_iter()
                            .map(|s| s.to_ascii_lowercase())
                            .collect(),
                        permissions: nodes(u.get("permissions")),
                    },
                );
            }
        }
        self.groups = groups;
        self.users = users;
        Ok(())
    }

    /// Reads `path`, writing [`TEMPLATE`] there first when it is missing.
    pub fn load_file(&mut self, path: &Path) -> Result<(), String> {
        if !path.exists() {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(path, TEMPLATE);
        }
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        self.load_yaml(&text)
            .map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Sets `node` and the children it grants or takes away.
    fn apply(&self, map: &mut HashMap<String, bool>, node: &str, on: bool, depth: usize) {
        map.insert(node.to_owned(), on);
        if depth > 16 {
            return;
        }
        if let Some(p) = self.declared.get(node) {
            for (child, same) in &p.children {
                self.apply(map, child, if *same { on } else { !on }, depth + 1);
            }
        }
    }

    fn apply_group(&self, map: &mut HashMap<String, bool>, name: &str, seen: &mut Vec<String>) {
        if seen.iter().any(|s| s == name) || seen.len() > 32 {
            return;
        }
        seen.push(name.to_owned());
        let Some(g) = self.groups.get(name) else {
            return;
        };
        for parent in &g.inherit {
            self.apply_group(map, parent, seen);
        }
        for (node, on) in &g.permissions {
            self.apply(map, node, *on, 0);
        }
    }

    /// What the plugins' declared defaults give `player` (with their
    /// children), and what `permissions.yml` sets for them, groups first.
    /// Only the second's wildcards are wildcards: a plugin's `homes.*` is
    /// a permission like another, as in Bukkit.
    fn effective(&self, player: &Player) -> (HashMap<String, bool>, HashMap<String, bool>) {
        let op = player.is_op();
        let mut defaults = HashMap::new();
        for p in self.declared.values() {
            if p.default.allows(op) {
                self.apply(&mut defaults, &p.name, true, 0);
            }
        }
        let mut set = HashMap::new();
        let user = self
            .users
            .get(&player.uuid.to_string())
            .or_else(|| self.users.get(&player.name.to_ascii_lowercase()));
        let groups: Vec<String> = match user {
            Some(u) if !u.groups.is_empty() => u.groups.clone(),
            _ => vec!["default".to_owned()],
        };
        let mut seen = Vec::new();
        for g in &groups {
            self.apply_group(&mut set, g, &mut seen);
        }
        if let Some(u) = user {
            for (node, on) in &u.permissions {
                self.apply(&mut set, node, *on, 0);
            }
        }
        (defaults, set)
    }

    /// Whether `player` has `node`.
    pub fn has(&self, player: &Player, node: &str) -> bool {
        let node = node.to_ascii_lowercase();
        let (defaults, set) = self.effective(player);
        if let Some(on) = set.get(&node) {
            return *on;
        }
        // The nearest wildcard set: `a.b.*`, then `a.*`, then `*`.
        let mut prefix = node.as_str();
        while let Some((head, _)) = prefix.rsplit_once('.') {
            if let Some(on) = set.get(&format!("{head}.*")) {
                return *on;
            }
            prefix = head;
        }
        if let Some(on) = set.get("*") {
            return *on;
        }
        if let Some(on) = defaults.get(&node) {
            return *on;
        }
        match self.declared.get(&node) {
            Some(p) => p.default.allows(player.is_op()),
            None => PermissionDefault::Op.allows(player.is_op()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_plugin_api::types::Uuid;

    fn player(name: &str, op: u8) -> Player {
        Player {
            uuid: Uuid(u128::from(name.len() as u8)),
            name: name.into(),
            op_level: op,
            ..Default::default()
        }
    }

    fn declared() -> Permissions {
        let mut p = Permissions::default();
        p.declare(
            &Permission::new("homes.*")
                .default(PermissionDefault::Op)
                .child("homes.use", true)
                .child("homes.limit.bypass", true)
                .child("homes.newbie", false),
        );
        p.declare(&Permission::new("homes.use").default(PermissionDefault::True));
        p.declare(&Permission::new("homes.newbie").default(PermissionDefault::NotOp));
        p.declare(&Permission::new("homes.delete").default(PermissionDefault::False));
        p
    }

    #[test]
    fn plugin_defaults_and_children() {
        let p = declared();
        let (alex, op) = (player("Alex", 0), player("Opal", 4));
        assert!(p.has(&alex, "homes.use"));
        assert!(!p.has(&alex, "homes.limit.bypass"));
        assert!(
            p.has(&op, "homes.limit.bypass"),
            "homes.* is op's and grants it"
        );
        assert!(p.has(&alex, "homes.newbie"));
        assert!(!p.has(&op, "homes.newbie"), "a false child takes it away");
        assert!(!p.has(&op, "homes.delete"));
        assert!(!p.has(&alex, "undeclared.node"));
        assert!(p.has(&op, "undeclared.node"));
    }

    #[test]
    fn groups_users_wildcards_and_negation() {
        let mut p = declared();
        p.load_yaml(
            "groups:\n  default:\n    permissions: [chat.color]\n  mod:\n    inherit: [default]\n    permissions:\n      - homes.*\n      - kick.use\n  admin:\n    inherit: [mod, admin]\n    permissions: ['*', -homes.delete]\nusers:\n  Steve:\n    groups: [admin]\n  Alex:\n    groups: [mod]\n    permissions:\n      - -kick.use\n      - homes.delete\n",
        )
        .unwrap();
        let (steve, alex, bob) = (player("Steve", 0), player("Alex", 0), player("Bob", 0));
        assert!(p.has(&bob, "chat.color"));
        assert!(!p.has(&bob, "kick.use"));
        assert!(p.has(&steve, "anything.at.all"), "admin has *");
        assert!(!p.has(&steve, "homes.delete"), "-homes.delete beats *");
        assert!(p.has(&steve, "chat.color"), "inherited through mod");
        assert!(
            p.has(&alex, "homes.limit.bypass"),
            "homes.* expands its children"
        );
        assert!(!p.has(&alex, "kick.use"), "the player's own -kick.use");
        assert!(
            p.has(&alex, "homes.delete"),
            "the player's own grant beats the default"
        );
        assert!(
            p.has(&alex, "homes.sub.thing"),
            "a wildcard reaches undeclared nodes"
        );
        assert!(
            !p.has(&alex, "chat.colors"),
            "no prefix matching without a wildcard"
        );
    }

    #[test]
    fn writes_and_reads_its_template() {
        let mut p = Permissions::default();
        p.load_yaml(TEMPLATE).unwrap();
        assert!(p.groups.contains_key("default"));
        assert!(p.users.is_empty());
    }
}
