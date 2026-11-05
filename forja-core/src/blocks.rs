//! Custom blocks drawn as note block states.
//!
//! With note blocks inert (`Server::set_note_blocks_inert`), a note block
//! keeps the state it was placed in, and a placed one is always the plain
//! state (harp, note 0). Every state with a note from 1 to 24 is then free:
//! 24 notes × 27 instruments × powered or not = 1296 states. Each state of
//! each custom block (`facing=north,active=false`...) gets one of them, kept
//! in `plugins/Forja/states.txt` so the world's blocks stay what they were
//! across restarts and new packs. The resource pack's
//! `assets/minecraft/blockstates/note_block.json` draws each one with its
//! block's model; note 0 keeps the vanilla look.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

/// `NoteBlockInstrument`'s names in 26.3, in declaration order.
pub const INSTRUMENTS: [&str; 27] = [
    "harp",
    "basedrum",
    "snare",
    "hat",
    "bass",
    "flute",
    "bell",
    "guitar",
    "chime",
    "xylophone",
    "iron_xylophone",
    "cow_bell",
    "didgeridoo",
    "bit",
    "banjo",
    "pling",
    "trumpet",
    "trumpet_exposed",
    "trumpet_oxidized",
    "trumpet_weathered",
    "zombie",
    "skeleton",
    "creeper",
    "dragon",
    "wither_skeleton",
    "piglin",
    "custom_head",
];

/// A note block state: instrument index, note (0 to 24), powered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NoteState {
    pub instrument: u8,
    pub note: u8,
    pub powered: bool,
}

impl NoteState {
    /// Every state, in the order states are handed out: notes 1 to 24 first
    /// for each instrument, unpowered before powered. Note 0 is never given.
    pub fn free_slots() -> impl Iterator<Item = NoteState> {
        [false, true].into_iter().flat_map(|powered| {
            (0..INSTRUMENTS.len() as u8).flat_map(move |instrument| {
                (1..=24u8).map(move |note| NoteState {
                    instrument,
                    note,
                    powered,
                })
            })
        })
    }

    /// Every note block state, note 0 included.
    pub fn all() -> impl Iterator<Item = NoteState> {
        (0..INSTRUMENTS.len() as u8).flat_map(|instrument| {
            (0..=24u8).flat_map(move |note| {
                [false, true].into_iter().map(move |powered| NoteState {
                    instrument,
                    note,
                    powered,
                })
            })
        })
    }

    /// `instrument=harp,note=1,powered=false`, as blockstate files key it.
    pub fn key(self) -> String {
        format!(
            "instrument={},note={},powered={}",
            INSTRUMENTS[usize::from(self.instrument)],
            self.note,
            self.powered
        )
    }

    /// The block as `/setblock` takes it.
    pub fn block_string(self) -> String {
        format!("minecraft:note_block[{}]", self.key())
    }

    /// The state of a note block's properties, as the server reports them.
    pub fn from_properties(props: &[(String, String)]) -> Option<NoteState> {
        let get = |k: &str| props.iter().find(|(p, _)| p == k).map(|(_, v)| v.as_str());
        let instrument = INSTRUMENTS
            .iter()
            .position(|i| Some(*i) == get("instrument"))? as u8;
        let note: u8 = get("note")?.parse().ok().filter(|n| *n <= 24)?;
        let powered = get("powered")? == "true";
        Some(NoteState {
            instrument,
            note,
            powered,
        })
    }

    fn parse(text: &str) -> Option<NoteState> {
        let props: Vec<(String, String)> = text
            .split(',')
            .filter_map(|kv| kv.split_once('='))
            .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
            .collect();
        NoteState::from_properties(&props)
    }
}

/// One state of a custom block: its id and its properties, in the order the
/// block declares them.
pub type BlockState = (String, Vec<(String, String)>);

/// `exemplo:maquina[active=false,facing=north]`.
pub fn state_name(block: &str, props: &[(String, String)]) -> String {
    if props.is_empty() {
        return block.to_owned();
    }
    let inner: Vec<String> = props.iter().map(|(k, v)| format!("{k}={v}")).collect();
    format!("{block}[{}]", inner.join(","))
}

fn parse_state_name(text: &str) -> Option<BlockState> {
    match text.split_once('[') {
        None => Some((text.to_owned(), Vec::new())),
        Some((id, rest)) => {
            let inner = rest.strip_suffix(']')?;
            let props = inner
                .split(',')
                .filter(|kv| !kv.is_empty())
                .map(|kv| {
                    let (k, v) = kv.split_once('=')?;
                    Some((k.to_owned(), v.to_owned()))
                })
                .collect::<Option<Vec<_>>>()?;
            Some((id.to_owned(), props))
        }
    }
}

/// Which note block state each custom block state is drawn as.
#[derive(Clone, Debug, Default)]
pub struct Allocation {
    by_block: BTreeMap<String, NoteState>,
    by_note: BTreeMap<NoteState, String>,
    /// Whether it changed since it was read.
    pub dirty: bool,
}

