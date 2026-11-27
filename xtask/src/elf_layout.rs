//! Host-side checks of the loadable segments emitted by the kernel linker.

use std::fs;
use std::ops::Range;

use super::{Opts, Result, build_kernel};

const ELF_HEADER: usize = 64;
const PROGRAM_HEADER: usize = 56;
const PAGE: u64 = 4096;
const EXECUTABLE: u32 = 1;
const WRITABLE: u32 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Image {
    Fixed,
    Dynamic,
}

pub(super) fn run(opts: &Opts, expect_pie: bool) -> Result<()> {
    if opts.bios || opts.expect_panic || !opts.expect.is_empty() || opts.initrd.is_some() {
        return Err("test-kernel-elf only checks the build artifact".into());
    }
    let path = build_kernel(opts, false)?;
    let bytes = fs::read(&path).map_err(|error| format!("reading {}: {error}", path.display()))?;
    let image = inspect(&bytes).map_err(|error| format!("{}: {error}", path.display()))?;
    if expect_pie && image != Image::Dynamic {
        return Err("expected an ET_DYN kernel image, found ET_EXEC".into());
    }
    let kind = match image {
        Image::Fixed => "ET_EXEC",
        Image::Dynamic => "ET_DYN",
    };
    println!("xtask: kernel ELF layout verified ({kind}, page separation, W^X, entry point)");
    Ok(())
}

fn at<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N]> {
    bytes
        .get(offset..)
        .and_then(|value| value.first_chunk::<N>())
        .copied()
        .ok_or_else(|| "truncated ELF field".into())
}

fn inspect(bytes: &[u8]) -> Result<Image> {
    if !bytes.starts_with(b"\x7fELF\x02\x01\x01")
        || u16::from_le_bytes(at(bytes, 18)?) != 62
        || u32::from_le_bytes(at(bytes, 20)?) != 1
        || usize::from(u16::from_le_bytes(at(bytes, 52)?)) != ELF_HEADER
        || usize::from(u16::from_le_bytes(at(bytes, 54)?)) != PROGRAM_HEADER
    {
        return Err("expected an ELF64 little-endian x86-64 kernel".into());
    }
    let image = match u16::from_le_bytes(at(bytes, 16)?) {
        2 => Image::Fixed,
        3 => Image::Dynamic,
        _ => return Err("unsupported kernel ELF type".into()),
    };
    let entry = u64::from_le_bytes(at(bytes, 24)?);
    let offset = usize::try_from(u64::from_le_bytes(at(bytes, 32)?))
        .map_err(|_| "ELF program table offset is out of range")?;
    let count = usize::from(u16::from_le_bytes(at(bytes, 56)?));
    let end = offset
        .checked_add(count * PROGRAM_HEADER)
        .ok_or("ELF program table offset overflow")?;
    let table = bytes
        .get(offset..end)
        .filter(|_| offset >= ELF_HEADER)
        .ok_or("ELF program table lies outside the image")?;
    let mut ranges = Vec::<Range<u64>>::new();
    let mut entry_is_executable = false;

    for (index, header) in table.as_chunks::<PROGRAM_HEADER>().0.iter().enumerate() {
        if u32::from_le_bytes(at(header, 0)?) != 1 {
            continue;
        }
        let flags = u32::from_le_bytes(at(header, 4)?);
        let offset = u64::from_le_bytes(at(header, 8)?);
        let address = u64::from_le_bytes(at(header, 16)?);
        let filesz = u64::from_le_bytes(at(header, 32)?);
        let memsz = u64::from_le_bytes(at(header, 40)?);
        if flags & (WRITABLE | EXECUTABLE) == (WRITABLE | EXECUTABLE) {
            return Err(format!("PT_LOAD {index} is writable and executable"));
        }
        if !address.is_multiple_of(PAGE) || !offset.is_multiple_of(PAGE) {
            return Err(format!("PT_LOAD {index} starts off a 4 KiB boundary"));
        }
        if memsz < filesz
            || offset
                .checked_add(filesz)
                .is_none_or(|end| end > bytes.len() as u64)
        {
            return Err(format!("PT_LOAD {index} has an invalid file range"));
        }
        if memsz == 0 {
            continue;
        }
        let length = memsz.checked_add(PAGE - 1).ok_or("PT_LOAD size overflow")? & !(PAGE - 1);
        let end = address
            .checked_add(length)
            .ok_or("PT_LOAD address overflow")?;
        if ranges
            .iter()
            .any(|range| address < range.end && range.start < end)
        {
            return Err(format!("PT_LOAD {index} overlaps another segment's pages"));
        }
        ranges.push(address..end);
        entry_is_executable |=
            flags & EXECUTABLE != 0 && (address..address + filesz).contains(&entry);
    }

    if !entry_is_executable {
        return Err("ELF entry does not point into file-backed executable code".into());
    }
    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(kind: u16) -> Vec<u8> {
        let mut bytes = vec![0; 8192];
        bytes[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
        bytes[16..18].copy_from_slice(&kind.to_le_bytes());
        bytes[18..20].copy_from_slice(&62u16.to_le_bytes());
        bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
        bytes[24..32].copy_from_slice(&0x2000u64.to_le_bytes());
        bytes[32..40].copy_from_slice(&64u64.to_le_bytes());
        bytes[52..54].copy_from_slice(&64u16.to_le_bytes());
        bytes[54..56].copy_from_slice(&56u16.to_le_bytes());
        bytes[56..58].copy_from_slice(&1u16.to_le_bytes());
        bytes[64..68].copy_from_slice(&1u32.to_le_bytes());
        bytes[68..72].copy_from_slice(&5u32.to_le_bytes());
        bytes[72..80].copy_from_slice(&4096u64.to_le_bytes());
        bytes[80..88].copy_from_slice(&0x2000u64.to_le_bytes());
        bytes[96..104].copy_from_slice(&64u64.to_le_bytes());
        bytes[104..112].copy_from_slice(&4096u64.to_le_bytes());
        bytes
    }

    #[test]
    fn accepts_fixed_and_dynamic_image_types() {
        assert_eq!(inspect(&image(2)), Ok(Image::Fixed));
        assert_eq!(inspect(&image(3)), Ok(Image::Dynamic));
    }

    #[test]
    fn rejects_invalid_layouts() {
        let mut bytes = image(2);
        bytes[68..72].copy_from_slice(&7u32.to_le_bytes());
        assert!(
            inspect(&bytes)
                .unwrap_err()
                .contains("writable and executable")
        );

        let mut bytes = image(2);
        bytes[24..32].copy_from_slice(&0x2040u64.to_le_bytes());
        assert!(inspect(&bytes).unwrap_err().contains("entry"));

        let mut bytes = image(2);
        bytes.copy_within(64..120, 120);
        bytes[56..58].copy_from_slice(&2u16.to_le_bytes());
        assert!(inspect(&bytes).unwrap_err().contains("overlaps"));

        assert!(
            inspect(&image(2)[..100])
                .unwrap_err()
                .contains("program table")
        );
    }
}
