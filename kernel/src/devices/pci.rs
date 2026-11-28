//! Adapts the kernel's configuration reader to the PCI driver's enumeration.

use crate::arch::x86_64::pci::{ConfigError, PciAddress, read_config};

pub fn enumerate() -> Result<usize, ConfigError> {
    let mut count = 0;
    kitty_pci::enumerate(
        |address, bytes| {
            let address = PciAddress::new(address.bus, address.device, address.function)?;
            match read_config(address, bytes) {
                Ok(()) => Ok(true),
                Err(ConfigError::Absent) => Ok(false),
                Err(error) => Err(error),
            }
        },
        |address, header| {
            count += 1;
            #[cfg(not(feature = "selftest-pci-enumeration"))]
            let _ = (address, header);
            #[cfg(feature = "selftest-pci-enumeration")]
            crate::kprintln!(
                "[pci] {:02x}:{:02x}.{} class {:02x}:{:02x}",
                address.bus,
                address.device,
                address.function,
                header.class,
                header.subclass
            );
            #[cfg(feature = "selftest-virtio-blk-pci")]
            if header.is_virtio_block() {
                probe_virtio_block(address);
            }
        },
    )?;
    Ok(count)
}

#[cfg(feature = "selftest-virtio-blk-pci")]
fn probe_virtio_block(address: kitty_pci::Address) {
    let mut snapshot = [0; crate::arch::x86_64::pci::CONVENTIONAL_LEN];
    let config = PciAddress::new(address.bus, address.device, address.function)
        .and_then(|address| read_config(address, &mut snapshot))
        .ok()
        .and_then(|()| kitty_pci::ConfigSpace::parse(&snapshot).ok());
    if let Some(config) = config
        && let Ok(Some(_)) = config.virtio_block_layout()
    {
        crate::kprintln!("[pci] virtio-blk modern configuration discovered");
    }
}
