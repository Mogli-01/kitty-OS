#![no_main]

use kitty_initrd::{Archive, EntryKind};
use kitty_vfs::{InitrdFs, Inode};
use libfuzzer_sys::fuzz_target;

const BLOCK: usize = 512;

fuzz_target!(|input: &[u8]| {
    exercise(input);

    // Keep arbitrary header bytes while satisfying the gates that precede
    // path validation, so short cases also reach the path and size logic.
    if input.len() <= 64 * 1024 {
        let (fields, payload) = input.split_at(input.len().min(BLOCK));
        let mut archive = vec![0; 3 * BLOCK + payload.len().next_multiple_of(BLOCK)];
        archive[..fields.len()].copy_from_slice(fields);
        archive[257..263].copy_from_slice(b"ustar\0");
        archive[263..265].copy_from_slice(b"00");
        if input.first().is_some_and(|byte| byte & 1 == 0) {
            archive[156] = b'0';
            archive[124..136].copy_from_slice(format!("{:011o}\0", payload.len()).as_bytes());
            archive[BLOCK..BLOCK + payload.len()].copy_from_slice(payload);
        }
        archive[148..156].fill(b' ');
        let sum: u32 = archive[..BLOCK].iter().map(|&byte| u32::from(byte)).sum();
        archive[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
        exercise(&archive);
    }
});

fn exercise(input: &[u8]) {
    if let Ok(archive) = Archive::parse(input) {
        let fs = InitrdFs::mount(input);
        for entry in archive.entries() {
            for part in entry.path.components() {
                assert!(!matches!(part, "" | "." | ".."));
                assert!(!part.contains(['/', '\\']));
            }
            if let EntryKind::File(data) = entry.kind {
                assert!(!entry.path.is_root());
                assert!(data.len() <= input.len());
            }
            if let Ok(fs) = &fs {
                let path = if entry.path.is_root() {
                    "/".to_owned()
                } else {
                    format!("/{}", entry.path)
                };
                match (entry.kind, fs.lookup(&path)) {
                    (EntryKind::Directory, Ok(Inode::Directory)) => {}
                    (EntryKind::File(data), Ok(Inode::File(file))) => {
                        let mut bytes = [0; 16];
                        let length = file.read_at(0, &mut bytes);
                        assert_eq!(file.len(), data.len());
                        assert_eq!(length, data.len().min(bytes.len()));
                        assert_eq!(&bytes[..length], &data[..length]);
                    }
                    _ => panic!("mounted entry did not resolve to its declared type"),
                }
            }
        }
    }
}
