//! A plugin's `config.yml` (Bukkit's `FileConfiguration`): values at
//! dotted paths, falling back to the defaults the plugin ships.

use crate::value::Value;
use crate::yaml;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default)]
pub struct Config {
    path: PathBuf,
    root: Value,
    defaults: Value,
}

impl Config {
    /// An empty configuration saved to `path`.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Config {
            path: path.into(),
            root: Value::map(),
            defaults: Value::map(),
        }
    }

    /// Reads `path`; a missing file is an empty configuration.
    pub fn load(path: impl Into<PathBuf>) -> Result<Self, String> {
        let mut c = Config::new(path);
        c.reload()?;
        Ok(c)
    }

    /// Reads YAML text, saved to `path`.
    pub fn from_yaml(path: impl Into<PathBuf>, text: &str) -> Result<Self, String> {
        let mut c = Config::new(path);
        c.root = parse_root(text)?;
        Ok(c)
    }

    /// Reads the file again, dropping changes not saved.
    pub fn reload(&mut self) -> Result<(), String> {
        self.root = match fs::read_to_string(&self.path) {
            Ok(text) => parse_root(&text).map_err(|e| format!("{}: {e}", self.path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Value::map(),
            Err(e) => return Err(format!("{}: {e}", self.path.display())),
        };
        Ok(())
    }

    /// Writes the values (not the defaults) to the file, making its folder.
    pub fn save(&self) -> Result<(), String> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        // Written beside it and moved, so a crash never leaves half a file.
        let tmp = self.path.with_extension("yml.tmp");
        fs::write(&tmp, yaml::to_string(&self.root))
            .and_then(|()| fs::rename(&tmp, &self.path))
            .map_err(|e| format!("{}: {e}", self.path.display()))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Sets the defaults from YAML text (the `config.yml` a plugin ships).
    pub fn set_defaults_yaml(&mut self, text: &str) -> Result<(), String> {
        self.defaults = parse_root(text)?;
        Ok(())
    }

    /// Copies every default missing from the values in (`options().copyDefaults(true)`).
    pub fn copy_defaults(&mut self) {
        fn merge(into: &mut Value, from: &Value) {
            if let Value::Map(entries) = from {
                for (k, v) in entries {
                    match into.get_mut(k) {
                        Some(existing @ Value::Map(_)) if matches!(v, Value::Map(_)) => {
                            merge(existing, v)
                        }
                        Some(_) => {}
                        None => into.set(k, v.clone()),
                    }
                }
            }
        }
        let defaults = self.defaults.clone();
        merge(&mut self.root, &defaults);
    }

    /// The value at `path`, or its default.
    pub fn get(&self, path: &str) -> Option<&Value> {
        self.root.path(path).or_else(|| self.defaults.path(path))
    }

    /// Whether `path` has a value (or a default).
    pub fn contains(&self, path: &str) -> bool {
        self.get(path).is_some_and(|v| !v.is_null())
    }

    pub fn get_string(&self, path: &str) -> Option<String> {
        self.get(path).and_then(Value::to_text)
    }
    pub fn string_or(&self, path: &str, default: &str) -> String {
        self.get_string(path).unwrap_or_else(|| default.to_owned())
    }
    pub fn get_int(&self, path: &str) -> Option<i64> {
        self.get(path).and_then(Value::as_i64)
    }
    pub fn int_or(&self, path: &str, default: i64) -> i64 {
        self.get_int(path).unwrap_or(default)
    }
    pub fn get_f64(&self, path: &str) -> Option<f64> {
        self.get(path).and_then(Value::as_f64)
    }
    pub fn f64_or(&self, path: &str, default: f64) -> f64 {
        self.get_f64(path).unwrap_or(default)
    }
    pub fn get_bool(&self, path: &str) -> Option<bool> {
        self.get(path).and_then(Value::as_bool)
    }
    pub fn bool_or(&self, path: &str, default: bool) -> bool {
        self.get_bool(path).unwrap_or(default)
    }
    /// A list as strings; one value is a list of one.
    pub fn get_strings(&self, path: &str) -> Vec<String> {
        self.get(path).map(Value::to_strings).unwrap_or_default()
    }

    /// The keys of the section at `path` (the root for `""`), with the
    /// defaults' keys; dotted all the way down when `deep`.
    pub fn keys(&self, path: &str, deep: bool) -> Vec<String> {
        let mut out = Vec::new();
        for tree in [&self.root, &self.defaults] {
            let section = if path.is_empty() {
                Some(tree)
            } else {
                tree.path(path)
            };
            if let Some(s) = section {
                collect_keys(s, "", deep, &mut out);
            }
        }
        out
    }

    /// Sets `path`; `Value::Null` takes it out.
    pub fn set(&mut self, path: &str, value: impl Into<Value>) {
        let value = value.into();
        if value.is_null() {
            self.root.remove_path(path);
        } else {
            self.root.set_path(path, value);
        }
    }

    /// The values as a tree, without the defaults.
    pub fn values(&self) -> &Value {
        &self.root
    }

    /// The values as YAML.
    pub fn to_yaml(&self) -> String {
        yaml::to_string(&self.root)
    }
}

fn parse_root(text: &str) -> Result<Value, String> {
    match yaml::parse(text)? {
        Value::Null => Ok(Value::map()),
        v @ Value::Map(_) => Ok(v),
        _ => Err("the top of a configuration must be `key: value` lines".to_owned()),
    }
}

fn collect_keys(v: &Value, prefix: &str, deep: bool, out: &mut Vec<String>) {
    if let Value::Map(entries) = v {
        for (k, child) in entries {
            let key = if prefix.is_empty() {
                k.clone()
            } else {
                format!("{prefix}.{k}")
            };
            if !out.contains(&key) {
                out.push(key.clone());
            }
            if deep {
                collect_keys(child, &key, deep, out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_fall_back_to_defaults_and_save() {
        let dir = std::env::temp_dir().join(format!("hz-plugin-config-{}", std::process::id()));
        let path = dir.join("Homes").join("config.yml");
        let _ = fs::remove_dir_all(&dir);
        let mut c = Config::load(&path).unwrap();
        c.set_defaults_yaml("max-homes: 3\nmessages:\n  set: '&aHome set'\n  gone: Gone\n")
            .unwrap();
        assert_eq!(c.int_or("max-homes", 0), 3);
        assert_eq!(c.get_string("messages.set").as_deref(), Some("&aHome set"));
        c.set("max-homes", 5);
        c.set("homes.Steve.x", 10.5);
        assert_eq!(c.int_or("max-homes", 0), 5);
        assert_eq!(c.keys("", false), vec!["max-homes", "homes", "messages"]);
        assert_eq!(
            c.keys("homes", true),
            vec!["Steve".to_owned(), "Steve.x".to_owned()]
        );
        c.save().unwrap();
        let again = Config::load(&path).unwrap();
        assert_eq!(again.f64_or("homes.Steve.x", 0.0), 10.5);
        assert!(!again.contains("messages.set"), "defaults are not saved");
        c.copy_defaults();
        assert_eq!(
            c.values().path("messages.gone").and_then(Value::as_str),
            Some("Gone")
        );
        c.set("homes", Value::Null);
        assert!(!c.contains("homes.Steve.x"));
        assert!(Config::from_yaml(&path, "- a\n- b\n").is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