impl Allocation {
    /// Reads `states.txt`: `<state name> <instrument>,<note>,<powered>` lines.
    pub fn load(path: &Path) -> Allocation {
        let mut a = Allocation::default();
        if let Ok(text) = fs::read_to_string(path) {
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Some((name, note)) = line.rsplit_once(' ')
                    && let Some(note) = NoteState::parse(note)
                    && !a.by_note.contains_key(&note)
                    && note.note > 0
                {
                    a.by_block.insert(name.to_owned(), note);
                    a.by_note.insert(note, name.to_owned());
                }
            }
        }
        a
    }

    pub fn save(&mut self, path: &Path) -> std::io::Result<()> {
        let mut text = String::from(
            "# Forja: the note block state each custom block state is drawn as.\n\
             # Keep this file: the world's blocks are these states.\n",
        );
        for (name, note) in &self.by_block {
            let _ = writeln!(text, "{name} {}", note.key());
        }
        fs::write(path, text)?;
        self.dirty = false;
        Ok(())
    }

    /// The note state of `block[props]`, given one if it has none yet; None
    /// when every state is taken.
    pub fn assign(&mut self, block: &str, props: &[(String, String)]) -> Option<NoteState> {
        let name = state_name(block, props);
        if let Some(n) = self.by_block.get(&name) {
            return Some(*n);
        }
        let free = NoteState::free_slots().find(|s| !self.by_note.contains_key(s))?;
        self.by_block.insert(name.clone(), free);
        self.by_note.insert(free, name);
        self.dirty = true;
        Some(free)
    }

    pub fn get(&self, block: &str, props: &[(String, String)]) -> Option<NoteState> {
        self.by_block.get(&state_name(block, props)).copied()
    }

    /// The custom block a note block state is.
    pub fn block_of(&self, note: NoteState) -> Option<BlockState> {
        parse_state_name(self.by_note.get(&note)?)
    }

    pub fn len(&self) -> usize {
        self.by_block.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_block.is_empty()
    }

    /// Every assigned state.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &NoteState)> {
        self.by_block.iter()
    }
}

/// Every combination of `properties`' values, in order: the first property
/// varies slowest.
pub fn combinations(properties: &[(String, Vec<String>)]) -> Vec<Vec<(String, String)>> {
    let mut out: Vec<Vec<(String, String)>> = vec![Vec::new()];
    for (name, values) in properties {
        let mut next = Vec::with_capacity(out.len() * values.len());
        for prefix in &out {
            for v in values {
                let mut p = prefix.clone();
                p.push((name.clone(), v.clone()));
                next.push(p);
            }
        }
        out = next;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn props(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(k, v)| ((*k).into(), (*v).into()))
            .collect()
    }

    #[test]
    fn slots_skip_note_zero_and_cover_the_rest() {
        let slots: Vec<NoteState> = NoteState::free_slots().collect();
        assert_eq!(slots.len(), 24 * 27 * 2);
        assert!(slots.iter().all(|s| s.note > 0));
        assert_eq!(slots[0].key(), "instrument=harp,note=1,powered=false");
        assert_eq!(NoteState::all().count(), 25 * 27 * 2);
    }

    #[test]
    fn states_are_kept_across_loads() {
        let path = std::env::temp_dir().join(format!("forja-states-{}.txt", std::process::id()));
        let mut a = Allocation::default();
        let n1 = a.assign("x:m", &props(&[("facing", "north")])).unwrap();
        let n2 = a.assign("x:m", &props(&[("facing", "east")])).unwrap();
        assert_ne!(n1, n2);
        assert_eq!(a.assign("x:m", &props(&[("facing", "north")])), Some(n1));
        a.save(&path).unwrap();
        let mut b = Allocation::load(&path);
        assert_eq!(b.get("x:m", &props(&[("facing", "east")])), Some(n2));
        assert_eq!(
            b.block_of(n2),
            Some(("x:m".into(), props(&[("facing", "east")])))
        );
        // A new block takes a slot nobody has.
        let n3 = b.assign("x:plain", &[]).unwrap();
        assert!(n3 != n1 && n3 != n2);
        assert_eq!(b.block_of(n3), Some(("x:plain".into(), Vec::new())));
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn reads_server_properties() {
        let n = NoteState::from_properties(&props(&[
            ("instrument", "bit"),
            ("note", "7"),
            ("powered", "true"),
        ]))
        .unwrap();
        assert_eq!(
            n.block_string(),
            "minecraft:note_block[instrument=bit,note=7,powered=true]"
        );
        assert!(NoteState::from_properties(&props(&[("note", "7")])).is_none());
    }

    #[test]
    fn combinations_in_order() {
        let c = combinations(&[
            ("facing".into(), vec!["north".into(), "east".into()]),
            ("active".into(), vec!["false".into(), "true".into()]),
        ]);
        assert_eq!(c.len(), 4);
        assert_eq!(c[1], props(&[("facing", "north"), ("active", "true")]));
        assert_eq!(combinations(&[]), vec![Vec::<(String, String)>::new()]);
    }
}
