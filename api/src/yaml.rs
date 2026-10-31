//! The YAML that `plugin.yml`, `config.yml` and `permissions.yml` are
//! written in: block maps and lists, flow lists and maps (`[a, b]`,
//! `{a: 1}`), plain, single- and double-quoted scalars, `|` and `>` blocks,
//! and comments. Anchors, tags and multi-document streams are not read.

use crate::value::{Value, format_float};

/// Reads a YAML document. An empty one is `Null`.
pub fn parse(text: &str) -> Result<Value, String> {
    let lines: Vec<Line> = text
        .lines()
        .enumerate()
        .filter_map(|(n, raw)| Line::new(n + 1, raw))
        .collect();
    if let Some(l) = lines.iter().find(|l| l.text.starts_with('\t')) {
        return Err(format!("line {}: tabs can't indent YAML", l.number));
    }
    let mut p = Parser {
        lines,
        i: 0,
        raw: text.lines().map(str::to_owned).collect(),
    };
    if p.lines.is_empty() {
        return Ok(Value::Null);
    }
    if p.lines[0].text == "---" {
        p.i = 1;
    }
    let Some(first) = p.lines.get(p.i) else {
        return Ok(Value::Null);
    };
    let v = p.node(first.indent)?;
    if let Some(l) = p.lines.get(p.i) {
        return Err(format!("line {}: unexpected `{}`", l.number, l.text));
    }
    Ok(v)
}

/// Writes `value` as a block-style YAML document.
pub fn to_string(value: &Value) -> String {
    let mut out = String::new();
    match value {
        Value::Map(m) if !m.is_empty() => write_map(m, 0, &mut out),
        Value::List(l) if !l.is_empty() => write_list(l, 0, &mut out),
        v => {
            out.push_str(&scalar(v));
            out.push('\n');
        }
    }
    out
}

#[derive(Debug)]
struct Line {
    number: usize,
    indent: usize,
    /// Without the indentation and the trailing comment.
    text: String,
}

impl Line {
    fn new(number: usize, raw: &str) -> Option<Line> {
        let indent = raw.len() - raw.trim_start_matches(' ').len();
        let text = strip_comment(&raw[indent..]).trim_end().to_owned();
        (!text.is_empty()).then_some(Line {
            number,
            indent,
            text,
        })
    }
}

/// `text` without a `#` comment outside quotes.
fn strip_comment(text: &str) -> &str {
    let mut quote = None;
    let mut prev = ' ';
    for (i, c) in text.char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == '#' && (prev == ' ' || i == 0) => return &text[..i],
            None => {}
        }
        prev = c;
    }
    text
}

struct Parser {
    lines: Vec<Line>,
    i: usize,
    raw: Vec<String>,
}

impl Parser {
    fn node(&mut self, indent: usize) -> Result<Value, String> {
        let line = &self.lines[self.i];
        if line.text == "-" || line.text.starts_with("- ") {
            self.list(indent)
        } else if split_key(&line.text).is_some() {
            self.map(indent)
        } else {
            let number = line.number;
            let text = line.text.clone();
            self.i += 1;
            parse_inline(&text).map_err(|e| format!("line {number}: {e}"))
        }
    }

    fn map(&mut self, indent: usize) -> Result<Value, String> {
        let mut entries: Vec<(String, Value)> = Vec::new();
        while let Some(line) = self.lines.get(self.i) {
            if line.indent < indent {
                break;
            }
            if line.indent > indent {
                return Err(format!("line {}: bad indentation", line.number));
            }
            let number = line.number;
            let Some((key, rest)) = split_key(&line.text) else {
                return Err(format!("line {number}: expected `key: value`"));
            };
            let key = unquote_key(key).map_err(|e| format!("line {number}: {e}"))?;
            let rest = rest.to_owned();
            self.i += 1;
            let value = self.value_after(indent, &rest, number, true)?;
            match entries.iter_mut().find(|(k, _)| *k == key) {
                Some((_, v)) => *v = value,
                None => entries.push((key, value)),
            }
        }
        Ok(Value::Map(entries))
    }

