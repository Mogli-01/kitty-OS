//! Read-only, zero-copy parser for the kitty-OS USTAR initrd format.

#![no_std]
#![forbid(unsafe_code)]

use core::{fmt, str};

const BLOCK_LEN: usize = 512;
const END_LEN: usize = 2 * BLOCK_LEN;
const CHECKSUM_START: usize = 148;
const CHECKSUM_END: usize = 156;
const MAX_PATH_LEN: usize = 155 + 1 + 100;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidArchiveLength,
    MissingEnd,
    TrailingData,
    UnsupportedFormat,
    InvalidChecksum,
    InvalidSize,
    TruncatedFile,
    UnsupportedType(u8),
    InvalidPath,
}

/// A validated relative path. The USTAR prefix and name remain borrowed
/// separately, including when they jointly exceed 100 bytes.
#[derive(Clone, Copy, Debug)]
pub struct Path<'a> {
    prefix: &'a str,
    name: &'a str,
}

impl<'a> Path<'a> {
    /// Validates a relative lookup path with the same component rules as an
    /// archive entry. An empty path denotes the root.
    pub fn from_relative(path: &'a str) -> Option<Self> {
        if path.len() > MAX_PATH_LEN || (!path.is_empty() && !valid_components(path)) {
            return None;
        }
        Some(Self {
            prefix: "",
            name: path,
        })
    }

    /// The optional entry for the archive root has no components.
    pub const fn is_root(self) -> bool {
        self.name.is_empty()
    }

    pub fn components(self) -> impl Iterator<Item = &'a str> {
        self.prefix
            .split('/')
            .chain(self.name.split('/'))
            .filter(|component| !component.is_empty())
    }
}

impl PartialEq for Path<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.components().eq(other.components())
    }
}

impl Eq for Path<'_> {}

impl fmt::Display for Path<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_root() {
            return f.write_str(".");
        }
        if !self.prefix.is_empty() {
            write!(f, "{}/", self.prefix)?;
        }
        f.write_str(self.name)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryKind<'a> {
    File(&'a [u8]),
    Directory,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Entry<'a> {
    pub path: Path<'a>,
    pub kind: EntryKind<'a>,
}

/// A structurally validated USTAR image. The VFS mount must check conflicts
/// between entries, such as duplicate paths or files used as directories.
#[derive(Debug)]
pub struct Archive<'a> {
    records: &'a [u8],
    entry_count: usize,
}

impl<'a> Archive<'a> {
    /// Validates every header and payload before exposing entries. Accepts
    /// regular files, directories and zero padding after the two end blocks.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, Error> {
        if !bytes.len().is_multiple_of(BLOCK_LEN) {
            return Err(Error::InvalidArchiveLength);
        }

        let mut remaining = bytes;
        let mut entry_count = 0;
        while let Some(header) = remaining.first_chunk::<BLOCK_LEN>() {
            if all_zero(header) {
                let (end, padding) = remaining
                    .split_at_checked(END_LEN)
                    .ok_or(Error::MissingEnd)?;
                if !all_zero(end) {
                    return Err(Error::MissingEnd);
                }
                if !all_zero(padding) {
                    return Err(Error::TrailingData);
                }
                let records = &bytes[..bytes.len() - remaining.len()];
                return Ok(Self {
                    records,
                    entry_count,
                });
            }
            let (_, tail) = read_record(remaining)?;
            remaining = tail;
            entry_count += 1;
        }
        Err(Error::MissingEnd)
    }

    /// The number of validated headers, including an explicit root entry.
    pub const fn entry_count(&self) -> usize {
        self.entry_count
    }

    /// Entries in archive order. Each file slice ends before its block padding.
    pub fn entries(&self) -> impl Iterator<Item = Entry<'a>> + '_ {
        let mut remaining = self.records;
        core::iter::from_fn(move || {
            if remaining.is_empty() {
                return None;
            }
            // The same immutable bytes were validated by Archive::parse.
            let (entry, tail) = read_record(remaining).ok()?;
            remaining = tail;
            Some(entry)
        })
    }
}

