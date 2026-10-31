//! A plugin's description, written as Bukkit's `plugin.yml` (or built in
//! code): its name and version, what it needs loaded first, its commands
//! and its permissions.

use crate::value::Value;
use crate::yaml;

/// When a plugin loads: before the worlds (`STARTUP`) or after them
/// (`POSTWORLD`, the default).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LoadOrder {
    Startup,
    #[default]
    PostWorld,
}

/// Who has a permission nobody set: everyone, nobody, operators, or
/// everyone but operators.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PermissionDefault {
    True,
    False,
    #[default]
    Op,
    NotOp,
}

impl PermissionDefault {
    /// Bukkit's spellings: `true`, `false`, `op`, `not op` (and `isop`,
    /// `notop`, `!op`...).
    pub fn parse(text: &str) -> Option<PermissionDefault> {
        Some(match text.trim().to_ascii_lowercase().as_str() {
            "true" | "yes" => PermissionDefault::True,
            "false" | "no" => PermissionDefault::False,
            "op" | "isop" | "operator" | "isoperator" | "admin" | "isadmin" => {
                PermissionDefault::Op
            }
            "!op" | "notop" | "not op" | "!operator" | "notoperator" | "!admin" | "notadmin" => {
                PermissionDefault::NotOp
            }
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            PermissionDefault::True => "true",
            PermissionDefault::False => "false",
            PermissionDefault::Op => "op",
            PermissionDefault::NotOp => "not op",
        }
    }

    /// Whether a player has it by default.
    pub fn allows(self, op: bool) -> bool {
        match self {
            PermissionDefault::True => true,
            PermissionDefault::False => false,
            PermissionDefault::Op => op,
            PermissionDefault::NotOp => !op,
        }
    }
}

/// A permission a plugin declares.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Permission {
    pub name: String,
    pub description: String,
    pub default: PermissionDefault,
    /// Permissions this one grants (`true`) or takes away (`false`).
    pub children: Vec<(String, bool)>,
}

impl Permission {
    pub fn new(name: &str) -> Self {
        Permission {
            name: name.to_owned(),
            ..Default::default()
        }
    }
    pub fn description(mut self, text: &str) -> Self {
        self.description = text.to_owned();
        self
    }
    pub fn default(mut self, default: PermissionDefault) -> Self {
        self.default = default;
        self
    }
    pub fn child(mut self, name: &str, value: bool) -> Self {
        self.children.push((name.to_owned(), value));
        self
    }
}

/// A command a plugin declares.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommandSpec {
    /// Lower case, as typed after the slash.
    pub name: String,
    pub description: String,
    pub aliases: Vec<String>,
    /// Shown when the executor returns false; `<command>` becomes the label.
    pub usage: String,
    /// Needed to run it, when set.
    pub permission: Option<String>,
    /// Shown to a sender without the permission.
    pub permission_message: Option<String>,
}

impl CommandSpec {
    pub fn new(name: &str) -> Self {
        CommandSpec {
            name: name.to_ascii_lowercase(),
            usage: "/<command>".to_owned(),
            ..Default::default()
        }
    }
    pub fn description(mut self, text: &str) -> Self {
        self.description = text.to_owned();
        self
    }
    pub fn alias(mut self, alias: &str) -> Self {
        self.aliases.push(alias.to_ascii_lowercase());
        self
    }
    pub fn usage(mut self, usage: &str) -> Self {
        self.usage = usage.to_owned();
        self
    }
    pub fn permission(mut self, node: &str) -> Self {
        self.permission = Some(node.to_owned());
        self
    }
    pub fn permission_message(mut self, message: &str) -> Self {
        self.permission_message = Some(message.to_owned());
        self
    }

    pub fn to_value(&self) -> Value {
        Value::map()
            .with("name", &self.name)
            .with("description", &self.description)
            .with("aliases", self.aliases.clone())
            .with("usage", &self.usage)
            .with("permission", self.permission.clone())
            .with("permission_message", self.permission_message.clone())
    }

    pub fn from_value(v: &Value) -> CommandSpec {
        let text = |k: &str| v.get(k).and_then(Value::to_text);
        CommandSpec {
            name: text("name").unwrap_or_default().to_ascii_lowercase(),
            description: text("description").unwrap_or_default(),
            aliases: v
                .get("aliases")
                .map(Value::to_strings)
                .unwrap_or_default()
                .into_iter()
                .map(|a| a.to_ascii_lowercase())
                .collect(),
            usage: text("usage").unwrap_or_else(|| "/<command>".to_owned()),
            permission: text("permission"),
            permission_message: text("permission_message").or_else(|| text("permission-message")),
        }
    }
}

