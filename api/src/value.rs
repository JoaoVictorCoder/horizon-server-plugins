//! The data plugins and the server exchange: a JSON-shaped tree, written as
//! JSON across the ABI and as YAML in `plugin.yml` and `config.yml`.

use std::fmt::Write as _;

/// A JSON-shaped value. Maps keep their keys in insertion order, as YAML
/// files are read and written back.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Value {
    #[default]
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    List(Vec<Value>),
    Map(Vec<(String, Value)>),
}

impl Value {
    /// An empty map.
    pub fn map() -> Self {
        Value::Map(Vec::new())
    }

    /// The same map with `key` set: `Value::map().with("x", 1)`.
    pub fn with(mut self, key: &str, value: impl Into<Value>) -> Self {
        self.set(key, value);
        self
    }

    /// Sets `key` in a map (a `Null` becomes a map first); anything else is
    /// left as it is.
    pub fn set(&mut self, key: &str, value: impl Into<Value>) {
        if matches!(self, Value::Null) {
            *self = Value::map();
        }
        if let Value::Map(entries) = self {
            let value = value.into();
            match entries.iter_mut().find(|(k, _)| k == key) {
                Some((_, v)) => *v = value,
                None => entries.push((key.to_owned(), value)),
            }
        }
    }

    /// Takes `key` out of a map.
    pub fn remove(&mut self, key: &str) -> Option<Value> {
        let Value::Map(entries) = self else {
            return None;
        };
        let i = entries.iter().position(|(k, _)| k == key)?;
        Some(entries.remove(i).1)
    }

    /// The value at `key` in a map.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Map(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// The value at `key` in a map, to change it.
    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        match self {
            Value::Map(entries) => entries.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// The value at a dotted `path` (`database.host`).
    pub fn path(&self, path: &str) -> Option<&Value> {
        path.split('.').try_fold(self, |v, key| v.get(key))
    }

    /// Sets the value at a dotted `path`, making the maps on the way.
    pub fn set_path(&mut self, path: &str, value: impl Into<Value>) {
        match path.split_once('.') {
            None => self.set(path, value),
            Some((head, rest)) => {
                if !matches!(self.get(head), Some(Value::Map(_))) {
                    self.set(head, Value::map());
                }
                if let Some(child) = self.get_mut(head) {
                    child.set_path(rest, value);
                }
            }
        }
    }

    /// Takes the value at a dotted `path` out.
    pub fn remove_path(&mut self, path: &str) -> Option<Value> {
        match path.rsplit_once('.') {
            None => self.remove(path),
            Some((parent, key)) => {
                let mut v = self;
                for k in parent.split('.') {
                    v = v.get_mut(k)?;
                }
                v.remove(key)
            }
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// An integer; a float with no fraction counts.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            Value::Float(f) if f.fract() == 0.0 && f.abs() < 9.0e15 => Some(*f as i64),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(i) => Some(*i as f64),
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&[Value]> {
        match self {
            Value::List(l) => Some(l),
            _ => None,
        }
    }

    pub fn as_map(&self) -> Option<&[(String, Value)]> {
        match self {
            Value::Map(m) => Some(m),
            _ => None,
        }
    }

    /// A string, or a number or boolean written as one (YAML's `version: 1.0`).
    pub fn to_text(&self) -> Option<String> {
        match self {
            Value::Str(s) => Some(s.clone()),
            Value::Int(i) => Some(i.to_string()),
            Value::Float(f) => Some(format_float(*f)),
            Value::Bool(b) => Some(b.to_string()),
            _ => None,
        }
    }

    /// A list of strings; one string is a list of one (`depend: Vault`).
    pub fn to_strings(&self) -> Vec<String> {
        match self {
            Value::List(l) => l.iter().filter_map(Value::to_text).collect(),
            Value::Null => Vec::new(),
            v => v.to_text().into_iter().collect(),
        }
    }

    /// This value as compact JSON.
    pub fn to_json(&self) -> String {
        let mut out = String::new();
        write_json(self, &mut out);
        out
    }

    /// Reads JSON.
    pub fn from_json(text: &str) -> Result<Value, String> {
        let mut p = JsonParser {
            s: text.as_bytes(),
            i: 0,
        };
        p.ws();
        let v = p.value(0)?;
        p.ws();
        if p.i != p.s.len() {
            return Err(format!("JSON: trailing characters at byte {}", p.i));
        }
        Ok(v)
    }
}

pub(crate) fn format_float(f: f64) -> String {
    if f.is_finite() {
        let s = format!("{f:?}");
        if s.contains(['.', 'e', 'E']) {
            s
        } else {
            format!("{s}.0")
        }
    } else {
        // JSON has no infinities or NaN.
        "null".to_owned()
    }
}

fn write_json(v: &Value, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Int(i) => {
            let _ = write!(out, "{i}");
        }
        Value::Float(f) => out.push_str(&format_float(*f)),
        Value::Str(s) => write_json_str(s, out),
        Value::List(l) => {
            out.push('[');
            for (i, v) in l.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_json(v, out);
            }
            out.push(']');
        }
        Value::Map(m) => {
            out.push('{');
            for (i, (k, v)) in m.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_json_str(k, out);
                out.push(':');
                write_json(v, out);
            }
            out.push('}');
        }
    }
}