    fn list(&mut self, indent: usize) -> Result<Value, String> {
        let mut items = Vec::new();
        while let Some(line) = self.lines.get(self.i) {
            if line.indent < indent || !(line.text == "-" || line.text.starts_with("- ")) {
                if line.indent > indent {
                    return Err(format!("line {}: bad indentation", line.number));
                }
                break;
            }
            if line.indent > indent {
                return Err(format!("line {}: bad indentation", line.number));
            }
            let number = line.number;
            let rest = line.text[1..].trim_start().to_owned();
            let nested_list = rest == "-" || rest.starts_with("- ");
            let nested_map =
                !rest.is_empty() && split_key(&rest).is_some() && !rest.starts_with(['{', '[']);
            if nested_list || nested_map {
                // `- key: value` opens a map whose keys line up after the
                // dash, and `- - x` a list.
                let inner = indent + (line.text.len() - rest.len());
                self.lines[self.i].indent = inner;
                self.lines[self.i].text = rest;
                items.push(if nested_list {
                    self.list(inner)?
                } else {
                    self.map(inner)?
                });
                continue;
            }
            self.i += 1;
            items.push(self.value_after(indent, &rest, number, false)?);
        }
        Ok(Value::List(items))
    }

    /// The value after `key:` or `-` on line `number`: inline, a block
    /// scalar, or the more-indented lines below. A map's list may sit at the
    /// map's own indentation (`key:\n- a`).
    fn value_after(
        &mut self,
        indent: usize,
        rest: &str,
        number: usize,
        in_map: bool,
    ) -> Result<Value, String> {
        if rest.starts_with('|') || rest.starts_with('>') {
            return Ok(Value::Str(self.block_scalar(indent, rest, number)));
        }
        if !rest.is_empty() {
            return parse_inline(rest).map_err(|e| format!("line {number}: {e}"));
        }
        match self.lines.get(self.i) {
            Some(next) if next.indent > indent => {
                let child = next.indent;
                self.node(child)
            }
            Some(next)
                if in_map
                    && next.indent == indent
                    && (next.text == "-" || next.text.starts_with("- ")) =>
            {
                self.list(indent)
            }
            _ => Ok(Value::Null),
        }
    }

    /// A `|` (lines kept) or `>` (lines folded) block from the raw text, as
    /// comments inside it are text.
    fn block_scalar(&mut self, indent: usize, header: &str, number: usize) -> String {
        let fold = header.starts_with('>');
        let keep_last = !header.contains('-');
        let mut body: Vec<String> = Vec::new();
        let mut block_indent = None;
        let mut row = number; // 1-based line after the header
        while let Some(raw) = self.raw.get(row) {
            let ind = raw.len() - raw.trim_start_matches(' ').len();
            if raw.trim().is_empty() {
                body.push(String::new());
                row += 1;
                continue;
            }
            if ind <= indent {
                break;
            }
            let bi = *block_indent.get_or_insert(ind);
            if ind < bi {
                break;
            }
            body.push(raw[bi..].to_owned());
            row += 1;
        }
        while body.last().is_some_and(String::is_empty) {
            body.pop();
        }
        // Skip the parsed lines the block covered.
        while self.lines.get(self.i).is_some_and(|l| l.number <= row) {
            self.i += 1;
        }
        let mut text = if fold {
            let mut s = String::new();
            for (i, l) in body.iter().enumerate() {
                if i > 0 {
                    s.push(if l.is_empty() || body[i - 1].is_empty() {
                        '\n'
                    } else {
                        ' '
                    });
                }
                s.push_str(l);
            }
            s
        } else {
            body.join("\n")
        };
        if keep_last && !text.is_empty() {
            text.push('\n');
        }
        text
    }
}

