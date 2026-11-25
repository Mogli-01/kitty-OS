//! QEMU `isa-debug-exit` support, used only by automated tests.
//!
//! Compiled only with the `qemu-exit` feature so release kernels never poke
//! port 0xF4 on real hardware.

use super::port::Port;

/// I/O port of the isa-debug-exit device (`-device isa-debug-exit,iobase=0xf4`).
const DEBUG_EXIT_PORT: u16 = 0xF4;

/// Exit status reported to the host. QEMU exits with `(value << 1) | 1`,
/// so `Success` becomes host exit code 33 and `Failure` 35. Value 0 is
/// avoided on purpose: it would give exit code 1, which QEMU also uses for
/// its own errors.
#[repr(u32)]
#[derive(Clone, Copy)]
pub enum ExitCode {
    /// The test passed (host exit code 33).
    Success = 0x10,
    /// The test failed (host exit code 35).
    Failure = 0x11,
}

/// Powers QEMU off with `code`. Halts if the device is not present.
pub fn exit(code: ExitCode) -> ! {
    // SAFETY: this module only exists in test builds that run under QEMU
    // with isa-debug-exit at 0xF4. Writing it terminates the VM and cannot
    // affect memory. On a machine without the device, port 0xF4 is
    // unassigned on PCs and the write is ignored.
    let port = unsafe { Port::<u32>::new(DEBUG_EXIT_PORT) };
    port.write(code as u32);
    super::cpu::halt_forever()
}