/// Everything `plugin.yml` says.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PluginDescription {
    pub name: String,
    pub version: String,
    pub description: String,
    pub authors: Vec<String>,
    pub website: String,
    /// Shown before its log lines instead of the name.
    pub prefix: Option<String>,
    pub load: LoadOrder,
    /// Plugins it can't run without; it loads after them.
    pub depend: Vec<String>,
    /// Plugins it loads after when they are here.
    pub soft_depend: Vec<String>,
    /// Plugins that load after it.
    pub load_before: Vec<String>,
    pub commands: Vec<CommandSpec>,
    pub permissions: Vec<Permission>,
    /// `default-permission`: the default of a permission declared without one.
    pub default_permission: PermissionDefault,
    /// The Horizon plugin API it was made for (`api-version`).
    pub api_version: Option<String>,
}

impl PluginDescription {
    pub fn new(name: &str, version: &str) -> Self {
        PluginDescription {
            name: name.to_owned(),
            version: version.to_owned(),
            ..Default::default()
        }
    }
    pub fn description(mut self, text: &str) -> Self {
        self.description = text.to_owned();
        self
    }
    pub fn author(mut self, name: &str) -> Self {
        self.authors.push(name.to_owned());
        self
    }
    pub fn website(mut self, url: &str) -> Self {
        self.website = url.to_owned();
        self
    }
    pub fn depend(mut self, plugin: &str) -> Self {
        self.depend.push(plugin.to_owned());
        self
    }
    pub fn soft_depend(mut self, plugin: &str) -> Self {
        self.soft_depend.push(plugin.to_owned());
        self
    }
    pub fn load_before(mut self, plugin: &str) -> Self {
        self.load_before.push(plugin.to_owned());
        self
    }
    pub fn load(mut self, order: LoadOrder) -> Self {
        self.load = order;
        self
    }
    pub fn command(mut self, spec: CommandSpec) -> Self {
        self.commands.push(spec);
        self
    }
    pub fn permission(mut self, permission: Permission) -> Self {
        self.permissions.push(permission);
        self
    }

    /// Reads a `plugin.yml`. `main` is read and ignored: a Rust plugin's
    /// main is the type it names in [`horizon_plugin!`](crate::horizon_plugin).
    pub fn from_yaml(text: &str) -> Result<Self, String> {
        let v = yaml::parse(text).map_err(|e| format!("plugin.yml: {e}"))?;
        Self::from_value(&v).map_err(|e| format!("plugin.yml: {e}"))
    }

    /// Reads the description's tree (YAML's, or the ABI's JSON).
    pub fn from_value(v: &Value) -> Result<Self, String> {
        let text = |k: &str| v.get(k).and_then(Value::to_text);
        let list = |k: &str| v.get(k).map(Value::to_strings).unwrap_or_default();
        let name = text("name").ok_or("`name` is missing")?;
        if name.is_empty()
            || !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
        {
            return Err(format!(
                "`name: {name}` must be letters, digits, `_`, `-` and `.`"
            ));
        }
        let version = text("version").ok_or("`version` is missing")?;
        let default_permission = match text("default-permission") {
            Some(d) => PermissionDefault::parse(&d).ok_or_else(|| {
                format!("`default-permission: {d}` is not true, false, op or not op")
            })?,
            None => PermissionDefault::Op,
        };
        let mut authors = list("authors");
        if let Some(a) = text("author") {
            authors.insert(0, a);
        }
        let commands = match v.get("commands") {
            Some(Value::Map(m)) => m
                .iter()
                .map(|(name, spec)| {
                    let mut c = CommandSpec::from_value(spec);
                    c.name = name.to_ascii_lowercase();
                    c
                })
                .collect(),
            Some(Value::List(l)) => l.iter().map(CommandSpec::from_value).collect(),
            _ => Vec::new(),
        };
        let mut permissions = Vec::new();
        if let Some(Value::Map(m)) = v.get("permissions") {
            for (name, spec) in m {
                read_permission(name, spec, default_permission, &mut permissions)?;
            }
        } else if let Some(Value::List(l)) = v.get("permissions") {
            for spec in l {
                let name = spec
                    .get("name")
                    .and_then(Value::to_text)
                    .unwrap_or_default();
                read_permission(&name, spec, default_permission, &mut permissions)?;
            }
        }
        Ok(PluginDescription {
            name,
            version,
            description: text("description").unwrap_or_default(),
            authors,
            website: text("website").unwrap_or_default(),
            prefix: text("prefix"),
            load: match text("load").map(|l| l.to_ascii_uppercase()) {
                Some(l) if l == "STARTUP" => LoadOrder::Startup,
                _ => LoadOrder::PostWorld,
            },
            depend: list("depend"),
            soft_depend: {
                let mut s = list("softdepend");
                s.extend(list("soft_depend"));
                s
            },
            load_before: {
                let mut s = list("loadbefore");
                s.extend(list("load_before"));
                s
            },
            commands,
            permissions,
            default_permission,
            api_version: text("api-version").or_else(|| text("api_version")),
        })
    }