/// `key: rest` (or `key:` alone), with the key's quotes kept.
fn split_key(text: &str) -> Option<(&str, &str)> {
    if text.starts_with(['[', '{']) {
        return None;
    }
    let end = if let Some(q @ ('"' | '\'')) = text.chars().next() {
        let close = text[1..].find(q)? + 1;
        close + 1
    } else {
        0
    };
    let tail = &text[end..];
    let colon = tail
        .find(": ")
        .or_else(|| tail.ends_with(':').then(|| tail.len() - 1))?;
    let key = text[..end + colon].trim_end();
    let rest = text[end + colon + 1..].trim();
    (!key.is_empty()).then_some((key, rest))
}

fn unquote_key(key: &str) -> Result<String, String> {
    match parse_inline(key)? {
        Value::Str(s) => Ok(s),
        Value::Null => Ok(key.to_owned()),
        v => Ok(v.to_text().unwrap_or_else(|| key.to_owned())),
    }
}

/// A scalar or flow collection on one line.
fn parse_inline(text: &str) -> Result<Value, String> {
    let mut f = Flow {
        s: text.as_bytes(),
        i: 0,
    };
    let v = f.value(0, false)?;
    f.ws();
    if f.i != f.s.len() {
        return Err(format!("unexpected `{}`", &text[f.i..]));
    }
    Ok(v)
}

struct Flow<'a> {
    s: &'a [u8],
    i: usize,
}

impl Flow<'_> {
    fn ws(&mut self) {
        while self.s.get(self.i) == Some(&b' ') {
            self.i += 1;
        }
    }

    fn value(&mut self, depth: usize, in_flow: bool) -> Result<Value, String> {
        if depth > 64 {
            return Err("nesting too deep".into());
        }
        self.ws();
        match self.s.get(self.i) {
            Some(b'[') => {
                self.i += 1;
                let mut items = Vec::new();
                loop {
                    self.ws();
                    if self.s.get(self.i) == Some(&b']') {
                        self.i += 1;
                        return Ok(Value::List(items));
                    }
                    items.push(self.value(depth + 1, true)?);
                    self.ws();
                    match self.s.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b']') => {}
                        _ => return Err("expected `,` or `]`".into()),
                    }
                }
            }
            Some(b'{') => {
                self.i += 1;
                let mut entries: Vec<(String, Value)> = Vec::new();
                loop {
                    self.ws();
                    if self.s.get(self.i) == Some(&b'}') {
                        self.i += 1;
                        return Ok(Value::Map(entries));
                    }
                    let key = match self.value(depth + 1, true)? {
                        Value::Str(s) => s,
                        v => v.to_text().unwrap_or_default(),
                    };
                    self.ws();
                    if self.s.get(self.i) != Some(&b':') {
                        return Err("expected `:` in `{...}`".into());
                    }
                    self.i += 1;
                    let v = self.value(depth + 1, true)?;
                    entries.push((key, v));
                    self.ws();
                    match self.s.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {}
                        _ => return Err("expected `,` or `}`".into()),
                    }
                }
            }
            Some(b'"') => self.double_quoted().map(Value::Str),
            Some(b'\'') => self.single_quoted().map(Value::Str),
            _ => {
                let start = self.i;
                while let Some(&b) = self.s.get(self.i) {
                    if in_flow && matches!(b, b',' | b']' | b'}') {
                        break;
                    }
                    if in_flow && b == b':' && self.s.get(self.i + 1).is_none_or(|&n| n == b' ') {
                        break;
                    }
                    self.i += 1;
                }
                let text = std::str::from_utf8(&self.s[start..self.i])
                    .map_err(|_| "invalid UTF-8".to_owned())?
                    .trim();
                Ok(plain_scalar(text))
            }
        }
    }

    fn double_quoted(&mut self) -> Result<String, String> {
        self.i += 1;
        let mut bytes = Vec::new();
        loop {
            match self.s.get(self.i) {
                None => return Err("unterminated `\"`".into()),
                Some(b'"') => {
                    self.i += 1;
                    return String::from_utf8(bytes).map_err(|_| "invalid UTF-8".into());
                }
                Some(b'\\') => {
                    self.i += 1;
                    let e = *self.s.get(self.i).ok_or("unterminated escape")?;
                    self.i += 1;
                    let c = match e {
                        b'n' => '\n',
                        b't' => '\t',
                        b'r' => '\r',
                        b'0' => '\0',
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b' ' => ' ',
                        b'x' | b'u' | b'U' => {
                            let n = match e {
                                b'x' => 2,
                                b'u' => 4,
                                _ => 8,
                            };
                            let hex = self
                                .s
                                .get(self.i..self.i + n)
                                .and_then(|h| std::str::from_utf8(h).ok())
                                .and_then(|h| u32::from_str_radix(h, 16).ok())
                                .and_then(char::from_u32)
                                .ok_or("bad escape")?;
                            self.i += n;
                            hex
                        }
                        _ => return Err(format!("unknown escape `\\{}`", e as char)),
                    };
                    let mut buf = [0; 4];
                    bytes.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                }
                Some(&b) => {
                    bytes.push(b);
                    self.i += 1;
                }
            }
        }
    }

    fn single_quoted(&mut self) -> Result<String, String> {
        self.i += 1;
        let mut bytes = Vec::new();
        loop {
            match self.s.get(self.i) {
                None => return Err("unterminated `'`".into()),
                Some(b'\'') if self.s.get(self.i + 1) == Some(&b'\'') => {
                    bytes.push(b'\'');
                    self.i += 2;
                }
                Some(b'\'') => {
                    self.i += 1;
                    return String::from_utf8(bytes).map_err(|_| "invalid UTF-8".into());
                }
                Some(&b) => {
                    bytes.push(b);
                    self.i += 1;
                }
            }
        }
    }
}

