//! Connects the validated boot MADT and APIC register capabilities to IRQs.

use alloc::vec::Vec;
#[cfg(feature = "selftest-xapic")]
use core::sync::atomic::{AtomicU8, AtomicU32, Ordering};
use kitty_acpi::madt::{IsaRoute, Madt, Polarity, TriggerMode};
use kitty_apic::io::{IoApic, Route};
use kitty_apic::local::{self, LocalApic, Nmi};

use crate::arch::apic::{self, IoApicRegisters, LocalApicRegisters};
use crate::arch::irq::{self, ISA_LINES, InterruptController, IrqError, Vector};
use crate::sync::IrqSpinLock;

#[cfg(all(feature = "selftest-lapic-timer", feature = "qemu-exit"))]
mod timer_test;

type Io = IoApic<IoApicRegisters>;

#[derive(Clone, Copy)]
struct Input {
    controller: usize,
    route: IsaRoute,
}

struct State {
    local: LocalApic<LocalApicRegisters>,
    io: Vec<Io>,
    isa: [Option<Input>; ISA_LINES as usize],
    bsp_id: u8,
}

struct Controller(IrqSpinLock<Option<State>>);

static CONTROLLER: Controller = Controller(IrqSpinLock::new(None));
#[cfg(feature = "selftest-xapic")]
static IRQ0_VECTOR: AtomicU8 = AtomicU8::new(0);
#[cfg(feature = "selftest-xapic")]
static IRQ0_EOIS: AtomicU32 = AtomicU32::new(0);

pub(super) fn init(madt: Option<&[u8]>) {
    let Some(madt) = madt else {
        return;
    };
    let state = match prepare(madt) {
        Ok(state) => state,
        Err(reason) => {
            crate::kprintln!("[apic] keeping the PIC ({reason})");
            return;
        }
    };
    // Publish the prepared state before the IRQ core can call any methods.
    *CONTROLLER.0.lock() = Some(state);
    irq::set_controller(&CONTROLLER);
    #[cfg(all(feature = "selftest-lapic-timer", feature = "qemu-exit"))]
    timer_test::start();
}

fn prepare(bytes: &[u8]) -> Result<State, &'static str> {
    let madt = Madt::parse(bytes).map_err(|_| "invalid MADT routing records")?;
    let registers = apic::local_apic().map_err(|_| "no xAPIC capability")?;
    let mut local = LocalApic::probe(registers).map_err(|_| "unsupported local APIC")?;
    let bsp_id = local.id().map_err(|_| "invalid boot CPU APIC ID")?;
    let [lint0, lint1] = madt
        .local_nmis(bsp_id)
        .map_err(|_| "unsupported NMI topology")?;
    if lint0.is_some() {
        return Err("NMI assigned to legacy LINT0");
    }
    if lint1.is_some_and(|pin| pin.trigger == TriggerMode::Level) {
        return Err("level-triggered LINT1 NMI is unsupported");
    }

    let mut io: Vec<Io> = Vec::new();
    io.try_reserve_exact(madt.io_apics().count())
        .map_err(|_| "no memory for I/O APIC list")?;
    for descriptor in madt.io_apics() {
        let registers = apic::io_apic(u64::from(descriptor.address))
            .map_err(|_| "I/O APIC capability refused")?;
        let device = IoApic::probe(registers, descriptor.gsi_base)
            .map_err(|_| "invalid I/O APIC register layout")?;
        let range = device.gsis();
        if io.iter().any(|other| {
            let other = other.gsis();
            range.start() <= other.end() && other.start() <= range.end()
        }) {
            return Err("overlapping I/O APIC input ranges");
        }
        io.push(device);
    }
    let isa = select_isa(&madt, &io)?;
    for chip in &mut io {
        chip.check_legacy_handoff()
            .map_err(|_| "active or unreadable firmware I/O APIC route")?;
    }
    for chip in &mut io {
        chip.quiesce_for_handoff()
            .map_err(|_| "could not quiesce I/O APIC inputs")?;
    }

    let nmi = lint1.map(|pin| Nmi {
        active_low: pin.polarity == Polarity::ActiveLow,
    });
    match local.enable_isa_delivery(nmi) {
        Ok(()) => Ok(State {
            local,
            io,
            isa,
            bsp_id,
        }),
        Err(local::Error::RollbackFailed) => panic!("could not restore legacy APIC delivery"),
        Err(error) => {
            crate::kprintln!("[apic] local delivery setup refused ({error:?})");
            Err("local APIC delivery could not be prepared")
        }
    }
}

