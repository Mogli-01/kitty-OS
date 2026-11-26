#![no_main]

use kitty_shell::{CommandLine, LineBuffer, MAX_LINE_BYTES, MAX_WORDS, ParseError, ShellSession};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: &[u8]| {
    if let Ok(command) = CommandLine::parse(input) {
        assert!(input.len() <= MAX_LINE_BYTES);
        let mut count = 0;
        let mut size = 0;
        for word in command.words() {
            count += 1;
            size += word.len();
            assert!(word.chars().all(|ch| ch == '\t' || !ch.is_control()));
        }
        assert!(count <= MAX_WORDS);
        assert!(size <= input.len());
        let mut output = String::new();
        let _ = command.execute(&mut output);
        assert!(output.len() <= MAX_LINE_BYTES + 1);
    }

    let mut line = LineBuffer::new();
    for &byte in input {
        match byte & 7 {
            0 => line.move_left(),
            1 => line.move_right(),
            2 => line.backspace(),
            3 => line.delete(),
            4 => line.move_home(),
            5 => line.move_end(),
            6 => {
                let _ = line.insert(char::from(byte));
            }
            _ => {
                if let Some(ch) = char::from_u32(u32::from(byte) * 257) {
                    let _ = line.insert(ch);
                }
            }
        }
    }
    let text = std::str::from_utf8(line.as_bytes()).unwrap();
    assert!(text.is_char_boundary(line.cursor()));
    assert!(text.len() <= MAX_LINE_BYTES);
    assert!(!matches!(
        CommandLine::parse(line.as_bytes()),
        Err(ParseError::InvalidUtf8 | ParseError::ControlCharacter | ParseError::LineTooLong)
    ));

    let mut shell = ShellSession::default();
    let mut output = String::new();
    for &byte in input {
        assert!(shell.push(byte, &mut output).is_ok());
    }
    let text = std::str::from_utf8(shell.line().as_bytes()).unwrap();
    assert!(text.is_char_boundary(shell.line().cursor()));
    assert!(text.len() <= MAX_LINE_BYTES);
    assert!(
        output
            .chars()
            .all(|ch| matches!(ch, '\t' | '\n') || !ch.is_control())
    );
});
