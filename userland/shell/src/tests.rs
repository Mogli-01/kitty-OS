extern crate std;

use std::{string::String, vec, vec::Vec};

use super::*;

#[test]
fn quotes_and_escapes_preserve_word_boundaries_and_utf8() {
    let parsed =
        CommandLine::parse("\techo  \"olá mundo\" 'a\\b' um\\ dois \"\" pré\"-fixo\"  ".as_bytes())
            .unwrap();
    assert_eq!(
        parsed.words().collect::<Vec<_>>(),
        ["echo", "olá mundo", "a\\b", "um dois", "", "pré-fixo"]
    );
    let parsed = CommandLine::parse(b"echo \"a\\\"b\" 'literal \"quotes\"'").unwrap();
    assert_eq!(
        parsed.words().collect::<Vec<_>>(),
        ["echo", "a\"b", "literal \"quotes\""]
    );
}

#[test]
fn rejects_invalid_input_before_dispatch() {
    for (input, error) in [
        (b"echo \xff".as_slice(), ParseError::InvalidUtf8),
        (b"echo \x1b".as_slice(), ParseError::ControlCharacter),
        ("echo \u{0085}".as_bytes(), ParseError::ControlCharacter),
        (b"echo 'unfinished".as_slice(), ParseError::UnclosedQuote),
        (b"echo unfinished\\".as_slice(), ParseError::TrailingEscape),
        (b"echo \"quote\\".as_slice(), ParseError::TrailingEscape),
    ] {
        assert_eq!(CommandLine::parse(input).err(), Some(error));
    }
}

#[test]
fn checks_the_byte_and_word_limits() {
    let max = vec![b'a'; MAX_LINE_BYTES];
    let line = CommandLine::parse(&max).unwrap();
    assert_eq!(line.words().next().map(str::len), Some(MAX_LINE_BYTES));
    assert_eq!(
        CommandLine::parse(&vec![b'a'; MAX_LINE_BYTES + 1]).err(),
        Some(ParseError::LineTooLong)
    );
    let line = "x ".repeat(MAX_WORDS);
    assert_eq!(
        CommandLine::parse(line.as_bytes()).unwrap().words().count(),
        MAX_WORDS
    );
    let line = "'' ".repeat(MAX_WORDS + 1);
    assert_eq!(
        CommandLine::parse(line.as_bytes()).err(),
        Some(ParseError::TooManyWords)
    );
}

#[test]
fn dispatches_builtins_and_reports_unknown_commands() {
    let mut output = String::new();
    CommandLine::parse(b" \t ")
        .unwrap()
        .execute(&mut output)
        .unwrap();
    assert!(output.is_empty());
    CommandLine::parse("echo \"olá mundo\" daqui".as_bytes())
        .unwrap()
        .execute(&mut output)
        .unwrap();
    assert_eq!(output, "olá mundo daqui\n");
    output.clear();
    CommandLine::parse(b"help")
        .unwrap()
        .execute(&mut output)
        .unwrap();
    assert!(output.contains("echo TEXT"));
    assert_eq!(
        CommandLine::parse(b"help extra")
            .unwrap()
            .execute(&mut output),
        Err(ExecuteError::UnexpectedArguments)
    );
    assert_eq!(
        CommandLine::parse(b"unknown").unwrap().execute(&mut output),
        Err(ExecuteError::UnknownCommand)
    );
}

#[test]
fn propagates_output_errors() {
    struct Unavailable;
    impl fmt::Write for Unavailable {
        fn write_str(&mut self, _: &str) -> fmt::Result {
            Err(fmt::Error)
        }
    }
    let line = CommandLine::parse(b"echo hello").unwrap();
    assert_eq!(
        line.execute(&mut Unavailable),
        Err(ExecuteError::Output(fmt::Error))
    );
}

#[test]
fn moves_and_deletes_at_utf8_character_boundaries() {
    let mut line = LineBuffer::new();
    for ch in "aé🦀z".chars() {
        line.insert(ch).unwrap();
    }
    line.move_left();
    assert_eq!(line.cursor(), "aé🦀".len());
    line.move_left();
    assert_eq!(line.cursor(), "aé".len());
    line.delete();
    assert_eq!(line.as_bytes(), "aéz".as_bytes());
    line.backspace();
    assert_eq!(line.as_bytes(), b"az");
    line.insert('ç').unwrap();
    assert_eq!(line.as_bytes(), "açz".as_bytes());
    line.move_right();
    line.move_right();
    line.delete();
    assert_eq!(line.cursor(), "açz".len());
    line.move_home();
    line.backspace();
    line.move_left();
    assert_eq!(line.cursor(), 0);
    line.move_right();
    assert_eq!(line.cursor(), 1);
    line.move_right();
    assert_eq!(line.cursor(), "aç".len());
}

