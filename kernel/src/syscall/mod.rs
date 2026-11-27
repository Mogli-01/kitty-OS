//! Safe syscall dispatch after the architecture switches to the kernel stack.

use crate::mm::user::{UserSlice, copy_from_user};

pub const EXIT: u64 = 0;
pub const YIELD: u64 = 1;
pub const CONSOLE_READ: u64 = 2;
pub const CONSOLE_WRITE: u64 = 3;
const MAX_CONSOLE_WRITE: usize = 4096;

const E_FAULT: i64 = -2;
const E_BADHANDLE: i64 = -3;
const E_NOSYS: i64 = -9;
const E_TOOBIG: i64 = -10;

pub(crate) fn dispatch(number: u64, first: u64, second: u64, third: u64) -> i64 {
    match number {
        EXIT => crate::proc::exit_current(first),
        YIELD => {
            crate::sched::yield_now();
            0
        }
        CONSOLE_WRITE => console_write(first, second, third).unwrap_or_else(|error| error),
        CONSOLE_READ => console_read(first, second, third).unwrap_or_else(|error| error),
        _ => E_NOSYS,
    }
}

fn console_args(handle: u64, len: u64) -> Result<usize, i64> {
    if handle != 0 || !crate::sched::current_process().is_some_and(|process| process.has_console())
    {
        return Err(E_BADHANDLE);
    }
    usize::try_from(len)
        .ok()
        .filter(|&len| len <= MAX_CONSOLE_WRITE)
        .ok_or(E_TOOBIG)
}

fn console_read(handle: u64, address: u64, len: u64) -> Result<i64, i64> {
    let len = console_args(handle, len)?;
    crate::console::input_read(address, len)
        .map(|count| count as i64)
        .map_err(|_| E_FAULT)
}

fn console_write(handle: u64, address: u64, len: u64) -> Result<i64, i64> {
    let len = console_args(handle, len)?;
    let user = UserSlice::new(address, len).map_err(|_| E_FAULT)?;
    let mut bytes = [0u8; MAX_CONSOLE_WRITE];
    let incoming = bytes.get_mut(..len).ok_or(E_TOOBIG)?;
    copy_from_user(incoming, user).map_err(|_| E_FAULT)?;
    let mut input = &incoming[..];
    while let Err(error) = core::str::from_utf8(input) {
        let (valid, rest) = input.split_at(error.valid_up_to());
        if let Ok(text) = core::str::from_utf8(valid) {
            crate::kprint!("{text}");
        }
        crate::kprint!("?");
        input = &rest[error.error_len().unwrap_or(rest.len())..];
    }
    if let Ok(text) = core::str::from_utf8(input) {
        crate::kprint!("{text}");
    }
    Ok(len as i64)
}
