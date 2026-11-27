//! QEMU-only check that the active timer preempts a busy thread.

use core::sync::atomic::{AtomicU64, Ordering};

use crate::{kprintln, sched};

static IRQS: AtomicU64 = AtomicU64::new(0);

pub(super) fn count_irq() {
    IRQS.fetch_add(1, Ordering::Relaxed);
}

pub(super) fn verify_preemption(timer: &str) {
    for _ in 0..2 {
        let irq = IRQS.load(Ordering::Relaxed);
        let switches = sched::switches();
        while IRQS.load(Ordering::Relaxed) == irq || sched::switches() == switches {
            core::hint::spin_loop();
        }
    }
    kprintln!("[{timer}] driver preempted a busy thread twice");
    // Stay alive so the cooperative boot test sees the same thread count
    // at entry and exit. QEMU exits through the standard boot-test path.
    loop {
        sched::yield_now();
    }
}
