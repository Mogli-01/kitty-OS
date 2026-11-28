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
//! in the modules listed in `docs/UNSAFE.md` (`arch`, `mm`, `sync`, `sched`). Every
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
pub mod firmware;
pub mod input;
#[allow(unsafe_code)]
pub mod mm;
mod panic;
pub mod proc;
pub mod rng;
#[allow(unsafe_code)]
pub mod sched;
pub mod security;
#[allow(unsafe_code)]
pub mod sync;
pub mod syscall;

use core::sync::atomic::{AtomicU64, Ordering};

use boot_info::BootInfo;

/// Architecture-independent kernel entry point, called by the arch layer once
/// the bootloader hand-off has been validated.
pub fn kmain(mut boot: BootInfo) -> ! {
    console::init(boot.framebuffer.take());

    kprintln!("Hello from kitty-OS");
    if let Some(loader) = boot.bootloader {
        kprintln!("[boot] loaded by {} {}", loader.name, loader.version);
    }
    // KASLR (Phase 8c, D-068): the kernel is a relocatable PIE linked at
    // LINK_BASE; the bootloader picks a random base. Only whether it moved
    // is shown, never where (the address would defeat the point).
    const LINK_BASE: u64 = 0xFFFF_FFFF_8000_0000;
    // Where the 4-level HHDM sits when the bootloader does not move it.
    const HHDM_FIXED: u64 = 0xFFFF_8000_0000_0000;
    // The VMM maps the image from linker symbols; they must agree with the
    // base the bootloader relocated us to (the address is not printed).
    assert!(
        mm::vmm::kernel_image_start() == boot.kernel_image.virt_base,
        "kernel image base disagrees with the bootloader"
    );
    if boot.kernel_image.virt_base == LINK_BASE {
        kprintln!("[kaslr] WARNING: kernel at its link address (KASLR is off)");
    } else {
        kprintln!("[kaslr] kernel base randomized by the bootloader");
    }
    if boot.hhdm_offset == HHDM_FIXED {
        kprintln!("[kaslr] WARNING: direct map (HHDM) at its fixed address");
    } else {
        kprintln!("[kaslr] direct map (HHDM) base randomized");
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

    // The second half of boot runs on another stack and cannot see `boot`:
    // keep what it needs. (Only a physical address; never printed.)
    RSDP_PHYS.store(boot.rsdp_phys.unwrap_or(0), Ordering::Release);
    *INITRD.lock() = Some(boot.initrd);

    // Leave the bootloader's stack for our own, which has a guard page.
    arch::x86_64::stacks::run_on_main_stack(kmain_on_kernel_stack)
}

static RSDP_PHYS: AtomicU64 = AtomicU64::new(0);
static INITRD: sync::IrqSpinLock<Option<boot_info::InitrdModule>> = sync::IrqSpinLock::new(None);

fn rsdp_phys() -> Option<u64> {
    Some(RSDP_PHYS.load(Ordering::Acquire)).filter(|&a| a != 0)
}

/// Second half of boot, running on the guarded kernel stack.
extern "C" fn kmain_on_kernel_stack() -> ! {
    kprintln!("[mm] running on the kernel stack (guard page below it)");
    // The RNG needs no heap; seeding it first gives the heap's free-list
    // secret real entropy (it used to come from the TSC).
    let seed = rng::init();
    mm::heap::init();
    mm::heap::selftest();
    sched::init();
    kprintln!(
        "[mm] kernel heap ready ({} pages mapped); heap self-test passed",
        mm::heap::stats().pages
    );

    let prot = arch::x86_64::cpu::enable_user_protections();
    arch::x86_64::cpu::enable_user_sse();
    arch::x86_64::syscall_entry::init();
    if let Err(why) = rng::selftest() {
        panic!("RNG self-test failed: {}", why);
    }
    kprintln!(
        "[rng] seeded (RDSEED {} words, RDRAND {} words, {} rejected, {} jitter samples){}; SHA-256 and ChaCha20 known-answer tests passed",
        seed.rdseed,
        seed.rdrand,
        seed.rejected,
        seed.jitter,
        if seed.weak {
            " -- WARNING: no trusted hardware RNG, weak entropy"
        } else {
            ""
        }
    );
    kprintln!(
        "[cpu] user-mode protections: SMEP {}, SMAP {}, UMIP {}",
        if prot.smep { "on" } else { "UNSUPPORTED" },
        if prot.smap { "on" } else { "UNSUPPORTED" },
        if prot.umip { "on" } else { "UNSUPPORTED" }
    );
    if !prot.umip {
        // sgdt/sidt in ring 3 return the GDT/IDT addresses, which live in
        // the kernel image: on this CPU they reveal the KASLR base.
        kprintln!("[kaslr] WARNING: no UMIP, user code can learn the kernel base (sgdt/sidt)");
    }
    assert!(mm::user::selftest(), "user-copy self-test failed");
    kprintln!("[mm] user-copy self-test passed (round trip, fault recovery, range checks)");
    #[cfg(feature = "selftest-smap")]
    mm::user::smap_violation();
    #[cfg(feature = "selftest-smep")]
    mm::user::smep_violation();

    match mm::vmm::mmio_selftest() {
        Some(v) => kprintln!("[mm] MMIO self-test passed (local APIC version {:#x})", v),
        None => panic!("MMIO self-test failed"),
    }

    assert!(arch::irq::selftest(), "interrupt dispatch self-test failed");
    assert!(
        arch::legacy::selftest(),
        "legacy port filter self-test failed"
    );

    // ACPI: find and validate the MADT. Without it, stay on the 8259 PIC.
    let acpi = firmware::acpi::discover(rsdp_phys());
    match &acpi {
        Ok(tables) => {
            arch::apic::set_declared_io_apics(&tables.io_apics);
            kprintln!(
                "[acpi] MADT validated ({} bytes, {} I/O APIC(s))",
                tables.madt.len(),
                tables.io_apics.len()
            );
        }
        Err(e) => kprintln!("[acpi] no usable MADT ({:?}); keeping the 8259 PIC", e),
    }
    match arch::apic::selftest() {
        Some((lapic, Some(io))) => kprintln!(
            "[apic] capability self-test passed (local APIC version {:#x}, I/O APIC version {:#x})",
            lapic,
            io
        ),
        Some((lapic, None)) => kprintln!(
            "[apic] capability self-test passed (local APIC version {:#x}, no I/O APIC)",
            lapic
        ),
        None => kprintln!("[apic] capability self-test: APIC unavailable or check failed"),
    }

    assert!(
        mm::vmm::initrd_selftest(),
        "initrd validation self-test failed"
    );
    let module = INITRD
        .lock()
        .take()
        .unwrap_or(boot_info::InitrdModule::Absent);
    let initrd = match mm::vmm::initrd_slice(module) {
        Ok(Some(bytes)) => {
            kprintln!(
                "[initrd] module validated ({} KiB, read-only)",
                bytes.len().div_ceil(1024)
            );
            Some(bytes)
        }
        Ok(None) => {
            kprintln!("[initrd] no module (booting without initrd)");
            None
        }
        Err(e) => {
            kprintln!("[initrd] module REFUSED ({:?}); booting without initrd", e);
            None
        }
    };

    let platform = devices::Platform {
        madt: acpi.as_ref().ok().map(|t| &t.madt[..]),
        initrd,
    };
    devices::init(&platform);
    // The processes' read-only root (their own view of the initrd).
    if let Some(bytes) = platform.initrd
        && let Err(e) = proc::fs::mount(bytes)
    {
        kprintln!("[init] initrd not usable as the process root ({:?})", e);
    }
    arch::enable_interrupts();
    assert!(
        sync::irq_spinlock::selftest(),
        "IrqSpinLock self-test failed"
    );
    kprintln!(
        "[irq] interrupts enabled (controller: {}); dispatch, IrqSpinLock and port-filter self-tests passed",
        arch::irq::controller_name()
    );

    match sched::selftest() {
        Some(cycles) => kprintln!(
            "[sched] cooperative self-test passed: {} switches so far, ~{} cycles per yield+switch",
            sched::switches(),
            cycles
        ),
        None => panic!("scheduler self-test failed"),
    }

    assert!(sched::wait::selftest(), "wait queue self-test failed");
    kprintln!("[sched] wait queue self-test passed: waiter blocked, woken, value seen");
    assert!(
        sched::user_entry_selftest(),
        "user entry validation self-test failed"
    );
    kprintln!("[sched] user entry validation self-test passed (RIP/RSP strictly below USER_END)");

    match proc::selftest::run() {
        Ok(r) => {
            kprintln!(
                "[proc] user-mode self-test passed: {} ring-3 faults killed their process (UMIP {}), preemption + register hygiene {}",
                r.faults_ok,
                if r.umip { "on" } else { "off" },
                if r.preemption_tested {
                    "checked"
                } else {
                    "skipped (no timer)"
                }
            );
            kprintln!(
                "[syscall] self-test passed: every call and error path, handles with generations, blocked process killed (~{} cycles per null syscall)",
                r.syscall_cycles
            );
            kprintln!(
                "[proc] ELF loader and process_spawn self-test passed: valid image ran, 14 malformed images refused, COPY/MOVE rules held"
            );
            kprintln!(
                "[input] self-test passed: one reader at a time, lost marker, no dup, ring emptied for the next owner"
            );
            kprintln!("[rng] random_fill self-test passed from ring 3");
            kprintln!(
                "[aslr] self-test passed: static PIE relocated at a random base, random stacks, bad relocations refused"
            );
        }
        Err(why) => panic!("user-mode self-test failed: {}", why),
    }

    #[cfg(feature = "selftest-preempt")]
    {
        assert!(
            arch::x86_64::selftest::preempt(),
            "preemption self-test failed"
        );
        kprintln!(
            "[sched] preemption self-test passed: busy threads were preempted, idle + wake from IRQ work"
        );
    }

    #[cfg(feature = "selftest-thread-stack-overflow")]
    arch::x86_64::selftest::thread_stack_overflow();

    #[cfg(feature = "selftest-irq-pit")]
    {
        assert!(arch::x86_64::selftest::irq_pit(), "PIT IRQ0 never arrived");
        kprintln!("[irq] hardware IRQ0 (PIT) delivered twice through the 8259 PIC (EOI works)");
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

    // Interrupts have been running for a while: their timing is in the
    // pool. Mix it in before the first real process gets random bytes.
    rng::reseed_now();

    match proc::start_init() {
        Ok(init) => {
            kprintln!("[init] /sbin/init started (pid {})", init.pid());
            // Test builds exit QEMU right after the boot: give init a
            // bounded chance to run (and, if it is short, to end) first,
            // so its output is deterministic in tests. Bounded by time
            // (about 1 s of timer ticks) as well as by iterations, so an
            // init that never blocks cannot hang a test run.
            #[cfg(feature = "qemu-exit")]
            {
                let deadline = sched::ticks() + 100;
                for _ in 0..200_000 {
                    if init.status() != proc::Status::Running || sched::ticks() > deadline {
                        break;
                    }
                    sched::yield_now();
                }
            }
        }
        Err(why) => kprintln!("[init] /sbin/init missing or invalid ({})", why),
    }

    kprintln!("[boot] boot complete, idling with interrupts enabled");

    #[cfg(feature = "qemu-exit")]
    arch::qemu::exit(arch::qemu::ExitCode::Success);

    // The boot thread becomes the idle thread: run whatever is ready, and
    // sleep (interrupts on) when nothing is.
    #[cfg(not(feature = "qemu-exit"))]
    loop {
        sched::yield_now();
        arch::x86_64::cpu::enable_interrupts_and_halt();
    }
}
