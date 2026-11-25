extern crate std;

use super::*;
use std::{format, string::ToString, vec, vec::Vec};

fn check_sum(header: &mut [u8; BLOCK_LEN]) {
    header[148..156].fill(b' ');
    let sum: u32 = header.iter().map(|&byte| u32::from(byte)).sum();
    header[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
}

fn header(prefix: &[u8], name: &[u8], kind: u8, size: usize) -> [u8; BLOCK_LEN] {
    let mut block = [0; BLOCK_LEN];
    block[..name.len()].copy_from_slice(name);
    block[124..136].copy_from_slice(format!("{size:011o}\0").as_bytes());
    block[156] = kind;
    block[257..263].copy_from_slice(b"ustar\0");
    block[263..265].copy_from_slice(b"00");
    block[345..345 + prefix.len()].copy_from_slice(prefix);
    check_sum(&mut block);
    block
}

fn append(bytes: &mut Vec<u8>, header: &[u8; BLOCK_LEN], data: &[u8]) {
    bytes.extend_from_slice(header);
    bytes.extend_from_slice(data);
    bytes.resize(bytes.len().next_multiple_of(BLOCK_LEN), 0);
}

fn terminated(mut bytes: Vec<u8>) -> Vec<u8> {
    bytes.resize(bytes.len() + END_LEN, 0);
    bytes
}

#[test]
fn reads_regular_files_and_directories_without_exposing_padding() {
    let mut bytes = Vec::new();
    let data = vec![0x7a; 513];
    append(&mut bytes, &header(b"", b"./", b'5', 0), b"");
    // Some USTAR producers put a directory capacity in its size field.
    append(&mut bytes, &header(b"", b"./bin/", b'5', 1024), b"");
    append(
        &mut bytes,
        &header(b"./bin", b"sample", b'0', data.len()),
        &data,
    );
    *bytes.last_mut().unwrap() = 0xff;
    append(&mut bytes, &header(b"", b"bin/empty", 0, 0), b"");
    let mut bytes = terminated(bytes);
    bytes.extend_from_slice(&[0; BLOCK_LEN]);

    let archive = Archive::parse(&bytes).unwrap();
    let entries: Vec<_> = archive.entries().collect();
    assert_eq!(entries.len(), 4);
    assert!(entries[0].path.is_root());
    assert_eq!(entries[0].kind, EntryKind::Directory);
    assert_eq!(entries[1].path.to_string(), "bin");
    assert_eq!(entries[1].kind, EntryKind::Directory);
    assert_eq!(
        entries[2].path.components().collect::<Vec<_>>(),
        ["bin", "sample"]
    );
    assert_eq!(entries[2].kind, EntryKind::File(&data));
    assert_eq!(entries[3].kind, EntryKind::File(&[]));
}

#[test]
fn compares_paths_independently_of_the_header_split() {
    let mut bytes = Vec::new();
    append(&mut bytes, &header(b"", b"./share/readme", b'0', 1), b"a");
    append(&mut bytes, &header(b"./share", b"readme", b'7', 1), b"b");
    let bytes = terminated(bytes);
    let archive = Archive::parse(&bytes).unwrap();
    let entries: Vec<_> = archive.entries().collect();
    assert_eq!(entries[0].path, entries[1].path);
    assert_eq!(entries[1].kind, EntryKind::File(b"b"));
}

#[test]
fn accepts_maximum_width_path_fields_without_terminators() {
    let bytes = terminated(header(&[b'a'; 155], &[b'b'; 100], b'0', 0).to_vec());
    let archive = Archive::parse(&bytes).unwrap();
    let entry = archive.entries().next().unwrap();
    assert_eq!(entry.path.to_string().len(), 256);
    assert_eq!(
        entry.path.components().map(str::len).collect::<Vec<_>>(),
        [155, 100]
    );
}

#[test]
fn validates_checksums_and_rejects_other_formats() {
    let mut block = header(b"", b"boot", b'0', 0);
    block[99] = 1;
    assert_eq!(
        Archive::parse(&terminated(block.to_vec())).unwrap_err(),
        Error::InvalidChecksum
    );
    block[263] = b'1';
    check_sum(&mut block);
    assert_eq!(
        Archive::parse(&terminated(block.to_vec())).unwrap_err(),
        Error::UnsupportedFormat
    );
}

#[test]
fn validates_size_syntax_and_bounds() {
    let bytes = terminated(header(b"", b"huge", b'0', 2048).to_vec());
    assert_eq!(Archive::parse(&bytes).unwrap_err(), Error::TruncatedFile);
    for size_field in [
        b"00000000008\0",
        b"000000000000",
        b"00003\0 5    ",
        b"\x800000000000\0",
    ] {
        let mut block = header(b"", b"boot", b'0', 0);
        block[124..136].copy_from_slice(size_field);
        check_sum(&mut block);
        assert_eq!(
            Archive::parse(&terminated(block.to_vec())).unwrap_err(),
            Error::InvalidSize
        );
    }
}

#[test]
fn requires_two_end_blocks_and_only_zero_archive_padding() {
    assert_eq!(Archive::parse(&[]).unwrap_err(), Error::MissingEnd);
    assert_eq!(
        Archive::parse(&[0; BLOCK_LEN]).unwrap_err(),
        Error::MissingEnd
    );
    assert_eq!(
        Archive::parse(&[0; END_LEN + 1]).unwrap_err(),
        Error::InvalidArchiveLength
    );
    assert!(
        Archive::parse(&[0; END_LEN])
            .unwrap()
            .entries()
            .next()
            .is_none()
    );
    let mut bytes = vec![0; END_LEN + BLOCK_LEN];
    bytes[BLOCK_LEN] = 1;
    assert_eq!(Archive::parse(&bytes).unwrap_err(), Error::MissingEnd);
    bytes[BLOCK_LEN] = 0;
    bytes[END_LEN] = 1;
    assert_eq!(Archive::parse(&bytes).unwrap_err(), Error::TrailingData);
}

#[test]
fn checks_later_headers_before_exposing_earlier_files() {
    let mut bytes = header(b"", b"first", b'0', 0).to_vec();
    let mut damaged = header(b"", b"second", b'0', 0);
    damaged[99] = 1;
    bytes.extend_from_slice(&damaged);
    assert_eq!(
        Archive::parse(&terminated(bytes)).unwrap_err(),
        Error::InvalidChecksum
    );
}

#[test]
fn rejects_links_devices_and_extension_headers() {
    for kind in *b"12346gxLS" {
        let bytes = terminated(header(b"", b"file", kind, 0).to_vec());
        assert_eq!(
            Archive::parse(&bytes).unwrap_err(),
            Error::UnsupportedType(kind)
        );
    }
}

#[test]
fn rejects_absolute_traversing_or_ambiguous_components() {
    for name in [
        &b""[..],
        b"/boot",
        b"./../boot",
        b"bin/../boot",
        b"bin/./boot",
        b"bin//boot",
        b"bin/boot/",
        b"bin\\boot",
        b"bin/\x1bboot",
        b"bin/\xff",
    ] {
        let bytes = terminated(header(b"", name, b'0', 0).to_vec());
        assert_eq!(
            Archive::parse(&bytes).unwrap_err(),
            Error::InvalidPath,
            "{name:?}"
        );
    }
    for prefix in [&b"/bin"[..], b"../bin", b"bin/./bin", b"bin/"] {
        let bytes = terminated(header(prefix, b"boot", b'0', 0).to_vec());
        assert_eq!(
            Archive::parse(&bytes).unwrap_err(),
            Error::InvalidPath,
            "{prefix:?}"
        );
    }
}