fn read_record(bytes: &[u8]) -> Result<(Entry<'_>, &[u8]), Error> {
    let (header, rest) = bytes
        .split_first_chunk::<BLOCK_LEN>()
        .ok_or(Error::MissingEnd)?;
    if &header[257..263] != b"ustar\0" || &header[263..265] != b"00" {
        return Err(Error::UnsupportedFormat);
    }
    let sum = u64::from(b' ') * (CHECKSUM_END - CHECKSUM_START) as u64
        + header[..CHECKSUM_START]
            .iter()
            .chain(&header[CHECKSUM_END..])
            .map(|&byte| u64::from(byte))
            .sum::<u64>();
    if octal(&header[CHECKSUM_START..CHECKSUM_END]) != Some(sum) {
        return Err(Error::InvalidChecksum);
    }

    let size = octal(&header[124..136]).ok_or(Error::InvalidSize)?;
    let (kind, tail) = match header[156] {
        0 | b'0' | b'7' => {
            let length = usize::try_from(size).map_err(|_| Error::InvalidSize)?;
            let padded = length
                .checked_next_multiple_of(BLOCK_LEN)
                .ok_or(Error::InvalidSize)?;
            let (data, tail) = rest.split_at_checked(padded).ok_or(Error::TruncatedFile)?;
            (EntryKind::File(&data[..length]), tail)
        }
        // Keep directory records header-only in both the host and kernel tools.
        b'5' if size == 0 => (EntryKind::Directory, rest),
        b'5' => return Err(Error::InvalidSize),
        other => return Err(Error::UnsupportedType(other)),
    };
    let prefix = read_name(&header[345..500])?;
    let name = read_name(&header[..100])?;
    let path = validate_path(prefix, name, matches!(kind, EntryKind::Directory))?;
    Ok((Entry { path, kind }, tail))
}

fn all_zero(bytes: &[u8]) -> bool {
    bytes.iter().all(|&byte| byte == 0)
}

fn read_name(field: &[u8]) -> Result<&str, Error> {
    let end = field
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(field.len());
    if !all_zero(&field[end..]) {
        return Err(Error::InvalidPath);
    }
    str::from_utf8(&field[..end]).map_err(|_| Error::InvalidPath)
}

fn validate_path<'a>(prefix: &'a str, name: &'a str, directory: bool) -> Result<Path<'a>, Error> {
    if name.is_empty() {
        return Err(Error::InvalidPath);
    }
    let prefix = prefix.trim_start_matches("./");
    let prefix = if prefix == "." { "" } else { prefix };
    let mut name = name;
    if prefix.is_empty() {
        name = name.trim_start_matches("./");
        if directory && (name.is_empty() || name == ".") {
            return Ok(Path {
                prefix: "",
                name: "",
            });
        }
    }
    if directory {
        name = name.strip_suffix('/').unwrap_or(name);
    }
    if !valid_components(name) || (!prefix.is_empty() && !valid_components(prefix)) {
        return Err(Error::InvalidPath);
    }
    Ok(Path { prefix, name })
}

fn valid_components(path: &str) -> bool {
    !path
        .chars()
        .any(|character| character == '\\' || character.is_control())
        && path.split('/').all(|part| !matches!(part, "" | "." | ".."))
}

fn octal(field: &[u8]) -> Option<u64> {
    let mut field = field;
    while let [b' ', rest @ ..] = field {
        field = rest;
    }
    let end = field
        .iter()
        .position(|byte| !(b'0'..=b'7').contains(byte))?;
    let (digits, padding) = field.split_at(end);
    if digits.is_empty() || padding.iter().any(|&byte| byte != b' ' && byte != 0) {
        return None;
    }
    digits.iter().try_fold(0u64, |value, &digit| {
        value.checked_mul(8)?.checked_add(u64::from(digit - b'0'))
    })
}

#[cfg(test)]
mod tests;
