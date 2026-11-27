use super::*;

fn snapshot(header_type: u8) -> [u8; 256] {
    let mut bytes = [0; 256];
    bytes[..4].copy_from_slice(&[0x34, 0x12, 0xcd, 0xab]);
    bytes[0x0e] = header_type;
    bytes
}

fn enable_list(bytes: &mut [u8; 256], offset: u8) {
    bytes[6] = 0x10;
    bytes[0x34] = offset;
}

#[test]
fn decodes_the_multifunction_endpoint_prefix() {
    let mut image = snapshot(0x80);
    image[4..12].copy_from_slice(&[0x07, 0x01, 0x10, 0x08, 0x42, 0x11, 0x22, 0x33]);
    image[16..20].copy_from_slice(&[0x81, 0x45, 0x23, 0x01]);
    image[36..40].copy_from_slice(&[0x00, 0xef, 0xcd, 0xab]);
    image[44..48].copy_from_slice(&[0x33, 0x22, 0x55, 0x44]);
    image[60..62].copy_from_slice(&[0xfd, 1]);
    let config = ConfigSpace::parse(&image[..64]).unwrap();
    assert_eq!(
        *config.header(),
        Header {
            vendor_id: 0x1234,
            device_id: 0xabcd,
            command: 0x0107,
            status: 0x0810,
            revision: 0x42,
            programming_interface: 0x11,
            subclass: 0x22,
            class: 0x33,
            multifunction: true,
            interrupt_line: 0xfd,
            interrupt_pin: 1,
            layout: Layout::Endpoint(Endpoint {
                bars: [0x0123_4581, 0, 0, 0, 0, 0xabcd_ef00],
                subsystem_vendor_id: 0x2233,
                subsystem_id: 0x4455,
            }),
        }
    );
}

#[test]
fn decodes_bridge_bus_numbers_separately_from_bars() {
    let mut image = snapshot(1);
    image[16..27].copy_from_slice(&[0x80, 0x76, 0x54, 0x32, 0x01, 0xef, 0xcd, 0xab, 2, 3, 8]);
    enable_list(&mut image, 0x44);
    image[0x44] = 0x13;

    let config = ConfigSpace::parse(&image).unwrap();
    assert!(!config.header().multifunction);
    assert_eq!(
        config.header().layout,
        Layout::Bridge(Bridge {
            bars: [0x3254_7680, 0xabcd_ef01],
            primary_bus: 2,
            secondary_bus: 3,
            subordinate_bus: 8,
        })
    );
    assert_eq!(
        config.capabilities().next(),
        Some(Ok(Capability {
            offset: 0x44,
            id: 0x13
        }))
    );
}

#[test]
fn rejects_truncated_absent_and_unsupported_headers() {
    let image = snapshot(0);
    for length in 0..64 {
        assert!(matches!(
            ConfigSpace::parse(&image[..length]),
            Err(HeaderError::Truncated)
        ));
    }
    assert!(matches!(
        ConfigSpace::parse(&[0xff; 64]),
        Err(HeaderError::Absent)
    ));
    for kind in [2, 0x55, 0x7f] {
        assert!(
            matches!(ConfigSpace::parse(&snapshot(kind | 0x80)), Err(HeaderError::UnsupportedType(code)) if code == kind)
        );
    }
}

#[test]
fn ignores_unadvertised_capabilities_and_allows_backward_links() {
    let mut image = snapshot(0);
    image[0x34] = 0xff;
    assert_eq!(
        ConfigSpace::parse(&image).unwrap().capabilities().next(),
        None
    );

    enable_list(&mut image, 0x80);
    image[0x80..0x82].copy_from_slice(&[0xc3, 0x40]);
    image[0x40..0x42].copy_from_slice(&[0x05, 0x00]);
    let config = ConfigSpace::parse(&image).unwrap();
    assert!(config.capabilities().eq([
        Ok(Capability {
            offset: 0x80,
            id: 0xc3
        }),
        Ok(Capability {
            offset: 0x40,
            id: 0x05
        }),
    ]));
}

#[test]
fn checks_every_possible_initial_pointer_value() {
    let mut image = snapshot(0);
    for offset in 0..=u8::MAX {
        enable_list(&mut image, offset);
        let config = ConfigSpace::parse(&image).unwrap();
        let mut caps = config.capabilities();
        let expected = match offset {
            0 => None,
            position if position >= 64 && position & 3 == 0 => {
                Some(Ok(Capability { offset, id: 0 }))
            }
            position => Some(Err(CapabilityError::InvalidPointer(position))),
        };
        assert_eq!(caps.next(), expected);
        assert_eq!(caps.next(), None);
    }
}

