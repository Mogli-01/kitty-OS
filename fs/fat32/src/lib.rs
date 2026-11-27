//! Read-only FAT32 volumes over a stable, bounded byte reader.

#![no_std]
#![forbid(unsafe_code)]

mod layout;
mod name;

use layout::{CLUSTER_MASK, Layout, dword, word};
use name::{LongName, Name, short_name, valid_component};

/// The source could not fill the entire requested range.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReadError;

/// Byte access to a FAT32 volume, beginning at the volume boot sector.
///
/// The caller selects and bounds the volume before mounting. The source must
/// remain unchanged for the mount's lifetime, including across multiple reads.
/// Implementations for sector devices must support unaligned byte requests.
pub trait ReadAt {
    fn byte_len(&self) -> u64;
    fn read_exact_at(&self, offset: u64, out: &mut [u8]) -> Result<(), ReadError>;
}

impl ReadAt for &[u8] {
    fn byte_len(&self) -> u64 {
        self.len() as u64
    }

    fn read_exact_at(&self, offset: u64, out: &mut [u8]) -> Result<(), ReadError> {
        let offset = usize::try_from(offset).map_err(|_| ReadError)?;
        let bytes = self
            .get(offset..)
            .and_then(|tail| tail.get(..out.len()))
            .ok_or(ReadError)?;
        out.copy_from_slice(bytes);
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Read,
    InvalidBootSector,
    InvalidGeometry,
    TruncatedVolume,
    InvalidChain,
    InvalidEntry,
    InvalidPath,
    AmbiguousName,
    NotFound,
    NotDirectory,
    NotFile,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    File,
    Directory,
}

pub struct DirectoryEntry {
    name: Name,
    pub kind: Kind,
    pub size: u32,
}

impl DirectoryEntry {
    pub fn name(&self) -> &str {
        self.name.as_str()
    }
}

pub struct Fat32<R> {
    source: R,
    layout: Layout,
}

impl<R: ReadAt> Fat32<R> {
    pub fn mount(source: R) -> Result<Self, Error> {
        let mut boot = [0; 512];
        if source.byte_len() < boot.len() as u64 {
            return Err(Error::TruncatedVolume);
        }
        source
            .read_exact_at(0, &mut boot)
            .map_err(|_| Error::Read)?;
        let layout = Layout::parse(&boot, source.byte_len())?;
        Ok(Self { source, layout })
    }

    /// Finds a regular file by its long name or 8.3 alias, and validates
    /// its entire allocation chain against its declared size.
    pub fn open(&self, path: &str) -> Result<File<'_, R>, Error> {
        let Node::File { cluster, size } = self.lookup(path)? else {
            return Err(Error::NotFile);
        };
        if size != 0 {
            let expected = size.div_ceil(self.layout.cluster_bytes);
            if expected > self.layout.cluster_count {
                return Err(Error::InvalidChain);
            }
            let mut chain = Chain::new(cluster, self.layout);
            for _ in 1..expected {
                if !chain.advance(self)? {
                    return Err(Error::InvalidChain);
                }
            }
            if chain.advance(self)? {
                return Err(Error::InvalidChain);
            }
        }
        Ok(File {
            fs: self,
            start: cluster,
            current: cluster,
            index: 0,
            size,
        })
    }

