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

pub mod elf;
pub mod fs;
pub mod handles;
pub mod spawn;

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
    /// Its address space. Freed as soon as its last thread is reaped
    /// (a dead process kept by a parent's PROCESS handle holds only its
    /// pid and status), except for self-test images, which tests inspect
    /// after they end.
    space: SpinLock<Option<UserSpace>>,
    /// CR3 value (the PML4 of `space`), fixed for the process's life.
    root: u64,
    /// Keep `space` after death (self-test images only).
    retain_space: bool,
    /// Where the image and the stack were placed (ASLR; diagnostics and
    /// tests only — never shown to other processes).
    layout: core::sync::atomic::AtomicU64,
    stack_top: core::sync::atomic::AtomicU64,
    kill: AtomicBool,
    status: AtomicU32,
    exit_code: AtomicI32,
    /// Its handles (D-051). Never held across a user copy or a block.
    handles: SpinLock<HandleTable>,
}

static NEXT_PID: AtomicU32 = AtomicU32::new(1);

/// Most processes that may exist at once (alive or not yet released): a
/// spawn storm fails with `E_NOMEM` instead of exhausting kernel memory.
pub const MAX_PROCESSES: usize = 256;
static LIVE_PROCESSES: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

/// Rights of the CONSOLE handle a process starts with.
pub const CONSOLE_RIGHTS: Rights = Rights::READ
    .union(Rights::WRITE)
    .union(Rights::DUP)
    .union(Rights::TRANSFER);
/// Rights of the INPUT handle: read and move, **never** duplicate (one
/// reader at a time, D-053/D-065).
pub const INPUT_RIGHTS: Rights = Rights::READ.union(Rights::TRANSFER);
/// Rights of the root DIR handle `/sbin/init` gets.
pub const ROOT_RIGHTS: Rights = Rights::READ
    .union(Rights::EXEC)
    .union(Rights::DUP)
    .union(Rights::TRANSFER);

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
            .insert(Object::Console, CONSOLE_RIGHTS)
            .map_err(|_| ProcError::Map(MapError::OutOfMemory))?;
        Self::create(space, handles, true).ok_or(ProcError::Map(MapError::OutOfMemory))
    }

    /// A process with `space` and `handles` (nothing runs yet). `None` if
    /// [`MAX_PROCESSES`] processes already exist.
    pub fn new(space: UserSpace, handles: HandleTable) -> Option<Arc<Self>> {
        Self::create(space, handles, false)
    }

    /// Like [`Process::new`], but the address space survives the process
    /// (self-tests read what it wrote after it ended).
    pub fn new_for_test(space: UserSpace, handles: HandleTable) -> Option<Arc<Self>> {
        Self::create(space, handles, true)
    }

    fn create(space: UserSpace, handles: HandleTable, retain_space: bool) -> Option<Arc<Self>> {
        if LIVE_PROCESSES.fetch_add(1, Ordering::AcqRel) >= MAX_PROCESSES {
            LIVE_PROCESSES.fetch_sub(1, Ordering::AcqRel);
            return None;
        }
        Some(Arc::new(Self {
            pid: NEXT_PID.fetch_add(1, Ordering::Relaxed),
            uid: 0,
            gid: 0,
            root: space.root(),
            space: SpinLock::new(Some(space)),
            retain_space,
            layout: core::sync::atomic::AtomicU64::new(0),
            stack_top: core::sync::atomic::AtomicU64::new(0),
            kill: AtomicBool::new(false),
            status: AtomicU32::new(STATUS_RUNNING),
            exit_code: AtomicI32::new(0),
            handles: SpinLock::new(handles),
        }))
    }

    /// Called by the scheduler when the process's thread has been reaped
    /// (it is dead and its CR3 is no longer loaded anywhere): frees the
    /// whole address space at once, instead of when the last handle to the
    /// process is closed (security review: zombies held up to 64 MiB).
    pub fn thread_reaped(&self) {
        if self.retain_space || self.status() == Status::Running {
            return;
        }
        let space = self.space.lock().take();
        // Freed after the lock.
        drop(space);
    }

    /// Closes every handle of a process that has ended: its objects are
    /// released at once (not when the last reference to the process goes
    /// away), and handles between processes cannot keep each other alive.
    fn release_handles(&self) {
        let table = core::mem::take(&mut *self.handles.lock());
        // Dropped after the lock: objects may hold other processes.
        drop(table);
    }

    /// Process identifier.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// `(uid, gid)`; both 0 until Phase 15.
    pub fn identity(&self) -> (u32, u32) {
        (self.uid, self.gid)
    }

    /// Records where the loader put the image (`base`, 0 for fixed
    /// addresses) and the stack top.
    pub fn record_layout(&self, base: u64, stack_top: u64) {
        self.layout.store(base, Ordering::Relaxed);
        self.stack_top.store(stack_top, Ordering::Relaxed);
    }

    /// `(image base, stack top)` chosen by ASLR.
    pub fn layout(&self) -> (u64, u64) {
        (
            self.layout.load(Ordering::Relaxed),
            self.stack_top.load(Ordering::Relaxed),
        )
    }

    /// CR3 value for this process.
    pub fn root(&self) -> u64 {
        self.root
    }

    /// Reads a `u64` of the process's memory (diagnostics and tests);
    /// `None` once its address space has been freed.
    pub fn peek_u64(&self, virt: u64) -> Option<u64> {
        self.space.lock().as_ref()?.peek_u64(virt)
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
        crate::input::wake_readers();
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
        process.release_handles();
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
        process.release_handles();
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
        process.release_handles();
        drop(process);
        sched::exit();
    }
}

