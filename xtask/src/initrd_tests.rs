//! Boots known initrd fixtures and checks the complete Limine-to-VFS path.

use std::fs;

use crate::{Opts, Result, boot_test_with_initrd, build_dir};

const BLOCK: usize = 512;
const MODULE_VALIDATED: &str = "[initrd] module validated";
const MOUNTED: &str = "[vfs] initrd mounted read-only";
const BOOT_COMPLETE: &str = "[boot] boot complete";

pub(super) fn run(opts: &Opts) -> Result<()> {
    if opts.initrd.is_some()
        || opts.expect_panic
        || !opts.expect.is_empty()
        || !opts.features.is_empty()
    {
        return Err(
            "test-initrd selects the archive, features and expectations for each case".into(),
        );
    }

    let firmware = if opts.bios { "bios" } else { "uefi" };
    let profile = if opts.release { "release" } else { "debug" };
    let logs = build_dir()
        .join("initrd-tests")
        .join(format!("{firmware}-{profile}"));
    fs::create_dir_all(&logs).map_err(|e| format!("creating {}: {e}", logs.display()))?;

    let archive = fixture();
    let mut invalid = archive;
    // Change one byte in the filename after writing the header checksum.
    invalid[0] = b'E';
    let invalid_markers = [
        MODULE_VALIDATED,
        "[vfs] initrd archive rejected (Archive(InvalidChecksum))",
    ];
    let media = logs.join("media");
    for (name, bytes, markers) in [
        (
            "bad-checksum",
            Some(invalid.as_slice()),
            invalid_markers.as_slice(),
        ),
        ("absent", None, ["[initrd] no module"].as_slice()),
    ] {
        let case = Opts {
            bios: opts.bios,
            release: opts.release,
            expect: markers
                .iter()
                .copied()
                .chain([BOOT_COMPLETE])
                .map(String::from)
                .collect(),
            ..Opts::default()
        };
        let log = logs.join(format!("{name}.log"));
        let serial = boot_test_with_initrd(&case, &log, bytes, &media)
            .map_err(|e| format!("{name}: {e} (log: {})", log.display()))?;
        if serial.contains(MOUNTED) {
            return Err(format!(
                "{name}: unexpectedly mounted an initrd (log: {})",
                log.display()
            ));
        }
    }

    let path = logs.join("fixture.tar");
    fs::write(&path, archive).map_err(|e| format!("writing {}: {e}", path.display()))?;
    let case = Opts {
        bios: opts.bios,
        release: opts.release,
        initrd: Some(path),
        features: vec!["selftest-initrd".into()],
        expect: [
            MODULE_VALIDATED,
            MOUNTED,
            "[vfs] initrd read verified (/etc/kitty.txt)",
            BOOT_COMPLETE,
        ]
        .into_iter()
        .map(String::from)
        .collect(),
        ..Opts::default()
    };
    let log = logs.join("valid.log");
    let valid = case.load_initrd()?;
    boot_test_with_initrd(&case, &log, valid.as_deref(), &media)
        .map_err(|e| format!("valid: {e} (log: {})", log.display()))?;
    let staged = media
        .join(if opts.bios { "iso_root" } else { "esp" })
        .join("boot/initrd.tar");
    let staged_bytes =
        fs::read(&staged).map_err(|e| format!("reading {}: {e}", staged.display()))?;
    if staged_bytes != archive {
        return Err(format!(
            "staged initrd in {} differs from the fixture",
            staged.display()
        ));
    }
    println!("xtask: initrd tests passed ({firmware}, {profile})");
    Ok(())
}

fn fixture() -> [u8; 4 * BLOCK] {
    let name = b"etc/kitty.txt";
    let contents = b"kitty-OS initrd smoke test\n";
    let mut archive = [0; 4 * BLOCK];
    let header = &mut archive[..BLOCK];
    header[..name.len()].copy_from_slice(name);
    header[100..108].copy_from_slice(b"0000644\0");
    header[108..116].copy_from_slice(b"0000000\0");
    header[116..124].copy_from_slice(b"0000000\0");
    header[124..136].copy_from_slice(format!("{:011o}\0", contents.len()).as_bytes());
    header[136..148].copy_from_slice(b"00000000000\0");
    header[148..156].fill(b' ');
    header[156] = b'0';
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    let sum: u32 = header.iter().map(|&byte| u32::from(byte)).sum();
    header[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
    archive[BLOCK..BLOCK + contents.len()].copy_from_slice(contents);
    archive
}
