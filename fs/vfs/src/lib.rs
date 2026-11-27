//! Read-only filesystem operations on a validated USTAR initrd.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::vec::Vec;
use core::slice;
use kitty_initrd::{Archive, Entry, EntryKind, Path};

const MAX_ENTRIES: usize = 65_536;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Archive(kitty_initrd::Error),
    DuplicatePath,
    FileAsAncestor,
    TooManyEntries,
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
    /// Accepts at most 65,536 headers. Implicit parent directories are
    /// resolved from the same index.
    pub fn mount(bytes: &'a [u8]) -> Result<Self, Error> {
        let archive = Archive::parse(bytes).map_err(Error::Archive)?;
        let mut entries = allocate_index(archive.entry_count())?;
        entries.extend(archive.entries());
        entries.sort_unstable_by(|left, right| left.path.components().cmp(right.path.components()));
        // An ancestor sorts directly before its first descendant. Duplicate
        // canonical paths likewise appear next to one another.
        for [current, next] in entries.array_windows::<2>() {
            if current.path == next.path {
                return Err(Error::DuplicatePath);
            }
            if matches!(current.kind, EntryKind::File(_))
                && has_descendant_path(0, current.path, next.path)
            {
                return Err(Error::FileAsAncestor);
            }
        }
        Ok(Self { entries })
    }

    /// Looks up a canonical absolute path such as `/bin/kitty-shell`.
    pub fn lookup(&self, path: &str) -> Result<Inode<'a>, Error> {
        locate(&self.entries, 0, absolute_path(path)?).map(Location::inode)
    }

    /// Lists each direct child once, in path component order.
    pub fn read_dir(&self, path: &str) -> Result<ReadDir<'_, 'a>, Error> {
        Ok(self.directory(path)?.read_dir())
    }

    /// Opens a directory view for trusted kernel code. Subsequent lookups
    /// on the view can only resolve entries below that directory.
    pub fn directory(&self, path: &str) -> Result<Directory<'_, 'a>, Error> {
        open_directory(&self.entries, 0, absolute_path(path)?)
    }
}

/// A view of one subtree in the mounted archive. It carries no access
/// rights; the kernel checks the rights of the caller's handle first.
pub struct Directory<'fs, 'a> {
    entries: &'fs [Entry<'a>],
    depth: usize,
}

impl<'fs, 'a> Directory<'fs, 'a> {
    /// Looks up a nonempty canonical relative path beneath this directory.
    pub fn lookup(&self, path: &str) -> Result<Inode<'a>, Error> {
        locate(self.entries, self.depth, relative_path(path)?).map(Location::inode)
    }

    /// Narrows the view to a child directory.
    pub fn directory(&self, path: &str) -> Result<Self, Error> {
        open_directory(self.entries, self.depth, relative_path(path)?)
    }

    /// Lists the direct children of this directory.
    pub fn read_dir(&self) -> ReadDir<'fs, 'a> {
        ReadDir {
            entries: self.entries.iter(),
            depth: self.depth,
            last: None,
        }
    }
}

fn open_directory<'fs, 'a>(
    entries: &'fs [Entry<'a>],
    depth: usize,
    path: Path<'_>,
) -> Result<Directory<'fs, 'a>, Error> {
    let Location::Directory { first_child } = locate(entries, depth, path)? else {
        return Err(Error::NotDirectory);
    };
    let descendants = &entries[first_child..];
    let count = descendants.partition_point(|entry| has_descendant_path(depth, path, entry.path));
    Ok(Directory {
        entries: &descendants[..count],
        depth: depth + path.components().count(),
    })
}

fn locate<'a>(entries: &[Entry<'a>], depth: usize, path: Path<'_>) -> Result<Location<'a>, Error> {
    match entries
        .binary_search_by(|entry| entry.path.components().skip(depth).cmp(path.components()))
    {
        Ok(index) => match entries[index].kind {
            EntryKind::File(data) => Ok(Location::File(File(data))),
            EntryKind::Directory => Ok(Location::Directory {
                first_child: index + 1,
            }),
        },
        Err(index)
            if path.is_root()
                || entries
                    .get(index)
                    .is_some_and(|entry| has_descendant_path(depth, path, entry.path)) =>
        {
            Ok(Location::Directory { first_child: index })
        }
        Err(_) => Err(Error::NotFound),
    }
}

fn allocate_index<'a>(count: usize) -> Result<Vec<Entry<'a>>, Error> {
    if count > MAX_ENTRIES {
        return Err(Error::TooManyEntries);
    }
    let mut entries = Vec::new();
    entries
        .try_reserve_exact(count)
        .map_err(|_| Error::OutOfMemory)?;
    Ok(entries)
}

enum Location<'a> {
    File(File<'a>),
    Directory { first_child: usize },
}

impl<'a> Location<'a> {
    fn inode(self) -> Inode<'a> {
        match self {
            Self::File(file) => Inode::File(file),
            Self::Directory { .. } => Inode::Directory,
        }
    }
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

fn relative_path(path: &str) -> Result<Path<'_>, Error> {
    Path::from_relative(path)
        .filter(|path| !path.is_root())
        .ok_or(Error::InvalidPath)
}

fn has_descendant_path(depth: usize, parent: Path<'_>, child: Path<'_>) -> bool {
    let mut child = child.components().skip(depth);
    parent
        .components()
        .all(|component| child.next() == Some(component))
        && child.next().is_some()
}

#[cfg(test)]
mod tests;
