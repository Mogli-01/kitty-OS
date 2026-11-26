//! Three-byte PS/2 mouse movement and button reports.

const SYNC: u8 = 1 << 3;
const X_SIGN: u8 = 1 << 4;
const Y_SIGN: u8 = 1 << 5;
const X_OVERFLOW: u8 = 1 << 6;
const Y_OVERFLOW: u8 = 1 << 7;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Buttons {
    pub left: bool,
    pub right: bool,
    pub middle: bool,
}

/// Current button state and relative motion. Positive X is right and
/// positive Y is down. An overflowing axis returns `None` independently.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MouseReport {
    pub buttons: Buttons,
    pub delta_x: Option<i16>,
    pub delta_y: Option<i16>,
}

#[derive(Clone, Copy, Debug)]
enum State {
    Header,
    X { header: u8 },
    Y { header: u8, x: u8 },
}

/// Receives bytes from a PS/2 mouse in standard three-byte streaming mode.
/// Feed only mouse data. The controller separates AUX bytes and handles
/// command responses before enabling reports.
#[derive(Debug)]
pub struct MouseDecoder {
    state: State,
}

impl Default for MouseDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl MouseDecoder {
    pub const fn new() -> Self {
        Self {
            state: State::Header,
        }
    }

    /// Discards a partial report after the transport signals lost data.
    /// Start again at the next known packet boundary. Payload bytes can
    /// also have bit 3 set, so it is only a check at a header position.
    pub fn reset(&mut self) {
        self.state = State::Header;
    }

    pub fn feed(&mut self, byte: u8) -> Option<MouseReport> {
        match self.state {
            State::Header => {
                if byte & SYNC != 0 {
                    self.state = State::X { header: byte };
                }
                None
            }
            State::X { header } => {
                self.state = State::Y { header, x: byte };
                None
            }
            State::Y { header, x } => {
                self.reset();
                Some(MouseReport {
                    buttons: Buttons {
                        left: header & 1 != 0,
                        right: header & 2 != 0,
                        middle: header & 4 != 0,
                    },
                    delta_x: axis(x, header, X_SIGN, X_OVERFLOW),
                    delta_y: axis(byte, header, Y_SIGN, Y_OVERFLOW).map(|delta| -delta),
                })
            }
        }
    }
}

fn axis(low: u8, header: u8, sign: u8, overflow: u8) -> Option<i16> {
    if header & overflow != 0 {
        return None;
    }
    // The header carries bit 8 of the signed motion value.
    let high = if header & sign != 0 { 256 } else { 0 };
    Some(i16::from(low) - high)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(decoder: &mut MouseDecoder, bytes: [u8; 3]) -> MouseReport {
        assert_eq!(decoder.feed(bytes[0]), None);
        assert_eq!(decoder.feed(bytes[1]), None);
        decoder.feed(bytes[2]).unwrap()
    }

    #[test]
    fn consumes_one_packet_and_reports_current_buttons() {
        let mut mouse = MouseDecoder::new();
        assert_eq!(
            report(&mut mouse, [0x09, 5, 9]),
            MouseReport {
                buttons: Buttons {
                    left: true,
                    right: false,
                    middle: false
                },
                delta_x: Some(5),
                delta_y: Some(-9),
            }
        );
        assert_eq!(
            report(&mut mouse, [0x0e, 0, 0]).buttons,
            Buttons {
                left: false,
                right: true,
                middle: true,
            }
        );
        assert_eq!(
            report(&mut mouse, [0x08, 0, 0]).buttons,
            Buttons {
                left: false,
                right: false,
                middle: false,
            }
        );
    }

    #[test]
    fn decodes_nine_bit_signed_values_and_inverts_y() {
        let mut mouse = MouseDecoder::new();
        let motion = report(&mut mouse, [0x38, 0, 0]);
        assert_eq!((motion.delta_x, motion.delta_y), (Some(-256), Some(256)));
        let motion = report(&mut mouse, [0x08, 0xff, 0xff]);
        assert_eq!((motion.delta_x, motion.delta_y), (Some(255), Some(-255)));
        let motion = report(&mut mouse, [0x18, 0xff, 0x80]);
        assert_eq!((motion.delta_x, motion.delta_y), (Some(-1), Some(-128)));
    }

    #[test]
    fn preserves_button_releases_and_valid_axis_on_overflow() {
        let mut mouse = MouseDecoder::new();
        assert!(report(&mut mouse, [0x09, 0, 0]).buttons.left);
        let motion = report(&mut mouse, [0x48, 0xaa, 2]);
        assert_eq!((motion.delta_x, motion.delta_y), (None, Some(-2)));
        assert!(!motion.buttons.left);
        let motion = report(&mut mouse, [0x8a, 3, 0xff]);
        assert_eq!((motion.delta_x, motion.delta_y), (Some(3), None));
        assert!(motion.buttons.right);
    }

    #[test]
    fn skips_unmarked_headers_and_resets_partial_packets() {
        let mut mouse = MouseDecoder::new();
        for ignored in [0x00, 0x07, 0x10, 0xe0] {
            assert_eq!(mouse.feed(ignored), None);
        }
        assert_eq!(mouse.feed(0x09), None);
        assert_eq!(mouse.feed(0x7f), None);
        mouse.reset();
        assert_eq!(mouse.feed(0x01), None);
        let motion = report(&mut mouse, [0x08, 0x08, 0x08]);
        assert_eq!((motion.delta_x, motion.delta_y), (Some(8), Some(-8)));
        assert!(!motion.buttons.left);
    }

    #[test]
    fn all_headers_and_payload_patterns_stay_in_range() {
        let mut mouse = MouseDecoder::new();
        for header in 0..=u8::MAX {
            if header & SYNC == 0 {
                assert_eq!(mouse.feed(header), None);
                continue;
            }
            for data in [0x00, 0x08, 0x7f, 0x80, 0xff] {
                let motion = report(&mut mouse, [header, data, data]);
                assert_eq!(motion.delta_x.is_none(), header & X_OVERFLOW != 0);
                assert_eq!(motion.delta_y.is_none(), header & Y_OVERFLOW != 0);
                assert!(motion.delta_x.is_none_or(|dx| (-256..=255).contains(&dx)));
                assert!(motion.delta_y.is_none_or(|dy| (-255..=256).contains(&dy)));
            }
        }
    }
}
