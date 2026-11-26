#![no_main]

use kitty_shell::{CommandLine, MAX_LINE_BYTES, MAX_WORDS};
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
});