/// A plain scalar's type, as YAML 1.2's core schema reads it (plus
/// `yes`/`no`/`on`/`off`, which Bukkit's YAML 1.1 reader takes as booleans).
fn plain_scalar(text: &str) -> Value {
    match text {
        "" | "~" | "null" | "Null" | "NULL" => return Value::Null,
        "true" | "True" | "TRUE" | "yes" | "Yes" | "on" | "On" => return Value::Bool(true),
        "false" | "False" | "FALSE" | "no" | "No" | "off" | "Off" => return Value::Bool(false),
        ".inf" | "+.inf" => return Value::Float(f64::INFINITY),
        "-.inf" => return Value::Float(f64::NEG_INFINITY),
        ".nan" => return Value::Float(f64::NAN),
        _ => {}
    }
    let digits = text.trim_start_matches(['-', '+']);
    if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
        if let Ok(i) = text.parse::<i64>() {
            return Value::Int(i);
        }
    }
    if let Some(hex) = text.strip_prefix("0x")
        && let Ok(i) = i64::from_str_radix(hex, 16)
    {
        return Value::Int(i);
    }
    let numeric = text
        .bytes()
        .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'-' | b'+' | b'e' | b'E'))
        && text.bytes().any(|b| b.is_ascii_digit())
        && !text.ends_with('.')
        && text.matches('.').count() <= 1;
    if numeric && let Ok(f) = text.parse::<f64>() {
        return Value::Float(f);
    }
    Value::Str(text.to_owned())
}

fn write_map(m: &[(String, Value)], indent: usize, out: &mut String) {
    for (k, v) in m {
        push_indent(indent, out);
        out.push_str(&key(k));
        out.push(':');
        write_child(v, indent, out);
    }
}

fn write_list(l: &[Value], indent: usize, out: &mut String) {
    for v in l {
        push_indent(indent, out);
        match v {
            Value::Map(m) if !m.is_empty() => {
                out.push_str("- ");
                // The first key on the dash's line, the rest under it.
                let mut first = String::new();
                write_map(&m[..1], indent + 2, &mut first);
                out.push_str(first.trim_start_matches(' '));
                write_map(&m[1..], indent + 2, out);
            }
            Value::List(inner) if !inner.is_empty() => {
                out.push_str("-\n");
                write_list(inner, indent + 2, out);
            }
            v => {
                out.push_str("- ");
                out.push_str(&scalar(v));
                out.push('\n');
            }
        }
    }
}

