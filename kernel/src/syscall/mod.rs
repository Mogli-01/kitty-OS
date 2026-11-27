//! System call dispatch and argument validation (Phase 6c, docs/FASE6.md
//! §3, D-049, D-058, D-063).
//!
//! The entry (`arch::x86_64::syscall_entry`) hands us the number (RAX) and
//! six raw arguments (RDI, RSI, RDX, R10, R8, R9). Every argument is an
//! untrusted 64-bit value: it is checked (range, handle, rights, user
//! pointer) **before** anything happens, and a bad one is an error code,
//! never a panic. Pointers are only touched through `copy_*_user`.
//!
//! Return value: `>= 0` success, `< 0` error (table below, stable ABI).
//!
//! Capability-first (D-058): every call with an effect outside the calling
//! process takes a handle as its first argument. The only calls without
//! one act on the process itself.

use alloc::sync::Arc;

use crate::console;
use crate::mm::user::{UserError, UserSlice, copy_from_user, copy_to_user};
use crate::proc::handles::{HandleError, Object, Rights};
use crate::proc::{self, Process};
use crate::sched;

/// Error codes (D-049; `E_BADSIG` reserved for Phase 16, D-058).
pub mod errno {
    /// Bad argument (size, flags, alignment).
    pub const E_INVAL: i64 = -1;
    /// Bad or inaccessible user pointer.
    pub const E_FAULT: i64 = -2;
    /// No such handle (or closed).
    pub const E_BADHANDLE: i64 = -3;
    /// The handle lacks a needed right.
    pub const E_PERM: i64 = -4;
    /// Out of memory.
    pub const E_NOMEM: i64 = -5;
    /// Nothing available without blocking.
    pub const E_AGAIN: i64 = -6;
    /// Exclusive resource in use.
    pub const E_BUSY: i64 = -7;
    /// Does not exist.
    pub const E_NOENT: i64 = -8;
    /// Unknown system call.
    pub const E_NOSYS: i64 = -9;
    /// Size above the limit.
    pub const E_TOOBIG: i64 = -10;
    /// Image without a valid signature (Phase 16).
    pub const E_BADSIG: i64 = -11;
    /// Not a valid executable image (ELF refused by the loader).
    pub const E_BADIMAGE: i64 = -12;
}
use errno::*;

/// System call numbers (stable ABI).
pub mod nr {
    /// `exit(code: i32) -> !`
    pub const EXIT: u64 = 0;
    /// `yield() -> 0`
    pub const YIELD: u64 = 1;
    /// `handle_close(h) -> 0`
    pub const HANDLE_CLOSE: u64 = 2;
    /// `handle_dup(h, rights) -> h2` (needs DUP; rights ⊆ rights(h))
    pub const HANDLE_DUP: u64 = 3;
    /// `handle_info(h) -> (kind << 32) | rights`
    pub const HANDLE_INFO: u64 = 4;
    /// `console_write(h, buf, len) -> bytes accepted` (needs WRITE)
    pub const CONSOLE_WRITE: u64 = 5;
    /// `console_read(h, buf, len) -> bytes read`, blocks (needs READ)
    pub const CONSOLE_READ: u64 = 6;
    /// `clock_now() -> ms` (reserved: E_NOSYS until the clock lands)
    pub const CLOCK_NOW: u64 = 7;
    /// `random_fill(buf, len)` (reserved: E_NOSYS until Phase 8/16)
    pub const RANDOM_FILL: u64 = 8;
    /// `open_at(dir, path, path_len, rights) -> h` (needs READ on `dir`;
    /// rights ⊆ the directory's, never WRITE/MAP: the root is read-only)
    pub const OPEN_AT: u64 = 9;
    /// `file_read(h, buf, len, offset) -> bytes` (needs READ)
    pub const FILE_READ: u64 = 10;
    /// `process_spawn(image, grants, n) -> process handle` (needs EXEC on
    /// the image file; grants are `n` × {handle, rights, flags, 0} u32s)
    pub const PROCESS_SPAWN: u64 = 11;
    /// `process_status(h) -> status` (needs READ): 0 running,
    /// `1 << 32 | code` exited, `2 << 32 | vector` fault, `3 << 32` killed
    pub const PROCESS_STATUS: u64 = 12;
}

