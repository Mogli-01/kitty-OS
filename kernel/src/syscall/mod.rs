//! Safe syscall dispatch after the architecture switches to the kernel stack.

use crate::mm::user::{UserSlice, copy_from_user};

pub const EXIT: u64 = 0;
pub const YIELD: u64 = 1;
pub const CONSOLE_READ: u64 = 2;
pub const CONSOLE_WRITE: u64 = 3;
const OPEN_AT: u64 = 4;
const FILE_READ: u64 = 5;
const FILE_WRITE: u64 = 6;
const HANDLE_CLOSE: u64 = 7;
const MAX_CONSOLE_WRITE: usize = 4096;

const E_FAULT: i64 = -2;
const E_BADHANDLE: i64 = -3;
const E_INVAL: i64 = -1;
const E_PERM: i64 = -4;
const E_NOMEM: i64 = -5;
const E_NOENT: i64 = -8;
const E_NOSYS: i64 = -9;
const E_TOOBIG: i64 = -10;

pub(crate) fn dispatch(number: u64, first: u64, second: u64, third: u64, fourth: u64) -> i64 {
    match number {
        EXIT => crate::proc::exit_current(first),
        YIELD => {
            crate::sched::yield_now();
            0
        }
        CONSOLE_WRITE => console_write(first, second, third).unwrap_or_else(|error| error),
        CONSOLE_READ => console_read(first, second, third).unwrap_or_else(|error| error),
        OPEN_AT => open_at(first, second, third, fourth).unwrap_or_else(|error| error),
        FILE_READ => file_read(first, second, third).unwrap_or_else(|error| error),
        FILE_WRITE => match crate::sched::current_process() {
            Some(process) if process.files.lock().contains(first) => E_PERM,
            _ => E_BADHANDLE,
        },
        HANDLE_CLOSE => match crate::sched::current_process() {
            Some(process) if process.files.lock().close(first) => 0,
            _ => E_BADHANDLE,
        },
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

fn open_at(dir: u64, address: u64, len: u64, rights: u64) -> Result<i64, i64> {
    use crate::proc::files::DIR;
    let process = crate::sched::current_process().ok_or(E_BADHANDLE)?;
    if !process.root_dir || dir != DIR {
        return Err(E_BADHANDLE);
    }
    if rights != 1 {
        return Err(E_PERM);
    }
    let mut path = [0u8; 256];
    let len = usize::try_from(len).map_err(|_| E_TOOBIG)?;
    let path = path.get_mut(..len).ok_or(E_TOOBIG)?;
    copy_from_user(path, UserSlice::new(address, len).map_err(|_| E_FAULT)?)
        .map_err(|_| E_FAULT)?;
    let path = core::str::from_utf8(path).map_err(|_| E_INVAL)?;
    let file = crate::devices::open_root_file(path).map_err(|error| match error {
        kitty_vfs::Error::InvalidPath => E_INVAL,
        _ => E_NOENT,
    })?;
    let handle = process.files.lock().open(file).ok_or(E_NOMEM)?;
    Ok(handle as i64)
}

fn file_read(handle: u64, address: u64, len: u64) -> Result<i64, i64> {
    let process = crate::sched::current_process().ok_or(E_BADHANDLE)?;
    let len = usize::try_from(len).map_err(|_| E_TOOBIG)?;
    let mut bytes = [0u8; 4096];
    let bytes = bytes.get_mut(..len).ok_or(E_TOOBIG)?;
    let read = process.files.lock().read(handle, address, bytes)?;
    Ok(read as i64)
}
