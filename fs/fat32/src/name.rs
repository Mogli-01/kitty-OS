// DOS OEM code page 437 for the upper half of short aliases.
const CP437_HIGH: [char; 128] = [
    '\u{c7}', '\u{fc}', '\u{e9}', '\u{e2}', '\u{e4}', '\u{e0}', '\u{e5}', '\u{e7}', '\u{ea}',
    '\u{eb}', '\u{e8}', '\u{ef}', '\u{ee}', '\u{ec}', '\u{c4}', '\u{c5}', '\u{c9}', '\u{e6}',
    '\u{c6}', '\u{f4}', '\u{f6}', '\u{f2}', '\u{fb}', '\u{f9}', '\u{ff}', '\u{d6}', '\u{dc}',
    '\u{a2}', '\u{a3}', '\u{a5}', '\u{20a7}', '\u{192}', '\u{e1}', '\u{ed}', '\u{f3}', '\u{fa}',
    '\u{f1}', '\u{d1}', '\u{aa}', '\u{ba}', '\u{bf}', '\u{2310}', '\u{ac}', '\u{bd}', '\u{bc}',
    '\u{a1}', '\u{ab}', '\u{bb}', '\u{2591}', '\u{2592}', '\u{2593}', '\u{2502}', '\u{2524}',
    '\u{2561}', '\u{2562}', '\u{2556}', '\u{2555}', '\u{2563}', '\u{2551}', '\u{2557}', '\u{255d}',
    '\u{255c}', '\u{255b}', '\u{2510}', '\u{2514}', '\u{2534}', '\u{252c}', '\u{251c}', '\u{2500}',
    '\u{253c}', '\u{255e}', '\u{255f}', '\u{255a}', '\u{2554}', '\u{2569}', '\u{2566}', '\u{2560}',
    '\u{2550}', '\u{256c}', '\u{2567}', '\u{2568}', '\u{2564}', '\u{2565}', '\u{2559}', '\u{2558}',
    '\u{2552}', '\u{2553}', '\u{256b}', '\u{256a}', '\u{2518}', '\u{250c}', '\u{2588}', '\u{2584}',
    '\u{258c}', '\u{2590}', '\u{2580}', '\u{3b1}', '\u{df}', '\u{393}', '\u{3c0}', '\u{3a3}',
    '\u{3c3}', '\u{b5}', '\u{3c4}', '\u{3a6}', '\u{398}', '\u{3a9}', '\u{3b4}', '\u{221e}',
    '\u{3c6}', '\u{3b5}', '\u{2229}', '\u{2261}', '\u{b1}', '\u{2265}', '\u{2264}', '\u{2320}',
    '\u{2321}', '\u{f7}', '\u{2248}', '\u{b0}', '\u{2219}', '\u{b7}', '\u{221a}', '\u{207f}',
    '\u{b2}', '\u{25a0}', '\u{a0}',
];

// Long names use at most 255 UTF-16 code units. Each unit needs at most
// three UTF-8 bytes (supplementary characters use four bytes for two units).
const MAX_UNITS: usize = 255;
const SLOT_UNITS: usize = 13;
const MAX_SLOTS: usize = MAX_UNITS.div_ceil(SLOT_UNITS);

pub(super) struct Name {
    bytes: [u8; MAX_UNITS * 3],
    len: usize,
}

impl Name {
    fn new() -> Self {
        Self {
            bytes: [0; MAX_UNITS * 3],
            len: 0,
        }
    }

    fn push(&mut self, character: char) {
        self.len += character.encode_utf8(&mut self.bytes[self.len..]).len();
    }

    pub fn as_str(&self) -> &str {
        str::from_utf8(&self.bytes[..self.len]).unwrap_or_default()
    }
}

fn allowed(character: char) -> bool {
    !character.is_control()
        && !matches!(
            character,
            '"' | '*' | '/' | ':' | '<' | '>' | '?' | '\\' | '|'
        )
}

pub(super) fn valid_component(component: &str) -> bool {
    !component.is_empty()
        && !matches!(component, "." | "..")
        && component.encode_utf16().count() <= MAX_UNITS
        && component.chars().all(allowed)
}

pub(super) fn short_name(entry: &[u8; 32]) -> Option<Name> {
    let mut name = Name::new();
    for (part, lowercase) in [
        (&entry[..8], entry[12] & 0x08 != 0),
        (&entry[8..11], entry[12] & 0x10 != 0),
    ] {
        let end = part
            .iter()
            .rposition(|&byte| byte != b' ')
            .map_or(0, |index| index + 1);
        if end == 0 {
            if name.len == 0 {
                return None;
            }
            break;
        }
        if name.len != 0 {
            name.push('.');
        }
        for (index, &byte) in part[..end].iter().enumerate() {
            let byte = if name.len == 0 && index == 0 && byte == 0x05 {
                0xe5
            } else {
                byte
            };
            let character = if byte.is_ascii() {
                char::from(byte)
            } else {
                CP437_HIGH[usize::from(byte - 0x80)]
            };
            if !allowed(character)
                || matches!(character, ' ' | '+' | ',' | '.' | ';' | '=' | '[' | ']')
            {
                return None;
            }
            name.push(if lowercase {
                character.to_ascii_lowercase()
            } else {
                character
            });
        }
    }
    Some(name)
}

pub(super) struct LongName {
    units: [u16; MAX_SLOTS * SLOT_UNITS],
    slots: usize,
    next: usize,
    checksum: u8,
}

impl LongName {
    pub const fn new() -> Self {
        Self {
            units: [0; MAX_SLOTS * SLOT_UNITS],
            slots: 0,
            next: 0,
            checksum: 0,
        }
    }

    pub fn clear(&mut self) {
        self.slots = 0;
        self.next = 0;
    }

    pub fn entry(&mut self, entry: &[u8; 32]) {
        let order = entry[0];
        let index = usize::from(order & 0x1f);
        if order & 0xa0 != 0
            || !(1..=MAX_SLOTS).contains(&index)
            || entry[12] != 0
            || entry[26..28] != [0, 0]
        {
            self.clear();
            return;
        }
        if order & 0x40 != 0 {
            self.slots = index;
            self.next = index;
            self.checksum = entry[13];
        }
        if self.next != index || entry[13] != self.checksum {
            self.clear();
            return;
        }
        let start = (index - 1) * SLOT_UNITS;
        for (slot, offset) in self.units[start..start + SLOT_UNITS]
            .iter_mut()
            .zip([1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30])
        {
            *slot = u16::from_le_bytes([entry[offset], entry[offset + 1]]);
        }
        self.next -= 1;
    }

    pub fn take(&mut self, short: &[u8; 32]) -> Option<Name> {
        let slots = self.slots;
        let complete = self.next == 0;
        self.clear();
        if !complete || slots == 0 || self.checksum != checksum(&short[..11]) {
            return None;
        }
        let units = &self.units[..slots * SLOT_UNITS];
        let end = units
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(units.len());
        if end == 0
            || end > MAX_UNITS
            || units[..end].contains(&0xffff)
            || (end < units.len() && !units[end + 1..].iter().all(|&unit| unit == 0xffff))
        {
            return None;
        }
        let mut name = Name::new();
        for character in char::decode_utf16(units[..end].iter().copied()) {
            let character = character.ok().filter(|&character| allowed(character))?;
            name.push(character);
        }
        (!matches!(name.as_str(), "." | "..")).then_some(name)
    }
}

pub(super) fn checksum(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0u8, |checksum, &byte| {
        checksum.rotate_right(1).wrapping_add(byte)
    })
}
