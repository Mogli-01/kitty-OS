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
