extern crate std;

use super::*;
use std::{format, string::String};

fn append(bytes: &mut Vec<u8>, prefix: &str, name: &str, kind: u8, data: &[u8]) {
    let mut header = [0u8; 512];
    header[..name.len()].copy_from_slice(name.as_bytes());
    header[124..136].copy_from_slice(format!("{:011o}\0", data.len()).as_bytes());
    header[148..156].fill(b' ');
    header[156] = kind;
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    header[345..345 + prefix.len()].copy_from_slice(prefix.as_bytes());
    let sum = header.iter().map(|byte| u32::from(*byte)).sum::<u32>();
    header[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
    bytes.extend_from_slice(&header);
    bytes.extend_from_slice(data);
    bytes.resize(bytes.len().next_multiple_of(512), 0);
}

fn finish(mut bytes: Vec<u8>) -> Vec<u8> {
    bytes.resize(bytes.len() + 1024, 0);
    bytes
}

fn fixture() -> Vec<u8> {
    let mut bytes = Vec::new();
    // Archive order deliberately differs from lookup and directory order.
    append(&mut bytes, "", "etc/init/config", b'0', b"mode=test\n");
    append(&mut bytes, "", "bin/sh", b'0', b"shell");
    append(&mut bytes, "", "./", b'5', b"");
    append(&mut bytes, "", "empty/", b'5', b"");
    append(&mut bytes, "", "binutils", b'0', b"tool");
    append(&mut bytes, "./bin", "cat", b'0', b"012345");
    append(&mut bytes, "", "bin/", b'5', b"");
    finish(bytes)
}

#[test]
fn looks_up_explicit_and_implicit_directories() {
    let bytes = fixture();
    let fs = InitrdFs::mount(&bytes).unwrap();
    for path in ["/", "/bin", "/etc", "/etc/init", "/empty"] {
        assert_eq!(fs.lookup(path), Ok(Inode::Directory));
    }
    for path in ["/bins", "/et", "/bin/c", "/empty/sh", "/bin/sh/child"] {
        assert_eq!(fs.lookup(path), Err(Error::NotFound));
    }
}

#[test]
fn lists_each_direct_child_once_with_the_correct_kind() {
    let bytes = fixture();
    let fs = InitrdFs::mount(&bytes).unwrap();
    let children = |path| {
        fs.read_dir(path)
            .unwrap()
            .map(|entry| (entry.name, entry.kind))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        children("/"),
        [
            ("bin", Kind::Directory),
            ("binutils", Kind::File),
            ("empty", Kind::Directory),
            ("etc", Kind::Directory)
        ]
    );
    assert_eq!(children("/bin"), [("cat", Kind::File), ("sh", Kind::File)]);
    assert_eq!(children("/etc"), [("init", Kind::Directory)]);
    assert!(children("/empty").is_empty());
    assert!(matches!(fs.read_dir("/binutils"), Err(Error::NotDirectory)));
    assert!(matches!(fs.read_dir("/missing"), Err(Error::NotFound)));
}

#[test]
fn bounds_reads_to_the_file_and_leaves_the_rest_of_the_buffer_intact() {
    let bytes = fixture();
    let fs = InitrdFs::mount(&bytes).unwrap();
    let Inode::File(file) = fs.lookup("/bin/cat").unwrap() else {
        panic!("expected a file");
    };
    assert_eq!(file.len(), 6);
    let mut buffer = [b'X'; 4];
    assert_eq!(file.read_at(4, &mut buffer), 2);
    assert_eq!(&buffer, b"45XX");
    assert_eq!(file.read_at(6, &mut buffer), 0);
    assert_eq!(file.read_at(u64::MAX, &mut buffer), 0);
    assert_eq!(file.read_at(0, &mut []), 0);
    assert_eq!(&buffer, b"45XX");
}

#[test]
fn rejects_noncanonical_absolute_requests() {
    let bytes = fixture();
    let fs = InitrdFs::mount(&bytes).unwrap();
    for path in [
        "",
        "bin/sh",
        "//bin",
        "/bin/./sh",
        "/bin/../etc",
        "/bin/sh/",
        "/bin\0/sh",
    ] {
        assert_eq!(fs.lookup(path), Err(Error::InvalidPath), "{path:?}");
    }
    let long_path = String::from("/") + &"a".repeat(257);
    assert_eq!(fs.lookup(&long_path), Err(Error::InvalidPath));
}

#[test]
fn refuses_aliases_even_when_the_archive_splits_the_path_differently() {
    let mut bytes = Vec::new();
    append(&mut bytes, "./bin", "sh", b'0', b"a");
    append(&mut bytes, "", "./bin/sh", b'0', b"b");
    assert_eq!(
        InitrdFs::mount(&finish(bytes)).unwrap_err(),
        Error::DuplicatePath
    );
}

#[test]
fn refuses_a_regular_file_as_an_ancestor() {
    let mut bytes = Vec::new();
    append(&mut bytes, "", "tools/sub/item", b'0', b"item");
    append(&mut bytes, "", "tools-other", b'0', b"tools");
    append(&mut bytes, "", "tools", b'0', b"no directory");
    assert_eq!(
        InitrdFs::mount(&finish(bytes)).unwrap_err(),
        Error::FileAsAncestor
    );
}

#[test]
fn mounts_an_empty_root_while_propagating_archive_errors() {
    let fs = InitrdFs::mount(&[0; 1024]).unwrap();
    assert_eq!(fs.lookup("/"), Ok(Inode::Directory));
    assert!(fs.read_dir("/").unwrap().next().is_none());
    assert_eq!(
        InitrdFs::mount(&[0; 512]).unwrap_err(),
        Error::Archive(kitty_initrd::Error::MissingEnd)
    );
}