    /// Lists direct children of a canonical absolute directory path.
    pub fn read_dir(&self, path: &str) -> Result<ReadDir<'_, R>, Error> {
        let Node::Directory(cluster) = self.lookup(path)? else {
            return Err(Error::NotDirectory);
        };
        Ok(ReadDir {
            cursor: Some(Directory::new(self, cluster)),
        })
    }

    fn lookup(&self, path: &str) -> Result<Node, Error> {
        const MAX_PATH_BYTES: usize = 4096;
        let relative = path.strip_prefix('/').ok_or(Error::InvalidPath)?;
        let mut node = Node::Directory(self.layout.root);
        if relative.is_empty() {
            return Ok(node);
        }
        if path.len() > MAX_PATH_BYTES || !relative.split('/').all(valid_component) {
            return Err(Error::InvalidPath);
        }
        for component in relative.split('/') {
            let Node::Directory(cluster) = node else {
                return Err(Error::NotDirectory);
            };
            let mut directory = Directory::new(self, cluster);
            let mut matched = None;
            while let Some(entry) = directory.next_entry()? {
                let matches = entry.short.as_str().eq_ignore_ascii_case(component)
                    || entry
                        .long
                        .as_ref()
                        .is_some_and(|name| name.as_str().eq_ignore_ascii_case(component));
                if matches && matched.replace(entry.node).is_some() {
                    return Err(Error::AmbiguousName);
                }
            }
            node = matched.ok_or(Error::NotFound)?;
        }
        Ok(node)
    }

    fn read(&self, offset: u64, bytes: &mut [u8]) -> Result<(), Error> {
        self.source
            .read_exact_at(offset, bytes)
            .map_err(|_| Error::Read)
    }

    fn next(&self, cluster: u32) -> Result<Option<u32>, Error> {
        let mut bytes = [0; 4];
        self.read(self.layout.fat_entry_offset(cluster), &mut bytes)?;
        self.layout.next_cluster(u32::from_le_bytes(bytes))
    }
}

#[derive(Clone, Copy)]
enum Node {
    Directory(u32),
    File { cluster: u32, size: u32 },
}

struct Found {
    short: Name,
    long: Option<Name>,
    node: Node,
}

/// Cursor through a cluster chain, bounded by the volume size. Brent's
/// algorithm detects cycles in constant memory without additional FAT reads.
struct Chain {
    current: u32,
    left: u32,
    anchor: u32,
    span: u32,
    distance: u32,
}

impl Chain {
    fn new(first: u32, layout: Layout) -> Self {
        Self {
            current: first,
            left: layout.cluster_count - 1,
            anchor: first,
            span: 1,
            distance: 0,
        }
    }

    fn advance<R: ReadAt>(&mut self, fs: &Fat32<R>) -> Result<bool, Error> {
        let Some(next) = fs.next(self.current)? else {
            return Ok(false);
        };
        if self.left == 0 || next == self.anchor {
            return Err(Error::InvalidChain);
        }
        self.left -= 1;
        self.current = next;
        self.distance += 1;
        if self.distance == self.span {
            self.anchor = next;
            self.span *= 2;
            self.distance = 0;
        }
        Ok(true)
    }
}

const DIRECTORY_BUFFER_BYTES: usize = 512;
const DIRECTORY_ENTRY_BYTES: usize = 32;

struct Directory<'a, R> {
    fs: &'a Fat32<R>,
    chain: Chain,
    offset: u32,
    long: LongName,
    buffer: [u8; DIRECTORY_BUFFER_BYTES],
}

impl<'a, R: ReadAt> Directory<'a, R> {
    fn new(fs: &'a Fat32<R>, cluster: u32) -> Self {
        Self {
            fs,
            chain: Chain::new(cluster, fs.layout),
            offset: 0,
            long: LongName::new(),
            buffer: [0; DIRECTORY_BUFFER_BYTES],
        }
    }

