//! Per-process handle table (Phase 6c, D-051, D-058).
//!
//! A handle is a `u32` that only means something inside one process:
//! - bits 0-15: index into the table;
//! - bits 16-30: **generation** of that slot, bumped every time the slot
//!   is freed, so a closed handle number never reaches the object that
//!   reuses the slot (no use-after-close);
//! - bit 31: always 0 (handles are non-negative in the syscall return).
//!
//! Handle `0` (index 0, generation 0) is the first handle of a process:
//! the `init` ABI fixes it to CONSOLE.
//!
//! Rights can only shrink: `dup` gives a subset of the original's, and
//! only if the original has `DUP`.

use alloc::sync::Arc;
use alloc::vec::Vec;

use super::Process;
use super::fs::Dir;

/// Rights of a handle (D-058: bits 0-15 generic, 16-31 per object type).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rights(u32);

impl Rights {
    /// Read from the object.
    pub const READ: Rights = Rights(1 << 0);
    /// Write to the object.
    pub const WRITE: Rights = Rights(1 << 1);
    /// Map the object into the address space.
    pub const MAP: Rights = Rights(1 << 2);
    /// Pass the handle to another process (moving it).
    pub const TRANSFER: Rights = Rights(1 << 3);
    /// Duplicate the handle (with equal or fewer rights).
    pub const DUP: Rights = Rights(1 << 4);
    /// Execute (images).
    pub const EXEC: Rights = Rights(1 << 5);
    /// Every generic right defined today.
    const GENERIC: u32 = 0x3F;

    /// From raw bits; `None` if a bit is not defined (bits 6-15 reserved,
    /// 16-31 per type, none defined yet).
    pub fn from_bits(bits: u32) -> Option<Rights> {
        (bits & !Self::GENERIC == 0).then_some(Rights(bits))
    }

    /// Raw bits.
    pub fn bits(self) -> u32 {
        self.0
    }

    /// Whether every right in `other` is also in `self`.
    pub fn contains(self, other: Rights) -> bool {
        self.0 & other.0 == other.0
    }

    /// Both sets.
    pub const fn union(self, other: Rights) -> Rights {
        Rights(self.0 | other.0)
    }
}

/// Kernel objects a handle can refer to.
#[derive(Clone)]
pub enum Object {
    /// The kernel console (D-052).
    Console,
    /// A directory of the read-only root (initrd).
    Dir(Arc<Dir>),
    /// A file of the read-only root (never written).
    File(kitty_vfs::File<'static>),
    /// A process (the parent's handle to a child).
    Process(Arc<Process>),
}

impl Object {
    /// Whether two objects are the same kernel object (identity, not just
    /// the same type).
    pub fn same(&self, other: &Object) -> bool {
        match (self, other) {
            (Object::Console, Object::Console) => true,
            (Object::Dir(a), Object::Dir(b)) => Arc::ptr_eq(a, b),
            (Object::File(a), Object::File(b)) => a == b,
            (Object::Process(a), Object::Process(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }

    /// Type code reported by `handle_info` (stable ABI, D-058):
    /// 1 CONSOLE, 2 FILE, 3 DIR, 4 PROCESS.
    pub fn kind(&self) -> u32 {
        match self {
            Object::Console => 1,
            Object::File(_) => 2,
            Object::Dir(_) => 3,
            Object::Process(_) => 4,
        }
    }
}

/// Why a handle operation failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandleError {
    /// No such handle (never existed, closed, or stale generation).
    BadHandle,
    /// The handle lacks a needed right, or rights would grow.
    Perm,
    /// The table is full.
    Full,
}

#[derive(Clone)]
struct Entry {
    object: Object,
    rights: Rights,
}

#[derive(Clone)]
struct Slot {
    generation: u16,
    entry: Option<Entry>,
}

/// Most handles a process may hold.
pub const MAX_HANDLES: usize = 1024;
const GENERATION_MASK: u16 = 0x7FFF;

/// The handle table of one process.
#[derive(Default)]
pub struct HandleTable {
    slots: Vec<Slot>,
}

impl HandleTable {
    /// An empty table.
    pub const fn new() -> Self {
        Self { slots: Vec::new() }
    }

    fn encode(index: usize, generation: u16) -> u32 {
        (u32::from(generation & GENERATION_MASK) << 16) | (index as u32 & 0xFFFF)
    }

    fn slot(&self, handle: u32) -> Option<(usize, &Slot)> {
        let index = (handle & 0xFFFF) as usize;
        let generation = ((handle >> 16) & 0x7FFF) as u16;
        if handle >> 31 != 0 {
            return None;
        }
        let slot = self.slots.get(index)?;
        (slot.generation == generation && slot.entry.is_some()).then_some((index, slot))
    }

    /// Adds a handle for `object` with `rights`; returns its number.
    pub fn insert(&mut self, object: Object, rights: Rights) -> Result<u32, HandleError> {
        let entry = Some(Entry { object, rights });
        if let Some(index) = self.slots.iter().position(|s| s.entry.is_none()) {
            let slot = &mut self.slots[index];
            slot.entry = entry;
            return Ok(Self::encode(index, slot.generation));
        }
        if self.slots.len() >= MAX_HANDLES {
            return Err(HandleError::Full);
        }
        self.slots.try_reserve(1).map_err(|_| HandleError::Full)?;
        self.slots.push(Slot {
            generation: 0,
            entry,
        });
        Ok(Self::encode(self.slots.len() - 1, 0))
    }

    /// The object and rights of `handle`, if it is valid.
    pub fn get(&self, handle: u32) -> Result<(Object, Rights), HandleError> {
        let (_, slot) = self.slot(handle).ok_or(HandleError::BadHandle)?;
        let entry = slot.entry.as_ref().ok_or(HandleError::BadHandle)?;
        Ok((entry.object.clone(), entry.rights))
    }

    /// The object of `handle` if it has every right in `needed`.
    pub fn check(&self, handle: u32, needed: Rights) -> Result<Object, HandleError> {
        let (object, rights) = self.get(handle)?;
        if rights.contains(needed) {
            Ok(object)
        } else {
            Err(HandleError::Perm)
        }
    }

    /// Closes `handle`; its number is never valid again.
    pub fn close(&mut self, handle: u32) -> Result<(), HandleError> {
        self.take(handle).map(drop)
    }

    /// Removes `handle` and returns what it held (used to *move* a handle
    /// to another process). Its number is never valid again.
    pub fn take(&mut self, handle: u32) -> Result<(Object, Rights), HandleError> {
        let (index, _) = self.slot(handle).ok_or(HandleError::BadHandle)?;
        let slot = &mut self.slots[index];
        let entry = slot.entry.take().ok_or(HandleError::BadHandle)?;
        slot.generation = slot.generation.wrapping_add(1) & GENERATION_MASK;
        Ok((entry.object, entry.rights))
    }

    /// Number of live handles.
    pub fn len(&self) -> usize {
        self.slots.iter().filter(|s| s.entry.is_some()).count()
    }

    /// Whether the table has no live handle.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether one more handle fits.
    pub fn has_room(&self) -> bool {
        self.slots.iter().any(|s| s.entry.is_none()) || self.slots.len() < MAX_HANDLES
    }

    /// A new handle to the same object with `rights`, which must be a
    /// subset of the original's; the original needs `DUP`.
    pub fn dup(&mut self, handle: u32, rights: Rights) -> Result<u32, HandleError> {
        let (object, have) = self.get(handle)?;
        if !have.contains(Rights::DUP) || !have.contains(rights) {
            return Err(HandleError::Perm);
        }
        self.insert(object, rights)
    }
}