/// Largest buffer for one console call (docs/FASE6.md §6).
pub const CONSOLE_MAX: usize = 4096;
/// Bytes copied per step (keeps the kernel stack use small).
const CHUNK: usize = 512;

/// Number of system calls handled (diagnostics, tests).
static CALLS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// System calls handled so far.
pub fn calls() -> u64 {
    CALLS.load(core::sync::atomic::Ordering::Relaxed)
}

/// Handles one system call from the current user thread.
pub fn dispatch(number: u64, args: [u64; 6]) -> i64 {
    CALLS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    let Some(process) = sched::current_process() else {
        // Only user threads can get here; refuse rather than guess.
        return E_NOSYS;
    };
    let [a0, a1, a2, ..] = args;
    match number {
        nr::EXIT => {
            drop(process);
            // The code is the low 32 bits of the argument (documented ABI).
            proc::exit_current(a0 as u32 as i32)
        }
        nr::YIELD => {
            drop(process);
            sched::yield_now();
            0
        }
        nr::HANDLE_CLOSE => with_handle(a0, |h| {
            process
                .handles()
                .lock()
                .close(h)
                .map(|()| 0)
                .map_err(handle_err)
        }),
        nr::HANDLE_DUP => with_handle(a0, |h| {
            let rights = u32::try_from(a1)
                .ok()
                .and_then(Rights::from_bits)
                .ok_or(E_INVAL)?;
            process
                .handles()
                .lock()
                .dup(h, rights)
                .map(i64::from)
                .map_err(handle_err)
        }),
        nr::HANDLE_INFO => with_handle(a0, |h| {
            let (object, rights) = process.handles().lock().get(h).map_err(handle_err)?;
            Ok((i64::from(object.kind()) << 32) | i64::from(rights.bits()))
        }),
        nr::CONSOLE_WRITE => console_write(&process, a0, a1, a2),
        nr::CONSOLE_READ => console_read(&process, a0, a1, a2),
        nr::OPEN_AT => open_at(&process, a0, a1, a2, args[3]).unwrap_or_else(|e| e),
        nr::FILE_READ => file_read(&process, a0, a1, a2, args[3]).unwrap_or_else(|e| e),
        nr::PROCESS_SPAWN => process_spawn(&process, a0, a1, a2).unwrap_or_else(|e| e),
        nr::PROCESS_STATUS => process_status(&process, a0).unwrap_or_else(|e| e),
        nr::CLOCK_NOW | nr::RANDOM_FILL => E_NOSYS,
        _ => E_NOSYS,
    }
}

/// Validates a handle argument (it must fit in 32 bits) and runs `f`.
fn with_handle(raw: u64, f: impl FnOnce(u32) -> Result<i64, i64>) -> i64 {
    let Ok(handle) = u32::try_from(raw) else {
        return E_BADHANDLE;
    };
    f(handle).unwrap_or_else(|e| e)
}

fn handle_err(e: HandleError) -> i64 {
    match e {
        HandleError::BadHandle => E_BADHANDLE,
        HandleError::Perm => E_PERM,
        HandleError::Full => E_NOMEM,
    }
}

fn user_err(e: UserError) -> i64 {
    match e {
        UserError::BadRange | UserError::Fault => E_FAULT,
    }
}

/// Checks that `handle` is a CONSOLE handle with `needed`. The table lock
/// is released before returning (never held across copies or blocking).
fn console_handle(process: &Process, handle: u64, needed: Rights) -> Result<(), i64> {
    let handle = u32::try_from(handle).map_err(|_| E_BADHANDLE)?;
    match process
        .handles()
        .lock()
        .check(handle, needed)
        .map_err(handle_err)?
    {
        Object::Console => Ok(()),
        // A handle of another type: the call does not apply to it.
        _ => Err(E_INVAL),
    }
}