fn write_json_str(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Nesting deeper than this is refused rather than overflowing the stack.
const MAX_DEPTH: usize = 128;

struct JsonParser<'a> {
    s: &'a [u8],
    i: usize,
}

impl JsonParser<'_> {
    fn ws(&mut self) {
        while self.s.get(self.i).is_some_and(|b| b.is_ascii_whitespace()) {
            self.i += 1;
        }
    }

    fn err<T>(&self, what: &str) -> Result<T, String> {
        Err(format!("JSON: {what} at byte {}", self.i))
    }

    fn eat(&mut self, lit: &str) -> bool {
        if self.s[self.i..].starts_with(lit.as_bytes()) {
            self.i += lit.len();
            true
        } else {
            false
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value, String> {
        if depth > MAX_DEPTH {
            return self.err("nesting too deep");
        }
        match self.s.get(self.i) {
            None => self.err("unexpected end"),
            Some(b'n') if self.eat("null") => Ok(Value::Null),
            Some(b't') if self.eat("true") => Ok(Value::Bool(true)),
            Some(b'f') if self.eat("false") => Ok(Value::Bool(false)),
            Some(b'"') => self.string().map(Value::Str),
            Some(b'[') => {
                self.i += 1;
                let mut list = Vec::new();
                self.ws();
                if self.eat("]") {
                    return Ok(Value::List(list));
                }
                loop {
                    self.ws();
                    list.push(self.value(depth + 1)?);
                    self.ws();
                    if self.eat(",") {
                        continue;
                    }
                    if self.eat("]") {
                        return Ok(Value::List(list));
                    }
                    return self.err("expected `,` or `]`");
                }
            }
            Some(b'{') => {
                self.i += 1;
                let mut map: Vec<(String, Value)> = Vec::new();
                self.ws();
                if self.eat("}") {
                    return Ok(Value::Map(map));
                }
                loop {
                    self.ws();
                    if self.s.get(self.i) != Some(&b'"') {
                        return self.err("expected a key");
                    }
                    let key = self.string()?;
                    self.ws();
                    if !self.eat(":") {
                        return self.err("expected `:`");
                    }
                    self.ws();
                    let v = self.value(depth + 1)?;
                    match map.iter_mut().find(|(k, _)| *k == key) {
                        Some((_, old)) => *old = v,
                        None => map.push((key, v)),
                    }
                    self.ws();
                    if self.eat(",") {
                        continue;
                    }
                    if self.eat("}") {
                        return Ok(Value::Map(map));
                    }
                    return self.err("expected `,` or `}`");
                }
            }
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => self.err("unexpected character"),
        }
    }

    fn number(&mut self) -> Result<Value, String> {
        let start = self.i;
        let mut float = false;
        while let Some(&b) = self.s.get(self.i) {
            match b {
                b'0'..=b'9' | b'-' | b'+' => {}
                b'.' | b'e' | b'E' => float = true,
                _ => break,
            }
            self.i += 1;
        }
        let text = std::str::from_utf8(&self.s[start..self.i]).unwrap_or_default();
        if !float && let Ok(i) = text.parse::<i64>() {
            return Ok(Value::Int(i));
        }
        match text.parse::<f64>() {
            Ok(f) => Ok(Value::Float(f)),
            Err(_) => Err(format!("JSON: bad number `{text}` at byte {start}")),
        }
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let digits = self
            .s
            .get(self.i..self.i + 4)
            .and_then(|d| std::str::from_utf8(d).ok())
            .and_then(|d| u32::from_str_radix(d, 16).ok());
        match digits {
            Some(n) => {
                self.i += 4;
                Ok(n)
            }
            None => self.err("bad `\\u` escape"),
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.i += 1;
        let mut out = String::new();
        loop {
            let start = self.i;
            while self.s.get(self.i).is_some_and(|&b| b != b'"' && b != b'\\') {
                self.i += 1;
            }
            match std::str::from_utf8(&self.s[start..self.i]) {
                Ok(run) => out.push_str(run),
                Err(_) => return self.err("invalid UTF-8"),
            }
            match self.s.get(self.i) {
                None => return self.err("unterminated string"),
                Some(b'"') => {
                    self.i += 1;
                    return Ok(out);
                }
                Some(_) => {
                    self.i += 1;
                    let Some(&e) = self.s.get(self.i) else {
                        return self.err("unterminated escape");
                    };
                    self.i += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hi = self.hex4()?;
                            let code = if (0xD800..0xDC00).contains(&hi) {
                                if !self.eat("\\u") {
                                    return self.err("lone surrogate");
                                }
                                let lo = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&lo) {
                                    return self.err("bad surrogate pair");
                                }
                                0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                            } else {
                                hi
                            };
                            match char::from_u32(code) {
                                Some(c) => out.push(c),
                                None => return self.err("bad `\\u` escape"),
                            }
                        }
                        _ => return self.err("unknown escape"),
                    }
                }
            }
        }
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Bool(b)
    }
}
macro_rules! from_int {
    ($($t:ty),*) => {$(
        impl From<$t> for Value {
            fn from(i: $t) -> Self {
                Value::Int(i as i64)
            }
        }
    )*};
}
from_int!(i8, i16, i32, i64, u8, u16, u32, isize);
impl From<u64> for Value {
    fn from(i: u64) -> Self {
        i64::try_from(i).map_or(Value::Float(i as f64), Value::Int)
    }
}
impl From<usize> for Value {
    fn from(i: usize) -> Self {
        Value::from(i as u64)
    }
}
impl From<f32> for Value {
    fn from(f: f32) -> Self {
        Value::Float(f64::from(f))
    }
}
impl From<f64> for Value {
    fn from(f: f64) -> Self {
        Value::Float(f)
    }
}
impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::Str(s.to_owned())
    }
}
impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::Str(s)
    }
}
impl From<&String> for Value {
    fn from(s: &String) -> Self {
        Value::Str(s.clone())
    }
}
impl<T: Into<Value>> From<Vec<T>> for Value {
    fn from(l: Vec<T>) -> Self {
        Value::List(l.into_iter().map(Into::into).collect())
    }
}
impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(o: Option<T>) -> Self {
        o.map_or(Value::Null, Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trips() {
        let v = Value::map()
            .with("name", "Steve \"the\" \\ miner\n")
            .with("n", 42)
            .with("f", 1.5)
            .with("whole", 2.0)
            .with("neg", -7)
            .with("ok", true)
            .with("none", Value::Null)
            .with("list", vec![1, 2, 3])
            .with("nested", Value::map().with("a", Value::List(vec![])));
        let text = v.to_json();
        assert_eq!(Value::from_json(&text).unwrap(), v);
        assert!(text.contains("\"whole\":2.0"));
    }

    #[test]
    fn json_reads_escapes_and_surrogates() {
        let v = Value::from_json(r#" { "s" : "a\u00e9\ud83d\ude00\t/" , "e": 1e3 } "#).unwrap();
        assert_eq!(v.get("s").and_then(Value::as_str), Some("aé😀\t/"));
        assert_eq!(v.get("e").and_then(Value::as_f64), Some(1000.0));
        assert_eq!(v.get("e").and_then(Value::as_i64), Some(1000));
    }

    #[test]
    fn json_refuses_garbage() {
        for bad in [
            "",
            "{",
            "[1,]",
            "{\"a\" 1}",
            "tru",
            "\"x",
            "1 2",
            "\"\\ud800\"",
            "nul",
        ] {
            assert!(Value::from_json(bad).is_err(), "{bad:?} should fail");
        }
        let deep = "[".repeat(1000);
        assert!(Value::from_json(&deep).is_err());
    }

    #[test]
    fn paths_make_maps_on_the_way() {
        let mut v = Value::Null;
        v.set_path("database.host", "localhost");
        v.set_path("database.port", 5432);
        v.set_path("motd", "hi");
        assert_eq!(
            v.path("database.host").and_then(Value::as_str),
            Some("localhost")
        );
        assert_eq!(v.path("database.port").and_then(Value::as_i64), Some(5432));
        assert_eq!(v.remove_path("database.port"), Some(Value::Int(5432)));
        assert!(v.path("database.port").is_none());
        v.set_path("motd.inner", 1);
        assert_eq!(v.path("motd.inner").and_then(Value::as_i64), Some(1));
    }

    #[test]
    fn to_strings_takes_one_or_many() {
        assert_eq!(Value::from("Vault").to_strings(), vec!["Vault"]);
        assert_eq!(
            Value::from(vec!["a", "b"]).to_strings(),
            vec!["a".to_owned(), "b".to_owned()]
        );
        assert!(Value::Null.to_strings().is_empty());
        assert_eq!(Value::Float(1.0).to_text().as_deref(), Some("1.0"));
    }
}
