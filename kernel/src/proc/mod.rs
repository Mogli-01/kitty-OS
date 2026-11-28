//! Processes (Phase 6b, docs/FASE6.md §1-2, D-060).
//!
//! A [`Process`] is an address space ([`UserSpace`]) plus identity (PID,
//! and `uid`/`gid`, always 0 = "system" until Phase 15) and its fate. Its
//! threads are scheduler threads that run user code (`sched::spawn_user`);
//! today every process has exactly one.
//!
//! How a process ends (no syscalls yet, so it cannot exit by itself):
//! - **fault**: any exception caused by its code (bad pointer, privileged
//!   instruction, `sgdt` with UMIP, …) kills it — never the kernel;
//! - **kill**: [`Process::kill`] asks it to die; it does so the next time
//!   it would return to ring 3 (at the latest on the next timer tick).
//!
//! This module has no `unsafe`: the machine-level parts live in `arch`
//! (ring-3 entry, traps), `mm` (address spaces) and `sched` (switching).

use alloc::sync::Arc;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};

use crate::arch::x86_64::usermode;
use crate::mm::paging::MapError;
use crate::mm::uspace::{UserPerm, UserSpace};
use crate::sched::{self, SpawnError};
use crate::sync::SpinLock;

pub mod handles;

use handles::{HandleTable, Object, Rights};

/// Where test images put their code (docs/FASE6.md §1: ELF segments start
/// at 4 MiB).
pub const CODE_BASE: u64 = 0x0040_0000;
/// Data page of the test images (right after the code page).
pub const DATA_BASE: u64 = usermode::TEST_DATA;
/// Top of the main thread's stack (docs/FASE6.md §1). Only one page is
/// mapped below it for now; the page below that is never mapped (guard).
pub const STACK_TOP: u64 = 0x0000_7FFF_FFFF_0000;

/// How a process is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Its thread has not ended yet.
    Running,
    /// Ended by [`Process::kill`].
    Killed,
    /// Killed by the CPU exception `vector` it caused.
    Fault(u8),
    /// Ended itself with `exit(code)`.
    Exited(i32),
}

const STATUS_RUNNING: u32 = 0;
const STATUS_KILLED: u32 = 1;
const STATUS_EXITED: u32 = 2;
const STATUS_FAULT: u32 = 0x100;

/// Why a process could not be created.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcError {
    /// Building the address space failed.
    Map(MapError),
    /// Creating the thread failed.
    Spawn(SpawnError),
    /// The image does not fit the test layout (more than one code page).
    TooBig,
    /// The finished address space broke a user-half rule (kernel bug).
    AuditFailed,
}

/// A process.
pub struct Process {
    pid: u32,
    /// Owner (Phase 15). 0 = "system" for now.
    uid: u32,
    /// Group (Phase 15). 0 = "system" for now.
    gid: u32,
    space: UserSpace,
    kill: AtomicBool,
    status: AtomicU32,
    exit_code: AtomicI32,
    /// Its handles (D-051). Never held across a user copy or a block.
    handles: SpinLock<HandleTable>,
}

static NEXT_PID: AtomicU32 = AtomicU32::new(1);

