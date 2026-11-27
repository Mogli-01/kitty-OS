//! Creating processes from ELF images: `process_spawn` and `/sbin/init`
//! (Phase 6d, docs/FASE6.md §5-6, D-058, D-063, D-064, issue #79).
//!
//! A new process gets **only** the handles it is given:
//! - `SPAWN_COPY` needs `DUP` on the parent's handle; the parent keeps it;
//! - `SPAWN_MOVE` needs `TRANSFER`; the handle leaves the parent;
//! - in both, the child's rights must be a subset of the parent's.
//!
//! Child handle `i` is grant `i` (the order is the child's ABI).
//!
//! **All or nothing.** Every grant is validated, the image is loaded and
//! the child's thread is created *parked* before anything changes in the
//! parent. Only then, under the parent's handle-table lock, the moved
//! handles are re-checked (same number, so same generation), the parent's
//! PROCESS handle is inserted and the moved handles are taken. If any of
//! that fails, nothing in the parent changed and the parked child is
//! discarded. Only after the commit does the child run.

use alloc::sync::Arc;
use alloc::vec::Vec;

use super::elf::{ElfError, Image};
use super::handles::{HandleError, HandleTable, Object, Rights};
use super::{Process, STACK_TOP};
use crate::mm::uspace::{UserPerm, UserSpace};
use crate::sched;

/// Grant flag: copy the handle (needs `DUP`).
pub const SPAWN_COPY: u32 = 1;
/// Grant flag: move the handle (needs `TRANSFER`).
pub const SPAWN_MOVE: u32 = 2;
/// Most grants per spawn.
pub const MAX_GRANTS: usize = 64;
/// Pages of the main thread's stack (64 KiB, docs/FASE6.md §1); the page
/// below them is never mapped (guard).
pub const STACK_PAGES: u64 = 16;

/// One handle given to the child.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grant {
    /// Handle in the parent.
    pub handle: u32,
    /// Rights the child gets (⊆ the parent's).
    pub rights: u32,
    /// `SPAWN_COPY` or `SPAWN_MOVE`.
    pub flags: u32,
}

/// Why a spawn failed. The parent is unchanged in every case.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpawnFailure {
    /// A grant names no valid handle (or it changed meanwhile).
    BadHandle,
    /// A grant lacks DUP/TRANSFER or asks for more rights.
    Perm,
    /// Unknown flags or rights bits, too many grants, a moved handle
    /// listed twice.
    Invalid,
    /// The image was refused.
    Elf(ElfError),
    /// Out of memory, or the parent's table is full.
    NoMemory,
}

fn handle_failure(e: HandleError) -> SpawnFailure {
    match e {
        HandleError::BadHandle => SpawnFailure::BadHandle,
        HandleError::Perm => SpawnFailure::Perm,
        HandleError::Full => SpawnFailure::NoMemory,
    }
}

/// Builds a process from `image` with `objects` as its handles (index =
/// position) and a parked main thread. Nothing runs yet.
fn build(
    image: &[u8],
    objects: Vec<(Object, Rights)>,
) -> Result<(Arc<Process>, sched::ThreadId), SpawnFailure> {
    let elf = Image::parse(image).map_err(SpawnFailure::Elf)?;
    let mut space = UserSpace::new().map_err(|_| SpawnFailure::NoMemory)?;
    elf.load(&mut space).map_err(SpawnFailure::Elf)?;
    for i in 1..=STACK_PAGES {
        space
            .map_page(STACK_TOP - i * 4096, UserPerm::ReadWrite, &[])
            .map_err(|_| SpawnFailure::NoMemory)?;
    }
    if !space.audit().is_clean() {
        return Err(SpawnFailure::Elf(ElfError::OutOfRange));
    }
    let mut handles = HandleTable::new();
    for (object, rights) in objects {
        handles.insert(object, rights).map_err(handle_failure)?;
    }
    let process = Process::new(space, handles);
    let thread = sched::spawn_user_parked("user", Arc::clone(&process), elf.entry(), STACK_TOP)
        .map_err(|_| SpawnFailure::NoMemory)?;
    Ok((process, thread))
}

/// Starts a process directly from the kernel (no parent): `/sbin/init`.
pub fn spawn_from_kernel(
    image: &[u8],
    objects: Vec<(Object, Rights)>,
) -> Result<Arc<Process>, SpawnFailure> {
    let (process, thread) = build(image, objects)?;
    sched::unpark(thread);
    Ok(process)
}

/// `process_spawn` on behalf of `parent`. Returns the parent's new
/// PROCESS handle (with `READ`) to the child.
pub fn spawn(
    parent: &Process,
    image: &[u8],
    grants: &[Grant],
) -> Result<(u32, Arc<Process>), SpawnFailure> {
    if grants.len() > MAX_GRANTS {
        return Err(SpawnFailure::Invalid);
    }
    // 1. Validate every grant against the parent, collect the objects.
    let mut objects = Vec::new();
    objects
        .try_reserve(grants.len())
        .map_err(|_| SpawnFailure::NoMemory)?;
    {
        let table = parent.handles().lock();
        for (i, g) in grants.iter().enumerate() {
            let want = Rights::from_bits(g.rights).ok_or(SpawnFailure::Invalid)?;
            let needed = match g.flags {
                SPAWN_COPY => Rights::DUP,
                SPAWN_MOVE => Rights::TRANSFER,
                _ => return Err(SpawnFailure::Invalid),
            };
            if g.flags == SPAWN_MOVE && grants[..i].iter().any(|o| o.handle == g.handle) {
                return Err(SpawnFailure::Invalid);
            }
            if g.flags == SPAWN_MOVE && grants[i + 1..].iter().any(|o| o.handle == g.handle) {
                return Err(SpawnFailure::Invalid);
            }
            let (object, have) = table.get(g.handle).map_err(handle_failure)?;
            if !have.contains(needed) || !have.contains(want) {
                return Err(SpawnFailure::Perm);
            }
            objects.push((object, want));
        }
    }
    // 2. Build the child, parked (the only step that allocates a lot).
    let (child, thread) = build(image, objects)?;
    // 3. Commit under the parent's lock: re-check the moved handles, give
    //    the parent its PROCESS handle, take the moved handles.
    let committed = {
        let mut table = parent.handles().lock();
        let still_valid = grants.iter().filter(|g| g.flags == SPAWN_MOVE).all(|g| {
            table
                .get(g.handle)
                .is_ok_and(|(_, r)| r.contains(Rights::TRANSFER))
        });
        if !still_valid {
            Err(SpawnFailure::BadHandle)
        } else if !table.has_room() {
            Err(SpawnFailure::NoMemory)
        } else {
            match table.insert(Object::Process(Arc::clone(&child)), Rights::READ) {
                Ok(h) => {
                    let mut moved = Vec::new();
                    for g in grants.iter().filter(|g| g.flags == SPAWN_MOVE) {
                        if let Ok(taken) = table.take(g.handle) {
                            // Released after the lock (objects may hold
                            // processes, whose drop frees memory).
                            if moved.try_reserve(1).is_ok() {
                                moved.push(taken);
                            }
                        }
                    }
                    Ok((h, moved))
                }
                Err(e) => Err(handle_failure(e)),
            }
        }
    };
    match committed {
        Ok((h, moved)) => {
            drop(moved);
            sched::unpark(thread);
            Ok((h, child))
        }
        Err(e) => {
            sched::discard_parked(thread);
            Err(e)
        }
    }
}