fn console_write(process: &Arc<Process>, handle: u64, buf: u64, len: u64) -> i64 {
    let run = || -> Result<i64, i64> {
        console_handle(process, handle, Rights::WRITE)?;
        let len = usize::try_from(len).map_err(|_| E_TOOBIG)?;
        if len > CONSOLE_MAX {
            return Err(E_TOOBIG);
        }
        // Validate the whole range first: nothing is written for a bad one.
        UserSlice::new(buf, len).map_err(user_err)?;
        let mut done = 0;
        let mut bytes = [0u8; CHUNK];
        while done < len {
            let n = (len - done).min(CHUNK);
            let src = UserSlice::new(buf + done as u64, n).map_err(user_err)?;
            let dst = bytes.get_mut(..n).ok_or(E_TOOBIG)?;
            if copy_from_user(dst, src).is_err() {
                // Partial write: report what was accepted (if anything).
                return if done == 0 {
                    Err(E_FAULT)
                } else {
                    Ok(done as i64)
                };
            }
            // Each chunk is copied once, then used: no TOCTOU. (A UTF-8
            // character split between chunks shows as `?`.)
            console::write_user_text(dst);
            done += n;
        }
        Ok(len as i64)
    };
    run().unwrap_or_else(|e| e)
}

fn console_read(process: &Arc<Process>, handle: u64, buf: u64, len: u64) -> i64 {
    let run = || -> Result<i64, i64> {
        console_handle(process, handle, Rights::READ)?;
        // Reads may return fewer bytes than asked (like `read`): at most one
        // chunk per call.
        let len = usize::try_from(len).map_err(|_| E_TOOBIG)?.min(CHUNK);
        // Validate the destination before blocking (an obviously bad
        // pointer fails at once). A fault after the wait loses the bytes
        // read (they were the program's own input).
        UserSlice::new(buf, len).map_err(user_err)?;
        if len == 0 {
            return Ok(0);
        }
        let mut bytes = [0u8; CHUNK];
        let kbuf = bytes.get_mut(..len).ok_or(E_TOOBIG)?;
        let n = console::input_read(kbuf, || process.is_killed());
        if n == 0 {
            // Woken because the process is being killed.
            return Err(E_AGAIN);
        }
        let read = kbuf.get(..n).ok_or(E_INVAL)?;
        let out = UserSlice::new(buf, n).map_err(user_err)?;
        copy_to_user(out, read).map_err(user_err)?;
        Ok(n as i64)
    };
    run().unwrap_or_else(|e| e)
}

/// The object of `handle` if it has `needed` (lock released on return).
fn object(process: &Process, handle: u64, needed: Rights) -> Result<(Object, Rights), i64> {
    let handle = u32::try_from(handle).map_err(|_| E_BADHANDLE)?;
    let table = process.handles().lock();
    let (object, rights) = table.get(handle).map_err(handle_err)?;
    if rights.contains(needed) {
        Ok((object, rights))
    } else {
        Err(E_PERM)
    }
}

fn open_at(process: &Process, dir: u64, path: u64, path_len: u64, rights: u64) -> Result<i64, i64> {
    let (object, dir_rights) = object(process, dir, Rights::READ)?;
    let Object::Dir(dir) = object else {
        return Err(E_INVAL);
    };
    let want = u32::try_from(rights)
        .ok()
        .and_then(Rights::from_bits)
        .ok_or(E_INVAL)?;
    // Read-only root; and a handle never has more than its directory.
    if want.contains(Rights::WRITE) || want.contains(Rights::MAP) || !dir_rights.contains(want) {
        return Err(E_PERM);
    }
    let len = usize::try_from(path_len).map_err(|_| E_TOOBIG)?;
    if len == 0 || len > proc::fs::MAX_PATH {
        return Err(if len == 0 { E_INVAL } else { E_TOOBIG });
    }
    let mut raw = [0u8; proc::fs::MAX_PATH];
    let dst = raw.get_mut(..len).ok_or(E_TOOBIG)?;
    copy_from_user(dst, UserSlice::new(path, len).map_err(user_err)?).map_err(user_err)?;
    let rel = core::str::from_utf8(dst).map_err(|_| E_INVAL)?;
    let opened = match proc::fs::open_at(&dir, rel) {
        Ok(o) => o,
        Err(proc::fs::FsError::NotFound) => return Err(E_NOENT),
        Err(proc::fs::FsError::NoRoot) => return Err(E_NOENT),
        Err(proc::fs::FsError::BadPath) => return Err(E_INVAL),
    };
    let object = match opened {
        proc::fs::Opened::File(f) => Object::File(f),
        proc::fs::Opened::Dir(d) => Object::Dir(d),
    };
    process
        .handles()
        .lock()
        .insert(object, want)
        .map(i64::from)
        .map_err(handle_err)
}

