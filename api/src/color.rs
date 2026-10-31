//! Chat colours and formats as `§` codes, which every message to the
//! server is written in (`ChatColor`).

use std::fmt;

/// The section sign that starts a formatting code.
pub const SECTION: char = '§';

/// A colour or format code.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChatColor {
    Black,
    DarkBlue,
    DarkGreen,
    DarkAqua,
    DarkRed,
    DarkPurple,
    Gold,
    Gray,
    DarkGray,
    Blue,
    Green,
    Aqua,
    Red,
    LightPurple,
    Yellow,
    White,
    Obfuscated,
    Bold,
    Strikethrough,
    Underline,
    Italic,
    Reset,
}

const ALL: [(ChatColor, char); 22] = [
    (ChatColor::Black, '0'),
    (ChatColor::DarkBlue, '1'),
    (ChatColor::DarkGreen, '2'),
    (ChatColor::DarkAqua, '3'),
    (ChatColor::DarkRed, '4'),
    (ChatColor::DarkPurple, '5'),
    (ChatColor::Gold, '6'),
    (ChatColor::Gray, '7'),
    (ChatColor::DarkGray, '8'),
    (ChatColor::Blue, '9'),
    (ChatColor::Green, 'a'),
    (ChatColor::Aqua, 'b'),
    (ChatColor::Red, 'c'),
    (ChatColor::LightPurple, 'd'),
    (ChatColor::Yellow, 'e'),
    (ChatColor::White, 'f'),
    (ChatColor::Obfuscated, 'k'),
    (ChatColor::Bold, 'l'),
    (ChatColor::Strikethrough, 'm'),
    (ChatColor::Underline, 'n'),
    (ChatColor::Italic, 'o'),
    (ChatColor::Reset, 'r'),
];

impl ChatColor {
    /// Its code's letter or digit.
    pub fn code(self) -> char {
        ALL.iter()
            .find(|(c, _)| *c == self)
            .map_or('r', |(_, ch)| *ch)
    }

    /// The colour of a code (either case).
    pub fn by_code(code: char) -> Option<ChatColor> {
        let code = code.to_ascii_lowercase();
        ALL.iter().find(|(_, ch)| *ch == code).map(|(c, _)| *c)
    }

    /// Whether it formats rather than colours.
    pub fn is_format(self) -> bool {
        matches!(
            self,
            ChatColor::Obfuscated
                | ChatColor::Bold
                | ChatColor::Strikethrough
                | ChatColor::Underline
                | ChatColor::Italic
        )
    }
}

impl fmt::Display for ChatColor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{SECTION}{}", self.code())
    }
}

/// `text` with `alt` codes (`&a`) turned into `§` codes, as
/// `ChatColor.translateAlternateColorCodes`; `&&`... is left alone unless a
/// code follows. `&#rrggbb` hex colours become `§x§r§r§g§g§b§b`.
pub fn translate(alt: char, text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == alt && i + 1 < chars.len() {
            let next = chars[i + 1];
            if next == '#'
                && i + 8 <= chars.len()
                && chars[i + 2..i + 8].iter().all(char::is_ascii_hexdigit)
            {
                out.push(SECTION);
                out.push('x');
                for h in &chars[i + 2..i + 8] {
                    out.push(SECTION);
                    out.push(h.to_ascii_lowercase());
                }
                i += 8;
                continue;
            }
            if ChatColor::by_code(next).is_some() || next.eq_ignore_ascii_case(&'x') {
                out.push(SECTION);
                out.push(next.to_ascii_lowercase());
                i += 2;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// `text` without its `§` codes (`ChatColor.stripColor`).
pub fn strip(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == SECTION {
            chars.next();
        } else {
            out.push(c);
        }
    }
    out
}

/// The colour and formats in force at the end of `text`, to carry onto a
/// following line (`ChatColor.getLastColors`).
pub fn last_colors(text: &str) -> String {
    let mut colour = String::new();
    let mut formats = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != SECTION {
            continue;
        }
        let Some(code) = chars.next() else {
            break;
        };
        match ChatColor::by_code(code) {
            Some(ChatColor::Reset) => {
                colour.clear();
                formats.clear();
            }
            Some(cc) if cc.is_format() => formats.push_str(&cc.to_string()),
            Some(cc) => {
                colour = cc.to_string();
                formats.clear();
            }
            None => {}
        }
    }
    colour + &formats
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_ampersand_codes() {
        assert_eq!(translate('&', "&aHi &lthere&r!"), "§aHi §lthere§r!");
        assert_eq!(translate('&', "A&B rock & roll &"), "A§b rock & roll &");
        assert_eq!(translate('&', "&#FF8800x"), "§x§f§f§8§8§0§0x");
        assert_eq!(translate('&', "&#FF88"), "&#FF88");
    }

    #[test]
    fn strips_and_carries_colours() {
        assert_eq!(strip("§aHi §lthere§r!"), "Hi there!");
        assert_eq!(last_colors("§aGreen §lbold"), "§a§l");
        assert_eq!(last_colors("§a§lx§r"), "");
        assert_eq!(last_colors("§l§cred"), "§c");
        assert_eq!(format!("{}Hi", ChatColor::Gold), "§6Hi");
        assert_eq!(ChatColor::by_code('A'), Some(ChatColor::Green));
    }
}
