//! Installs the validated boot archive as the read-only VFS root.

use kitty_vfs::InitrdFs;

use crate::sync::IrqSpinLock;

static ROOT: IrqSpinLock<Option<InitrdFs<'static>>> = IrqSpinLock::new(None);

/// Called once from `devices::init`, after the heap is available.
pub(super) fn init(archive: Option<&'static [u8]>) {
    let Some(bytes) = archive else { return };
    let root = match InitrdFs::mount(bytes) {
        Ok(root) => root,
        Err(error) => {
            crate::kprintln!("[vfs] initrd archive rejected ({error:?})");
            return;
        }
    };
    *ROOT.lock() = Some(root);
    crate::kprintln!("[vfs] initrd mounted read-only");

    #[cfg(feature = "selftest-initrd")]
    {
        let verified = ROOT.lock().as_ref().is_some_and(check_fixture);
        if verified {
            crate::kprintln!("[vfs] initrd read verified (/etc/kitty.txt)");
        }
    }
}

#[cfg(feature = "selftest-initrd")]
fn check_fixture(root: &InitrdFs<'_>) -> bool {
    use kitty_vfs::{Inode, Kind};

    const EXPECTED: &[u8] = b"kitty-OS initrd smoke test\n";
    let Ok(Inode::File(file)) = root.lookup("/etc/kitty.txt") else {
        return false;
    };
    let mut data = [0; EXPECTED.len()];
    file.len() == EXPECTED.len()
        && file.read_at(0, &mut data) == data.len()
        && data.as_slice() == EXPECTED
        && root.read_dir("/etc").is_ok_and(|mut entries| {
            entries.any(|entry| entry.name == "kitty.txt" && entry.kind == Kind::File)
        })
}

pub(crate) fn initrd_file(path: &str) -> Option<&'static [u8]> {
    let inode = ROOT.lock().as_ref()?.lookup(path).ok()?;
    match inode {
        kitty_vfs::Inode::File(file) => Some(file.as_bytes()),
        kitty_vfs::Inode::Directory => None,
    }
}
