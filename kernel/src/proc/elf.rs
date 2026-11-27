//! Strict ELF64/ET_EXEC loading from the immutable boot archive.

use crate::mm::{
    paging::{USER_END, USER_MIN},
    uspace::{UserPerm, UserSpace},
};

const PAGE: u64 = 4096;
const STACK_PAGES: u64 = 16;
const MAX_IMAGE_PAGES: u64 = 2048;
const PHDR_SIZE: usize = 56;
const MAX_SEGMENTS: usize = 16;

#[derive(Clone, Copy, Debug)]
pub(super) enum LoadError {
    Format,
    Layout,
    Resource,
}

#[derive(Clone, Copy)]
struct Load<'a> {
    start: u64,
    end: u64,
    contents: &'a [u8],
    perm: UserPerm,
}

fn read<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], LoadError> {
    let slice = bytes.get(offset..).ok_or(LoadError::Format)?;
    let chunk = slice.first_chunk::<N>().ok_or(LoadError::Format)?;
    Ok(*chunk)
}

pub(super) fn load(image: &[u8]) -> Result<(UserSpace, u64), LoadError> {
    if !image.starts_with(b"\x7fELF\x02\x01\x01")
        || u16::from_le_bytes(read(image, 16)?) != 2
        || u16::from_le_bytes(read(image, 18)?) != 62
        || u32::from_le_bytes(read(image, 20)?) != 1
        || u16::from_le_bytes(read(image, 52)?) != 64
        || usize::from(u16::from_le_bytes(read(image, 54)?)) != PHDR_SIZE
    {
        return Err(LoadError::Format);
    }
    let entry = u64::from_le_bytes(read(image, 24)?);
    let start =
        usize::try_from(u64::from_le_bytes(read(image, 32)?)).map_err(|_| LoadError::Format)?;
    let count = usize::from(u16::from_le_bytes(read(image, 56)?));
    if !(1..=32).contains(&count) || start < 64 {
        return Err(LoadError::Format);
    }
    let end = start
        .checked_add(count * PHDR_SIZE)
        .ok_or(LoadError::Format)?;
    let table = image.get(start..end).ok_or(LoadError::Format)?;
    let mut segments: [Option<Load<'_>>; MAX_SEGMENTS] = [None; MAX_SEGMENTS];
    let mut used = 0;
    let mut pages = 0u64;
    let mut executable_entry = false;
    let guard = super::STACK_TOP - (STACK_PAGES + 1) * PAGE;
    for header in table.as_chunks::<PHDR_SIZE>().0 {
        match u32::from_le_bytes(read(header, 0)?) {
            2 | 3 => return Err(LoadError::Format), // dynamic or interpreter
            1 => {}
            _ => continue,
        }
        let flags = u32::from_le_bytes(read(header, 4)?);
        let perm = match flags {
            4 => UserPerm::Read,
            5 => UserPerm::ReadExec,
            6 => UserPerm::ReadWrite,
            _ => return Err(LoadError::Layout),
        };
        let off = u64::from_le_bytes(read(header, 8)?);
        let addr = u64::from_le_bytes(read(header, 16)?);
        let file_size = u64::from_le_bytes(read(header, 32)?);
        let mem_size = u64::from_le_bytes(read(header, 40)?);
        let align = u64::from_le_bytes(read(header, 48)?);
        if align != PAGE
            || addr < USER_MIN
            || !addr.is_multiple_of(PAGE)
            || !off.is_multiple_of(PAGE)
            || file_size > mem_size
            || mem_size == 0
        {
            return Err(LoadError::Layout);
        }
        let rounded = mem_size.checked_add(PAGE - 1).ok_or(LoadError::Layout)? & !(PAGE - 1);
        let end = addr.checked_add(rounded).ok_or(LoadError::Layout)?;
        if end > guard || end > USER_END {
            return Err(LoadError::Layout);
        }
        pages = pages.checked_add(rounded / PAGE).ok_or(LoadError::Layout)?;
        if pages > MAX_IMAGE_PAGES {
            return Err(LoadError::Resource);
        }
        let limit = off.checked_add(file_size).ok_or(LoadError::Format)?;
        let contents = image
            .get(
                usize::try_from(off).map_err(|_| LoadError::Format)?
                    ..usize::try_from(limit).map_err(|_| LoadError::Format)?,
            )
            .ok_or(LoadError::Format)?;
        if segments[..used]
            .iter()
            .flatten()
            .any(|other| addr < other.end && other.start < end)
        {
            return Err(LoadError::Layout);
        }
        executable_entry |= perm == UserPerm::ReadExec && (addr..addr + file_size).contains(&entry);
        *segments.get_mut(used).ok_or(LoadError::Layout)? = Some(Load {
            start: addr,
            end,
            contents,
            perm,
        });
        used += 1;
    }
    if !executable_entry {
        return Err(LoadError::Layout);
    }
    let mut space = UserSpace::new().map_err(|_| LoadError::Resource)?;
    for segment in segments.into_iter().flatten() {
        for (page, address) in (segment.start..segment.end)
            .step_by(PAGE as usize)
            .enumerate()
        {
            let offset = page * PAGE as usize;
            let contents = segment.contents.get(offset..).unwrap_or_default();
            space
                .map_page(
                    address,
                    segment.perm,
                    &contents[..contents.len().min(PAGE as usize)],
                )
                .map_err(|_| LoadError::Resource)?;
        }
    }
    for address in
        ((super::STACK_TOP - STACK_PAGES * PAGE)..super::STACK_TOP).step_by(PAGE as usize)
    {
        space
            .map_page(address, UserPerm::ReadWrite, &[])
            .map_err(|_| LoadError::Resource)?;
    }
    if !space.audit().is_clean() {
        return Err(LoadError::Layout);
    }
    Ok((space, entry))
}