fn write_child(v: &Value, indent: usize, out: &mut String) {
    match v {
        Value::Map(m) if !m.is_empty() => {
            out.push('\n');
            write_map(m, indent + 2, out);
        }
        Value::List(l) if !l.is_empty() => {
            out.push('\n');
            write_list(l, indent + 2, out);
        }
        Value::Str(s) if s.contains('\n') => {
            let body = s.strip_suffix('\n').unwrap_or(s);
            out.push_str(if s.ends_with('\n') { " |\n" } else { " |-\n" });
            for line in body.split('\n') {
                if !line.is_empty() {
                    push_indent(indent + 2, out);
                    out.push_str(line);
                }
                out.push('\n');
            }
        }
        v => {
            out.push(' ');
            out.push_str(&scalar(v));
            out.push('\n');
        }
    }
}

fn push_indent(n: usize, out: &mut String) {
    out.extend(std::iter::repeat_n(' ', n));
}

fn key(k: &str) -> String {
    if needs_quotes(k) {
        quote(k)
    } else {
        k.to_owned()
    }
}

fn scalar(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) if f.is_nan() => ".nan".into(),
        Value::Float(f) if f.is_infinite() => if *f > 0.0 { ".inf" } else { "-.inf" }.into(),
        Value::Float(f) => format_float(*f),
        Value::Str(s) if needs_quotes(s) => quote(s),
        Value::Str(s) => s.clone(),
        Value::List(_) => "[]".into(),
        Value::Map(_) => "{}".into(),
    }
}

/// Whether `s` would read back as something else, or not at all, unquoted.
fn needs_quotes(s: &str) -> bool {
    s.is_empty()
        || s != s.trim()
        || !matches!(plain_scalar(s), Value::Str(_))
        || s.starts_with([
            '-', '?', ':', ',', '[', ']', '{', '}', '#', '&', '*', '!', '|', '>', '\'', '"', '%',
            '@', '`',
        ])
        || s.contains(": ")
        || s.contains(" #")
        || s.ends_with(':')
        || s.chars().any(char::is_control)
}

fn quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLUGIN_YML: &str = r#"
# A plugin's description, as Bukkit writes it.
name: HomePlugin
version: 1.2
main: dev.example.Home
description: "Homes: set one, go back"
authors: [Alex, 'Steve O''Brien']
depend: Vault
softdepend:
- WorldGuard
- Essentials
commands:
  home:
    description: Teleport home   # trailing comment
    aliases: [h, homes]
    usage: /<command> [name]
    permission: home.use
  sethome:
    description: Set your home
permissions:
  home.*:
    default: op
    children:
      home.use: true
      home.set: true
  home.use:
    default: true
