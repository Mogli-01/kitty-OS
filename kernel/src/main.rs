//! kitty-OS kernel.
//!
//! Boot flow (Phase 1):
//!
//! 1. Limine loads this ELF into the higher half, maps every segment with the
//!    permissions from `linker-x86_64.ld`, fills in our boot requests and jumps
//!    to [`arch::x86_64::boot`]'s `kernel_entry`.
//! 2. The arch layer checks the protocol revision, turns the raw bootloader
//!    structures into the safe [`boot_info::BootInfo`] and calls [`kmain`].
//! 3. [`kmain`] brings up the consoles and prints the greeting.
//!
//! Unsafe code policy: `unsafe` is denied for the whole crate and only allowed
//! in the modules listed in `docs/UNSAFE.md` (`arch`, `mm`, `sync`). Every
//! other module must be written in safe Rust on top of their APIs.

#![no_std]
#![no_main]
#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(clippy::undocumented_unsafe_blocks)]
#![deny(clippy::missing_safety_doc)]
#![deny(clippy::multiple_unsafe_ops_per_block)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

extern crate alloc;

#[allow(unsafe_code)]
pub mod arch;
pub mod boot_info;
pub mod console;
pub mod devices;
#[allow(unsafe_code)]
pub mod mm;
mod panic;
pub mod sched;
pub mod security;
#[allow(unsafe_code)]
pub mod sync;
pub mod syscall;

use boot_info::BootInfo;

/// Architecture-independent kernel entry point, called by the arch layer once
/// the bootloader hand-off has been validated.
pub fn kmain(mut boot: BootInfo) -> ! {
    console::init(boot.framebuffer.take());

    kprintln!("Hello from kitty-OS");
    if let Some(loader) = boot.bootloader {
        kprintln!("[boot] loaded by {} {}", loader.name, loader.version);
    }

    arch::init();
    kprintln!("[cpu] GDT, TSS (IST stacks) and IDT loaded");

    // Self-test: #BP must reach our handler and come back.
    arch::breakpoint();
    kprintln!("[cpu] breakpoint exception handled, execution resumed");

    let vmm = mm::init(&boot);
    let stats = mm::pmm::stats();
    kprintln!(
        "[mm] physical memory: {} MiB managed, {} MiB free; frame allocator self-test passed",
        stats.total_frames * 4 / 1024,
        stats.free_frames * 4 / 1024
    );
    kprintln!(
        "[mm] own page tables active: {} kernel pages, {} MiB direct map, NX + WP on",
        vmm.image_pages,
        vmm.hhdm_bytes / (1024 * 1024)
    );
    kprintln!(
        "[mm] W^X audit: {} mappings, {} writable+executable, {} user pages in kernel half",
        vmm.audit.mappings,
        vmm.audit.write_and_exec,
        vmm.audit.user_in_kernel
    );

    // Leave the bootloader's stack for our own, which has a guard page.
    arch::x86_64::stacks::run_on_main_stack(kmain_on_kernel_stack)
}

/// Second half of boot, running on the guarded kernel stack.
extern "C" fn kmain_on_kernel_stack() -> ! {
    kprintln!("[mm] running on the kernel stack (guard page below it)");
    mm::heap::init();
    mm::heap::selftest();
    kprintln!(
        "[mm] kernel heap ready ({} pages mapped); heap self-test passed",
        mm::heap::stats().pages
    );

    match mm::vmm::mmio_selftest() {
        Some(v) => kprintln!("[mm] MMIO self-test passed (local APIC version {:#x})", v),
        None => panic!("MMIO self-test failed"),
    }

    assert!(arch::irq::selftest(), "interrupt dispatch self-test failed");
    devices::init();
    arch::enable_interrupts();
    assert!(
        sync::irq_spinlock::selftest(),
        "IrqSpinLock self-test failed"
    );
    kprintln!(
        "[irq] interrupts enabled (controller: {}); dispatch and IrqSpinLock self-tests passed",
        arch::irq::controller_name()
    );

    #[cfg(feature = "selftest-irq-pit")]
    {
        assert!(arch::x86_64::selftest::irq_pit(), "PIT IRQ0 never arrived");
        kprintln!("[irq] hardware IRQ0 (PIT) delivered through the 8259 PIC");
    }

    #[cfg(feature = "selftest-page-fault")]
    arch::x86_64::selftest::page_fault();
    #[cfg(feature = "selftest-double-fault")]
    arch::x86_64::selftest::double_fault();
    #[cfg(feature = "selftest-stack-overflow")]
    arch::x86_64::selftest::stack_overflow();
    #[cfg(feature = "selftest-write-text")]
    arch::x86_64::selftest::write_text();
    #[cfg(feature = "selftest-exec-data")]
    arch::x86_64::selftest::exec_data();
    #[cfg(feature = "selftest-write-idt")]
    arch::x86_64::selftest::write_idt();
    #[cfg(feature = "selftest-heap-overflow")]
    arch::x86_64::selftest::heap_overflow();

    kprintln!("[boot] boot complete, idling with interrupts enabled");

    #[cfg(feature = "qemu-exit")]
    arch::qemu::exit(arch::qemu::ExitCode::Success);

    // Idle with interrupts enabled so drivers keep receiving IRQs.
    #[cfg(not(feature = "qemu-exit"))]
    arch::idle_forever()
}