/// Starts `/sbin/init` from the root (docs/FASE6.md §6). Handles: 0 =
/// CONSOLE, 1 = root DIR. Returns a log line (no addresses).
pub fn start_init() -> Result<Arc<Process>, &'static str> {
    let root = fs::root_dir().ok_or("no initrd root")?;
    let file = match fs::open_at(&root, "sbin/init") {
        Ok(fs::Opened::File(f)) => f,
        Ok(fs::Opened::Dir(_)) => return Err("/sbin/init is a directory"),
        Err(_) => return Err("/sbin/init not found"),
    };
    let image = read_file(file).ok_or("/sbin/init too big or out of memory")?;
    // Fixed order (init ABI): 0 CONSOLE, 1 root DIR, 2 INPUT.
    let input = crate::input::grant().ok_or("the INPUT grant is already taken")?;
    let objects = alloc::vec![
        (Object::Console, CONSOLE_RIGHTS),
        (Object::Dir(root), ROOT_RIGHTS),
        (Object::Input(input), INPUT_RIGHTS),
    ];
    spawn::spawn_from_kernel(&image, objects).map_err(|e| match e {
        spawn::SpawnFailure::Elf(_) => "invalid ELF image",
        _ => "could not create the process",
    })
}

/// Largest executable image read into memory.
pub const MAX_EXEC_FILE: usize = 16 * 1024 * 1024;

/// Frames always left free for the kernel itself: a spawn that would dip
/// below this fails with `E_NOMEM` (the kernel's own allocations — heap,
/// stacks, page tables — must never run dry because of user requests).
pub const RESERVE_FRAMES: u64 = 2048;

/// Whether `pages` more frames can be given to user space.
pub fn can_commit(pages: u64) -> bool {
    crate::mm::pmm::stats().free_frames >= pages.saturating_add(RESERVE_FRAMES)
}

/// Reads a whole file of the root into a fresh buffer.
pub fn read_file(file: kitty_vfs::File<'static>) -> Option<alloc::vec::Vec<u8>> {
    if file.len() > MAX_EXEC_FILE || !can_commit((file.len() as u64).div_ceil(4096)) {
        return None;
    }
    let mut buf = alloc::vec::Vec::new();
    buf.try_reserve_exact(file.len()).ok()?;
    buf.resize(file.len(), 0);
    (file.read_at(0, &mut buf) == file.len()).then_some(buf)
}

impl Drop for Process {
    fn drop(&mut self) {
        LIVE_PROCESSES.fetch_sub(1, Ordering::AcqRel);
    }
}

pub mod selftest;