    /// The tree [`from_value`](Self::from_value) reads back, with
    /// `plugin.yml`'s keys.
    pub fn to_value(&self) -> Value {
        let mut commands = Value::map();
        for c in &self.commands {
            let mut spec = c.to_value();
            spec.remove("name");
            commands.set(&c.name, spec);
        }
        let mut permissions = Value::map();
        for p in &self.permissions {
            let mut children = Value::map();
            for (child, on) in &p.children {
                children.set(child, *on);
            }
            permissions.set(
                &p.name,
                Value::map()
                    .with("description", &p.description)
                    .with("default", p.default.name())
                    .with("children", children),
            );
        }
        Value::map()
            .with("name", &self.name)
            .with("version", &self.version)
            .with("description", &self.description)
            .with("authors", self.authors.clone())
            .with("website", &self.website)
            .with("prefix", self.prefix.clone())
            .with(
                "load",
                match self.load {
                    LoadOrder::Startup => "STARTUP",
                    LoadOrder::PostWorld => "POSTWORLD",
                },
            )
            .with("depend", self.depend.clone())
            .with("softdepend", self.soft_depend.clone())
            .with("loadbefore", self.load_before.clone())
            .with("commands", commands)
            .with("permissions", permissions)
            .with("default-permission", self.default_permission.name())
            .with("api-version", self.api_version.clone())
    }

    /// `Name v1.0`.
    pub fn full_name(&self) -> String {
        format!("{} v{}", self.name, self.version)
    }
}

fn read_permission(
    name: &str,
    spec: &Value,
    default: PermissionDefault,
    out: &mut Vec<Permission>,
) -> Result<(), String> {
    if name.is_empty() {
        return Err("a permission has no name".to_owned());
    }
    let default = match spec.get("default").and_then(Value::to_text) {
        Some(d) => PermissionDefault::parse(&d).ok_or_else(|| {
            format!("permission `{name}`: `default: {d}` is not true, false, op or not op")
        })?,
        None => default,
    };
    let mut children = Vec::new();
    match spec.get("children") {
        Some(Value::Map(m)) => {
            for (child, v) in m {
                match v {
                    // A child declared in place: `children: {a.b: {default: true}}`.
                    Value::Map(_) => {
                        read_permission(child, v, default, out)?;
                        children.push((child.clone(), true));
                    }
                    v => children.push((child.clone(), v.as_bool().unwrap_or(true))),
                }
            }
        }
        Some(Value::List(l)) => {
            children.extend(l.iter().filter_map(Value::to_text).map(|c| (c, true)));
        }
        _ => {}
    }
    out.push(Permission {
        name: name.to_owned(),
        description: spec
            .get("description")
            .and_then(Value::to_text)
            .unwrap_or_default(),
        default,
        children,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_bukkit_plugin_yml() {
        let d = PluginDescription::from_yaml(
            "name: Homes\nversion: 2\nauthor: Alex\nauthors: [Steve]\nload: STARTUP\ndepend: [Economy]\nsoftdepend: Maps\ndefault-permission: true\ncommands:\n  Home:\n    aliases: H\n    permission: homes.use\n    permission-message: No!\npermissions:\n  homes.*:\n    default: op\n    children:\n      homes.use: true\n      homes.other:\n        default: false\n  homes.use: {}\n",
        )
        .unwrap();
        assert_eq!(d.full_name(), "Homes v2");
        assert_eq!(d.authors, vec!["Alex", "Steve"]);
        assert_eq!(d.load, LoadOrder::Startup);
        assert_eq!(d.depend, vec!["Economy"]);
        assert_eq!(d.soft_depend, vec!["Maps"]);
        assert_eq!(d.commands[0].name, "home");
        assert_eq!(d.commands[0].aliases, vec!["h"]);
        assert_eq!(d.commands[0].permission_message.as_deref(), Some("No!"));
        let perm = |n: &str| d.permissions.iter().find(|p| p.name == n).unwrap();
        assert_eq!(perm("homes.*").default, PermissionDefault::Op);
        assert_eq!(perm("homes.other").default, PermissionDefault::False);
        assert_eq!(perm("homes.use").default, PermissionDefault::True);
        assert_eq!(
            perm("homes.*").children,
            vec![
                ("homes.use".to_owned(), true),
                ("homes.other".to_owned(), true)
            ]
        );
        let back = PluginDescription::from_value(&d.to_value()).unwrap();
        assert_eq!(back.commands, d.commands);
        assert_eq!(back.name, d.name);
        assert_eq!(back.load, d.load);
    }

    #[test]
    fn refuses_bad_descriptions() {
        assert!(PluginDescription::from_yaml("version: 1").is_err());
        assert!(PluginDescription::from_yaml("name: a b\nversion: 1").is_err());
        assert!(PluginDescription::from_yaml("name: a\n").is_err());
        assert!(
            PluginDescription::from_yaml(
                "name: a\nversion: 1\npermissions:\n  x:\n    default: maybe\n"
            )
            .is_err()
        );
    }

    #[test]
    fn permission_defaults() {
        assert_eq!(
            PermissionDefault::parse("not op"),
            Some(PermissionDefault::NotOp)
        );
        assert_eq!(
            PermissionDefault::parse("!op"),
            Some(PermissionDefault::NotOp)
        );
        assert!(PermissionDefault::NotOp.allows(false));
        assert!(!PermissionDefault::Op.allows(false));
    }
}