fn file_read(process: &Process, handle: u64, buf: u64, len: u64, offset: u64) -> Result<i64, i64> {
    let (object, _) = object(process, handle, Rights::READ)?;
    let Object::File(file) = object else {
        return Err(E_INVAL);
    };
    let len = usize::try_from(len).map_err(|_| E_TOOBIG)?.min(CHUNK);
    UserSlice::new(buf, len).map_err(user_err)?;
    let mut bytes = [0u8; CHUNK];
    let kbuf = bytes.get_mut(..len).ok_or(E_TOOBIG)?;
    let n = file.read_at(offset, kbuf);
    let data = kbuf.get(..n).ok_or(E_INVAL)?;
    copy_to_user(UserSlice::new(buf, n).map_err(user_err)?, data).map_err(user_err)?;
    Ok(n as i64)
}

fn process_spawn(process: &Process, image: u64, grants: u64, count: u64) -> Result<i64, i64> {
    use proc::spawn::{Grant, MAX_GRANTS, SpawnFailure};
    let (object, _) = object(process, image, Rights::EXEC)?;
    let Object::File(file) = object else {
        return Err(E_INVAL);
    };
    let count = usize::try_from(count).map_err(|_| E_TOOBIG)?;
    if count > MAX_GRANTS {
        return Err(E_TOOBIG);
    }
    // The grant list is copied **once** into the kernel, then validated.
    let mut raw = [0u8; MAX_GRANTS * 16];
    let dst = raw.get_mut(..count * 16).ok_or(E_TOOBIG)?;
    copy_from_user(dst, UserSlice::new(grants, count * 16).map_err(user_err)?).map_err(user_err)?;
    let mut list = alloc::vec::Vec::new();
    list.try_reserve(count).map_err(|_| E_NOMEM)?;
    for chunk in dst.as_chunks::<16>().0 {
        let word =
            |i: usize| u32::from_le_bytes([chunk[i], chunk[i + 1], chunk[i + 2], chunk[i + 3]]);
        if word(12) != 0 {
            return Err(E_INVAL);
        }
        list.push(Grant {
            handle: word(0),
            rights: word(4),
            flags: word(8),
        });
    }
    let image = proc::read_file(file).ok_or(E_TOOBIG)?;
    match proc::spawn::spawn(process, &image, &list) {
        Ok((handle, _child)) => Ok(i64::from(handle)),
        Err(SpawnFailure::BadHandle) => Err(E_BADHANDLE),
        Err(SpawnFailure::Perm) => Err(E_PERM),
        Err(SpawnFailure::Invalid) => Err(E_INVAL),
        Err(SpawnFailure::Elf(_)) => Err(E_BADIMAGE),
        Err(SpawnFailure::NoMemory) => Err(E_NOMEM),
    }
}

fn process_status(process: &Process, handle: u64) -> Result<i64, i64> {
    let (object, _) = object(process, handle, Rights::READ)?;
    let Object::Process(child) = object else {
        return Err(E_INVAL);
    };
    Ok(match child.status() {
        proc::Status::Running => 0,
        proc::Status::Exited(code) => (1 << 32) | i64::from(code as u32),
        proc::Status::Fault(v) => (2 << 32) | i64::from(v),
        proc::Status::Killed => 3 << 32,
    })
}