#[test]
fn full_line_and_rejected_controls_leave_the_current_edit_intact() {
    let mut line = LineBuffer::new();
    for _ in 0..MAX_LINE_BYTES - 1 {
        line.insert('a').unwrap();
    }
    line.move_home();
    assert_eq!(line.insert('é'), Err(EditError::LineFull));
    assert_eq!(line.insert('\x1b'), Err(EditError::ControlCharacter));
    assert_eq!(line.cursor(), 0);
    assert_eq!(line.as_bytes(), vec![b'a'; MAX_LINE_BYTES - 1]);
    line.insert('\t').unwrap();
    assert_eq!(line.as_bytes()[0], b'\t');
    assert_eq!(line.as_bytes().len(), MAX_LINE_BYTES);
    line.clear();
    assert_eq!(line.as_bytes(), b"");
    assert_eq!(line.cursor(), 0);
}

#[test]
fn executes_an_edited_command() {
    let mut line = LineBuffer::new();
    for ch in "eco olá".chars() {
        line.insert(ch).unwrap();
    }
    line.move_home();
    line.move_right();
    line.move_right();
    line.insert('h').unwrap();
    let mut output = String::new();
    CommandLine::parse(line.as_bytes())
        .unwrap()
        .execute(&mut output)
        .unwrap();
    assert_eq!(output, "olá\n");
}
#[test]
fn console_submits_an_edited_line_after_split_sequences() {
    use crate::{SessionEvent, ShellSession};

    let mut shell = ShellSession::default();
    let mut output = String::new();
    for chunk in [
        b"echo cax".as_slice(),
        b"\x1b",
        b"[D",
        b"\x1b[3",
        b"~f\xc3",
        b"\xa9\xf0",
        b"\x9f\xa6",
        b"\x80",
    ] {
        for &byte in chunk {
            assert!(matches!(
                shell.push(byte, &mut output),
                Ok(SessionEvent::Edited | SessionEvent::Ignored)
            ));
        }
    }
    assert_eq!(shell.line().as_bytes(), "echo café🦀".as_bytes());
    assert_eq!(shell.push(b'\n', &mut output), Ok(SessionEvent::Submitted));
    assert_eq!(output, "café🦀\n");
    assert!(shell.line().as_bytes().is_empty());
}

#[test]
fn console_cancels_and_resynchronises_after_invalid_input() {
    use crate::{SessionEvent, ShellSession};

    let mut shell = ShellSession::default();
    let mut output = String::new();
    for &byte in b"echo discarded\x1b[" {
        assert!(shell.push(byte, &mut output).is_ok());
    }
    assert_eq!(shell.push(0x03, &mut output), Ok(SessionEvent::Cancelled));
    assert!(shell.line().as_bytes().is_empty());
    assert!(output.is_empty());

    // Move to the end via Home/Right/End. Up and the unknown CSI are ignored.
    // The incomplete and invalid UTF-8 sequences cannot swallow Enter.
    for &byte in b"echo\tx!\x1b[H\x1b[C\x1b[F\x7f\x1b[A\x1b[1;5D\xf0\xf0\x80\x80\x80\xe2\n" {
        assert!(shell.push(byte, &mut output).is_ok());
    }
    assert_eq!(output, "x\n");
}

#[test]
fn console_keeps_working_after_invalid_command_or_failed_output() {
    use crate::{SessionEvent, ShellSession};
    use core::fmt;

    struct BrokenOutput;

    impl fmt::Write for BrokenOutput {
        fn write_str(&mut self, _: &str) -> fmt::Result {
            Err(fmt::Error)
        }
    }

    let mut shell = ShellSession::default();
    let mut output = String::new();
    for &byte in b"echo 'unclosed\nno_such_command\nhelp extra\necho previous" {
        assert!(shell.push(byte, &mut output).is_ok());
    }
    assert_eq!(
        output,
        "unclosed quote\nunknown command\nunexpected arguments\n"
    );
    assert_eq!(shell.push(b'\n', &mut BrokenOutput), Err(fmt::Error));
    assert!(shell.line().as_bytes().is_empty());
    assert_eq!(shell.push(b'\n', &mut output), Ok(SessionEvent::Submitted));
    assert_eq!(
        output,
        "unclosed quote\nunknown command\nunexpected arguments\n"
    );
}

#[test]
fn console_keeps_the_line_editable_at_capacity() {
    use crate::{MAX_LINE_BYTES, SessionEvent, ShellSession};

    let mut shell = ShellSession::default();
    let mut output = String::new();
    for _ in 0..MAX_LINE_BYTES {
        assert_eq!(shell.push(b'x', &mut output), Ok(SessionEvent::Edited));
    }
    assert_eq!(
        shell.push(b'y', &mut output),
        Ok(SessionEvent::EditRejected)
    );
    assert_eq!(shell.line().as_bytes().len(), MAX_LINE_BYTES);
    assert_eq!(shell.push(0x7f, &mut output), Ok(SessionEvent::Edited));
    assert_eq!(shell.push(b'y', &mut output), Ok(SessionEvent::Edited));
    assert_eq!(shell.line().as_bytes().last(), Some(&b'y'));
    assert!(output.is_empty());
}
