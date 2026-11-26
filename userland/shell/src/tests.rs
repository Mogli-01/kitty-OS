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
