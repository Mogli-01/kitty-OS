//! Validates the optional boot module before staging the boot media.

use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use kitty_vfs::InitrdFs;

use crate::{LIMINE_CONF, Result};

// Match the kernel module limit documented in docs/INTERFACES.md.
const MAX_INITRD_BYTES: u64 = 128 * 1024 * 1024;

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
    if !(1..=MAX_INITRD_BYTES).contains(&metadata.len()) {
        return Err(format!(
            "initrd {} must be between 1 byte and 128 MiB",
            path.display()
        ));
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

// Give the artwork a brief, interruptible display before booting the kernel.
const SPLASH_SETTINGS: &str = "\
timeout: 2
wallpaper: boot():/boot/kitty-welcome.png
wallpaper_style: centered
backdrop: 161B27
term_background: FF161B27
interface_branding: kitty-OS
interface_branding_colour: 7
interface_help_colour: 7
term_palette: 161B27;FF9696;92D1BA;F3B477;25384F;D87F94;B5BED0;B5BED0
term_palette_bright: 394159;FF9696;92D1BA;F3B477;465978;D87F94;F4EDE7;F4EDE7
interface_help_hidden: yes
";

pub(crate) fn limine_config(with_initrd: bool, splash: bool) -> String {
    let mut config = if splash {
        LIMINE_CONF.replacen("timeout: 0\n", SPLASH_SETTINGS, 1)
    } else {
        LIMINE_CONF.to_owned()
    };
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
        assert_eq!(limine_config(false, false), LIMINE_CONF);
        let enabled = limine_config(true, false);
        assert!(enabled.starts_with(LIMINE_CONF));
        assert!(enabled.ends_with(
            "    module_path: boot():/boot/initrd.tar\n    module_string: kitty-initrd\n"
        ));
    }
    #[test]
    fn adds_splash_options_without_changing_initrd_or_default_timeout() {
        let config = limine_config(true, true);
        assert!(config.contains("timeout: 2\nwallpaper: boot():/boot/kitty-welcome.png\n"));
        assert_eq!(config.matches("timeout:").count(), 1);
        assert!(config.contains("module_string: kitty-initrd"));
        assert!(!limine_config(false, false).contains("wallpaper"));
    }
}