#[test]
fn stops_on_repeated_or_invalid_links() {
    let mut image = snapshot(0);
    enable_list(&mut image, 0x40);
    image[0x41] = 0x44;
    for next in [0x40, 0x44, 0x3c] {
        image[0x45] = next;
        let config = ConfigSpace::parse(&image).unwrap();
        let mut caps = config.capabilities();
        assert_eq!(
            caps.next(),
            Some(Ok(Capability {
                offset: 0x40,
                id: 0
            }))
        );
        assert_eq!(
            caps.next(),
            Some(Ok(Capability {
                offset: 0x44,
                id: 0
            }))
        );
        let error = if next == 0x3c {
            CapabilityError::InvalidPointer(next)
        } else {
            CapabilityError::Cycle(next)
        };
        assert_eq!(caps.next(), Some(Err(error)));
        assert_eq!(caps.next(), None);
    }
}

#[test]
fn truncated_capabilities_need_both_id_and_link_bytes() {
    let mut image = snapshot(0);
    enable_list(&mut image, 0x40);
    image[0x40] = 9;
    for length in [64, 65] {
        let config = ConfigSpace::parse(&image[..length]).unwrap();
        let mut caps = config.capabilities();
        assert_eq!(caps.next(), Some(Err(CapabilityError::Truncated(0x40))));
        assert_eq!(caps.next(), None);
    }
    let config = ConfigSpace::parse(&image[..66]).unwrap();
    assert!(config.capabilities().eq([Ok(Capability {
        offset: 0x40,
        id: 9
    })]));
}

#[test]
fn walks_all_48_aligned_positions_in_conventional_space() {
    let mut image = snapshot(0);
    enable_list(&mut image, 0x40);
    for offset in (0x40_u8..=0xfc).step_by(4) {
        image[usize::from(offset)] = 9;
        image[usize::from(offset) + 1] = offset.checked_add(4).unwrap_or(0);
    }
    let config = ConfigSpace::parse(&image).unwrap();
    let expected = (0x40_u8..=0xfc)
        .step_by(4)
        .map(|offset| Ok(Capability { offset, id: 9 }));
    assert!(config.capabilities().eq(expected));
}

#[test]
fn decodes_io_and_memory_bars_without_reusing_the_upper_word() {
    let mut bytes = snapshot(0);
    for (slot, raw) in [0, 0xc001, 0xabcd0008, 0x1234500c, 0xfeed, 0]
        .into_iter()
        .enumerate()
    {
        let offset = 0x10 + slot * 4;
        bytes[offset..offset + 4].copy_from_slice(&u32::to_le_bytes(raw));
    }
    let cfg = ConfigSpace::parse(&bytes).unwrap();
    let mut bars = cfg.bars();
    assert_eq!(
        bars.next(),
        Some(Ok(Bar {
            slot: 1,
            resource: BarResource::Io { base: 0xc000 }
        }))
    );
    assert_eq!(
        bars.next(),
        Some(Ok(Bar {
            slot: 2,
            resource: BarResource::Memory32 {
                base: 0xabcd0000,
                prefetchable: true
            }
        }))
    );
    assert_eq!(
        bars.next(),
        Some(Ok(Bar {
            slot: 3,
            resource: BarResource::Memory64 {
                base: 0xfeed_12345000,
                prefetchable: true
            }
        }))
    );
    assert_eq!(bars.next(), None);
    assert_eq!(bars.next(), None);
}

#[test]
fn bridge_bus_words_cannot_complete_a_64_bit_bar() {
    let mut bytes = snapshot(1);
    bytes[0x14..0x18].copy_from_slice(&0x1004u32.to_le_bytes());
    bytes[0x18..0x1c].copy_from_slice(&[0x01, 0x02, 0x03, 0x04]);
    let cfg = ConfigSpace::parse(&bytes).unwrap();
    let mut bars = cfg.bars();
    assert_eq!(
        bars.next(),
        Some(Err(BarError::MissingUpperWord { slot: 1 }))
    );
    assert_eq!(bars.next(), None);
}

#[test]
fn reserved_encodings_report_their_slots_and_preserve_later_bars() {
    let mut bytes = snapshot(0);
    for (slot, raw) in [0x4003, 0x8002, 0x9006, 0xa001].into_iter().enumerate() {
        let offset = 0x10 + slot * 4;
        bytes[offset..offset + 4].copy_from_slice(&u32::to_le_bytes(raw));
    }
    let cfg = ConfigSpace::parse(&bytes).unwrap();
    let mut bars = cfg.bars();
    assert_eq!(bars.next(), Some(Err(BarError::ReservedIoBit { slot: 0 })));
    assert_eq!(
        bars.next(),
        Some(Err(BarError::UnsupportedMemoryType {
            slot: 1,
            memory_type: 1
        }))
    );
    assert_eq!(
        bars.next(),
        Some(Err(BarError::UnsupportedMemoryType {
            slot: 2,
            memory_type: 3
        }))
    );
    assert_eq!(
        bars.next(),
        Some(Ok(Bar {
            slot: 3,
            resource: BarResource::Io { base: 0xa000 }
        }))
    );
}
