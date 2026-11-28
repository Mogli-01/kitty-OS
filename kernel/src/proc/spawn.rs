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
/// Possible stack tops (Phase 8b, D-067): the top is `STACK_TOP` minus a
/// random number of pages below this (2^16: 16 bits, a 256 MiB window).
pub const STACK_SLIDE_PAGES: u64 = 1 << 16;
/// Possible 16-byte offsets of the initial RSP below the top (8 bits).
pub const STACK_SUBPAGE_SLOTS: u64 = 256;

/// Lowest address the stack (or its guard page) may ever occupy.
pub const fn stack_window_bottom() -> u64 {
    STACK_TOP - (STACK_SLIDE_PAGES + STACK_PAGES + 1) * 4096
}

/// A random stack top and the initial RSP below it (16-byte aligned, as
/// the ABI wants at process entry).
fn random_stack() -> (u64, u64) {
    let top = STACK_TOP - crate::rng::below(STACK_SLIDE_PAGES) * 4096;
    let rsp = top - crate::rng::below(STACK_SUBPAGE_SLOTS) * 16;
    (top, rsp)
}

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
    retain_space: bool,
) -> Result<(Arc<Process>, sched::ThreadId), SpawnFailure> {
    let elf = Image::parse(image).map_err(SpawnFailure::Elf)?;
    // Image + stack + a generous allowance for page tables and the thread.
    if !super::can_commit(elf.pages() + STACK_PAGES + 32) {
        return Err(SpawnFailure::NoMemory);
    }
    let mut space = UserSpace::new().map_err(|_| SpawnFailure::NoMemory)?;
    // ASLR (Phase 8b): a random base for a PIE, a random stack for all.
    let base = elf.choose_base();
    elf.load(&mut space, base).map_err(SpawnFailure::Elf)?;
    let (stack_top, rsp) = random_stack();
    for i in 1..=STACK_PAGES {
        space
            .map_page(stack_top - i * 4096, UserPerm::ReadWrite, &[])
            .map_err(|_| SpawnFailure::NoMemory)?;
    }
    if !space.audit().is_clean() {
        return Err(SpawnFailure::Elf(ElfError::OutOfRange));
    }
    let mut handles = HandleTable::new();
    for (object, rights) in objects {
        handles.insert(object, rights).map_err(handle_failure)?;
    }
    let process = if retain_space {
        Process::new_for_test(space, handles)
    } else {
        Process::new(space, handles)
    }
    .ok_or(SpawnFailure::NoMemory)?;
    process.record_layout(base, stack_top);
    let thread = sched::spawn_user_parked("user", Arc::clone(&process), elf.entry(base), rsp)
        .map_err(|_| SpawnFailure::NoMemory)?;
    Ok((process, thread))
}

/// Starts a process directly from the kernel (no parent): `/sbin/init`.
pub fn spawn_from_kernel(
    image: &[u8],
    objects: Vec<(Object, Rights)>,
) -> Result<Arc<Process>, SpawnFailure> {
    let (process, thread) = build(image, objects, false)?;
    sched::unpark(thread);
    Ok(process)
}

/// Like [`spawn_from_kernel`] for self-tests: the address space is kept
/// after the process ends, so the test can inspect what it wrote.
pub fn spawn_test_from_kernel(
    image: &[u8],
    objects: Vec<(Object, Rights)>,
) -> Result<Arc<Process>, SpawnFailure> {
    let (process, thread) = build(image, objects, true)?;
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
    // What each moved handle referred to when validated: at commit time
    // the parent must still hold *the same object* under that number.
    let mut expected = Vec::new();
    expected
        .try_reserve(objects.len())
        .map_err(|_| SpawnFailure::NoMemory)?;
    expected.extend(objects.iter().map(|(o, _)| o.clone()));
    let mut moved = Vec::new();
    moved
        .try_reserve(grants.len())
        .map_err(|_| SpawnFailure::NoMemory)?;
    // 2. Build the child, parked (the only step that allocates a lot).
    let (child, thread) = build(image, objects, false)?;
    // 3. Commit under the parent's lock: re-check the moved handles, give
    //    the parent its PROCESS handle, take the moved handles.
    let committed = {
        let mut table = parent.handles().lock();
        let still_valid = grants
            .iter()
            .zip(expected.iter())
            .filter(|(g, _)| g.flags == SPAWN_MOVE)
            .all(|(g, was)| {
                table
                    .get(g.handle)
                    .is_ok_and(|(now, r)| r.contains(Rights::TRANSFER) && now.same(was))
            });
        if !still_valid {
            Err(SpawnFailure::BadHandle)
        } else if !table.has_room() {
            Err(SpawnFailure::NoMemory)
        } else {
            match table.insert(Object::Process(Arc::clone(&child)), Rights::READ) {
                Ok(h) => {
                    for g in grants.iter().filter(|g| g.flags == SPAWN_MOVE) {
                        if let Ok(taken) = table.take(g.handle) {
                            // Capacity reserved above: no allocation and
                            // no drop under the lock.
                            moved.push(taken);
                        }
                    }
                    Ok(h)
                }
                Err(e) => Err(handle_failure(e)),
            }
        }
    };
    // A moved INPUT changes reader: the new one must not see events
    // queued for the old one (D-053).
    if moved.iter().any(|(o, _)| matches!(o, Object::Input(_))) {
        crate::input::flush();
    }
    // Released after the lock (objects may hold processes, whose drop
    // frees memory).
    drop(moved);
    drop(expected);
    match committed {
        Ok(h) => {
            sched::unpark(thread);
            Ok((h, child))
        }
        Err(e) => {
            sched::discard_parked(thread);
            Err(e)
        }
    }
}
