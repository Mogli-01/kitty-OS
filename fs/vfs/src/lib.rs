//! Read-only filesystem operations on a validated USTAR initrd.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::vec::Vec;
use core::slice;
use kitty_initrd::{Archive, Entry, EntryKind, Path};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Archive(kitty_initrd::Error),
    DuplicatePath,
    FileAsAncestor,
    OutOfMemory,
    InvalidPath,
    NotFound,
    NotDirectory,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    File,
    Directory,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Inode<'a> {
    File(File<'a>),
    Directory,
}

/// A read-only view into the file contents, with the mount image lifetime.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct File<'a>(&'a [u8]);

impl File<'_> {
    pub const fn len(self) -> usize {
        self.0.len()
    }

    pub const fn is_empty(self) -> bool {
        self.0.is_empty()
    }

    /// Copies up to the requested length, returning zero at or past EOF.
    pub fn read_at(self, offset: u64, destination: &mut [u8]) -> usize {
        let Ok(offset) = usize::try_from(offset) else {
            return 0;
        };
        let Some(data) = self.0.get(offset..) else {
            return 0;
        };
        let length = destination.len().min(data.len());
        destination[..length].copy_from_slice(&data[..length]);
        length
    }
}

/// Mounted root backed by immutable initrd bytes.
#[derive(Debug)]
pub struct InitrdFs<'a> {
    entries: Vec<Entry<'a>>,
}

impl<'a> InitrdFs<'a> {
    /// Parses the image and creates an index ordered by canonical path.
    /// Implicit parent directories are resolved from that index.
    pub fn mount(bytes: &'a [u8]) -> Result<Self, Error> {
        let archive = Archive::parse(bytes).map_err(Error::Archive)?;
        let mut entries = Vec::new();
        for entry in archive.entries() {
            entries.try_reserve(1).map_err(|_| Error::OutOfMemory)?;
            entries.push(entry);
        }
        entries.sort_unstable_by(|left, right| left.path.components().cmp(right.path.components()));
        // An ancestor sorts directly before its first descendant. Duplicate
        // canonical paths likewise appear next to one another.
        for [current, next] in entries.array_windows::<2>() {
            if current.path == next.path {
                return Err(Error::DuplicatePath);
            }
            if matches!(current.kind, EntryKind::File(_)) && descends_from(current.path, next.path)
            {
                return Err(Error::FileAsAncestor);
            }
        }
        Ok(Self { entries })
    }

    /// Looks up a canonical absolute path such as `/bin/kitty-shell`.
    pub fn lookup(&self, path: &str) -> Result<Inode<'a>, Error> {
        match self.locate(absolute_path(path)?)? {
            Location::File(file) => Ok(Inode::File(file)),
            Location::Directory { .. } => Ok(Inode::Directory),
        }
    }

    /// Lists each direct child once, in path component order.
    pub fn read_dir(&self, path: &str) -> Result<ReadDir<'_, 'a>, Error> {
        let path = absolute_path(path)?;
        let Location::Directory { first_child } = self.locate(path)? else {
            return Err(Error::NotDirectory);
        };
        let descendants = &self.entries[first_child..];
        let count = descendants.partition_point(|entry| descends_from(path, entry.path));
        Ok(ReadDir {
            entries: descendants[..count].iter(),
            depth: path.components().count(),
            last: None,
        })
    }

    fn locate(&self, path: Path<'_>) -> Result<Location<'a>, Error> {
        match self
            .entries
            .binary_search_by(|entry| entry.path.components().cmp(path.components()))
        {
            Ok(index) => match self.entries[index].kind {
                EntryKind::File(data) => Ok(Location::File(File(data))),
                EntryKind::Directory => Ok(Location::Directory {
                    first_child: index + 1,
                }),
            },
            Err(index)
                if path.is_root()
                    || self
                        .entries
                        .get(index)
                        .is_some_and(|entry| descends_from(path, entry.path)) =>
            {
                Ok(Location::Directory { first_child: index })
            }
            Err(_) => Err(Error::NotFound),
        }
    }
}

enum Location<'a> {
    File(File<'a>),
    Directory { first_child: usize },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DirectoryEntry<'a> {
    pub name: &'a str,
    pub kind: Kind,
}

pub struct ReadDir<'fs, 'a> {
    entries: slice::Iter<'fs, Entry<'a>>,
    depth: usize,
    last: Option<&'a str>,
}

impl<'a> Iterator for ReadDir<'_, 'a> {
    type Item = DirectoryEntry<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        for entry in self.entries.by_ref() {
            let mut components = entry.path.components().skip(self.depth);
            let name = components.next()?;
            if self.last == Some(name) {
                continue;
            }
            self.last = Some(name);
            let kind = match (components.next(), entry.kind) {
                (None, EntryKind::File(_)) => Kind::File,
                _ => Kind::Directory,
            };
            return Some(DirectoryEntry { name, kind });
        }
        None
    }
}

fn absolute_path(path: &str) -> Result<Path<'_>, Error> {
    path.strip_prefix('/')
        .and_then(Path::from_relative)
        .ok_or(Error::InvalidPath)
}

fn descends_from(parent: Path<'_>, child: Path<'_>) -> bool {
    let mut child = child.components();
    parent
        .components()
        .all(|component| child.next() == Some(component))
        && child.next().is_some()
}

#[cfg(test)]
mod tests;
