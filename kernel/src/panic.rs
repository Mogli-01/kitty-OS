//! Panic handler.
//!
//! Prints the source location and message on every console, then stops the
//! CPU. There is no unwinding (`panic = "abort"`): a kernel panic is fatal.
//!
//! Security: the message contains only file:line:column and the panic text,
//! never raw addresses. Panic messages must not format pointers (`{:p}`).

use core::panic::PanicInfo;

use crate::arch;
use crate::console::emergency_print;

#[panic_handler]
fn panic(info: &PanicInfo<'_>) -> ! {
    // First thing: no interrupts, hence no preemption. Other threads must
    // not keep running on a kernel state that just failed an invariant.
    arch::x86_64::cpu::disable_interrupts();
    match info.location() {
        Some(loc) => emergency_print(format_args!(
            "\n*** KERNEL PANIC at {}:{}:{}\n",
            loc.file(),
            loc.line(),
            loc.column()
        )),
        None => emergency_print(format_args!("\n*** KERNEL PANIC (unknown location)\n")),
    }
    emergency_print(format_args!("    {}\n", info.message()));

    #[cfg(feature = "qemu-exit")]
    arch::qemu::exit(arch::qemu::ExitCode::Failure);

    #[cfg(not(feature = "qemu-exit"))]
    arch::halt_forever()
}