fn select_isa(
    madt: &Madt<'_>,
    io: &[Io],
) -> Result<[Option<Input>; ISA_LINES as usize], &'static str> {
    let controller_for = |gsi| io.iter().position(|chip| chip.gsis().contains(&gsi));
    let mut inputs: [Option<Input>; ISA_LINES as usize] = [None; ISA_LINES as usize];
    // Firmware overrides take precedence over default identity routing.
    // For example, QEMU routes IRQ0 to GSI 2, consuming the default GSI of IRQ2.
    for line in 0..ISA_LINES {
        let Some(route) = madt.isa_override(line) else {
            continue;
        };
        if inputs
            .iter()
            .flatten()
            .any(|input| input.route.gsi == route.gsi)
        {
            return Err("overrides share an I/O APIC input");
        }
        let controller = controller_for(route.gsi).ok_or("override outside I/O APIC inputs")?;
        inputs[usize::from(line)] = Some(Input { controller, route });
    }
    for line in 0..ISA_LINES {
        let slot = usize::from(line);
        let gsi = u32::from(line);
        if inputs[slot].is_some() || inputs.iter().flatten().any(|input| input.route.gsi == gsi) {
            continue;
        }
        if let Some(controller) = controller_for(gsi) {
            let route = IsaRoute {
                gsi,
                polarity: Polarity::ActiveHigh,
                trigger: TriggerMode::Edge,
            };
            inputs[slot] = Some(Input { controller, route });
        }
    }
    if inputs[0].is_none() || inputs[1].is_none() {
        return Err("missing PIT or keyboard route");
    }
    Ok(inputs)
}

impl Controller {
    fn with_input<T>(
        &self,
        line: u8,
        work: impl FnOnce(&mut Io, IsaRoute, u8) -> Result<T, kitty_apic::io::Error>,
    ) -> Result<T, IrqError> {
        let mut guard = self.0.lock();
        let state = guard.as_mut().ok_or(IrqError::NotInitialised)?;
        let input = state
            .isa
            .get(usize::from(line))
            .copied()
            .flatten()
            .ok_or(IrqError::InvalidLine)?;
        work(&mut state.io[input.controller], input.route, state.bsp_id)
            .map_err(|_| IrqError::ControllerError)
    }
}

impl InterruptController for Controller {
    fn name(&self) -> &'static str {
        "I/O APIC + xAPIC"
    }

    fn connect_isa(&self, line: u8, preferred: Vector) -> Result<Vector, IrqError> {
        self.with_input(line, |chip, isa, destination_id| {
            chip.configure_masked(
                isa.gsi,
                Route {
                    vector: preferred.0,
                    destination_id,
                    active_low: isa.polarity == Polarity::ActiveLow,
                    level_triggered: isa.trigger == TriggerMode::Level,
                },
            )
        })?;
        #[cfg(feature = "selftest-xapic")]
        if line == 0 {
            IRQ0_VECTOR.store(preferred.0, Ordering::Relaxed);
        }
        Ok(preferred)
    }

    fn unmask_isa(&self, line: u8) {
        if self
            .with_input(line, |chip, route, _| chip.unmask(route.gsi))
            .is_err()
        {
            crate::console::debug_print(format_args!("[apic] could not unmask ISA IRQ {line}\n"));
        }
    }

    fn mask_isa(&self, line: u8) {
        if self
            .with_input(line, |chip, route, _| chip.mask(route.gsi))
            .is_err()
        {
            crate::console::debug_print(format_args!("[apic] could not mask ISA IRQ {line}\n"));
        }
    }

    fn eoi(&self, vector: Vector) {
        let _acknowledged = self
            .0
            .lock()
            .as_mut()
            .is_some_and(|state| state.local.end_of_interrupt(vector.0).is_ok());
        #[cfg(all(feature = "selftest-lapic-timer", feature = "qemu-exit"))]
        if _acknowledged {
            timer_test::acknowledged(vector);
        }
        #[cfg(feature = "selftest-xapic")]
        if _acknowledged
            && vector.0 == IRQ0_VECTOR.load(Ordering::Relaxed)
            && IRQ0_EOIS.fetch_add(1, Ordering::Relaxed) == 1
        {
            crate::console::debug_print(format_args!("[apic] two IRQ0 deliveries acknowledged\n"));
        }
    }

    fn is_spurious(&self, vector: Vector) -> bool {
        vector.0 == local::SPURIOUS_VECTOR
    }

    fn disable(&self) {
        if let Some(state) = self.0.lock().as_mut() {
            for chip in &mut state.io {
                if chip.mask_all().is_err() {
                    crate::console::debug_print(format_args!("[apic] could not mask all inputs\n"));
                }
            }
        }
    }
}
