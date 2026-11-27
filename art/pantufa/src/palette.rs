//! The 16 colours of the "Serão de Inverno" identity.
//!
//! Art is stored as palette indices (`0..=15`), never as RGB, so fades and
//! day/night changes are a palette operation instead of a redraw.

/// Index used for "nothing here" in sprites and scratch canvases.
pub const TRANSPARENT: u8 = 0xFF;

/// Number of palette entries.
pub const COLORS: usize = 16;

/// RGB values, indexed by the constants in [`idx`].
pub const PALETTE: [[u8; 3]; COLORS] = [
    [0x0F, 0x0E, 0x17],
    [0x1A, 0x1C, 0x2C],
    [0x29, 0x26, 0x4A],
    [0x3B, 0x3F, 0x73],
    [0x5B, 0x6E, 0xE1],
    [0x94, 0xB0, 0xC2],
    [0xE8, 0xE4, 0xD8],
    [0xF4, 0xC2, 0x7A],
    [0xFF, 0xD8, 0x4A],
    [0xF2, 0x9E, 0x4C],
    [0xD9, 0x73, 0x3B],
    [0xA1, 0x4A, 0x2A],
    [0x5E, 0x2F, 0x25],
    [0x7A, 0x8C, 0x5C],
    [0xC9, 0xA3, 0xD6],
    [0xE7, 0x6A, 0x8A],
];

/// Names for the palette indices.
pub mod idx {
    pub const BLACK: u8 = 0x0;
    pub const NIGHT: u8 = 0x1;
    pub const SHADOW: u8 = 0x2;
    pub const WALL: u8 = 0x3;
    pub const GLASS: u8 = 0x4;
    pub const RAIN: u8 = 0x5;
    pub const CREAM: u8 = 0x6;
    pub const LAMP: u8 = 0x7;
    pub const HIGHLIGHT: u8 = 0x8;
    pub const FUR: u8 = 0x9;
    pub const FUR_MID: u8 = 0xA;
    pub const STRIPES: u8 = 0xB;
    pub const OUTLINE: u8 = 0xC;
    pub const PLANT: u8 = 0xD;
    pub const BLANKET: u8 = 0xE;
    pub const NOSE: u8 = 0xF;
}

/// Maps each colour to one step darker. Applying it twice gives the dimmed
/// room used behind the login screen and the screen saver.
pub const DARKER: [u8; COLORS] = [0, 0, 1, 2, 2, 3, 5, 10, 7, 10, 11, 12, 1, 3, 3, 11];

/// RGB of `index` at `quarters`/4 brightness (0 = black, 4 = full).
/// Out-of-range indices (including [`TRANSPARENT`]) give the night colour.
pub const fn rgb(index: u8, quarters: u8) -> [u8; 3] {
    let base = if (index as usize) < COLORS {
        PALETTE[index as usize]
    } else {
        PALETTE[idx::NIGHT as usize]
    };
    let q = if quarters > 4 { 4 } else { quarters } as u16;
    [
        (base[0] as u16 * q / 4) as u8,
        (base[1] as u16 * q / 4) as u8,
        (base[2] as u16 * q / 4) as u8,
    ]
}

/// Same as [`rgb`], as the four BGRA8888 bytes used by the framebuffer.
pub const fn bgra(index: u8, quarters: u8) -> [u8; 4] {
    let [r, g, b] = rgb(index, quarters);
    [b, g, r, 0xFF]
}
