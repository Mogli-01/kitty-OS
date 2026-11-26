//! Validates the optional boot module before staging the boot media.

use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use kitty_vfs::InitrdFs;

use crate::{LIMINE_CONF, Result};

pub(crate) fn load(path: &Path) -> Result<Vec<u8>> {
    let file_error = |e: std::io::Error| format!("reading initrd {}: {e}", path.display());
    if !fs::metadata(path).map_err(file_error)?.is_file() {
        return Err(format!("initrd {} must be a regular file", path.display()));
    }
    let mut file = File::open(path).map_err(file_error)?;
    let metadata = file.metadata().map_err(file_error)?;
    if !metadata.is_file() {
        return Err(format!("initrd {} changed before opening", path.display()));
    }
    let length = usize::try_from(metadata.len()).map_err(|_| "initrd length overflows usize")?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| format!("cannot reserve memory for initrd {}", path.display()))?;
    bytes.resize(length, 0);
    file.read_exact(&mut bytes).map_err(file_error)?;
    if file.read(&mut [0]).map_err(file_error)? != 0 {
        return Err(format!("initrd {} grew while reading", path.display()));
    }
    InitrdFs::mount(&bytes)
        .map_err(|error| format!("invalid initrd {}: {error:?}", path.display()))?;
    Ok(bytes)
}

pub(crate) fn limine_config(with_initrd: bool) -> String {
    let mut config = LIMINE_CONF.to_owned();
    if with_initrd {
        config.push_str(
            "    module_path: boot():/boot/initrd.tar\n    module_string: kitty-initrd\n",
        );
    }
    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_module_metadata_only_when_staging_initrd() {
        assert_eq!(limine_config(false), LIMINE_CONF);
        let enabled = limine_config(true);
        assert!(enabled.starts_with(LIMINE_CONF));
        assert!(enabled.ends_with(
            "    module_path: boot():/boot/initrd.tar\n    module_string: kitty-initrd\n"
        ));
    }
}
