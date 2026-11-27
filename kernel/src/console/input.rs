//! Bounded console input shared by the IRQ producer and the console reader.

use crate::mm::user::{UserSlice, copy_to_user};
use crate::sched::WaitQueue;
use crate::sync::IrqSpinLock;

const CAPACITY: usize = 4096;
static READY: WaitQueue = WaitQueue::new();
static INPUT: IrqSpinLock<Input> = IrqSpinLock::new(Input {
    bytes: [0; CAPACITY],
    head: 0,
    len: 0,
    dropped: 0,
});

struct Input {
    bytes: [u8; CAPACITY],
    head: usize,
    len: usize,
    dropped: u64,
}

/// Adds one unit of input from the driver. Drops the whole unit on overflow
/// so an escape sequence or multibyte character is never partially queued.
pub fn push(bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    let mut input = INPUT.lock();
    if bytes.len() > CAPACITY - input.len {
        input.dropped = input.dropped.saturating_add(bytes.len() as u64);
        return;
    }
    for &byte in bytes {
        let tail = (input.head + input.len) % CAPACITY;
        input.bytes[tail] = byte;
        input.len += 1;
    }
    drop(input);
    READY.wake_one();
}

/// Copies queued bytes to a validated user range. A failed copy leaves the
/// queue untouched. Called from a syscall with interrupts enabled.
pub(crate) fn read(address: u64, len: usize) -> Result<usize, crate::mm::user::UserError> {
    if len == 0 {
        return Ok(0);
    }
    UserSlice::new(address, len)?;
    let mut bytes = [0u8; CAPACITY];
    loop {
        READY.wait_until(|| INPUT.lock().len != 0);
        let mut input = INPUT.lock();
        let count = len.min(input.len);
        if count == 0 {
            continue;
        }
        for (index, byte) in bytes[..count].iter_mut().enumerate() {
            *byte = input.bytes[(input.head + index) % CAPACITY];
        }
        copy_to_user(UserSlice::new(address, count)?, &bytes[..count])?;
        input.head = (input.head + count) % CAPACITY;
        input.len -= count;
        return Ok(count);
    }
}