impl Process {
    /// Builds a process from a raw code image for the self-tests: code at
    /// [`CODE_BASE`] (read + execute), a zeroed data page at [`DATA_BASE`]
    /// (read/write) and one stack page below [`STACK_TOP`]. The user half
    /// is audited before the process can run.
    pub fn from_test_image(code: &[u8]) -> Result<Arc<Self>, ProcError> {
        if code.len() > 4096 {
            return Err(ProcError::TooBig);
        }
        let mut space = UserSpace::new().map_err(ProcError::Map)?;
        space
            .map_page(CODE_BASE, UserPerm::ReadExec, code)
            .map_err(ProcError::Map)?;
        space
            .map_page(DATA_BASE, UserPerm::ReadWrite, &[])
            .map_err(ProcError::Map)?;
        space
            .map_page(STACK_TOP - 4096, UserPerm::ReadWrite, &[])
            .map_err(ProcError::Map)?;
        let audit = space.audit();
        if !audit.is_clean() || audit.mappings != 3 {
            return Err(ProcError::AuditFailed);
        }
        // The `init` ABI (docs/FASE6.md §6): handle 0 = CONSOLE.
        let mut handles = HandleTable::new();
        handles
            .insert(
                Object::Console,
                Rights::READ
                    .union(Rights::WRITE)
                    .union(Rights::DUP)
                    .union(Rights::TRANSFER),
            )
            .map_err(|_| ProcError::Map(MapError::OutOfMemory))?;
        Ok(Arc::new(Self {
            pid: NEXT_PID.fetch_add(1, Ordering::Relaxed),
            uid: 0,
            gid: 0,
            space,
            kill: AtomicBool::new(false),
            status: AtomicU32::new(STATUS_RUNNING),
            exit_code: AtomicI32::new(0),
            handles: SpinLock::new(handles),
        }))
    }

    /// Process identifier.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// `(uid, gid)`; both 0 until Phase 15.
    pub fn identity(&self) -> (u32, u32) {
        (self.uid, self.gid)
    }

    /// CR3 value for this process.
    pub fn root(&self) -> u64 {
        self.space.root()
    }

    /// Its address space (read-only access, for diagnostics and tests).
    pub fn space(&self) -> &UserSpace {
        &self.space
    }

    /// Current status.
    pub fn status(&self) -> Status {
        match self.status.load(Ordering::Acquire) {
            STATUS_RUNNING => Status::Running,
            STATUS_KILLED => Status::Killed,
            STATUS_EXITED => Status::Exited(self.exit_code.load(Ordering::Acquire)),
            other => Status::Fault((other & 0xFF) as u8),
        }
    }

    /// Asks the process to die. It ends before running another user
    /// instruction after its next trap into the kernel; a thread blocked in
    /// a system call is woken so it can notice.
    pub fn kill(&self) {
        self.kill.store(true, Ordering::Release);
        crate::console::wake_input_readers();
    }

    /// Whether [`Process::kill`] was called (blocking calls give up).
    pub fn is_killed(&self) -> bool {
        self.kill.load(Ordering::Acquire)
    }

    /// Its handle table (short critical sections only).
    pub fn handles(&self) -> &SpinLock<HandleTable> {
        &self.handles
    }

    fn end(&self, status: u32) {
        // First ending wins (a fault while being killed stays a fault).
        let _ = self.status.compare_exchange(
            STATUS_RUNNING,
            status,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
}

/// Starts the process's main thread at [`CODE_BASE`].
pub fn start(process: &Arc<Process>, name: &'static str) -> Result<sched::ThreadId, ProcError> {
    sched::spawn_user(name, Arc::clone(process), CODE_BASE, STACK_TOP).map_err(ProcError::Spawn)
}

/// Called by the exception handler when user code caused `vector`: ends
/// the current thread and records why. Never returns.
pub fn exit_current_on_fault(vector: u8) -> ! {
    if let Some(process) = sched::current_process() {
        process.end(STATUS_FAULT | u32::from(vector));
        // Drop our reference *before* leaving: `exit` never returns.
        drop(process);
    }
    sched::exit()
}

/// `exit(code)` system call: ends the current thread. Never returns.
pub fn exit_current(code: i32) -> ! {
    if let Some(process) = sched::current_process() {
        process.exit_code.store(code, Ordering::Release);
        process.end(STATUS_EXITED);
        drop(process);
    }
    sched::exit()
}

/// Called on every return to ring 3: honours a pending [`Process::kill`].
pub fn on_return_to_user() {
    let Some(process) = sched::current_process() else {
        return;
    };
    if process.kill.load(Ordering::Acquire) {
        process.end(STATUS_KILLED);
        drop(process);
        sched::exit();
    }
}

pub mod selftest;