"#;

    #[test]
    fn reads_a_plugin_yml() {
        let v = parse(PLUGIN_YML).unwrap();
        assert_eq!(v.get("name").and_then(Value::as_str), Some("HomePlugin"));
        assert_eq!(
            v.get("version").and_then(Value::to_text).as_deref(),
            Some("1.2")
        );
        assert_eq!(
            v.get("description").and_then(Value::as_str),
            Some("Homes: set one, go back")
        );
        assert_eq!(
            v.get("authors").unwrap().to_strings(),
            vec!["Alex", "Steve O'Brien"]
        );
        assert_eq!(v.get("depend").unwrap().to_strings(), vec!["Vault"]);
        assert_eq!(
            v.get("softdepend").unwrap().to_strings(),
            vec!["WorldGuard", "Essentials"]
        );
        assert_eq!(
            v.path("commands.home.description").and_then(Value::as_str),
            Some("Teleport home")
        );
        assert_eq!(
            v.path("commands.home.aliases").unwrap().to_strings(),
            vec!["h", "homes"]
        );
        assert_eq!(
            v.path("commands.home.usage").and_then(Value::as_str),
            Some("/<command> [name]")
        );
        assert_eq!(
            v.get("permissions")
                .and_then(|p| p.get("home.*"))
                .and_then(|p| p.path("children"))
                .and_then(|c| c.get("home.set"))
                .and_then(Value::as_bool),
            Some(true)
        );
    }

    #[test]
    fn reads_lists_of_maps_and_blocks() {
        let text = "\
kits:
  - name: starter
    items: [stone, dirt]
    cooldown: 60
  - name: vip
    price: 9.5
motd: |
  Welcome!
  # not a comment
  Enjoy.
folded: >-
  one
  two
empty:
nested:
  - - 1
    - 2
";
        let v = parse(text).unwrap();
        let kits = v.get("kits").and_then(Value::as_list).unwrap();
        assert_eq!(kits.len(), 2);
        assert_eq!(kits[0].get("name").and_then(Value::as_str), Some("starter"));
        assert_eq!(kits[0].get("cooldown").and_then(Value::as_i64), Some(60));
        assert_eq!(kits[1].get("price").and_then(Value::as_f64), Some(9.5));
        assert_eq!(
            v.get("motd").and_then(Value::as_str),
            Some("Welcome!\n# not a comment\nEnjoy.\n")
        );
        assert_eq!(v.get("folded").and_then(Value::as_str), Some("one two"));
        assert_eq!(v.get("empty"), Some(&Value::Null));
        assert_eq!(
            v.get("nested"),
            Some(&Value::List(vec![Value::List(vec![
                Value::Int(1),
                Value::Int(2)
            ])]))
        );
    }

    #[test]
    fn scalars_keep_their_types() {
        let v = parse(
            "a: 1\nb: -2.5\nc: yes\nd: ~\ne: \"012\"\nf: 0x1F\ng: hello world\nh: 1.2.3\ni: \"\\u00e9\\n\"\nj: 'it''s'\nk: -\n",
        )
        .unwrap();
        assert_eq!(v.get("a"), Some(&Value::Int(1)));
        assert_eq!(v.get("b"), Some(&Value::Float(-2.5)));
        assert_eq!(v.get("c"), Some(&Value::Bool(true)));
        assert_eq!(v.get("d"), Some(&Value::Null));
        assert_eq!(v.get("e"), Some(&Value::from("012")));
        assert_eq!(v.get("f"), Some(&Value::Int(31)));
        assert_eq!(v.get("g"), Some(&Value::from("hello world")));
        assert_eq!(v.get("h"), Some(&Value::from("1.2.3")));
        assert_eq!(v.get("i"), Some(&Value::from("é\n")));
        assert_eq!(v.get("j"), Some(&Value::from("it's")));
        assert_eq!(v.get("k"), Some(&Value::from("-")));
    }

    #[test]
    fn writes_what_it_reads() {
        let v = Value::map()
            .with("name", "Test")
            .with("version", "1.0")
            .with("count", 3)
            .with("ratio", 0.5)
            .with("on", true)
            .with("note", "a: b # c")
            .with("empty", "")
            .with("multi", "line one\nline two\n")
            .with("list", vec!["x", "y"])
            .with(
                "kits",
                Value::List(vec![
                    Value::map().with("name", "a").with("n", 1),
                    Value::map().with("name", "b"),
                ]),
            )
            .with(
                "deep",
                Value::map().with("inner", Value::map().with("k", "v")),
            )
            .with("none", Value::Null)
            .with("blank_list", Value::List(vec![]));
        let text = to_string(&v);
        assert_eq!(parse(&text).unwrap(), v, "{text}");
    }

    #[test]
    fn refuses_tabs_and_bad_indents() {
        assert!(parse("a:\n\tb: 1\n").is_err());
        assert!(parse("a: 1\n   b: 2\n").is_err());
        assert!(parse("a: [1, 2\n").is_err());
        assert_eq!(parse("# only a comment\n").unwrap(), Value::Null);
    }
}
