use kitty_ps2::{Decoder, KeyAction, KeyCode, KeyEvent};

const PAUSE: [u8; 8] = [0xe1, 0x14, 0x77, 0xe1, 0xf0, 0x14, 0xf0, 0x77];

fn event(key: KeyCode, action: KeyAction) -> KeyEvent {
    KeyEvent { key, action }
}

fn decode(bytes: &[u8]) -> Vec<KeyEvent> {
    let mut decoder = Decoder::new();
    bytes
        .iter()
        .filter_map(|&byte| decoder.push(byte))
        .collect()
}

#[test]
fn preserves_presses_repeats_and_releases() {
    use KeyAction::{Pressed, Released};
    assert_eq!(
        decode(&[0x1c, 0x1c, 0xf0, 0x1c, 0x83, 0xf0, 0x83]),
        [
            event(KeyCode::A, Pressed),
            event(KeyCode::A, Pressed),
            event(KeyCode::A, Released),
            event(KeyCode::F7, Pressed),
            event(KeyCode::F7, Released),
        ]
    );
}

#[test]
fn preserves_physical_positions_for_iso_layout_and_keypad() {
    use KeyAction::{Pressed, Released};
    assert_eq!(
        decode(&[0x61, 0x75, 0xe0, 0x75, 0xe0, 0xf0, 0x75]),
        [
            event(KeyCode::IsoExtra, Pressed),
            event(KeyCode::Keypad8, Pressed),
            event(KeyCode::ArrowUp, Pressed),
            event(KeyCode::ArrowUp, Released),
        ]
    );
}

#[test]
fn distinguishes_left_and_right_modifiers() {
    use KeyAction::{Pressed, Released};
    assert_eq!(
        decode(&[0x11, 0xe0, 0x11, 0xe0, 0xf0, 0x11, 0xf0, 0x11]),
        [
            event(KeyCode::LeftAlt, Pressed),
            event(KeyCode::RightAlt, Pressed),
            event(KeyCode::RightAlt, Released),
            event(KeyCode::LeftAlt, Released),
        ]
    );
}

#[test]
fn print_screen_variants_do_not_emit_padding_shifts() {
    use KeyAction::{Pressed, Released};
    let expected = [
        event(KeyCode::PrintScreen, Pressed),
        event(KeyCode::PrintScreen, Released),
    ];
    assert_eq!(
        decode(&[0xe0, 0x12, 0xe0, 0x7c, 0xe0, 0xf0, 0x7c, 0xe0, 0xf0, 0x12]),
        expected
    );
    assert_eq!(decode(&[0xe0, 0x7c, 0xe0, 0xf0, 0x7c]), expected);
    assert_eq!(decode(&[0x84, 0xf0, 0x84]), expected);
}

#[test]
fn pause_emits_one_tap_and_ctrl_pause_emits_a_pair() {
    use KeyAction::{Pressed, Released, Tapped};
    assert_eq!(decode(&PAUSE), [event(KeyCode::Pause, Tapped)]);
    assert_eq!(
        decode(&[0xe0, 0x7e, 0xe0, 0xf0, 0x7e]),
        [
            event(KeyCode::Pause, Pressed),
            event(KeyCode::Pause, Released)
        ]
    );
}

#[test]
fn malformed_sequences_drop_pending_keys() {
    assert_eq!(
        decode(&[0xe1, 0x14, 0x77, 0xe1, 0xf0, 0x14, 0x1c, 0x32]),
        [event(KeyCode::B, KeyAction::Pressed)]
    );
    assert_eq!(
        decode(&[0xe1, 0x14, 0xe0, 0x75]),
        [event(KeyCode::ArrowUp, KeyAction::Pressed)]
    );
    assert_eq!(
        decode(&[0xf0, 0xfa, 0xfe, 0xaa, 0x00, 0xff, 0x1c]),
        [event(KeyCode::A, KeyAction::Pressed)]
    );
}

#[test]
fn reset_drops_a_pending_release_after_data_loss() {
    let mut decoder = Decoder::new();
    assert_eq!(decoder.push(0xe0), None);
    assert_eq!(decoder.push(0xf0), None);
    decoder.reset();
    assert_eq!(
        decoder.push(0x75),
        Some(event(KeyCode::Keypad8, KeyAction::Pressed))
    );
}

#[test]
fn every_byte_in_every_state_can_recover_after_an_error() {
    let base_prefixes: [&[u8]; 4] = [&[], &[0xe0], &[0xf0], &[0xe0, 0xf0]];
    let pause_prefixes = (1..PAUSE.len()).map(|length| &PAUSE[..length]);
    for prefix in base_prefixes.into_iter().chain(pause_prefixes) {
        for byte in 0..=u8::MAX {
            let mut decoder = Decoder::new();
            for &part in prefix {
                assert_eq!(decoder.push(part), None);
            }
            let _ = decoder.push(byte);
            assert_eq!(decoder.push(0x00), None);
            assert_eq!(
                decoder.push(0x1c),
                Some(event(KeyCode::A, KeyAction::Pressed))
            );
        }
    }
}