    fn next_entry(&mut self) -> Result<Option<Found>, Error> {
        loop {
            if self.offset == self.fs.layout.cluster_bytes {
                if !self.chain.advance(self.fs)? {
                    return Ok(None);
                }
                self.offset = 0;
            }
            let position = self.offset as usize % self.buffer.len();
            if position == 0 {
                let offset =
                    self.fs.layout.cluster_offset(self.chain.current) + u64::from(self.offset);
                self.fs.read(offset, &mut self.buffer)?;
            }
            // Every supported cluster is a multiple of the 512-byte buffer,
            // and each 32-byte record starts and ends inside one buffer.
            let entry: &[u8; DIRECTORY_ENTRY_BYTES] = self.buffer[position..]
                .first_chunk()
                .ok_or(Error::InvalidEntry)?;
            self.offset += DIRECTORY_ENTRY_BYTES as u32;
            match entry[0] {
                0x00 => return Ok(None),
                0xe5 => self.long.clear(),
                _ if entry[11] == 0x0f => self.long.entry(entry),
                _ => {
                    let long = self.long.take(entry);
                    if matches!(&entry[..11], b".          " | b"..         ")
                        || entry[11] & 0x08 != 0
                    {
                        continue;
                    }
                    let short = short_name(entry).ok_or(Error::InvalidEntry)?;
                    let cluster = ((u32::from(word(entry, 20)) << 16) | u32::from(word(entry, 26)))
                        & CLUSTER_MASK;
                    let size = dword(entry, 28);
                    let node = match (entry[11] & 0xd0, size, cluster) {
                        (0x10, 0, cluster) if self.fs.layout.contains(cluster) => {
                            Node::Directory(cluster)
                        }
                        (0x00, 0, 0) => Node::File { cluster, size },
                        (0x00, 1.., cluster) if self.fs.layout.contains(cluster) => {
                            Node::File { cluster, size }
                        }
                        _ => return Err(Error::InvalidEntry),
                    };
                    return Ok(Some(Found { short, long, node }));
                }
            }
        }
    }
}

pub struct ReadDir<'a, R> {
    cursor: Option<Directory<'a, R>>,
}

impl<R: ReadAt> Iterator for ReadDir<'_, R> {
    type Item = Result<DirectoryEntry, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.cursor.as_mut()?.next_entry() {
            Ok(Some(Found { short, long, node })) => {
                let (kind, size) = match node {
                    Node::Directory(_) => (Kind::Directory, 0),
                    Node::File { size, .. } => (Kind::File, size),
                };
                Some(Ok(DirectoryEntry {
                    name: long.unwrap_or(short),
                    kind,
                    size,
                }))
            }
            Ok(None) => {
                self.cursor = None;
                None
            }
            Err(error) => {
                self.cursor = None;
                Some(Err(error))
            }
        }
    }
}

pub struct File<'a, R> {
    fs: &'a Fat32<R>,
    start: u32,
    current: u32,
    index: u32,
    size: u32,
}

impl<R: ReadAt> File<'_, R> {
    pub fn len(&self) -> u32 {
        self.size
    }

    pub fn is_empty(&self) -> bool {
        self.size == 0
    }

    /// Copies bytes up to EOF. The destination may be partly written on
    /// a source error. Reading forward reuses the last visited cluster.
    pub fn read_at(&mut self, offset: u64, out: &mut [u8]) -> Result<usize, Error> {
        if offset >= u64::from(self.size) || out.is_empty() {
            return Ok(0);
        }
        let cluster_bytes = self.fs.layout.cluster_bytes;
        let offset = offset as u32;
        let length = out.len().min((self.size - offset) as usize);
        let wanted = offset / cluster_bytes;
        if wanted < self.index {
            self.current = self.start;
            self.index = 0;
        }
        while self.index < wanted {
            self.advance()?;
        }

        let mut within = offset % cluster_bytes;
        let mut remaining = &mut out[..length];
        loop {
            let take = remaining.len().min((cluster_bytes - within) as usize);
            let offset = self.fs.layout.cluster_offset(self.current) + u64::from(within);
            let (chunk, tail) = remaining.split_at_mut(take);
            self.fs.read(offset, chunk)?;
            if tail.is_empty() {
                return Ok(length);
            }
            self.advance()?;
            within = 0;
            remaining = tail;
        }
    }

    fn advance(&mut self) -> Result<(), Error> {
        self.current = self.fs.next(self.current)?.ok_or(Error::InvalidChain)?;
        self.index += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
