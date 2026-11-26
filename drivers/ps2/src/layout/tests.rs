use super::*;
use crate::{Decoder, KeyEvent};
use KeyAction::{Pressed, Released};
use KeyCode as K;

fn key(layout: &mut PortugueseKeyboard, key: KeyCode, action: KeyAction) -> Option<ConsoleBytes> {
    layout.push(InputEvent::Key(KeyEvent { key, action }))
}

fn writes(layout: &mut PortugueseKeyboard, code: KeyCode, expected: &[u8]) {
    let bytes = key(layout, code, Pressed).unwrap();
    assert_eq!(bytes.as_bytes(), expected);
}

#[test]
fn translates_a_real_set2_sequence_to_portuguese_text() {
    let mut decoder = Decoder::new();
    let mut layout = PortugueseKeyboard::default();
    let mut output = [0; 7];
    let mut len = 0;
    // C A F <acute> E <space> Enter, in the physical keyboard set.
    for byte in [0x21, 0x1c, 0x2b, 0x5b, 0x24, 0x29, 0x5a] {
        if let Some(bytes) = decoder
            .push(byte)
            .and_then(|event| layout.push(InputEvent::Key(event)))
        {
            let next = len + bytes.as_bytes().len();
            output[len..next].copy_from_slice(bytes.as_bytes());
            len = next;
        }
    }
    assert_eq!(&output[..len], "café \n".as_bytes());
}

#[test]
fn tracks_both_shifts_and_toggles_caps_once_per_press() {
    let mut layout = PortugueseKeyboard::default();
    assert!(key(&mut layout, K::LeftShift, Pressed).is_none());
    assert!(key(&mut layout, K::RightShift, Pressed).is_none());
    assert!(key(&mut layout, K::LeftShift, Released).is_none());
    writes(&mut layout, K::Semicolon, "Ç".as_bytes());
    assert!(key(&mut layout, K::RightShift, Released).is_none());
    writes(&mut layout, K::Semicolon, "ç".as_bytes());
    assert!(key(&mut layout, K::CapsLock, Pressed).is_none());
    assert!(key(&mut layout, K::CapsLock, Pressed).is_none());
    writes(&mut layout, K::A, b"A");
    assert!(key(&mut layout, K::RightShift, Pressed).is_none());
    writes(&mut layout, K::A, b"a");
    writes(&mut layout, K::Digit2, b"\"");
    assert!(key(&mut layout, K::CapsLock, Released).is_none());
    assert!(key(&mut layout, K::CapsLock, Pressed).is_none());
    assert!(key(&mut layout, K::RightShift, Released).is_none());
    writes(&mut layout, K::A, b"a");
}

#[test]
fn publishes_complete_controls_and_ignores_unhandled_shortcuts() {
    let mut layout = PortugueseKeyboard::default();
    writes(&mut layout, K::Delete, b"\x1b[3~");
    writes(&mut layout, K::ArrowLeft, b"\x1b[D");
    writes(&mut layout, K::Home, b"\x1b[H");
    assert!(key(&mut layout, K::RightAlt, Pressed).is_none());
    writes(&mut layout, K::Digit2, b"@");
    writes(&mut layout, K::E, "€".as_bytes());
    assert!(key(&mut layout, K::LeftControl, Pressed).is_none());
    assert!(key(&mut layout, K::C, Pressed).is_none());
    assert!(key(&mut layout, K::RightAlt, Released).is_none());
    writes(&mut layout, K::C, b"\x03");
    assert!(key(&mut layout, K::LeftControl, Released).is_none());
    assert!(key(&mut layout, K::LeftAlt, Pressed).is_none());
    assert!(key(&mut layout, K::A, Pressed).is_none());
    assert!(key(&mut layout, K::LeftAlt, Released).is_none());
    writes(&mut layout, K::A, b"a");
}

#[test]
fn applies_accents_and_clears_partial_state_after_loss() {
    let mut layout = PortugueseKeyboard::default();
    assert!(key(&mut layout, K::Backslash, Pressed).is_none());
    writes(&mut layout, K::O, "õ".as_bytes());
    assert!(key(&mut layout, K::Backslash, Pressed).is_none());
    writes(&mut layout, K::Space, b"~");
    assert!(key(&mut layout, K::RightBracket, Pressed).is_none());
    writes(&mut layout, K::X, "´x".as_bytes());
    assert!(key(&mut layout, K::Backslash, Pressed).is_none());
    assert!(key(&mut layout, K::Backspace, Pressed).is_none());
    writes(&mut layout, K::O, b"o");
    assert!(key(&mut layout, K::RightBracket, Pressed).is_none());
    assert!(key(&mut layout, K::LeftShift, Pressed).is_none());
    assert!(layout.push(InputEvent::Lost).is_none());
    writes(&mut layout, K::E, b"e");
}
