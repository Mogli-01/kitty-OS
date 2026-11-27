//! Pixel-art identity of kitty-OS: the 16-colour palette, a palette-indexed
//! canvas, the Pantufa mascot and the system scenes (boot, wallpaper room,
//! shutdown, kernel panic).
//!
//! Pure drawing code: no allocation, no `unsafe`, no hardware access. Every
//! buffer belongs to the caller, so the same code runs in the kernel, in the
//! compositor and in host tests. Scenes are drawn at a small base resolution
//! (320×180) in palette indices and scaled by an integer factor when they
//! are presented (`present_bgra`).

#![no_std]
#![forbid(unsafe_code)]

mod boot;
mod canvas;
mod cat;
mod font;
mod head;
mod math;
pub mod palette;
mod panic;
mod present;
mod room;
mod shutdown;

pub use boot::{BOOT_LENGTH_MS, BOOT_STAGES, BootFrame, BootSplash, SCENE_HEIGHT, SCENE_WIDTH};
pub use canvas::{BAYER, Canvas, bayer};
pub use cat::{CatSprite, CatStyle, Eyes, Mouth, Pose, SPRITE_HEIGHT, SPRITE_WIDTH, draw_cat};
pub use font::{GLYPH_ADVANCE, GLYPH_HEIGHT, MAX_TEXT_SCALE, has_glyph, text, text_width};
pub use head::{
    HEAD_SPRITE_HEIGHT, HEAD_SPRITE_WIDTH, HeadEyes, HeadMouth, HeadSprite, HeadStyle,
    MAX_HEAD_SCALE, Paws, draw_head,
};
pub use panic::{PANIC_MAX_CHARS, PANIC_MAX_LINES, PANIC_SCENE_MS, PanicKind, PanicScene};
pub use present::{PresentError, Target, present_bgra};
pub use room::{Room, Sky};
pub use shutdown::{SHUTDOWN_LENGTH_MS, Shutdown};

#[cfg(test)]
mod tests;
