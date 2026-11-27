//! The read-only root seen by processes: DIR and FILE objects on the initrd
//! (Phase 6d, docs/FASE6.md §6, D-058, D-064).
//!
//! There are no global paths: a process can only open something
//! *relative to a directory handle it already has* (`open_at`). The path
//! is untrusted: it must be relative, made of plain components (no empty,
//! `.` or `..` component), so it can never leave the directory of the
//! handle. Everything is read-only; nothing here can write.

use alloc::string::String;
use alloc::sync::Arc;

use kitty_vfs::{InitrdFs, Inode};

use crate::sync::IrqSpinLock;

/// Longest relative path accepted by `open_at`.
pub const MAX_PATH: usize = 255;

/// A directory of the root, by canonical absolute path (`/`, `/sbin`).
#[derive(Debug)]
pub struct Dir {
    path: String,
}

impl Dir {
    /// Its canonical path (for diagnostics; no kernel addresses).
    pub fn path(&self) -> &str {
        &self.path
    }
}

/// What `open_at` found.
pub enum Opened {
    /// A file.
    File(kitty_vfs::File<'static>),
    /// A directory.
    Dir(Arc<Dir>),
}

/// Why `open_at` failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FsError {
    /// Malformed path (absolute, empty, `.`/`..`, too long, not UTF-8).
    BadPath,
    /// Nothing there.
    NotFound,
    /// No root mounted (booted without an initrd).
    NoRoot,
}

static ROOT: IrqSpinLock<Option<Arc<InitrdFs<'static>>>> = IrqSpinLock::new(None);

/// Mounts the validated initrd as the processes' root. First call wins.
pub fn mount(archive: &'static [u8]) -> Result<(), kitty_vfs::Error> {
    let fs = InitrdFs::mount(archive)?;
    let mut root = ROOT.lock();
    if root.is_none() {
        *root = Some(Arc::new(fs));
    }
    Ok(())
}

/// The root directory, if a root is mounted.
pub fn root_dir() -> Option<Arc<Dir>> {
    ROOT.lock().as_ref().map(|_| {
        Arc::new(Dir {
            path: String::from("/"),
        })
    })
}

/// Checks that `rel` is a relative path of plain components.
fn valid_relative(rel: &str) -> bool {
    !rel.is_empty()
        && rel.len() <= MAX_PATH
        && !rel.starts_with('/')
        && rel
            .split('/')
            .all(|c| !c.is_empty() && c != "." && c != ".." && !c.contains('\0'))
}

/// Opens `rel` inside `dir`.
pub fn open_at(dir: &Dir, rel: &str) -> Result<Opened, FsError> {
    if !valid_relative(rel) {
        return Err(FsError::BadPath);
    }
    let mut path = String::new();
    path.try_reserve(dir.path.len() + 1 + rel.len())
        .map_err(|_| FsError::BadPath)?;
    path.push_str(&dir.path);
    if !path.ends_with('/') {
        path.push('/');
    }
    path.push_str(rel);
    let fs = ROOT
        .lock()
        .as_ref()
        .map(Arc::clone)
        .ok_or(FsError::NoRoot)?;
    match fs.lookup(&path) {
        Ok(Inode::File(file)) => Ok(Opened::File(file)),
        Ok(Inode::Directory) => Ok(Opened::Dir(Arc::new(Dir { path }))),
        Err(kitty_vfs::Error::NotFound) => Err(FsError::NotFound),
        Err(_) => Err(FsError::BadPath),
    }
}

/// Self-test of the path rules (no root needed).
pub fn selftest_paths() -> bool {
    let bad = ["", "/etc", "../etc", "etc/..", "./x", "a//b", "a/", "a\0b"];
    let good = ["etc", "etc/kitty.txt", "sbin/init"];
    bad.iter().all(|p| !valid_relative(p)) && good.iter().all(|p| valid_relative(p))
}
